use std::fs;
use std::path::Path;

use xgent_core::spatial::{EdgeKind, SpatialNodeKind};
use xgent_spatial::{ScanOptions, scan_with_options};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("创建目录");
    }
    fs::write(path, contents).expect("写入文件");
}

fn l1() -> ScanOptions {
    ScanOptions {
        l1: true,
        node_limit: 2000,
        ignore: true,
    }
}

fn has_path(graph: &xgent_core::spatial::SpatialGraph, needle: &str) -> bool {
    graph.nodes.iter().any(|node| {
        node.path.as_ref().is_some_and(|path| {
            path.to_string_lossy().replace('\\', "/").contains(needle)
        })
    })
}

#[test]
fn l1_nodes_and_contains_edges() {
    let temp = tempfile::tempdir().expect("临时目录");
    let root = temp.path();
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    write(&root.join("src/main.rs"), "fn main() {}\n");
    write(&root.join("src/lib.rs"), "pub fn lib() {}\n");
    write(&root.join("README.md"), "# demo\n");

    let graph = scan_with_options(root, l1()).expect("L1 扫描成功");

    assert!(
        graph
            .nodes
            .iter()
            .any(|node| node.kind == SpatialNodeKind::File && node.label == "main.rs"),
        "应包含 main.rs 文件节点"
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| node.kind == SpatialNodeKind::Module && node.label == "src"),
        "应包含 src 目录节点"
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| node.kind == SpatialNodeKind::Crate && node.label == "demo"),
        "应保留 crate 节点"
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::Contains),
        "应有目录 → 文件/目录的 Contains 边"
    );
    assert_eq!(graph.omitted_nodes, 0);
}

#[test]
fn ignore_rules_skip_dependency_dirs_and_gitignore() {
    let temp = tempfile::tempdir().expect("临时目录");
    let root = temp.path();
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    write(&root.join("src/main.rs"), "fn main() {}\n");
    write(&root.join(".gitignore"), "ignored.txt\n");
    write(&root.join("ignored.txt"), "ignore me\n");
    write(&root.join("node_modules/pkg/index.js"), "x\n");
    write(&root.join("target/debug/out.bin"), "x\n");

    let graph = scan_with_options(root, l1()).expect("L1 扫描成功");

    assert!(!has_path(&graph, "node_modules"), "node_modules 应被忽略");
    assert!(!has_path(&graph, "target"), "target 应被忽略");
    assert!(!has_path(&graph, "ignored.txt"), ".gitignore 命中的文件应被忽略");
    assert!(has_path(&graph, "src/main.rs"), "正常源码应保留");
}

#[test]
fn node_cap_aggregates_and_reports_omitted() {
    let temp = tempfile::tempdir().expect("临时目录");
    let root = temp.path();
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    for index in 0..20 {
        write(&root.join(format!("src/file{index}.rs")), "fn f() {}\n");
    }

    let graph = scan_with_options(
        root,
        ScanOptions {
            l1: true,
            node_limit: 5,
            ignore: true,
        },
    )
    .expect("L1 扫描成功");

    assert!(
        graph.nodes.len() <= 5,
        "节点数应不超过上限，实际 {}",
        graph.nodes.len()
    );
    assert!(graph.omitted_nodes > 0, "应报告被省略的节点数");
}

#[test]
fn ten_thousand_files_are_capped() {
    let temp = tempfile::tempdir().expect("临时目录");
    let root = temp.path();
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    for index in 0..10_000 {
        write(&root.join(format!("src/file{index}.rs")), "x\n");
    }

    let graph = scan_with_options(root, l1()).expect("L1 扫描成功");

    assert!(
        graph.nodes.len() <= 2000,
        "10k 文件应被裁剪到 2k 内，实际 {}",
        graph.nodes.len()
    );
    assert!(
        graph.omitted_nodes >= 8000,
        "应报告至少 8000 个省略节点，实际 {}",
        graph.omitted_nodes
    );
}

#[test]
fn l1_scan_is_deterministic() {
    let temp = tempfile::tempdir().expect("临时目录");
    let root = temp.path();
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    write(&root.join("src/main.rs"), "fn main() {}\n");
    write(&root.join("src/lib.rs"), "pub fn lib() {}\n");
    write(&root.join("docs/readme.md"), "# docs\n");

    let first = scan_with_options(root, l1()).expect("第一次扫描");
    let second = scan_with_options(root, l1()).expect("第二次扫描");

    assert_eq!(first.generation, second.generation);
    assert_eq!(first.nodes, second.nodes);
    assert_eq!(first.edges, second.edges);
    assert_eq!(first.clusters, second.clusters);
}
