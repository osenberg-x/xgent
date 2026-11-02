//! Rust 代码智能索引（F-21）：tree-sitter 符号 / import / 复杂度。
//!
//! 只读、确定性、无 Bevy；供 RepoMap 检索与后续 3D L2/诊断使用。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

/// 扫描时跳过的目录（与 `xgent_spatial` 的 L1 扫描同口径）。
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

/// 索引错误。
#[derive(Debug, thiserror::Error)]
pub enum CodeIntelError {
    /// 根目录不存在或不是目录。
    #[error("工作区根不是目录：{0}")]
    RootNotDir(PathBuf),
    /// 读取文件失败。
    #[error("读取 {} 失败：{source}", path.display())]
    Io {
        /// 出错路径。
        path: PathBuf,
        /// 底层错误。
        #[source]
        source: std::io::Error,
    },
}

/// 符号种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolKind {
    /// 自由函数。
    Function,
    /// impl/trait 内方法。
    Method,
    /// 结构体。
    Struct,
    /// 枚举。
    Enum,
    /// trait。
    Trait,
    /// impl 块。
    Impl,
    /// 模块。
    Module,
    /// 常量。
    Const,
    /// 静态变量。
    Static,
    /// 类型别名。
    TypeAlias,
}

impl SymbolKind {
    /// 稳定的短名称（查询/显示用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Module => "module",
            Self::Const => "const",
            Self::Static => "static",
            Self::TypeAlias => "type",
        }
    }
}

/// 源码行范围（1-based，含首尾）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceRange {
    /// 起始行。
    pub start_line: usize,
    /// 结束行。
    pub end_line: usize,
}

/// 一个符号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// 符号名（方法为原始方法名，不拼接容器）。
    pub name: String,
    /// 符号种类。
    pub kind: SymbolKind,
    /// 所属文件（相对工作区根）。
    pub file: PathBuf,
    /// 源码范围。
    pub range: SourceRange,
    /// 函数/方法的决策复杂度；非函数为 0。
    pub complexity: u32,
    /// 所属 impl/trait 名称（方法有值）。
    pub container: Option<String>,
}

/// import 的解析目标。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportTarget {
    /// 解析到工作区内文件（相对根）。
    Workspace(PathBuf),
    /// 工作区外 crate/路径。
    External(String),
    /// 看起来是工作区路径但找不到对应文件。
    Unresolved(String),
}

/// 一条 import/模块声明。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Import {
    /// 来源文件（相对根）。
    pub from: PathBuf,
    /// 解析目标。
    pub target: ImportTarget,
    /// 原始声明文本（调试/展示）。
    pub raw: String,
}

/// 单文件索引。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileEntry {
    /// 符号（按行/名称排序）。
    pub symbols: Vec<Symbol>,
    /// import（按 raw/target 排序）。
    pub imports: Vec<Import>,
    /// 内容哈希（增量 generation 用）。
    pub content_hash: u64,
}

/// 代码索引。
#[derive(Debug, Clone, Default)]
pub struct CodeIndex {
    generation: u64,
    files: BTreeMap<PathBuf, FileEntry>,
}

impl CodeIndex {
    /// 扫描工作区下的 Rust 文件；不可读文件跳过，根无效返回错误。
    pub fn scan(root: &Path) -> Result<Self, CodeIntelError> {
        if !root.is_dir() {
            return Err(CodeIntelError::RootNotDir(root.to_path_buf()));
        }
        let mut paths = Vec::new();
        let mut builder = ignore::WalkBuilder::new(root);
        builder
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .parents(true)
            .ignore(true)
            .require_git(false)
            .follow_links(false);
        builder.filter_entry(|entry| {
            !entry
                .file_name()
                .to_str()
                .is_some_and(|name| SKIP_DIRS.contains(&name))
        });
        for entry in builder.build().filter_map(Result::ok) {
            if entry.file_type().is_some_and(|ty| ty.is_file())
                && entry.path().extension().is_some_and(|ext| ext == "rs")
            {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or(entry.path())
                    .to_path_buf();
                paths.push((relative, entry.path().to_path_buf()));
            }
        }
        paths.sort_by(|a, b| a.0.cmp(&b.0));
        let all_files: BTreeSet<PathBuf> =
            paths.iter().map(|(relative, _)| relative.clone()).collect();
        let mut files = BTreeMap::new();
        for (relative, absolute) in paths {
            let Ok(source) = std::fs::read_to_string(&absolute) else {
                continue;
            };
            files.insert(
                relative.clone(),
                parse_source(&relative, &source, &all_files),
            );
        }
        let generation = generation_of(&files);
        Ok(Self { generation, files })
    }

