//! 工作区扫描：把 crate 依赖投影成 L0 `SpatialGraph`。
//!
//! 只读文件系统，不修改任何内容；输出顺序完全确定（节点按 package 名、边按 id 对），
//! 因此同一工作区快照必然得到同一 `generation` 与同一节点顺序。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use xgent_core::spatial::{
    ClusterId, EdgeKind, SpatialCluster, SpatialEdge, SpatialGraph, SpatialNode, SpatialNodeId,
    SpatialNodeKind, SpatialStatus,
};

use crate::error::SpatialError;
use crate::manifest::{DependencyKind, ManifestDependency, parse_manifest_at};

/// 扫描时跳过的目录名：构建产物、版本库、Agent 状态与工作树。
const SKIP_DIRS: &[&str] = &[
    "target",
    ".git",
    ".comet",
    ".worktrees",
    "node_modules",
    ".claude",
    ".agents",
    ".zcode",
    ".omp",
    ".dsh",
    ".workbuddy",
    ".workbuddy-ai",
    ".xgent",
];

struct Package {
    name: String,
    dir: PathBuf,
    dependencies: Vec<ManifestDependency>,
}

/// 扫描 `root` 下的所有 `Cargo.toml`，生成 crate 级空间图。
///
/// - 只有含 `[package]` 的 manifest 成为节点；纯 `[workspace]` 仅用于发现根。
/// - `Normal` 与 `Build` 依赖中能解析到工作区内部的，生成 `DependsOn` 边；
///   `Dev` 依赖只解析、不生成边。
/// - 无法解析到工作区内部的依赖视为外部依赖，不生成节点或边，也不报错。
#[derive(Debug, Clone, Copy)]
/// L1 扫描选项。
pub struct ScanOptions {
    /// 是否生成文件/目录级 L1 节点。
    pub l1: bool,
    /// 节点上限；`0` 表示不限。
    pub node_limit: usize,
    /// 是否应用 `.gitignore` / 隐藏文件规则。
    pub ignore: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            l1: false,
            node_limit: 2000,
            ignore: true,
        }
    }
}

pub fn scan(root: &Path) -> Result<SpatialGraph, SpatialError> {
    scan_with_options(root, ScanOptions::default())
}

