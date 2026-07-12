//! 路径安全工具。
//!
//! 所有文件工具的路径基于 `project_root` join，并校验规范化后的路径
//! 仍在项目目录内，防止 `..` 逃逸。

use std::path::{Path, PathBuf};

/// 把 `input`（相对或绝对）解析为项目内的绝对路径，并校验不越界。
///
/// - 若 `input` 是绝对路径，直接使用（但仍需在项目内）；
/// - 否则 join 到 `project_root`；
/// - 规范化（词法，不访问文件系统）后检查是否以 `project_root` 为前缀。
///
/// 越界返回 `Err`。
pub fn resolve_in_project(project_root: &Path, input: &str) -> Result<PathBuf, String> {
    let raw: PathBuf = if Path::new(input).is_absolute() {
        PathBuf::from(input)
    } else {
        project_root.join(input)
    };
    // 词法规范化（不访问文件系统），处理 ..
    let canonical = lexical_canonicalize(&raw);
    let root_canonical = lexical_canonicalize(project_root);
    if !canonical.starts_with(&root_canonical) {
        return Err(format!(
            "路径越界：{} 不在项目目录 {} 内",
            canonical.display(),
            root_canonical.display()
        ));
    }
    Ok(canonical)
}

/// 词法规范化路径：解析 `.` 与 `..`，不访问文件系统。
///
/// 与 `std::fs::canonicalize` 不同，不要求路径存在。
fn lexical_canonicalize(path: &Path) -> PathBuf {
    let mut out = Vec::new();
    for comp in path.components() {
        use std::path::Component;
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // 弹出最后一个正常组件（不弹根前缀）
                if let Some(last) = out.last()
                    && matches!(last, Component::Normal(_))
                {
                    out.pop();
                    continue;
                }
                out.push(comp);
            }
            c => out.push(c),
        }
    }
    out.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/proj")
    }

    #[test]
    fn relative_path_resolves_in_project() {
        let p = resolve_in_project(&root(), "src/main.rs").unwrap();
        assert_eq!(p, PathBuf::from("/proj/src/main.rs"));
    }

    #[test]
    fn absolute_path_inside_project_ok() {
        let p = resolve_in_project(&root(), "/proj/src/x.rs").unwrap();
        assert_eq!(p, PathBuf::from("/proj/src/x.rs"));
    }

    #[test]
    fn dot_segments_collapsed() {
        let p = resolve_in_project(&root(), "src/./../src/main.rs").unwrap();
        assert_eq!(p, PathBuf::from("/proj/src/main.rs"));
    }

    #[test]
    fn parent_escape_rejected() {
        let err = resolve_in_project(&root(), "../../etc/passwd").unwrap_err();
        assert!(err.contains("越界"));
    }

    #[test]
    fn absolute_path_outside_project_rejected() {
        let err = resolve_in_project(&root(), "/etc/passwd").unwrap_err();
        assert!(err.contains("越界"));
    }

    #[test]
    fn escape_via_subdir_parent_rejected() {
        let err = resolve_in_project(&root(), "src/../../etc/shadow").unwrap_err();
        assert!(err.contains("越界"));
    }

    #[test]
    fn exactly_project_root_ok() {
        let p = resolve_in_project(&root(), ".").unwrap();
        assert_eq!(p, PathBuf::from("/proj"));
    }
}
