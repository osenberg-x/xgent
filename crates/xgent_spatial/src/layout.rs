//! 确定性三维布局。
//!
//! 设计约束（`ui-v8-3d-viewport.md` §2.4）：
//! - **同一 commit 同一坐标**：初始位置只由节点/簇的稳定键与固定种子决定，
//!   迭代顺序固定为 `Vec` 顺序，不使用时间、系统随机或 `HashMap` 迭代顺序。
//! - **增量钉住**：已有 `anchor` 的节点不累加力、不写坐标，位移严格为 0。
//! - **有界计算**：簇级斥力在簇数 ≤ 256 时做全对（数量本身很小），更多时走均匀网格；
//!   节点级只在本簇内做有限轮松弛，不做全图 O(n²)。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use xgent_core::spatial::{ClusterId, SpatialGraph};

/// 布局参数。
#[derive(Debug, Clone, Copy)]
pub struct LayoutConfig {
    /// 簇级迭代轮数。
    pub iterations: u32,
    /// 固定种子；改变它会产生另一套（同样确定的）坐标。
    pub seed: u64,
    /// 簇心分布半径。
    pub cluster_radius: f32,
    /// 节点相对簇心的分布半径。
    pub node_radius: f32,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            iterations: 60,
            seed: 0x9E37_79B9_7F4A_7C15,
            cluster_radius: 420.0,
            node_radius: 80.0,
        }
    }
}

/// 一次布局的统计。
#[derive(Debug, Clone, Copy)]
pub struct LayoutReport {
    /// 参与布局的节点数。
    pub nodes: usize,
    /// 星团数。
    pub clusters: usize,
    /// 边数。
    pub edges: usize,
    /// 实际执行的簇级迭代轮数。
    pub iterations: u32,
    /// 布局耗时。
    pub elapsed: Duration,
}

/// 全量布局：忽略已有 `anchor`，从确定性初始位置重新计算所有坐标。
pub fn layout(graph: &mut SpatialGraph, config: &LayoutConfig) -> LayoutReport {
    layout_impl(graph, config, false)
}

/// 增量布局：已有 `anchor` 的簇与节点保持不动，只为新节点补坐标。
pub fn layout_incremental(graph: &mut SpatialGraph, config: &LayoutConfig) -> LayoutReport {
    layout_impl(graph, config, true)
}

fn layout_impl(graph: &mut SpatialGraph, config: &LayoutConfig, incremental: bool) -> LayoutReport {
    let started = Instant::now();
    let iterations = config.iterations.min(512);

    if graph.clusters.is_empty() {
        return LayoutReport {
            nodes: graph.nodes.len(),
            clusters: 0,
            edges: graph.edges.len(),
            iterations: 0,
            elapsed: started.elapsed(),
        };
    }

    let cluster_index: BTreeMap<ClusterId, usize> = graph
        .clusters
        .iter()
        .enumerate()
        .map(|(index, cluster)| (cluster.id, index))
        .collect();

    let mut cluster_positions: Vec<[f32; 3]> = graph
        .clusters
        .iter()
        .map(|cluster| match (incremental, cluster.anchor) {
            (true, Some(anchor)) => anchor,
            _ => {
                let key = cluster
                    .path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| cluster.label.clone());
                scale(
                    unit_from_hash(hash_str(&key, config.seed)),
                    config.cluster_radius,
                )
            }
        })
        .collect();

    let cluster_pinned: Vec<bool> = graph
        .clusters
        .iter()
        .map(|cluster| incremental && cluster.anchor.is_some())
        .collect();

    let mut cluster_edges: Vec<(usize, usize)> = Vec::with_capacity(graph.edges.len());
    let node_cluster: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .filter_map(|node| {
            cluster_index
                .get(&node.cluster)
                .map(|cluster| (node.id, *cluster))
        })
        .collect();
    for edge in &graph.edges {
        let from = node_cluster.get(&edge.from).copied();
        let to = node_cluster.get(&edge.to).copied();
        if let (Some(from), Some(to)) = (from, to)
            && from != to
        {
            cluster_edges.push((from, to));
        }
    }

    relax_clusters(
        &mut cluster_positions,
        &cluster_pinned,
        &cluster_edges,
        config,
        iterations,
    );

    for (index, cluster) in graph.clusters.iter_mut().enumerate() {
        cluster.anchor = Some(cluster_positions[index]);
    }

    // 节点：按 graph.nodes 顺序分组，保证初始位置与松弛顺序都确定。
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); graph.clusters.len()];
    for (index, node) in graph.nodes.iter().enumerate() {
        if let Some(cluster) = cluster_index.get(&node.cluster) {
            members[*cluster].push(index);
        }
    }

    let mut node_positions: Vec<[f32; 3]> = graph
        .nodes
        .iter()
        .map(|node| node.anchor.unwrap_or([0.0, 0.0, 0.0]))
        .collect();
    let node_pinned: Vec<bool> = graph
        .nodes
        .iter()
        .map(|node| incremental && node.anchor.is_some())
        .collect();

    let relax_iterations = (iterations / 8).clamp(0, 8);
    for (cluster_index_value, member_indices) in members.iter().enumerate() {
        let center = cluster_positions[cluster_index_value];
        for (order, node_index) in member_indices.iter().enumerate() {
            if node_pinned[*node_index] {
                continue;
            }
            let node = &graph.nodes[*node_index];
            let key = node
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| node.label.clone());
            let hash = hash_str(
                &key,
                config.seed ^ (order as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            );
            let direction = unit_from_hash(hash);
            let distance = config.node_radius * (0.45 + 0.55 * rand01(hash));
            node_positions[*node_index] = add(center, scale(direction, distance));
        }

        relax_nodes(
            &mut node_positions,
            member_indices,
            &node_pinned,
            center,
            config.node_radius,
            relax_iterations,
        );
    }

    for (index, node) in graph.nodes.iter_mut().enumerate() {
        if !node_pinned[index] {
            node.anchor = Some(node_positions[index]);
        }
    }

    LayoutReport {
        nodes: graph.nodes.len(),
        clusters: graph.clusters.len(),
        edges: graph.edges.len(),
        iterations,
        elapsed: started.elapsed(),
    }
}