    /// 内容 generation（文件集合 + 内容哈希）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 全部已索引文件（按路径排序）。
    pub fn files(&self) -> impl Iterator<Item = (&Path, &FileEntry)> {
        self.files
            .iter()
            .map(|(path, entry)| (path.as_path(), entry))
    }

    /// 查询单文件。
    pub fn file(&self, path: &Path) -> Option<&FileEntry> {
        self.files.get(path)
    }

    /// 查询单文件符号。
    pub fn symbols_of(&self, path: &Path) -> Option<&[Symbol]> {
        self.files.get(path).map(|entry| entry.symbols.as_slice())
    }

    /// 按名称查符号（大小写敏感，稳定排序）。
    pub fn symbols_named(&self, name: &str) -> Vec<&Symbol> {
        self.files
            .values()
            .flat_map(|entry| entry.symbols.iter())
            .filter(|symbol| symbol.name == name)
            .collect()
    }

    /// 全部符号（文件/行/名排序）。
    pub fn all_symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.files.values().flat_map(|entry| entry.symbols.iter())
    }

    /// 就地更新一个文件（`path` 相对工作区根）。
    pub fn update_file(&mut self, path: &Path, source: &str) {
        let all_files: BTreeSet<PathBuf> = self.files.keys().cloned().collect();
        let entry = parse_source(path, source, &all_files);
        self.files.insert(path.to_path_buf(), entry);
        self.generation = generation_of(&self.files);
    }

    /// 删除一个文件；返回是否存在。
    pub fn remove_file(&mut self, path: &Path) -> bool {
        let removed = self.files.remove(path).is_some();
        if removed {
            self.generation = generation_of(&self.files);
        }
        removed
    }

    /// 从磁盘重读一个绝对或相对路径；文件不存在时删除索引条目。
    pub fn update_file_from_disk(
        &mut self,
        root: &Path,
        path: &Path,
    ) -> Result<(), CodeIntelError> {
        let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        match std::fs::read_to_string(path) {
            Ok(source) => {
                self.update_file(&relative, &source);
                Ok(())
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                self.remove_file(&relative);
                Ok(())
            }
            Err(source) => Err(CodeIntelError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }
}

fn parse_source(path: &Path, source: &str, all_files: &BTreeSet<PathBuf>) -> FileEntry {
    let mut parser = Parser::new();
    let language = tree_sitter_rust::LANGUAGE;
    if parser.set_language(&language.into()).is_err() {
        return FileEntry::default();
    }
    let Some(tree) = parser.parse(source, None) else {
        return FileEntry::default();
    };
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    walk_node(
        tree.root_node(),
        source,
        path,
        all_files,
        None,
        &mut symbols,
        &mut imports,
    );
    symbols.sort_by(|a, b| {
        a.range
            .start_line
            .cmp(&b.range.start_line)
            .then_with(|| a.name.cmp(&b.name))
    });
    imports.sort();
    imports.dedup();
    FileEntry {
        symbols,
        imports,
        content_hash: fnv(source.as_bytes()),
    }
}

fn walk_node(
    node: Node<'_>,
    source: &str,
    file: &Path,
    all_files: &BTreeSet<PathBuf>,
    container: Option<&str>,
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
) {
    match node.kind() {
        "function_item" | "function_signature_item" => {
            if let Some(symbol) = node_symbol(
                node,
                source,
                file,
                if container.is_some() {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                },
                container.map(str::to_string),
            ) {
                symbols.push(Symbol {
                    complexity: if node.kind() == "function_item" {
                        complexity(node, source)
                    } else {
                        1
                    },
                    ..symbol
                });
            }
        }
        "struct_item" => push_symbol(node, source, file, SymbolKind::Struct, None, symbols),
        "enum_item" => push_symbol(node, source, file, SymbolKind::Enum, None, symbols),
        "trait_item" => {
            push_symbol(node, source, file, SymbolKind::Trait, None, symbols);
            let trait_name = node
                .child_by_field_name("name")
                .map(|name| node_text(name, source).to_string());
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk_node(
                    child,
                    source,
                    file,
                    all_files,
                    trait_name.as_deref().or(container),
                    symbols,
                    imports,
                );
            }
            return;
        }
        "type_item" => push_symbol(node, source, file, SymbolKind::TypeAlias, None, symbols),
        "const_item" => push_symbol(node, source, file, SymbolKind::Const, None, symbols),
        "static_item" => push_symbol(node, source, file, SymbolKind::Static, None, symbols),
        "mod_item" => {
            push_symbol(node, source, file, SymbolKind::Module, None, symbols);
            if node.child_by_field_name("body").is_none() {
                if let Some(name) = node.child_by_field_name("name") {
                    let raw = node_text(name, source).to_string();
                    if let Some(target) = resolve_mod(file, &raw, all_files) {
                        imports.push(Import {
                            from: file.to_path_buf(),
                            target,
                            raw: format!("mod {raw};"),
                        });
                    }
                }
            }
        }
        "impl_item" => {
            let label = node
                .child_by_field_name("type")
                .map(|node| node_text(node, source).to_string());
            let name = label.clone().unwrap_or_else(|| "<impl>".to_string());
            let range = range_of(node);
            symbols.push(Symbol {
                name,
                kind: SymbolKind::Impl,
                file: file.to_path_buf(),
                range,
                complexity: 0,
                container: None,
            });
            let new_container = label.as_deref();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk_node(
                    child,
                    source,
                    file,
                    all_files,
                    new_container.or(container),
                    symbols,
                    imports,
                );
            }
            return;
        }
        "use_declaration" => {
            let raw = node_text(node, source).trim().to_string();
            let body = raw
                .strip_prefix("use")
                .unwrap_or(&raw)
                .trim_end_matches(';')
                .trim();
            for path in expand_use_paths(body) {
                let target = resolve_use(file, &path, all_files);
                imports.push(Import {
                    from: file.to_path_buf(),
                    target,
                    raw: raw.clone(),
                });
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(child, source, file, all_files, container, symbols, imports);
    }
}

fn push_symbol(
    node: Node<'_>,
    source: &str,
    file: &Path,
    kind: SymbolKind,
    container: Option<String>,
    symbols: &mut Vec<Symbol>,
) {
    if let Some(symbol) = node_symbol(node, source, file, kind, container) {
        symbols.push(symbol);
    }
}

fn node_symbol(
    node: Node<'_>,
    source: &str,
    file: &Path,
    kind: SymbolKind,
    container: Option<String>,
) -> Option<Symbol> {
    let name = node
        .child_by_field_name("name")
        .map(|node| node_text(node, source).to_string())?;
    Some(Symbol {
        name,
        kind,
        file: file.to_path_buf(),
        range: range_of(node),
        complexity: 0,
        container,
    })
}

fn range_of(node: Node<'_>) -> SourceRange {
    SourceRange {
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
    }
}

fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

/// 函数复杂度 = 1 + 决策节点数；嵌套函数只统计外层。
fn complexity(node: Node<'_>, source: &str) -> u32 {
    let mut count = 1;
    count_decisions(node, source, node.id(), &mut count);
    count
}

fn count_decisions(node: Node<'_>, source: &str, root_id: usize, count: &mut u32) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_item" && child.id() != root_id {
            continue;
        }
        match child.kind() {
            "if_expression" | "match_arm" | "while_expression" | "loop_expression"
            | "for_expression" | "try_expression" => *count += 1,
            "binary_expression" => {
                let operator = child
                    .child_by_field_name("operator")
                    .map(|node| node_text(node, source));
                if matches!(operator, Some("&&" | "||")) {
                    *count += 1;
                }
            }
            _ => {}
        }
        count_decisions(child, source, root_id, count);
    }
}

