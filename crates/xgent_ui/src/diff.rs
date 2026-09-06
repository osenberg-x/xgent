//! 行级 diff（v7，M5-T3）：自 confirm_dialog 抽取的共享纯函数。
//!
//! 简单行级 diff：求公共前缀与后缀，中间旧行标 Del、新行标 Add。
//! 无需外部依赖，MVP 足够。复杂 diff（跨行移动）留待 P1。

/// diff 行的类型（增/删/上下文）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffKind {
    Add,
    Del,
    Context,
}

/// 一行 diff（kind + 文本）。
#[derive(Clone, Debug)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

/// 行级 diff 主入口。
pub fn line_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    // 公共前缀
    let mut prefix = 0;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    // 公共后缀
    let mut suffix = 0;
    while suffix < old_lines.len() - prefix
        && suffix < new_lines.len() - prefix
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let mut out = Vec::new();
    // 前缀上下文
    for i in 0..prefix {
        out.push(DiffLine {
            kind: DiffKind::Context,
            text: old_lines[i].into(),
        });
    }
    // 中间：先删后增
    for i in prefix..old_lines.len() - suffix {
        out.push(DiffLine {
            kind: DiffKind::Del,
            text: old_lines[i].into(),
        });
    }
    for i in prefix..new_lines.len() - suffix {
        out.push(DiffLine {
            kind: DiffKind::Add,
            text: new_lines[i].into(),
        });
    }
    // 后缀上下文
    for i in old_lines.len() - suffix..old_lines.len() {
        out.push(DiffLine {
            kind: DiffKind::Context,
            text: old_lines[i].into(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 纯增：尾部追加行全部标 Add。
    #[test]
    fn pure_add() {
        let d = line_diff("a\nb", "a\nb\nc");
        assert_eq!(d.last().unwrap().kind, DiffKind::Add);
        assert_eq!(d.last().unwrap().text, "c");
        assert!(d.iter().all(|l| l.kind != DiffKind::Del));
    }

    /// 纯删：被移除的行标 Del。
    #[test]
    fn pure_del() {
        let d = line_diff("a\nb\nc", "a\nc");
        assert!(d.iter().any(|l| l.kind == DiffKind::Del && l.text == "b"));
        assert!(d.iter().all(|l| l.kind != DiffKind::Add));
    }

    /// 增删混合：中间段既有 Del 又有 Add。
    #[test]
    fn mixed_change() {
        let d = line_diff("a\nold\nz", "a\nnew\nz");
        assert!(d.iter().any(|l| l.kind == DiffKind::Del && l.text == "old"));
        assert!(d.iter().any(|l| l.kind == DiffKind::Add && l.text == "new"));
        assert_eq!(d.first().unwrap().kind, DiffKind::Context);
        assert_eq!(d.last().unwrap().kind, DiffKind::Context);
    }
}
