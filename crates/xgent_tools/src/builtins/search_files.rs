//! search_files 工具：在项目内递归搜索文本匹配。
//!
//! MVP 用内置简单递归遍历（不依赖系统 ripgrep），保证可移植。
//! 匹配方式默认字面量子串；`regex: true` 时按正则逐行匹配，
//! `case_insensitive` 对两种模式都生效，`glob` / `file_type` 收窄被搜索的文件。
//!
//! 关键契约（R3-3）：**坏参数是错误，不是零结果**。此前只支持子串，模型写
//! `"(foo"` 之类的模式时得到「未找到匹配」，于是断定代码不存在并给出错答。

use async_trait::async_trait;
use regex::{Regex, RegexBuilder};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use xgent_core::chat::ToolSchema;

use crate::glob::Glob;
use crate::path::resolve_in_project;
use crate::tool::{
    Concurrency, Tool, ToolCtx, ToolError, ToolResult, ToolTier, ToolUpdateCallback,
};

/// 递归搜索限制的最大结果数与最大深度，避免超大仓库卡死。
const MAX_RESULTS: usize = 200;
const MAX_DEPTH: u32 = 16;

/// 单文件读取上限：超过直接跳过（lockfile、大 JSON、日志全量读入逐行
/// 匹配既慢又无意义）。
const MAX_FILE_BYTES: u64 = 1 << 20;

/// `file_type` 名称 → 扩展名集合。名称取自 ripgrep 的 `--type` 词表，
/// 模型对它有先验；一个名称可覆盖多个扩展名（`cpp` 含 `hpp`）。
const FILE_TYPES: &[(&str, &[&str])] = &[
    ("rust", &["rs"]),
    ("python", &["py", "pyi"]),
    ("javascript", &["js", "mjs", "cjs"]),
    ("typescript", &["ts", "mts"]),
    ("tsx", &["tsx"]),
    ("jsx", &["jsx"]),
    ("go", &["go"]),
    ("java", &["java"]),
    ("c", &["c", "h"]),
    (
        "cpp",
        &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "cppm", "ixx"],
    ),
    ("markdown", &["md"]),
    ("json", &["json"]),
    ("toml", &["toml"]),
    ("yaml", &["yaml", "yml"]),
    ("shell", &["sh", "bash", "zsh"]),
    ("html", &["html", "htm"]),
    ("css", &["css", "scss"]),
    ("sql", &["sql"]),
];

/// 内容匹配方式（已按参数编译）。
enum Matcher {
    /// 字面量子串。`folded` 为小写形态，仅在忽略大小写时预备。
    Literal {
        needle: String,
        folded: Option<String>,
    },
    /// 正则，按行匹配（不跨行）。
    Regex(Regex),
}

impl Matcher {
    /// 单行是否命中。
    fn is_match(&self, line: &str) -> bool {
        match self {
            Matcher::Literal {
                needle,
                folded: None,
            } => line.contains(needle.as_str()),
            Matcher::Literal {
                folded: Some(folded),
                ..
            } => line.to_lowercase().contains(folded.as_str()),
            Matcher::Regex(re) => re.is_match(line),
        }
    }
}

/// 文件筛选条件（内容匹配之外的两层）。
struct Filters<'a> {
    matcher: &'a Matcher,
    /// glob 只对照文件名（模式不含 `/`）或相对路径，见 [`Glob::matches`]
    glob: Option<&'a Glob>,
    /// `file_type` 展开出的扩展名集合
    extensions: Option<&'static [&'static str]>,
}

impl Filters<'_> {
    /// 该文件是否参与搜索。判定在读取**之前**，被排除的文件不产生 IO。
    fn keeps_file(&self, rel_path: &str, file_name: &str) -> bool {
        if let Some(exts) = self.extensions
            && !extension_matches(file_name, exts)
        {
            return false;
        }
        if let Some(glob) = self.glob
            && !glob.matches(rel_path, file_name)
        {
            return false;
        }
        true
    }
}

/// 在项目内递归搜索文本匹配。
pub struct SearchFiles;