/// 展开 `use` 路径（处理简单 `{a, b}` 分组；复杂嵌套保持 best-effort）。
fn expand_use_paths(body: &str) -> Vec<String> {
    let body = body.trim();
    if body.is_empty() || body.ends_with('*') {
        return Vec::new();
    }
    let Some(open) = body.find('{') else {
        return vec![strip_alias(body).to_string()];
    };
    let Some(close) = body.rfind('}') else {
        return vec![strip_alias(body).to_string()];
    };
    let prefix = body[..open].trim_end_matches("::").trim();
    let inside = &body[open + 1..close];
    let mut out = Vec::new();
    for part in inside.split(',') {
        let part = part.trim();
        if part.is_empty() || part == "*" {
            continue;
        }
        let part = strip_alias(part);
        let combined = if prefix.is_empty() {
            part.to_string()
        } else {
            format!("{prefix}::{part}")
        };
        out.extend(expand_use_paths(&combined));
    }
    out
}

fn strip_alias(path: &str) -> &str {
    path.split(" as ").next().unwrap_or(path).trim()
}

fn resolve_mod(file: &Path, name: &str, all_files: &BTreeSet<PathBuf>) -> Option<ImportTarget> {
    let dir = file.parent().unwrap_or(Path::new(""));
    for candidate in [
        dir.join(format!("{name}.rs")),
        dir.join(name).join("mod.rs"),
    ] {
        if all_files.contains(&candidate) {
            return Some(ImportTarget::Workspace(candidate));
        }
    }
    Some(ImportTarget::Unresolved(name.to_string()))
}