fn relax_clusters(
    positions: &mut [[f32; 3]],
    pinned: &[bool],
    edges: &[(usize, usize)],
    config: &LayoutConfig,
    iterations: u32,
) {
    let count = positions.len();
    if count < 2 || iterations == 0 {
        return;
    }

    let radius = config.cluster_radius.max(1.0);
    let repulsion = 1_200.0_f32;
    let attraction = 0.08_f32;
    let damping = 0.15_f32;
    let rest = radius * 0.9;

    for _ in 0..iterations {
        let mut displacement = vec![[0.0_f32; 3]; count];

        if count <= 256 {
            for a in 0..count {
                for b in (a + 1)..count {
                    accumulate_repulsion(
                        positions,
                        &mut displacement,
                        pinned,
                        a,
                        b,
                        repulsion,
                        damping,
                    );
                }
            }
        } else {
            grid_repulsion(
                positions,
                &mut displacement,
                pinned,
                radius,
                repulsion,
                damping,
            );
        }

        for &(from, to) in edges {
            let delta = sub(positions[from], positions[to]);
            let distance = length(delta).max(1e-3);
            let force = (distance - rest) * attraction * damping;
            let direction = scale(delta, 1.0 / distance);
            if !pinned[from] {
                displacement[from] = add(displacement[from], scale(direction, -force));
            }
            if !pinned[to] {
                displacement[to] = add(displacement[to], scale(direction, force));
            }
        }

        for index in 0..count {
            if pinned[index] {
                continue;
            }
            let next = add(positions[index], displacement[index]);
            let distance = length(next).max(1e-3);
            let clamped = distance.clamp(radius * 0.30, radius * 1.10);
            positions[index] = scale(next, clamped / distance);
        }
    }
}

fn accumulate_repulsion(
    positions: &[[f32; 3]],
    displacement: &mut [[f32; 3]],
    pinned: &[bool],
    a: usize,
    b: usize,
    strength: f32,
    damping: f32,
) {
    if pinned[a] && pinned[b] {
        return;
    }
    let delta = sub(positions[a], positions[b]);
    let mut distance_squared = dot(delta, delta);
    let direction = if distance_squared < 1e-4 {
        // 完全重合时用下标构造一个确定的方向，避免随机。
        distance_squared = 1e-4;
        unit_from_hash((a as u64) << 32 | b as u64)
    } else {
        scale(delta, 1.0 / distance_squared.sqrt())
    };
    let force = strength / distance_squared * damping;
    if !pinned[a] {
        displacement[a] = add(displacement[a], scale(direction, force));
    }
    if !pinned[b] {
        displacement[b] = sub(displacement[b], scale(direction, force));
    }
}