#[async_trait]
impl Tool for SearchFiles {
    fn id(&self) -> &str {
        "search_files"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.id().to_string(),
            description: format!(
                "在项目内递归搜索文本匹配，返回 `路径:行号: 内容`。默认按字面量子串、区分大小写。\
                 regex=true 时 pattern 按正则逐行匹配（不跨行）；\
                 glob 按路径/文件名筛选文件（* 不跨目录、** 跨目录、{{a,b}} 分支，不含 / 的模式只匹配文件名）；\
                 file_type 按文件类型筛选，支持：{}。\
                 pattern/glob/file_type 非法会返回错误而不是「未找到匹配」——据此区分「搜不到」与「写错了」。",
                supported_type_names()
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "搜索的子串（regex=true 时为正则）" },
                    "path": { "type": "string", "description": "搜索起始目录（可选，默认项目根）" },
                    "regex": { "type": "boolean", "description": "pattern 按正则解释（默认 false，即字面量子串）" },
                    "case_insensitive": { "type": "boolean", "description": "忽略大小写（默认 false，字面与正则模式都适用）" },
                    "glob": { "type": "string", "description": "只搜索匹配该 glob 的文件，如 \"*.rs\" 或 \"src/**/*.ts\"" },
                    "file_type": { "type": "string", "description": format!("只搜索该类型的文件，如 rust/python/go；可选：{}", supported_type_names()) }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Shared
    }

    async fn summarize(&self, input: &Value) -> String {
        let pattern = input["pattern"].as_str().unwrap_or("?");
        let mut s = match input["path"].as_str() {
            Some(p) => format!("在 {p} 搜索 “{pattern}”"),
            None => format!("搜索 “{pattern}”"),
        };
        if input["regex"].as_bool().unwrap_or(false) {
            s.push_str("（正则）");
        }
        if input["case_insensitive"].as_bool().unwrap_or(false) {
            s.push_str("（忽略大小写）");
        }
        if let Some(g) = input["glob"].as_str() {
            s.push_str(&format!("（仅 {g}）"));
        }
        if let Some(t) = input["file_type"].as_str() {
            s.push_str(&format!("（类型 {t}）"));
        }
        s
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolCtx,
        signal: CancellationToken,
        _on_update: Option<Arc<ToolUpdateCallback>>,
    ) -> Result<ToolResult, ToolError> {
        let Some(pattern) = input["pattern"].as_str() else {
            return Ok(err_result("缺少参数 pattern"));
        };
        let use_regex = input["regex"].as_bool().unwrap_or(false);
        let fold_case = input["case_insensitive"].as_bool().unwrap_or(false);

        // 三个参数一律在遍历之前编译：坏参数必须是错误，且不能先扫过全仓
        // 再报错（那会让 Abort 与「搜不到」都混进来）。
        let matcher = match compile_matcher(pattern, use_regex, fold_case) {
            Ok(m) => m,
            Err(e) => return Ok(err_result(e)),
        };
        let glob = match input["glob"].as_str() {
            Some(g) => match crate::glob::compile(g) {
                Ok(gl) => Some(gl),
                Err(e) => return Ok(err_result(format!("参数 glob 的模式无效: {e}"))),
            },
            None => None,
        };
        let extensions = match input["file_type"].as_str() {
            Some(t) => match type_extensions(t) {
                Some(exts) => Some(exts),
                None => {
                    return Ok(err_result(format!(
                        "不支持的 file_type: “{t}”。支持的文件类型: {}",
                        supported_type_names()
                    )));
                }
            },
            None => None,
        };

        let start_rel = input["path"].as_str().unwrap_or(".");
        let start = match resolve_in_project(&ctx.project_root, start_rel) {
            Ok(p) => p,
            Err(e) => return Ok(err_result(e)),
        };

        let filters = Filters {
            matcher: &matcher,
            glob: glob.as_ref(),
            extensions,
        };

        let mut matches = Vec::new();
        let mut count = 0usize;
        walk(
            &start,
            &start,
            &filters,
            0,
            &signal,
            &mut matches,
            &mut count,
        )
        .await;
        // 取消检查：walk 只在边界响应，末尾兜底确保 Abort 不丢
        if signal.is_cancelled() {
            return Err(ToolError::Aborted);
        }

        let mut output = if matches.is_empty() {
            format!("未找到匹配 “{pattern}”")
        } else {
            let mut s = String::new();
            for m in &matches {
                s.push_str(&format!("{}:{}: {}\n", m.path, m.line_no, m.line));
            }
            s
        };
        if count >= MAX_RESULTS {
            output.push_str(&format!("\n（已达最大结果数 {MAX_RESULTS}，截断）\n"));
        }
        Ok(ToolResult {
            output,
            is_error: false,
            denied: false,
            side_effect: None,
        })
    }
}