fn resolve_use(file: &Path, raw: &str, all_files: &BTreeSet<PathBuf>) -> ImportTarget {
    let raw = raw.trim().trim_end_matches(';').trim();
    if raw.is_empty() {
        return ImportTarget::Unresolved(String::new());
    }
    let segments: Vec<&str> = raw.split("::").filter(|part| !part.is_empty()).collect();
    if segments.is_empty() {
        return ImportTarget::Unresolved(raw.to_string());
    }
    let (base, start) = match segments[0] {
        "crate" => (crate_root(file), 1),
        "self" => (file.parent().unwrap_or(Path::new("")).to_path_buf(), 1),
        "super" => {
            let mut base = file.parent().unwrap_or(Path::new("")).to_path_buf();
            let mut index = 0;
            while segments.get(index) == Some(&"super") {
                base = base.parent().unwrap_or(Path::new("")).to_path_buf();
                index += 1;
            }
            (base, index)
        }
        _ => (file.parent().unwrap_or(Path::new("")).to_path_buf(), 0),
    };
    let tail = &segments[start..];
    if tail.is_empty() {
        return ImportTarget::Unresolved(raw.to_string());
    }
    // 从最长前缀开始找模块文件：use 的最后一段通常是符号名。
    for take in (1..=tail.len()).rev() {
        let module = tail[..take]
            .iter()
            .fold(base.clone(), |path, part| path.join(part));
        for candidate in [module.with_extension("rs"), module.join("mod.rs")] {
            if all_files.contains(&candidate) {
                return ImportTarget::Workspace(candidate);
            }
        }
    }
    if start == 0 && segments[0] != "crate" && segments[0] != "self" && segments[0] != "super" {
        ImportTarget::External(segments[0].to_string())
    } else {
        ImportTarget::Unresolved(raw.to_string())
    }
}

fn crate_root(file: &Path) -> PathBuf {
    let mut current = file.parent();
    while let Some(dir) = current {
        if dir.file_name().is_some_and(|name| name == "src") {
            return dir.to_path_buf();
        }
        current = dir.parent();
    }
    file.parent().unwrap_or(Path::new("")).to_path_buf()
}

