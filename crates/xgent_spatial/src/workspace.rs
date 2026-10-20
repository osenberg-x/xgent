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
pub fn scan(root: &Path) -> Result<SpatialGraph, SpatialError> {
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