struct Match {
    path: String,
    line_no: usize,
    line: String,
}

/// 构造错误结果（工具层错误经 output 回传模型，故包成 `is_error` 结果）。
fn err_result(output: impl Into<String>) -> ToolResult {
    ToolResult {
        output: output.into(),
        is_error: true,
        denied: false,
        side_effect: None,
    }
}

/// 按参数编译内容匹配器；语法错误返回可直接展示的错因。
fn compile_matcher(pattern: &str, use_regex: bool, fold_case: bool) -> Result<Matcher, String> {
    if !use_regex {
        return Ok(Matcher::Literal {
            needle: pattern.to_string(),
            folded: fold_case.then(|| pattern.to_lowercase()),
        });
    }
    RegexBuilder::new(pattern)
        .case_insensitive(fold_case)
        .build()
        .map(Matcher::Regex)
        .map_err(|e| format!("参数 pattern 的正则表达式无效: {e}"))
}

/// 查 `file_type` 对应的扩展名集合（名称大小写不敏感）。
fn type_extensions(name: &str) -> Option<&'static [&'static str]> {
    let key = name.trim().to_lowercase();
    FILE_TYPES.iter().find(|(n, _)| *n == key).map(|(_, e)| *e)
}

/// 支持的文件类型名（用于报错与工具描述）。
fn supported_type_names() -> String {
    FILE_TYPES
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join(", ")
}

/// 文件名的扩展名是否落在给定集合内（大小写不敏感）。
fn extension_matches(file_name: &str, exts: &[&str]) -> bool {
    let Some((_, ext)) = file_name.rsplit_once('.') else {
        return false;
    };
    // `.gitignore` 这类点开头文件：rsplit_once 给出的 ext 是 "gitignore"，
    // 与「无扩展名」的判断保持一致——要求存在真正的 `name.ext` 主体。
    let has_stem = file_name.contains('.') && !file_name.starts_with('.');
    has_stem && exts.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// 递归遍历目录，收集匹配行。取消信号在目录/文件边界响应——大仓库
/// 搜索期间 Abort 必须能中断，不能跑到自然结束。
async fn walk(
    root: &Path,
    dir: &Path,
    filters: &Filters<'_>,
    depth: u32,
    signal: &CancellationToken,
    matches: &mut Vec<Match>,
    count: &mut usize,
) {
    if depth > MAX_DEPTH || *count >= MAX_RESULTS || signal.is_cancelled() {
        return;
    }
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        if *count >= MAX_RESULTS || signal.is_cancelled() {
            return;
        }
        let path = entry.path();
        let ft = match entry.file_type().await {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            // 跳过常见忽略目录
            let name = entry.file_name();
            if ignores(&name.to_string_lossy()) {
                continue;
            }
            // Box::pin 递归 async fn
            Box::pin(walk(
                root,
                &path,
                filters,
                depth + 1,
                signal,
                matches,
                count,
            ))
            .await;
        } else if ft.is_file() {
            // 超大文件跳过
            if let Ok(meta) = entry.metadata().await
                && meta.len() > MAX_FILE_BYTES
            {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            let rel = relative(root, &path);
            if !filters.keeps_file(&rel, &file_name) {
                continue;
            }
            read_and_match(&path, &rel, filters, matches, count).await;
        }
    }
}

/// 读取单个文件，按行匹配。`rel` 已由调用方算好（同一值也用于 glob 判定）。
async fn read_and_match(
    path: &Path,
    rel: &str,
    filters: &Filters<'_>,
    matches: &mut Vec<Match>,
    count: &mut usize,
) {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return; // 非文本/无法读，跳过
    };
    for (i, line) in content.lines().enumerate() {
        if *count >= MAX_RESULTS {
            return;
        }
        if filters.matcher.is_match(line) {
            matches.push(Match {
                path: rel.to_string(),
                line_no: i + 1,
                line: line.to_string(),
            });
            *count += 1;
        }
    }
}

