//! 单个 `Cargo.toml` 的依赖解析。
//!
//! 只做「读结构」：package 名、三类依赖的键名、path 与 `workspace = true` 标记。
//! 不解析版本、不展开 `[workspace.dependencies]`、不联网。

use std::path::PathBuf;

use crate::error::SpatialError;

/// 依赖类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DependencyKind {
    /// `[dependencies]`。
    Normal,
    /// `[dev-dependencies]`。
    Dev,
    /// `[build-dependencies]`。
    Build,
}

/// 一条依赖声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestDependency {
    /// 依赖名（即 manifest 中的键）。
    pub name: String,
    /// 依赖类别。
    pub kind: DependencyKind,
    /// `path = "..."` 的原始值。
    pub path: Option<PathBuf>,
    /// 是否声明了 `workspace = true`。
    pub workspace: bool,
}

/// 解析后的 manifest 摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestInfo {
    /// `[package].name`。
    pub package: String,
    /// 三类依赖，按 `(kind, name)` 排序，保证稳定。
    pub dependencies: Vec<ManifestDependency>,
}

/// 解析一段 `Cargo.toml` 文本。
pub fn parse_manifest(source: &str) -> Result<ManifestInfo, SpatialError> {
    parse_manifest_inner(source, None)
}

/// 解析磁盘上的 `Cargo.toml`，错误里带上路径。
pub(crate) fn parse_manifest_at(
    path: &std::path::Path,
    source: &str,
) -> Result<ManifestInfo, SpatialError> {
    parse_manifest_inner(source, Some(path.to_path_buf()))
}

fn parse_manifest_inner(source: &str, path: Option<PathBuf>) -> Result<ManifestInfo, SpatialError> {
    let value: toml::Value = toml::from_str(source).map_err(|source| SpatialError::Toml {
        path: path.clone(),
        source: Box::new(source),
    })?;

    let package = value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| SpatialError::MissingPackage { path: path.clone() })?
        .to_string();

    let mut dependencies = Vec::new();
    for (table, kind) in [
        ("dependencies", DependencyKind::Normal),
        ("dev-dependencies", DependencyKind::Dev),
        ("build-dependencies", DependencyKind::Build),
    ] {
        let Some(table) = value.get(table).and_then(toml::Value::as_table) else {
            continue;
        };
        for (name, spec) in table {
            let (dep_path, workspace) = match spec {
                toml::Value::Table(spec) => (
                    spec.get("path")
                        .and_then(toml::Value::as_str)
                        .map(PathBuf::from),
                    spec.get("workspace")
                        .and_then(toml::Value::as_bool)
                        .unwrap_or(false),
                ),
                _ => (None, false),
            };
            dependencies.push(ManifestDependency {
                name: name.clone(),
                kind,
                path: dep_path,
                workspace,
            });
        }
    }
    dependencies.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));

    Ok(ManifestInfo {
        package,
        dependencies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[package]
name = "demo"

[dependencies]
xgent_core = { path = "../xgent_core" }
serde = { workspace = true }
toml = "0.8"

[dev-dependencies]
tempfile = "3"

[build-dependencies]
cc = "1"
"#;

    #[test]
    fn parses_package_and_three_dependency_kinds() {
        let info = parse_manifest(SAMPLE).expect("应解析成功");
        assert_eq!(info.package, "demo");
        assert_eq!(info.dependencies.len(), 5);

        let core = info
            .dependencies
            .iter()
            .find(|dep| dep.name == "xgent_core")
            .expect("应有 xgent_core");
        assert_eq!(core.kind, DependencyKind::Normal);
        assert_eq!(
            core.path.as_deref(),
            Some(std::path::Path::new("../xgent_core"))
        );
        assert!(!core.workspace);

        let serde = info
            .dependencies
            .iter()
            .find(|dep| dep.name == "serde")
            .expect("应有 serde");
        assert!(serde.workspace);
        assert!(serde.path.is_none());

        assert_eq!(
            info.dependencies
                .iter()
                .filter(|dep| dep.kind == DependencyKind::Dev)
                .count(),
            1
        );
        assert_eq!(
            info.dependencies
                .iter()
                .filter(|dep| dep.kind == DependencyKind::Build)
                .count(),
            1
        );
    }

    #[test]
    fn dependencies_are_sorted_by_kind_then_name() {
        let info = parse_manifest(SAMPLE).expect("应解析成功");
        let mut sorted = info.dependencies.clone();
        sorted.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        assert_eq!(info.dependencies, sorted);
    }

    #[test]
    fn rejects_manifest_without_package_name() {
        let err = parse_manifest("[workspace]\nmembers = []\n").expect_err("应报错");
        assert!(matches!(err, SpatialError::MissingPackage { .. }));
    }

    #[test]
    fn reports_toml_syntax_error() {
        let err = parse_manifest("[package\nname = \"x\"").expect_err("应报错");
        assert!(matches!(err, SpatialError::Toml { .. }));
    }
}