fn generation_of(files: &BTreeMap<PathBuf, FileEntry>) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for (path, entry) in files {
        for byte in path.to_string_lossy().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        for byte in entry.content_hash.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

fn fnv(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("创建目录");
        }
        fs::write(path, contents).expect("写入文件");
    }

    #[test]
    fn extracts_symbols_ranges_and_complexity() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(
            temp.path(),
            "src/lib.rs",
            r#"
mod child;

const LIMIT: u32 = 3;
static NAME: &str = "x";
type Alias = u32;

struct Point { x: i32 }
enum Color { Red, Blue }
trait Draw { fn draw(&self); }

impl Point {
    fn len(&self, value: i32) -> i32 {
        if value > 0 && value < 10 { value } else { match value { 0 => 1, _ => 2 } }
    }
}

fn helper(a: bool, b: bool) -> bool { a && b }
fn plain() {}
"#,
        );
        write(temp.path(), "src/child.rs", "pub fn child_fn() {}\n");
        let index = CodeIndex::scan(temp.path()).expect("扫描成功");
        let symbols = index.symbols_of(Path::new("src/lib.rs")).expect("有文件");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        for expected in [
            "LIMIT", "NAME", "Alias", "Point", "Color", "Draw", "draw", "Point", "len", "helper",
            "plain",
        ] {
            assert!(names.contains(&expected), "缺少符号 {expected}: {names:?}");
        }
        for (name, kind) in [
            ("child", SymbolKind::Module),
            ("LIMIT", SymbolKind::Const),
            ("NAME", SymbolKind::Static),
            ("Alias", SymbolKind::TypeAlias),
            ("Color", SymbolKind::Enum),
            ("Draw", SymbolKind::Trait),
            ("helper", SymbolKind::Function),
        ] {
            let symbol = symbols.iter().find(|s| s.name == name).expect(name);
            assert_eq!(symbol.kind, kind, "{name} 种类错误");
        }
        let point = symbols
            .iter()
            .find(|s| s.name == "Point" && s.kind == SymbolKind::Struct)
            .expect("Point 结构体");
        assert!(point.range.start_line <= point.range.end_line);
        let impl_block = symbols
            .iter()
            .find(|s| s.name == "Point" && s.kind == SymbolKind::Impl)
            .expect("Point impl");
        assert!(impl_block.range.end_line >= point.range.end_line);
        let len_symbol = symbols.iter().find(|s| s.name == "len").expect("len");
        assert!(impl_block.range.start_line <= len_symbol.range.start_line);
        assert!(impl_block.range.end_line >= len_symbol.range.end_line);
        let drew = symbols.iter().find(|s| s.name == "draw").expect("draw");
        assert_eq!(drew.kind, SymbolKind::Method);
        assert_eq!(drew.container.as_deref(), Some("Draw"));
        let len = symbols.iter().find(|s| s.name == "len").expect("len");
        assert_eq!(len.kind, SymbolKind::Method);
        assert!(
            len.complexity >= 4,
            "if + && + match arm 应计入：{}",
            len.complexity
        );
        let helper = symbols.iter().find(|s| s.name == "helper").expect("helper");
        assert!(helper.complexity >= 2, "&& 应计入");
    }

    #[test]
    fn resolves_workspace_imports_and_marks_external() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(
            temp.path(),
            "src/lib.rs",
            r#"
mod foo;
use crate::foo::Hello;
use crate::foo::bar::Nested;
use external_crate::Thing;
"#,
        );
        write(temp.path(), "src/foo.rs", "pub struct Hello;\n");
        write(temp.path(), "src/foo/bar.rs", "pub struct Nested;\n");
        let index = CodeIndex::scan(temp.path()).expect("扫描成功");
        let imports = &index.file(Path::new("src/lib.rs")).expect("lib").imports;
        assert!(imports.iter().any(|i| matches!(&i.target, ImportTarget::Workspace(path) if path == Path::new("src/foo.rs"))));
        assert!(imports.iter().any(|i| matches!(&i.target, ImportTarget::Workspace(path) if path == Path::new("src/foo/bar.rs"))));
        assert!(imports.iter().any(
            |i| matches!(&i.target, ImportTarget::External(name) if name == "external_crate")
        ));
    }

    #[test]
    fn symbol_ranges_match_known_lines() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(
            temp.path(),
            "src/range.rs",
            "pub struct S;\n\npub fn f() {\n    if true {}\n}\n",
        );
        let index = CodeIndex::scan(temp.path()).expect("扫描成功");
        let symbols = index.symbols_of(Path::new("src/range.rs")).expect("有文件");
        let struct_symbol = symbols.iter().find(|s| s.name == "S").expect("S");
        assert_eq!(
            struct_symbol.range,
            SourceRange {
                start_line: 1,
                end_line: 1
            }
        );
        let function = symbols.iter().find(|s| s.name == "f").expect("f");
        assert_eq!(
            function.range,
            SourceRange {
                start_line: 3,
                end_line: 5
            }
        );
        assert_eq!(function.complexity, 2, "1 + if");
    }

    #[test]
    fn unresolved_imports_stay_unresolved_without_error() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(
            temp.path(),
            "src/lib.rs",
            "use crate::missing::Thing;\nuse super::up::Other;\nuse external_crate::X;\n",
        );
        let index = CodeIndex::scan(temp.path()).expect("扫描成功");
        let imports = &index.file(Path::new("src/lib.rs")).expect("lib").imports;
        assert!(imports.iter().any(
            |i| matches!(&i.target, ImportTarget::Unresolved(path) if path.contains("missing"))
        ));
        assert!(imports.iter().any(
            |i| matches!(&i.target, ImportTarget::Unresolved(path) if path.contains("super"))
        ));
        assert!(imports.iter().any(
            |i| matches!(&i.target, ImportTarget::External(name) if name == "external_crate")
        ));
        assert!(
            !imports
                .iter()
                .any(|i| matches!(&i.target, ImportTarget::Workspace(_))),
            "不存在的路径不应产生工作区边：{imports:?}"
        );
    }

    #[test]
    fn scan_is_read_only_and_skips_unreadable_files() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(temp.path(), "src/a.rs", "pub fn a() {}\n");
        fs::write(temp.path().join("src/bad.rs"), [0x66, 0xff, 0xfe, 0x00]).expect("写坏文件");
        let before = fs::read(temp.path().join("src/a.rs")).expect("读 a");
        let bad_before = fs::read(temp.path().join("src/bad.rs")).expect("读 bad");

        let index = CodeIndex::scan(temp.path()).expect("扫描成功");
        assert!(index.symbols_named("a").len() == 1);
        assert!(
            index.file(Path::new("src/bad.rs")).is_none(),
            "无法解码的文件应跳过"
        );
        assert_eq!(
            fs::read(temp.path().join("src/a.rs")).expect("再读 a"),
            before
        );
        assert_eq!(
            fs::read(temp.path().join("src/bad.rs")).expect("再读 bad"),
            bad_before
        );
    }

    #[test]
    fn scan_is_deterministic_and_incremental() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(temp.path(), "src/a.rs", "pub fn a() {}\n");
        write(temp.path(), "src/b.rs", "pub fn b() {}\n");
        let first = CodeIndex::scan(temp.path()).expect("第一次");
        let second = CodeIndex::scan(temp.path()).expect("第二次");
        assert_eq!(first.generation(), second.generation());
        assert_eq!(
            first.file(Path::new("src/a.rs")),
            second.file(Path::new("src/a.rs"))
        );

        let mut index = first;
        let b_before = index.file(Path::new("src/b.rs")).cloned();
        index.update_file(Path::new("src/a.rs"), "pub fn a() {}\npub fn added() {}\n");
        assert!(index.symbols_named("added").len() == 1);
        assert_eq!(
            index.file(Path::new("src/b.rs")).cloned(),
            b_before,
            "其他文件不变"
        );
        let generation = index.generation();
        index.remove_file(Path::new("src/a.rs"));
        assert!(index.symbols_named("added").is_empty());
        assert_ne!(generation, index.generation());
    }

    #[test]
    fn skips_ignored_and_non_rust_files() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(temp.path(), ".gitignore", "ignored/\n");
        write(temp.path(), "src/a.rs", "pub fn a() {}\n");
        write(temp.path(), "target/b.rs", "pub fn b() {}\n");
        write(temp.path(), "ignored/c.rs", "pub fn c() {}\n");
        write(temp.path(), "node_modules/d.rs", "pub fn d() {}\n");
        write(temp.path(), "src/.hidden.rs", "pub fn hidden() {}\n");
        write(temp.path(), "src/readme.md", "not rust\n");
        let index = CodeIndex::scan(temp.path()).expect("扫描成功");
        let files: Vec<String> = index
            .files()
            .map(|(path, _)| path.display().to_string())
            .collect();
        assert_eq!(files.len(), 1, "只应索引 src/a.rs: {files:?}");
        assert!(index.file(Path::new("src/a.rs")).is_some());
    }

    #[test]
    fn query_missing_file_returns_none_without_panic() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(temp.path(), "src/a.rs", "pub fn a() {}\n");
        let index = CodeIndex::scan(temp.path()).expect("扫描成功");
        assert!(index.file(Path::new("src/missing.rs")).is_none());
        assert!(index.symbols_of(Path::new("src/missing.rs")).is_none());
        assert!(index.symbols_named("missing").is_empty());
    }

    #[test]
    fn update_from_disk_handles_removed_files() {
        let temp = tempfile::tempdir().expect("临时目录");
        write(temp.path(), "src/a.rs", "pub fn a() {}\n");
        let mut index = CodeIndex::scan(temp.path()).expect("扫描成功");
        let path = temp.path().join("src/a.rs");
        fs::remove_file(&path).expect("删除");
        index
            .update_file_from_disk(temp.path(), &path)
            .expect("更新");
        assert!(index.file(Path::new("src/a.rs")).is_none());
    }
}