/// 相对搜索根的路径，统一用 `/` 分隔（Windows 反斜杠归一）。
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 是否应忽略的目录名。
fn ignores(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".hg" | ".svn" | "node_modules" | "target" | "dist" | "build" | ".next"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个临时项目根：写入若干文件（路径含子目录）。
    async fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, content) in files {
            let full = dir.path().join(rel);
            if let Some(parent) = full.parent() {
                tokio::fs::create_dir_all(parent).await.unwrap();
            }
            tokio::fs::write(&full, content).await.unwrap();
        }
        dir
    }

    fn ctx_for(root: &Path) -> ToolCtx {
        ToolCtx {
            project_root: root.to_path_buf(),
            tool_policy: Default::default(),
        }
    }

    async fn search(root: &Path, input: Value) -> Result<ToolResult, ToolError> {
        SearchFiles
            .execute(input, &ctx_for(root), CancellationToken::new(), None)
            .await
    }

    #[tokio::test]
    async fn search_finds_matches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("src/a.rs"), "fn foo() {}\nfn bar() {}\n")
            .await
            .unwrap();
        tokio::fs::write(root.join("b.txt"), "hello foo\n")
            .await
            .unwrap();

        let r = search(&root, json!({"pattern": "foo"})).await.unwrap();
        assert!(!r.is_error);
        assert!(r.output.contains("src/a.rs:1"));
        assert!(r.output.contains("b.txt:1"));
    }

    #[tokio::test]
    async fn search_no_match() {
        let dir = project(&[("x.txt", "abc")]).await;
        let root = dir.path().to_path_buf();
        let r = search(&root, json!({"pattern": "zzz"})).await.unwrap();
        assert!(!r.is_error);
        assert!(r.output.contains("未找到"));
    }

    #[tokio::test]
    async fn search_ignores_target_dir() {
        let dir = project(&[("target/junk.rs", "findme")]).await;
        let root = dir.path().to_path_buf();
        let r = search(&root, json!({"pattern": "findme"})).await.unwrap();
        assert!(!r.output.contains("target/junk.rs"), "应忽略 target 目录");
    }

    /// A1：字面模式下正则元字符仍是字面量（改前行为不变）。
    #[tokio::test]
    async fn literal_pattern_is_literal() {
        let dir = project(&[
            ("lit.rs", "let a_plus_b = \"a+b\";\n"),
            ("aab.rs", "let aab = 1;\n"),
        ])
        .await;
        let root = dir.path().to_path_buf();
        let r = search(&root, json!({"pattern": "a+b"})).await.unwrap();
        assert!(!r.is_error);
        assert!(r.output.contains("lit.rs"), "字面 a+b 应命中: {}", r.output);
        assert!(
            !r.output.contains("aab.rs"),
            "字面 a+b 不该命中 aab: {}",
            r.output
        );
    }

    /// A2：正则模式命中锚点/字符类。
    #[tokio::test]
    async fn regex_mode_matches_by_pattern() {
        let dir = project(&[
            ("a.rs", "fn foo_bar() {}\nlet x = foo;\n"),
            ("b.rs", "struct Foo;\n"),
        ])
        .await;
        let root = dir.path().to_path_buf();
        let r = search(&root, json!({"pattern": "^fn\\s+foo_\\w+", "regex": true}))
            .await
            .unwrap();
        assert!(!r.is_error);
        assert!(
            r.output.contains("a.rs:1"),
            "应命中 fn foo_bar: {}",
            r.output
        );
        assert_eq!(
            r.output.matches('\n').count(),
            1,
            "只应有 1 行命中: {}",
            r.output
        );
    }

    /// A2：非法正则是错误，且**不发生在遍历之后**。
    ///
    /// 判别方式：同时把取消信号置位。若实现在遍历后才校验 pattern，
    /// 走的是 Abort 分支（`Err(ToolError::Aborted)`）；先编译才可能返回
    /// pattern 错误。
    #[tokio::test]
    async fn invalid_regex_is_reported_before_traversal() {
        let dir = project(&[("a.rs", "fn foo() {}\n")]).await;
        let root = dir.path().to_path_buf();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let r = SearchFiles
            .execute(
                json!({"pattern": "(foo", "regex": true}),
                &ctx_for(&root),
                cancel,
                None,
            )
            .await;
        let out = match r {
            Ok(res) => res,
            Err(e) => panic!("应返回 pattern 错误而非 Abort: {e:?}"),
        };
        assert!(out.is_error, "非法 pattern 必须是错误: {}", out.output);
        assert!(
            out.output.contains("pattern"),
            "错误需点名 pattern 参数: {}",
            out.output
        );
        assert!(
            !out.output.contains("未找到"),
            "坏参数不得伪装成零结果: {}",
            out.output
        );
    }

    /// A2：`regex` crate 不认识的构造（lookaround / 反向引用）与括号不配对
    /// 归为同一类非法模式——工具描述里承诺了「不跨行匹配」，这两类正是跨行
    /// 表达的常见写法，必须报错而不是静默零命中。
    #[tokio::test]
    async fn lookaround_and_backreference_are_invalid_patterns() {
        let dir = project(&[("a.rs", "fn foo() {}\n")]).await;
        let root = dir.path().to_path_buf();
        for bad in [r"foo(?=\(\))", r"(a)\1"] {
            let r = search(&root, json!({"pattern": bad, "regex": true}))
                .await
                .unwrap();
            assert!(r.is_error, "非法正则应报错: {bad} → {}", r.output);
            assert!(
                r.output.contains("pattern"),
                "{bad} 的错误应点名参数: {}",
                r.output
            );
            assert!(
                !r.output.contains("未找到"),
                "{bad} 不得伪装成零结果: {}",
                r.output
            );
        }
    }

    /// A3：忽略大小写对字面与正则两种模式都生效，缺省仍区分大小写。
    #[tokio::test]
    async fn case_insensitive_applies_to_both_modes() {
        let dir = project(&[("a.txt", "FOO\nfoo\nFoo\nbar\n")]).await;
        let root = dir.path().to_path_buf();

        let r = search(&root, json!({"pattern": "foo", "case_insensitive": true}))
            .await
            .unwrap();
        let hits = r.output.lines().filter(|l| l.contains("a.txt:")).count();
        assert_eq!(hits, 3, "字面模式忽略大小写应命中 3 行: {}", r.output);

        let r = search(
            &root,
            json!({"pattern": "foo", "regex": true, "case_insensitive": true}),
        )
        .await
        .unwrap();
        let hits = r.output.lines().filter(|l| l.contains("a.txt:")).count();
        assert_eq!(hits, 3, "正则模式忽略大小写应命中 3 行: {}", r.output);

        let r = search(&root, json!({"pattern": "foo"})).await.unwrap();
        let hits = r.output.lines().filter(|l| l.contains("a.txt:")).count();
        assert_eq!(hits, 1, "缺省应区分大小写: {}", r.output);
    }

    /// A4：`*.rs` 这类只含文件名的 glob 在任意深度生效。
    #[tokio::test]
    async fn glob_extension_filter_narrows_files() {
        let dir = project(&[
            ("src/a.rs", "foo\n"),
            ("src/b.ts", "foo\n"),
            ("c.md", "foo\n"),
        ])
        .await;
        let root = dir.path().to_path_buf();
        let r = search(&root, json!({"pattern": "foo", "glob": "*.rs"}))
            .await
            .unwrap();
        assert!(r.output.contains("src/a.rs"), "{}", r.output);
        assert!(!r.output.contains("b.ts"), "{}", r.output);
        assert!(!r.output.contains("c.md"), "{}", r.output);
    }

    /// A4：含 `/` 的 glob 对照相对路径，`**` 跨目录。
    #[tokio::test]
    async fn glob_path_filter_with_globstar() {
        let dir = project(&[
            ("src/deep/x.rs", "needle\n"),
            ("src/y.rs", "needle\n"),
            ("tests/z.rs", "needle\n"),
        ])
        .await;
        let root = dir.path().to_path_buf();
        let r = search(&root, json!({"pattern": "needle", "glob": "src/**/*.rs"}))
            .await
            .unwrap();
        assert!(r.output.contains("src/deep/x.rs"), "{}", r.output);
        assert!(
            r.output.contains("src/y.rs"),
            "** 应允许零层目录: {}",
            r.output
        );
        assert!(!r.output.contains("tests/z.rs"), "{}", r.output);
    }

    /// A4：非法 glob 是错误而不是零结果。
    #[tokio::test]
    async fn invalid_glob_is_reported_before_traversal() {
        let dir = project(&[("a.rs", "foo\n")]).await;
        let root = dir.path().to_path_buf();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let r = SearchFiles
            .execute(
                json!({"pattern": "foo", "glob": "src/{a,b"}),
                &ctx_for(&root),
                cancel,
                None,
            )
            .await
            .expect("坏 glob 应返回错误结果而非 Abort");
        assert!(r.is_error, "非法 glob 必须是错误: {}", r.output);
        assert!(r.output.contains("glob"), "{}", r.output);
        assert!(!r.output.contains("未找到"), "{}", r.output);
    }

    /// A5：file_type 按扩展名集合收窄。
    #[tokio::test]
    async fn file_type_filter_selects_extensions() {
        let dir = project(&[
            ("main.py", "def_or_fn\n"),
            ("main.rs", "def_or_fn\n"),
            ("notes.md", "def_or_fn\n"),
        ])
        .await;
        let root = dir.path().to_path_buf();
        let r = search(
            &root,
            json!({"pattern": "def_or_fn", "file_type": "python"}),
        )
        .await
        .unwrap();
        assert!(r.output.contains("main.py"), "{}", r.output);
        assert!(!r.output.contains("main.rs"), "{}", r.output);

        // 一个类型名覆盖多扩展名
        let dir = project(&[("a.cc", "tick\n"), ("b.hpp", "tick\n"), ("c.md", "tick\n")]).await;
        let root = dir.path().to_path_buf();
        let r = search(&root, json!({"pattern": "tick", "file_type": "cpp"}))
            .await
            .unwrap();
        assert!(
            r.output.contains("a.cc") && r.output.contains("b.hpp"),
            "{}",
            r.output
        );
        assert!(!r.output.contains("c.md"), "{}", r.output);
    }

    /// A5：未知类型名报错并列出支持集合；合法但无命中不是错误。
    #[tokio::test]
    async fn unknown_file_type_lists_supported_names() {
        let dir = project(&[("a.rs", "foo\n")]).await;
        let root = dir.path().to_path_buf();
        let r = search(&root, json!({"pattern": "foo", "file_type": "foobar"}))
            .await
            .unwrap();
        assert!(r.is_error, "未知 file_type 必须是错误: {}", r.output);
        assert!(r.output.contains("rust"), "应列出支持类型: {}", r.output);
        assert!(!r.output.contains("未找到"), "{}", r.output);

        // 合法类型、项目内无该类型文件 → 正常空结果
        let r = search(&root, json!({"pattern": "foo", "file_type": "go"}))
            .await
            .unwrap();
        assert!(!r.is_error, "合法类型无命中不应报错: {}", r.output);
        assert!(r.output.contains("未找到"), "{}", r.output);
    }

    /// A5：glob 与 file_type 同时给出时须两者都命中。
    #[tokio::test]
    async fn glob_and_file_type_intersect() {
        let dir = project(&[
            ("src/keep.rs", "both\n"),
            ("src/skip.md", "both\n"),
            ("tests/other.rs", "both\n"),
        ])
        .await;
        let root = dir.path().to_path_buf();
        let r = search(
            &root,
            json!({"pattern": "both", "glob": "src/**/*.rs", "file_type": "rust"}),
        )
        .await
        .unwrap();
        assert!(r.output.contains("src/keep.rs"), "{}", r.output);
        assert!(!r.output.contains("skip.md"), "{}", r.output);
        assert!(!r.output.contains("other.rs"), "{}", r.output);
    }

    /// 被筛掉的文件不占用结果上限（上限只数真正的命中）。
    #[tokio::test]
    async fn filtered_files_do_not_consume_result_cap() {
        let mut files: Vec<(String, String)> = Vec::new();
        for i in 0..(MAX_RESULTS + 5) {
            files.push((format!("skip{i}.txt"), format!("noise {i}\n")));
        }
        files.push(("hit.rs".into(), "needle\n".into()));
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect();
        let dir = project(&refs).await;
        let root = dir.path().to_path_buf();
        let r = search(&root, json!({"pattern": "needle", "glob": "*.rs"}))
            .await
            .unwrap();
        assert!(r.output.contains("hit.rs:1"), "{}", r.output);
        assert!(
            !r.output.contains("已达最大结果数"),
            "筛掉的文件不应触发截断: {}",
            r.output
        );
    }

    /// 字面模式下 Abort 仍即时生效（新参数不得削弱取消语义）。
    #[tokio::test]
    async fn abort_during_filtered_search_returns_aborted() {
        let dir = project(&[("a.rs", "foo\n"), ("src/b.rs", "foo\n")]).await;
        let root = dir.path().to_path_buf();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let r = SearchFiles
            .execute(
                json!({"pattern": "foo", "regex": true}),
                &ctx_for(&root),
                cancel,
                None,
            )
            .await;
        assert!(
            matches!(r, Err(ToolError::Aborted)),
            "已取消的搜索应报 Abort: {r:?}"
        );
    }

    #[test]
    fn ignores_common_dirs() {
        assert!(ignores("target"));
        assert!(ignores(".git"));
        assert!(ignores("node_modules"));
        assert!(!ignores("src"));
    }

    #[test]
    fn extension_rules() {
        assert!(extension_matches("a.rs", &["rs"]));
        assert!(extension_matches("A.RS", &["rs"]));
        assert!(!extension_matches("a.rs2", &["rs"]));
        // 点开头文件不算有该扩展名
        assert!(!extension_matches(".rs", &["rs"]));
        assert!(!extension_matches("Makefile", &["rs"]));
    }

    #[tokio::test]
    async fn summarize_with_path() {
        let s = SearchFiles
            .summarize(&json!({"pattern": "foo", "path": "src"}))
            .await;
        assert!(s.contains("src"));
        assert!(s.contains("foo"));
    }

    #[tokio::test]
    async fn summarize_reports_active_filters() {
        let s = SearchFiles
            .summarize(&json!({"pattern": "foo", "regex": true, "glob": "*.rs", "file_type": "rust", "case_insensitive": true}))
            .await;
        assert!(s.contains("正则"), "{s}");
        assert!(s.contains("忽略大小写"), "{s}");
        assert!(s.contains("*.rs"), "{s}");
        assert!(s.contains("rust"), "{s}");
    }

    #[test]
    fn schema_documents_new_arguments_and_type_list() {
        let schema = SearchFiles.schema();
        let props = schema.input_schema["properties"].as_object().unwrap();
        for key in ["regex", "case_insensitive", "glob", "file_type"] {
            assert!(props.contains_key(key), "schema 应声明 {key}");
        }
        assert!(
            schema.description.contains("regex=true"),
            "{}",
            schema.description
        );
        assert!(
            schema.description.contains("rust") && schema.description.contains("markdown"),
            "描述应列出支持类型: {}",
            schema.description
        );
    }
}