fn grid_repulsion(
    positions: &[[f32; 3]],
    displacement: &mut [[f32; 3]],
    pinned: &[bool],
    radius: f32,
    strength: f32,
    damping: f32,
) {
    let count = positions.len();
    let mut side = 1usize;
    while side * side * side < count {
        side += 1;
    }
    let cell = (2.0 * radius) / side as f32;
    let cells = side * side * side;

    let index_of = |position: [f32; 3]| -> usize {
        let x =
            (((position[0] + radius) / cell).floor() as isize).clamp(0, side as isize - 1) as usize;
        let y =
            (((position[1] + radius) / cell).floor() as isize).clamp(0, side as isize - 1) as usize;
        let z =
            (((position[2] + radius) / cell).floor() as isize).clamp(0, side as isize - 1) as usize;
        (z * side + y) * side + x
    };

    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); cells];
    for (index, position) in positions.iter().enumerate() {
        buckets[index_of(*position)].push(index as u32);
    }

    let cutoff = cell * 1.5;
    let cutoff_squared = cutoff * cutoff;

    for a in 0..count {
        let base = index_of(positions[a]);
        let bx = base % side;
        let by = (base / side) % side;
        let bz = base / (side * side);
        for dz in -1..=1isize {
            for dy in -1..=1isize {
                for dx in -1..=1isize {
                    let x = bx as isize + dx;
                    let y = by as isize + dy;
                    let z = bz as isize + dz;
                    if x < 0
                        || y < 0
                        || z < 0
                        || x >= side as isize
                        || y >= side as isize
                        || z >= side as isize
                    {
                        continue;
                    }
                    let bucket = (z as usize * side + y as usize) * side + x as usize;
                    for &b in &buckets[bucket] {
                        let b = b as usize;
                        if b <= a {
                            continue;
                        }
                        let delta = sub(positions[a], positions[b]);
                        let distance_squared = dot(delta, delta);
                        if distance_squared > cutoff_squared {
                            continue;
                        }
                        accumulate_repulsion(
                            positions,
                            displacement,
                            pinned,
                            a,
                            b,
                            strength,
                            damping,
                        );
                    }
                }
            }
        }
    }
}

fn relax_nodes(
    positions: &mut [[f32; 3]],
    members: &[usize],
    pinned: &[bool],
    center: [f32; 3],
    node_radius: f32,
    iterations: u32,
) {
    if members.len() < 2 || iterations == 0 {
        return;
    }
    let strength = 1.0_f32;
    let damping = 0.12_f32;
    let max_radius = node_radius * 1.6;

    for _ in 0..iterations {
        let mut displacement: Vec<[f32; 3]> = vec![[0.0; 3]; members.len()];
        for a in 0..members.len() {
            for b in (a + 1)..members.len() {
                let index_a = members[a];
                let index_b = members[b];
                if pinned[index_a] && pinned[index_b] {
                    continue;
                }
                let delta = sub(positions[index_a], positions[index_b]);
                let mut distance_squared = dot(delta, delta);
                let direction = if distance_squared < 1e-4 {
                    distance_squared = 1e-4;
                    unit_from_hash((index_a as u64) << 32 | index_b as u64)
                } else {
                    scale(delta, 1.0 / distance_squared.sqrt())
                };
                let force = strength / distance_squared * damping;
                if !pinned[index_a] {
                    displacement[a] = add(displacement[a], scale(direction, force));
                }
                if !pinned[index_b] {
                    displacement[b] = sub(displacement[b], scale(direction, force));
                }
            }
        }
        for (slot, node_index) in members.iter().enumerate() {
            if pinned[*node_index] {
                continue;
            }
            let next = add(positions[*node_index], displacement[slot]);
            let offset = sub(next, center);
            let distance = length(offset).max(1e-3);
            let clamped = distance.min(max_radius);
            positions[*node_index] = add(center, scale(offset, clamped / distance));
        }
    }
}

// ===== 确定性数学工具 =====

fn hash_str(value: &str, seed: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    splitmix64(hash)
}

fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn rand01(hash: u64) -> f32 {
    ((hash >> 11) as f32) / ((1u64 << 53) as f32)
}

fn unit_from_hash(hash: u64) -> [f32; 3] {
    let x = rand01(hash) * 2.0 - 1.0;
    let y = rand01(hash.rotate_left(21)) * 2.0 - 1.0;
    let z = rand01(hash.rotate_left(42)) * 2.0 - 1.0;
    let magnitude = (x * x + y * y + z * z).sqrt();
    if magnitude < 1e-6 {
        [1.0, 0.0, 0.0]
    } else {
        [x / magnitude, y / magnitude, z / magnitude]
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(value: [f32; 3], factor: f32) -> [f32; 3] {
    [value[0] * factor, value[1] * factor, value[2] * factor]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(value: [f32; 3]) -> f32 {
    dot(value, value).sqrt()
}
