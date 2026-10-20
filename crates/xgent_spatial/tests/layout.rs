//! S0 布局验收：确定性、增量钉住、10k 性能。

use std::path::Path;

use xgent_core::spatial::{
    ClusterId, EdgeKind, SpatialCluster, SpatialEdge, SpatialGraph, SpatialNode, SpatialNodeId,
    SpatialNodeKind, SpatialStatus,
};
use xgent_spatial::{LayoutConfig, layout, layout_incremental, scan};

fn synthetic(node_count: usize, cluster_count: usize, edge_count: usize) -> SpatialGraph {
    assert!(cluster_count > 0 && node_count > 0);

    let clusters: Vec<SpatialCluster> = (0..cluster_count)
        .map(|index| SpatialCluster {
            id: ClusterId(index as u32),
            label: format!("c{index}"),
            path: None,
            weight: 1.0,
            anchor: None,
        })
        .collect();

    let nodes: Vec<SpatialNode> = (0..node_count)
        .map(|index| SpatialNode {
            id: SpatialNodeId(index as u32),
            kind: SpatialNodeKind::Crate,
            label: format!("n{index}"),
            path: None,
            cluster: ClusterId((index % clusters.len().max(1)) as u32),
            weight: 1.0,
            status: SpatialStatus::Idle,
            anchor: None,
        })
        .collect();

    let edges: Vec<SpatialEdge> = (0..edge_count)
        .map(|index| {
            let from = index % nodes.len();
            let mut to = (index.wrapping_mul(7).wrapping_add(3)) % nodes.len();
            if to == from {
                to = (to + 1) % nodes.len();
            }
            SpatialEdge {
                from: SpatialNodeId(from as u32),
                to: SpatialNodeId(to as u32),
                kind: EdgeKind::DependsOn,
                weight: 1.0,
            }
        })
        .collect();

    SpatialGraph {
        generation: 1,
        nodes,
        edges,
        clusters,
    }
}

fn node_bits(graph: &SpatialGraph) -> Vec<[u32; 3]> {
    graph
        .nodes
        .iter()
        .map(|node| {
            node.anchor
                .expect("布局后每个节点都应有 anchor")
                .map(f32::to_bits)
        })
        .collect()
}

fn cluster_bits(graph: &SpatialGraph) -> Vec<[u32; 3]> {
    graph
        .clusters
        .iter()
        .map(|cluster| {
            cluster
                .anchor
                .expect("布局后每个簇都应有 anchor")
                .map(f32::to_bits)
        })
        .collect()
}

#[test]
fn full_layout_is_bitwise_deterministic() {
    let config = LayoutConfig::default();
    let mut first = synthetic(300, 12, 400);
    layout(&mut first, &config);
    let first_nodes = node_bits(&first);
    let first_clusters = cluster_bits(&first);

    for node in &mut first.nodes {
        node.anchor = None;
    }
    for cluster in &mut first.clusters {
        cluster.anchor = None;
    }
    layout(&mut first, &config);

    assert_eq!(first_nodes, node_bits(&first));
    assert_eq!(first_clusters, cluster_bits(&first));
}

#[test]
fn workspace_layout_is_bitwise_deterministic() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace 根目录");
    let config = LayoutConfig::default();

    let mut graph = scan(workspace_root).expect("扫描工作区");
    layout(&mut graph, &config);
    let first = node_bits(&graph);

    for node in &mut graph.nodes {
        node.anchor = None;
    }
    for cluster in &mut graph.clusters {
        cluster.anchor = None;
    }
    layout(&mut graph, &config);

    assert_eq!(first, node_bits(&graph));
}

#[test]
fn incremental_layout_keeps_existing_positions() {
    let config = LayoutConfig::default();
    let mut graph = synthetic(500, 20, 600);
    layout(&mut graph, &config);

    let existing_nodes = node_bits(&graph);
    let existing_clusters = cluster_bits(&graph);

    for index in 500..600usize {
        graph.nodes.push(SpatialNode {
            id: SpatialNodeId(index as u32),
            kind: SpatialNodeKind::Crate,
            label: format!("n{index}"),
            path: None,
            cluster: ClusterId((index % graph.clusters.len()) as u32),
            weight: 1.0,
            status: SpatialStatus::Idle,
            anchor: None,
        });
    }

    layout_incremental(&mut graph, &config);

    assert_eq!(existing_nodes, node_bits(&graph)[..500]);
    assert_eq!(existing_clusters, cluster_bits(&graph));
    assert!(graph.nodes[500..].iter().all(|node| node.anchor.is_some()));
}

#[test]
fn layout_fills_every_anchor() {
    let mut graph = synthetic(120, 8, 150);
    layout(&mut graph, &LayoutConfig::default());
    assert!(graph.nodes.iter().all(|node| node.anchor.is_some()));
    assert!(
        graph
            .clusters
            .iter()
            .all(|cluster| cluster.anchor.is_some())
    );
}

#[test]
#[ignore = "性能验收：用 cargo test -p xgent_spatial --release -- --ignored 运行"]
fn ten_thousand_nodes_layout_under_300ms() {
    let mut graph = synthetic(10_000, 200, 20_000);
    let report = layout(&mut graph, &LayoutConfig::default());
    println!(
        "10k 布局：{} 节点 / {} 边 / {} 轮 / {:?}",
        report.nodes, report.edges, report.iterations, report.elapsed
    );
    assert!(graph.nodes.iter().all(|node| node.anchor.is_some()));
    assert!(
        report.elapsed.as_millis() < 300,
        "10k 布局耗时 {:?}，超过 300ms",
        report.elapsed
    );
}