/// 按选项扫描：`l1=false` 保持 S0 的 crate 级行为。
pub fn scan_with_options(
    root: &Path,
    options: ScanOptions,
) -> Result<SpatialGraph, SpatialError> {
    let l0 = scan_l0(root)?;
    if !options.l1 {
        return Ok(l0);
    }
    build_l1(root, options, l0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FsKind {
    Dir,
    File,
}

#[derive(Clone)]
struct FsEntry {
    path: PathBuf,
    kind: FsKind,
    depth: usize,
    size: u64,
    top: Option<String>,
}

impl FsEntry {
    fn weight(&self) -> f32 {
        if self.kind == FsKind::File {
            ((self.size as f32) + 1.0).ln()
        } else {
            1.0
        }
    }
}

fn build_l1(
    root: &Path,
    options: ScanOptions,
    l0: SpatialGraph,
) -> Result<SpatialGraph, SpatialError> {
    let entries = collect_fs_entries(root, options.ignore)?;
    let crate_count = l0.nodes.len();
    let l1_limit = if options.node_limit == 0 {
        usize::MAX
    } else {
        options.node_limit.saturating_sub(crate_count)
    };
    let (selected, omitted_nodes) = select_entries(&entries, l1_limit);
    let (clusters, cluster_of_top) = build_clusters(root, &selected);

    let mut nodes = Vec::with_capacity(selected.len() + l0.nodes.len());
    let mut path_to_id = BTreeMap::new();
    for entry in &selected {
        let id = SpatialNodeId(nodes.len() as u32);
        path_to_id.insert(entry.path.clone(), id);
        let label = entry
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| root_label(root));
        let cluster = entry
            .top
            .as_ref()
            .and_then(|top| cluster_of_top.get(top))
            .copied()
            .unwrap_or(ClusterId(0));
        nodes.push(SpatialNode {
            id,
            kind: match entry.kind {
                FsKind::Dir => SpatialNodeKind::Module,
                FsKind::File => SpatialNodeKind::File,
            },
            label,
            path: Some(entry.path.clone()),
            cluster,
            weight: entry.weight(),
            status: SpatialStatus::Idle,
            anchor: None,
        });
    }

    // 目录权重 = 1 + 后代文件权重；用于 LOD/半径。
    let dir_ids: BTreeMap<PathBuf, SpatialNodeId> = nodes
        .iter()
        .filter(|node| node.kind == SpatialNodeKind::Module)
        .filter_map(|node| node.path.clone().map(|path| (path, node.id)))
        .collect();
    let mut dir_weights: BTreeMap<SpatialNodeId, f32> =
        dir_ids.values().map(|id| (*id, 1.0)).collect();
    for node in nodes.iter().filter(|node| node.kind == SpatialNodeKind::File) {
        if let Some(path) = &node.path {
            for ancestor in path.ancestors().skip(1) {
                if let Some(id) = dir_ids.get(ancestor) {
                    if let Some(weight) = dir_weights.get_mut(id) {
                        *weight += node.weight;
                    }
                } else {
                    break;
                }
            }
        }
    }
    for node in &mut nodes {
        if node.kind == SpatialNodeKind::Module
            && let Some(weight) = dir_weights.get(&node.id)
        {
            node.weight = *weight;
        }
    }

    let mut edges = Vec::new();
    for entry in &selected {
        if entry.path == root {
            continue;
        }
        let Some(parent) = entry.path.parent() else {
            continue;
        };
        if let (Some(&from), Some(&to)) =
            (path_to_id.get(parent), path_to_id.get(&entry.path))
        {
            edges.push(SpatialEdge {
                from,
                to,
                kind: EdgeKind::Contains,
                weight: 1.0,
            });
        }
    }

    // 叠加 crate 节点与 DependsOn 边；crate 节点使用新的连续 id。
    let base = nodes.len() as u32;
    for node in &l0.nodes {
        let mut node = node.clone();
        node.id = SpatialNodeId(base + node.id.0);
        node.cluster = node
            .path
            .as_deref()
            .and_then(|path| cluster_for_path(path, root, &cluster_of_top))
            .unwrap_or(ClusterId(0));
        node.anchor = None;
        nodes.push(node);
    }
    for edge in &l0.edges {
        edges.push(SpatialEdge {
            from: SpatialNodeId(base + edge.from.0),
            to: SpatialNodeId(base + edge.to.0),
            kind: edge.kind,
            weight: edge.weight,
        });
    }
    edges.sort_by_key(|edge| (edge.from.0, edge.to.0));

    let generation = generation_of_graph(&nodes, &edges, &clusters);
    Ok(SpatialGraph {
        generation,
        nodes,
        edges,
        clusters,
        omitted_nodes,
    })
}

fn collect_fs_entries(root: &Path, use_ignore: bool) -> Result<Vec<FsEntry>, SpatialError> {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(use_ignore)
        .git_ignore(use_ignore)
        .git_global(use_ignore)
        .git_exclude(use_ignore)
        .parents(use_ignore)
        .ignore(use_ignore)
        .require_git(false)
        .follow_links(false);
    builder.filter_entry(|entry| !is_skipped_dir(entry));

    let mut entries = Vec::new();
    for result in builder.build() {
        let Ok(entry) = result else {
            continue;
        };
        let Some(file_type) = entry.file_type() else {
            continue;
        };
        let kind = if file_type.is_dir() {
            FsKind::Dir
        } else if file_type.is_file() {
            FsKind::File
        } else {
            continue;
        };
        let size = if kind == FsKind::File {
            entry.metadata().map(|metadata| metadata.len()).unwrap_or(0)
        } else {
            0
        };
        let path = entry.path().to_path_buf();
        let top = top_component(&path, root, kind);
        entries.push(FsEntry {
            path,
            kind,
            depth: entry.depth(),
            size,
            top,
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn is_skipped_dir(entry: &ignore::DirEntry) -> bool {
    entry.file_type().is_some_and(|file_type| file_type.is_dir())
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| SKIP_DIRS.contains(&name))
}

fn top_component(path: &Path, root: &Path, kind: FsKind) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let first = components.next()?;
    let nested = relative.components().count() >= 2;
    if nested || kind == FsKind::Dir {
        Some(first.as_os_str().to_string_lossy().into_owned())
    } else {
        None
    }
}

fn select_entries(entries: &[FsEntry], limit: usize) -> (Vec<FsEntry>, usize) {
    if limit == usize::MAX || entries.len() <= limit {
        return (entries.to_vec(), 0);
    }

    let mut dirs: Vec<&FsEntry> = entries
        .iter()
        .filter(|entry| entry.kind == FsKind::Dir)
        .collect();
    dirs.sort_by_key(|entry| (entry.depth, entry.path.clone()));
    let kept_dirs: BTreeSet<PathBuf> = dirs
        .iter()
        .take(limit)
        .map(|entry| entry.path.clone())
        .collect();

    let mut selected: Vec<FsEntry> = entries
        .iter()
        .filter(|entry| entry.kind == FsKind::Dir && kept_dirs.contains(&entry.path))
        .cloned()
        .collect();
    let remaining = limit.saturating_sub(selected.len());
    let mut files: Vec<&FsEntry> = entries
        .iter()
        .filter(|entry| entry.kind == FsKind::File)
        .filter(|entry| entry.path.parent().is_some_and(|parent| kept_dirs.contains(parent)))
        .collect();
    files.sort_by(|a, b| {
        b.weight()
            .total_cmp(&a.weight())
            .then_with(|| a.path.cmp(&b.path))
    });
    selected.extend(files.into_iter().take(remaining).cloned());
    selected.sort_by(|a, b| a.path.cmp(&b.path));

    let omitted = entries.len().saturating_sub(selected.len());
    (selected, omitted)
}

fn build_clusters(
    root: &Path,
    entries: &[FsEntry],
) -> (Vec<SpatialCluster>, BTreeMap<String, ClusterId>) {
    let tops: BTreeSet<String> = entries.iter().filter_map(|entry| entry.top.clone()).collect();
    let mut clusters = vec![SpatialCluster {
        id: ClusterId(0),
        label: root_label(root),
        path: Some(root.to_path_buf()),
        weight: 1.0,
        anchor: None,
    }];
    let mut cluster_of_top = BTreeMap::new();
    for (index, top) in tops.into_iter().enumerate() {
        let id = ClusterId((index + 1) as u32);
        clusters.push(SpatialCluster {
            id,
            label: top.clone(),
            path: Some(root.join(&top)),
            weight: 1.0,
            anchor: None,
        });
        cluster_of_top.insert(top, id);
    }
    (clusters, cluster_of_top)
}

fn cluster_for_path(
    path: &Path,
    root: &Path,
    cluster_of_top: &BTreeMap<String, ClusterId>,
) -> Option<ClusterId> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let first = components.next()?;
    let nested = relative.components().count() >= 2;
    if nested || path.is_dir() {
        first
            .as_os_str()
            .to_str()
            .and_then(|top| cluster_of_top.get(top))
            .copied()
    } else {
        Some(ClusterId(0))
    }
}

fn root_label(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| root.display().to_string())
}

fn generation_of_graph(
    nodes: &[SpatialNode],
    edges: &[SpatialEdge],
    clusters: &[SpatialCluster],
) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    let mut hash = OFFSET;

    for node in nodes {
        mix(&mut hash, &node.id.0.to_le_bytes());
        mix(&mut hash, &[node_kind_rank(node.kind)]);
        mix(&mut hash, node.label.as_bytes());
        if let Some(path) = &node.path {
            mix(&mut hash, path.to_string_lossy().as_bytes());
        }
        mix(&mut hash, &node.cluster.0.to_le_bytes());
        mix(&mut hash, &node.weight.to_bits().to_le_bytes());
    }
    for edge in edges {
        mix(&mut hash, &edge.from.0.to_le_bytes());
        mix(&mut hash, &edge.to.0.to_le_bytes());
        mix(&mut hash, &[edge_kind_rank(edge.kind)]);
        mix(&mut hash, &edge.weight.to_bits().to_le_bytes());
    }
    for cluster in clusters {
        mix(&mut hash, &cluster.id.0.to_le_bytes());
        mix(&mut hash, cluster.label.as_bytes());
        if let Some(path) = &cluster.path {
            mix(&mut hash, path.to_string_lossy().as_bytes());
        }
        mix(&mut hash, &cluster.weight.to_bits().to_le_bytes());
    }
    hash
}

fn mix(hash: &mut u64, bytes: &[u8]) {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(PRIME);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(PRIME);
}

fn node_kind_rank(kind: SpatialNodeKind) -> u8 {
    match kind {
        SpatialNodeKind::Crate => 0,
        SpatialNodeKind::Module => 1,
        SpatialNodeKind::File => 2,
        SpatialNodeKind::Symbol => 3,
        SpatialNodeKind::External => 4,
    }
}

fn edge_kind_rank(kind: EdgeKind) -> u8 {
    match kind {
        EdgeKind::DependsOn => 0,
        EdgeKind::Contains => 1,
        EdgeKind::Imports => 2,
        EdgeKind::ChangedTogether => 3,
        EdgeKind::AgentTouched => 4,
    }
}

fn scan_l0(root: &Path) -> Result<SpatialGraph, SpatialError> {
    if !root.is_dir() {
        return Err(SpatialError::InvalidWorkspace {
            reason: format!("{} 不是目录", root.display()),
        });
    }

    let mut manifest_paths = Vec::new();
    collect_manifests(root, &mut manifest_paths)?;
    manifest_paths.sort();

    let mut packages = Vec::new();
    for path in &manifest_paths {
        let source = std::fs::read_to_string(path).map_err(|source| SpatialError::Io {
            path: path.clone(),
            source,
        })?;
        match parse_manifest_at(path, &source) {
            Ok(info) => {
                let dir = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| root.to_path_buf());
                packages.push(Package {
                    name: info.package,
                    dir,
                    dependencies: info.dependencies,
                });
            }
            // 纯 [workspace] manifest 没有 package 名，跳过。
            Err(SpatialError::MissingPackage { .. }) => {}
            Err(err) => return Err(err),
        }
    }

    packages.sort_by(|a, b| a.name.cmp(&b.name));

    let mut name_to_index = BTreeMap::new();
    for (index, package) in packages.iter().enumerate() {
        if let Some(previous) = name_to_index.insert(package.name.clone(), index) {
            return Err(SpatialError::DuplicatePackage {
                name: packages[previous].name.clone(),
            });
        }
    }

    let mut canonical_dirs = BTreeMap::new();
    for (index, package) in packages.iter().enumerate() {
        let key = canonical_or_self(&package.dir);
        canonical_dirs.insert(key, index);
    }

    let mut edges = BTreeSet::new();
    for (from, package) in packages.iter().enumerate() {
        for dependency in &package.dependencies {
            if dependency.kind == DependencyKind::Dev {
                continue;
            }
            let target = if let Some(relative) = &dependency.path {
                let joined = canonical_or_self(&package.dir.join(relative));
                canonical_dirs.get(&joined).copied()
            } else if dependency.workspace {
                name_to_index.get(&dependency.name).copied()
            } else {
                None
            };
            if let Some(to) = target
                && to != from
            {
                edges.insert((from as u32, to as u32));
            }
        }
    }

    let generation = generation_of(&packages, &edges);

    let clusters = packages
        .iter()
        .enumerate()
        .map(|(index, package)| SpatialCluster {
            id: ClusterId(index as u32),
            label: package.name.clone(),
            path: Some(package.dir.clone()),
            weight: 1.0,
            anchor: None,
        })
        .collect();

    let nodes = packages
        .iter()
        .enumerate()
        .map(|(index, package)| SpatialNode {
            id: SpatialNodeId(index as u32),
            kind: SpatialNodeKind::Crate,
            label: package.name.clone(),
            path: Some(package.dir.clone()),
            cluster: ClusterId(index as u32),
            weight: 1.0,
            status: SpatialStatus::Idle,
            anchor: None,
        })
        .collect();

    let edges = edges
        .into_iter()
        .map(|(from, to)| SpatialEdge {
            from: SpatialNodeId(from),
            to: SpatialNodeId(to),
            kind: EdgeKind::DependsOn,
            weight: 1.0,
        })
        .collect();

    Ok(SpatialGraph {
        generation,
        nodes,
        edges,
        clusters,
        omitted_nodes: 0,
    })
}

fn collect_manifests(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), SpatialError> {
    let entries = std::fs::read_dir(dir).map_err(|source| SpatialError::Io {
        path: dir.to_path_buf(),
        source,
    })?;

    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        let path = entry.path();
        if file_type.is_dir() {
            let skip = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SKIP_DIRS.contains(&name));
            if !skip {
                collect_manifests(&path, out)?;
            }
        } else if file_type.is_file() && path.file_name().is_some_and(|name| name == "Cargo.toml") {
            out.push(path);
        }
    }
    Ok(())
}

fn canonical_or_self(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn generation_of(packages: &[Package], edges: &BTreeSet<(u32, u32)>) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET;
    for package in packages {
        for byte in package.name.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(PRIME);
    }
    for (from, to) in edges {
        for byte in from.to_le_bytes().iter().chain(to.to_le_bytes().iter()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("创建目录");
        }
        fs::write(path, contents).expect("写入文件");
    }

    #[test]
    fn scans_path_dependencies_into_edges() {
        let temp = tempfile::tempdir().expect("临时目录");
        let root = temp.path();
        write(
            &root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"3\"\n",
        );
        write(
            &root.join("a/Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n\n[dependencies]\nb = { path = \"../b\" }\nserde = \"1\"\n",
        );
        write(
            &root.join("b/Cargo.toml"),
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\n",
        );

        let graph = scan(root).expect("扫描成功");
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.clusters.len(), 2);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.nodes[0].label, "a");
        assert_eq!(graph.nodes[1].label, "b");
        assert_eq!(graph.edges[0].from, SpatialNodeId(0));
        assert_eq!(graph.edges[0].to, SpatialNodeId(1));
        assert_eq!(graph.edges[0].kind, EdgeKind::DependsOn);
    }

    #[test]
    fn generation_is_stable_across_scans() {
        let temp = tempfile::tempdir().expect("临时目录");
        let root = temp.path();
        write(
            &root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\n",
        );
        write(
            &root.join("a/Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n\n[dependencies]\nb = { path = \"../b\" }\n",
        );
        write(
            &root.join("b/Cargo.toml"),
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\n",
        );

        let first = scan(root).expect("第一次扫描");
        let second = scan(root).expect("第二次扫描");
        assert_eq!(first.generation, second.generation);
        assert_eq!(first.nodes, second.nodes);
        assert_eq!(first.edges, second.edges);
    }

    #[test]
    fn rejects_duplicate_package_names() {
        let temp = tempfile::tempdir().expect("临时目录");
        let root = temp.path();
        write(
            &root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\n",
        );
        write(
            &root.join("a/Cargo.toml"),
            "[package]\nname = \"same\"\nversion = \"0.1.0\"\n",
        );
        write(
            &root.join("b/Cargo.toml"),
            "[package]\nname = \"same\"\nversion = \"0.1.0\"\n",
        );

        let err = scan(root).expect_err("应报重复");
        assert!(matches!(err, SpatialError::DuplicatePackage { .. }));
    }

    #[test]
    fn scans_this_workspace_and_resolves_ui_to_xui() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace 根目录");

        let graph = scan(workspace_root).expect("扫描本工作区");
        assert!(graph.nodes.len() >= 10, "节点数 {}", graph.nodes.len());

        for edge in &graph.edges {
            assert!((edge.from.0 as usize) < graph.nodes.len());
            assert!((edge.to.0 as usize) < graph.nodes.len());
        }

        let id_of = |name: &str| {
            graph
                .nodes
                .iter()
                .find(|node| node.label == name)
                .map(|node| node.id)
                .unwrap_or_else(|| panic!("缺少 crate {name}"))
        };
        let ui = id_of("xgent_ui");
        let xui = id_of("xui");
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.from == ui && edge.to == xui),
            "xgent_ui 应依赖 xui"
        );
    }
}
