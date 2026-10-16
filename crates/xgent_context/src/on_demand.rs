//! 方案 A：无索引·按需读取上下文提供者。
//!
//! 策略：
//! 1. 生成项目目录树摘要（限制深度与条目数，token 预算内）；
//! 2. 若有 current_file，优先读其内容；
//! 3. 解析 `@file:` 线索，直接读被引用的文件；
//! 4. 用用户问题关键词搜索（优先系统 rg 字面量匹配，降级内置遍历），读匹配文件；
//! 5. 组装 chunks + tree_summary，控制总 token 不超预算。
//!
//! 检索词由 [`extract_keywords`] 产出：CJK 词串切成相邻二字组合（bigram），
//! 因为子串与 rg 字面量匹配命中的正是这种片段——整句作为关键词时中文提问
//! 必然零命中（R3-1）。所有文件系统遍历都在非 async 函数里完成，再经
//! `spawn_blocking` 调用，不占用 tokio 工作线程（R3-8）。

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::provider::{
    ContextChunk, ContextProvider, ContextQuery, ContextResult, estimate_tokens, is_cjk_char,
};

/// 遍历/搜索/关键词的限额不再是本文件的常量，而是 [`RetrievalLimits`]
/// 的字段（项目配置 `[context]`，R3-17）。其缺省值与内置缺省同源，
/// 定义在 `xgent_settings_core::project::defaults`。

/// 降级遍历访问的节点数上限：纯止损闸，不属于「按工程调优」的参数，
/// 因此保留为常量。
const VISITED_NODE_LIMIT: usize = 2000;
/// `@file:` 线索前缀（编辑器 `@` 引用把文件路径带进 hints）。
const FILE_REF_PREFIX: &str = "@file:";
/// 常见英文虚词：几乎不会作为标识符出现，作检索词只引入噪声。
const EN_STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "from", "have", "has", "how", "why", "what",
    "when", "which", "about", "into", "your", "you", "are", "was", "were", "not", "but", "can",
    "will", "should", "would", "please", "to", "of", "in", "on", "is", "it", "we", "i",
];
/// 中文功能字。仅由这些字组成的 bigram 没有检索价值（"的是什么"、"如何"）。
///
/// 表里不放方位/量词字（上、下、中、个）：它们会组成真正有信息量的词片
/// （"上下文"、"中间"、"下载"），一并当虚词会把有效关键词切掉。
const CN_FUNC_CHARS: &[char] = &[
    '的', '了', '是', '在', '和', '与', '或', '就', '都', '而', '及', '着', '过', '很', '也', '吧',
    '呢', '吗', '啊', '把', '被', '让', '给', '从', '到', '对', '为', '以', '之', '什', '么', '这',
    '那', '我', '你', '他', '她', '它', '们', '有', '要', '会', '能', '如', '何', '但', '因', '此',
    '所', '其', '该', '怎',
];

/// chunk 的来源角色，决定 relevance 文案的前半段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkRole {
    /// 当前打开文件
    CurrentFile,
    /// 用户 `@file:` 引用的文件
    Referenced,
    /// 关键词检索命中
    SearchHit,
}

/// 检索遍历限额（项目配置 `[context]`，R3-17）。
///
/// 缺省即本次转配置前 `on_demand.rs` 顶部的五个常量，因此「不写配置」与
/// 「改前行为」逐字等价。放在实例上而非静态常量，是为了让 provider 的每次
/// 遍历都按调用方给定的预算收敛，而不用改一处影响全部工程。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrievalLimits {
    /// 目录树最大深度
    pub tree_max_depth: u32,
    /// 目录树最大条目数
    pub tree_max_entries: usize,
    /// ripgrep 超时秒数
    pub rg_timeout_secs: u64,
    /// 搜索返回的最大文件数
    pub max_search_files: usize,
    /// 关键词上限
    pub max_keywords: usize,
}

impl Default for RetrievalLimits {
    fn default() -> Self {
        use xgent_settings_core::project::defaults as d;
        Self {
            tree_max_depth: d::TREE_MAX_DEPTH,
            tree_max_entries: d::TREE_MAX_ENTRIES,
            rg_timeout_secs: d::RG_TIMEOUT_SECS,
            max_search_files: d::MAX_SEARCH_FILES,
            max_keywords: d::MAX_KEYWORDS,
        }
    }
}

impl RetrievalLimits {
    /// 由项目配置的 `[context]` 节派生（`0` 已解析为内置缺省）。
    pub fn from_context_limits(cfg: &xgent_settings_core::ContextLimits) -> Self {
        Self {
            tree_max_depth: cfg.effective_tree_max_depth(),
            tree_max_entries: cfg.effective_tree_max_entries(),
            rg_timeout_secs: cfg.effective_rg_timeout_secs(),
            max_search_files: cfg.effective_max_search_files(),
            max_keywords: cfg.effective_max_keywords(),
        }
    }
}

/// 方案 A 上下文提供者。
pub struct OnDemandContextProvider {
    project_root: PathBuf,
    /// 遍历/搜索限额，**运行期可换**（项目配置 `[context]`）。
    ///
    /// 与宿主共享同一句柄：设置面板改完限额后，下一次检索用的就是新值。
    /// 每次检索开始时取一次快照，单次检索内部保持一致。
    limits: Arc<parking_lot::RwLock<RetrievalLimits>>,
}

impl OnDemandContextProvider {
    pub fn new(project_root: PathBuf) -> Self {
        Self::with_shared_limits(
            project_root,
            Arc::new(parking_lot::RwLock::new(RetrievalLimits::default())),
        )
    }

    /// 用给定限额构造（`xgent_app` 由项目配置 `[context]` 派生）。限额按快照拷贝。
    pub fn with_limits(project_root: PathBuf, limits: RetrievalLimits) -> Self {
        Self::with_shared_limits(project_root, Arc::new(parking_lot::RwLock::new(limits)))
    }

    /// 用**共享**限额句柄构造：宿主改 `[context]` 后下一次检索即生效。
    pub fn with_shared_limits(
        project_root: PathBuf,
        limits: Arc<parking_lot::RwLock<RetrievalLimits>>,
    ) -> Self {
        Self {
            project_root,
            limits,
        }
    }

    /// 项目根。
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// 当前生效的限额。
    pub fn limits(&self) -> RetrievalLimits {
        *self.limits.read()
    }
}

#[async_trait]
impl ContextProvider for OnDemandContextProvider {
    async fn retrieve(&self, query: &ContextQuery) -> ContextResult {
        // 一次检索取一份限额快照，整次检索内部保持一致
        let limits = self.limits();
        let mut total_tokens: u32 = 0;
        let max = query.max_tokens.max(1);

        // 1. 目录树摘要
        let tree = self.tree_summary(&limits).await;
        let tree_tokens = tree.as_ref().map(|t| estimate_tokens(t)).unwrap_or(0);
        if tree_tokens < max {
            total_tokens += tree_tokens;
        }
        // 即使超预算也保留树摘要（它对 LLM 理解结构很关键），但不计入后续预算

        let mut chunks: Vec<ContextChunk> = Vec::new();
        let remaining = max.saturating_sub(total_tokens);

        // 2. 优先读 current_file
        if let Some(cur) = &query.current_file
            && let Some(chunk) = self
                .read_file_chunk(cur, remaining, ChunkRole::CurrentFile, &[])
                .await
        {
            total_tokens += chunk.token_estimate;
            chunks.push(chunk);
        }

        // 3. `@file:` 线索：引用的是文件，直接读它，不作为检索词（R3-1 家族）
        for path in file_refs(&query.hints) {
            if total_tokens >= max {
                break;
            }
            let budget = max - total_tokens;
            if let Some(chunk) = self
                .read_file_chunk(Path::new(&path), budget, ChunkRole::Referenced, &[])
                .await
            {
                total_tokens += chunk.token_estimate;
                chunks.push(chunk);
            }
        }

        // 4. 关键词搜索
        let keywords = extract_keywords(&query.user_message, &query.hints, &limits);
        if !keywords.is_empty() && total_tokens < max {
            let files = self.search(&keywords, &limits).await;
            for f in files {
                if total_tokens >= max {
                    break;
                }
                let budget = max - total_tokens;
                if let Some(chunk) = self
                    .read_file_chunk(&f, budget, ChunkRole::SearchHit, &keywords)
                    .await
                {
                    total_tokens += chunk.token_estimate;
                    chunks.push(chunk);
                }
            }
        }

        ContextResult {
            chunks,
            tree_summary: tree,
            total_tokens,
        }
    }
}

impl OnDemandContextProvider {
    /// 生成目录树摘要（跳过 .git/target 等）。遍历在 blocking pool 上执行。
    async fn tree_summary(&self, limits: &RetrievalLimits) -> Option<String> {
        let root = self.project_root.clone();
        // 拷贝一份交给 blocking 线程：闭包要 'static，不能借用 self 的参数
        let limits = *limits;
        let lines = tokio::task::spawn_blocking(move || collect_tree_lines(&root, &limits))
            .await
            .unwrap_or_default();
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    /// 关键词搜索：优先系统 rg，降级内置遍历。
    async fn search(&self, keywords: &[String], limits: &RetrievalLimits) -> Vec<PathBuf> {
        if let Some(files) = self.rg_search(keywords, limits).await {
            return files;
        }
        self.fallback_search(keywords, limits).await
    }

    /// 调系统 ripgrep，返回匹配文件列表（相对项目根）。
    /// rg 不存在或失败时返回 None，由调用方降级。
    async fn rg_search(
        &self,
        keywords: &[String],
        limits: &RetrievalLimits,
    ) -> Option<Vec<PathBuf>> {
        if keywords.is_empty() {
            return Some(Vec::new());
        }
        let mut cmd = tokio::process::Command::new("rg");
        cmd.args(rg_args(&self.project_root, keywords))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(limits.rg_timeout_secs),
            cmd.output(),
        )
        .await
        .ok()?
        .ok()?;
        // rg exit 1 = 无匹配（正常），不算失败、不触发全量回退
        if output.status.code() == Some(1) {
            return Some(Vec::new());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() && stdout.is_empty() {
            return None;
        }
        let files: Vec<PathBuf> = stdout
            .lines()
            .filter_map(|l| {
                Path::new(l)
                    .strip_prefix(&self.project_root)
                    .ok()
                    .map(|p| p.to_path_buf())
            })
            .take(limits.max_search_files)
            .collect();
        Some(files)
    }

    /// 降级搜索：内置递归遍历 + 字面量子串匹配（任一关键词命中即返回文件）。
    /// 遍历在 blocking pool 上执行。
    async fn fallback_search(&self, keywords: &[String], limits: &RetrievalLimits) -> Vec<PathBuf> {
        let root = self.project_root.clone();
        let keywords = keywords.to_vec();
        // 同上：blocking 线程需拥有副本
        let limits = *limits;
        tokio::task::spawn_blocking(move || collect_matching_files(&root, &keywords, &limits))
            .await
            .unwrap_or_default()
    }

    /// 读文件内容（带 token 估算与裁剪），并写明该 chunk 的真实来源。
    async fn read_file_chunk(
        &self,
        rel: &Path,
        max_tokens: u32,
        role: ChunkRole,
        keywords: &[String],
    ) -> Option<ContextChunk> {
        let full = if rel.is_absolute() {
            rel.to_path_buf()
        } else {
            self.project_root.join(rel)
        };
        let content = tokio::fs::read_to_string(&full).await.ok()?;
        let full_tokens = estimate_tokens(&content);
        let (content, tokens) = if full_tokens > max_tokens {
            // 按字符裁剪到约 max_tokens
            let limit = (max_tokens as usize).saturating_mul(4);
            let truncated: String = content.chars().take(limit).collect();
            let t = estimate_tokens(&truncated);
            (format!("{truncated}\n...（已截断）"), t)
        } else {
            (content, full_tokens)
        };
        Some(ContextChunk {
            path: rel.to_path_buf(),
            relevance: relevance_for(role, &content, keywords),
            content,
            token_estimate: tokens,
        })
    }
}

/// 构造 rg 参数：字面量匹配，每个关键词一个 `-e` 模式。
///
/// 用 `--fixed-strings` 而非拼 `a|b` 正则：关键词是用户输入的文本，不是本程序
/// 编写的正则。拼正则时 `.` 变成通配、`(`/`+` 让模式非法从而 rg 失败并触发全量
/// 回退（R3-1 家族）。多个 `-e` 由 rg 取并集，语义与原 `|` 相同。
///
/// 注意仍不加 `--no-ignore`：遵循 .gitignore，否则 target/node_modules 等构建
/// 产物会被读进上下文撑爆预算。
fn rg_args(root: &Path, keywords: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--files-with-matches".into(),
        "-i".into(),
        "--fixed-strings".into(),
    ];
    for kw in keywords {
        args.push("-e".into());
        args.push(kw.clone());
    }
    args.push(root.display().to_string());
    args
}

/// `@file:` 线索里解析出的路径列表。
fn file_refs(hints: &[String]) -> Vec<String> {
    hints
        .iter()
        .filter_map(|h| h.trim().strip_prefix(FILE_REF_PREFIX))
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect()
}

/// 从用户消息与 hints 提取搜索关键词。
///
/// 中文按相邻二字组合（bigram）产出：单字太泛（会命中大半棵源码树），整句太长
/// （任何源码都不会包含整句），二字片段正是子串与 rg 字面量能命中的单位。
///
/// 关键词上限取自 `limits`：CJK 分词会成倍放大数量，缺省 16 个，
/// 8 个会截掉中文问句的后半段。
fn extract_keywords(user_message: &str, hints: &[String], limits: &RetrievalLimits) -> Vec<String> {
    let mut kws: Vec<String> = Vec::new();

    // hints 优先，但 `@file:` 线索交由文件解析，不进检索词
    for hint in hints {
        let h = hint.trim();
        if h.is_empty() || h.starts_with(FILE_REF_PREFIX) {
            continue;
        }
        push_keyword(&mut kws, h.to_string());
    }

    for (is_cjk_run, run) in split_runs(user_message) {
        if is_cjk_run {
            // 单字 run 在 windows(2) 下自然产出 0 个 bigram：单字太泛，不作关键词
            for bigram in cjk_bigrams(&run) {
                if !is_func_bigram(&bigram) {
                    push_keyword(&mut kws, bigram);
                }
            }
        } else {
            for token in run.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                let t = token.trim();
                if t.chars().count() >= 3 && !is_en_stopword(t) {
                    push_keyword(&mut kws, t.to_string());
                }
            }
        }
    }

    kws.truncate(limits.max_keywords);
    kws
}

/// 追加关键词（去重，保持提问顺序）。
fn push_keyword(kws: &mut Vec<String>, kw: String) {
    if !kws.iter().any(|k| k == &kw) {
        kws.push(kw);
    }
}

/// 把文本切成「CJK 词串」与「非 CJK 词串」两类连续段。
///
/// 只在**字符类别变化**处断开：空白、标点既不是空白也不是 CJK，天然把两侧
/// 中文串隔开，因此 "上下文 检索" 是两段（`上下`/`文检`/`检索` 里的 "文检"
/// 跨空格粘成词即为错误），"bc好d" 切成 `bc` / `好` / `d` 三段（而不是因为
/// 整段字节长度够长就当一个关键词）。
///
/// 分隔符**不能跳过**（`continue`）——跳过后空格两侧的中文串会粘成一整段，
/// 产出的 bigram 里就包含跨分隔符的假词片。测试 `no_bigram_spans_a_separator` 守住这条。
fn split_runs(text: &str) -> Vec<(bool, String)> {
    let mut runs: Vec<(bool, String)> = Vec::new();
    for c in text.chars() {
        let is_cjk = is_cjk_char(c);
        match runs.last_mut() {
            Some((same, buf)) if *same == is_cjk => buf.push(c),
            _ => runs.push((is_cjk, c.to_string())),
        }
    }
    runs
}

/// CJK 词串的相邻二字组合，按出现顺序。
fn cjk_bigrams(run: &str) -> Vec<String> {
    let chars: Vec<char> = run.chars().collect();
    chars.windows(2).map(|w| w.iter().collect()).collect()
}

/// 是否纯功能字组成的 bigram。
fn is_func_bigram(bigram: &str) -> bool {
    bigram.chars().all(|c| CN_FUNC_CHARS.contains(&c))
}

/// 是否英文虚词（大小写不敏感）。
fn is_en_stopword(token: &str) -> bool {
    let lower = token.to_lowercase();
    EN_STOPWORDS.contains(&lower.as_str())
}

/// 内容里实际命中的关键词（保持关键词顺序，大小写折叠）。
fn matched_keywords(content: &str, keywords: &[String]) -> Vec<String> {
    let hay = content.to_lowercase();
    keywords
        .iter()
        .filter(|k| contains_ci(&hay, k))
        .cloned()
        .collect()
}

/// 在**已小写**的文本里做字面量包含判断，与 rg 的 `-i` 行为对齐。
fn contains_ci(lowered: &str, needle: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    !needle.is_empty() && lowered.contains(&needle)
}

/// 生成 relevance：说明该片段为何进入上下文，以及内容里真正命中的关键词。
///
/// 旧实现是一条固定文案「匹配用户问题或当前文件」，LLM 无法区分片段来源，
/// 也无从判断相关性（R3-1 第三条）。
fn relevance_for(role: ChunkRole, content: &str, keywords: &[String]) -> String {
    let matched = matched_keywords(content, keywords);
    let label = match role {
        ChunkRole::CurrentFile => "当前打开文件",
        ChunkRole::Referenced => "用户 @file 引用的文件",
        ChunkRole::SearchHit => "关键词检索命中",
    };
    if !matched.is_empty() {
        format!("{label}，命中关键词：{}", matched.join("、"))
    } else if role == ChunkRole::SearchHit {
        // 搜索确实命中过该文件，但裁剪后的片段里看不到关键词
        format!("{label}，但裁剪后的片段未包含任何关键词")
    } else {
        label.to_string()
    }
}

/// 是否应忽略的目录/文件名。
fn ignores(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".hg" | ".svn" | "node_modules" | "target" | "dist" | "build" | ".next" | ".xgent"
    )
}

/// 递归生成目录树文本。纯同步函数：全部 `std::fs` 调用都留在 async 之外，
/// 由 [`OnDemandContextProvider::tree_summary`] 经 `spawn_blocking` 调用。
fn collect_tree_lines(root: &Path, limits: &RetrievalLimits) -> Vec<String> {
    let mut lines = Vec::new();
    let mut count = 0usize;
    walk_tree(root, 0, limits, &mut lines, &mut count);
    lines
}

/// 降级搜索：递归遍历 + 字面量子串匹配。纯同步函数，同上。
fn collect_matching_files(
    root: &Path,
    keywords: &[String],
    limits: &RetrievalLimits,
) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let mut visited = 0usize;
    walk_search(root, root, keywords, 0, limits, &mut results, &mut visited);
    results
}

/// 目录树递归实现。`dir` 一路向下走，项目根不需要参与（无相对路径计算），
/// 因此不作为参数透传。
fn walk_tree(
    dir: &Path,
    depth: u32,
    limits: &RetrievalLimits,
    lines: &mut Vec<String>,
    count: &mut usize,
) {
    if depth > limits.tree_max_depth || *count >= limits.tree_max_entries {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *count >= limits.tree_max_entries {
            lines.push("...".into());
            return;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let path = entry.path();
        if ft.is_dir() {
            if ignores(&name_str) {
                continue;
            }
            lines.push(format!("{}{name_str}/", indent_str(depth)));
            *count += 1;
            walk_tree(&path, depth + 1, limits, lines, count);
        } else if ft.is_file() {
            lines.push(format!("{}{name_str}", indent_str(depth)));
            *count += 1;
        }
    }
}

fn indent_str(depth: u32) -> String {
    "  ".repeat(depth as usize)
}

fn walk_search(
    root: &Path,
    dir: &Path,
    keywords: &[String],
    depth: u32,
    limits: &RetrievalLimits,
    results: &mut Vec<PathBuf>,
    visited: &mut usize,
) {
    if depth > limits.tree_max_depth
        || results.len() >= limits.max_search_files
        || *visited > VISITED_NODE_LIMIT
    {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        if results.len() >= limits.max_search_files {
            return;
        }
        *visited += 1;
        let name = entry.file_name();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let path = entry.path();
        if ft.is_dir() {
            if ignores(&name.to_string_lossy()) {
                continue;
            }
            walk_search(root, &path, keywords, depth + 1, limits, results, visited);
        } else if ft.is_file()
            && let Ok(content) = std::fs::read_to_string(&path)
            && keywords.iter().any(|k| content.contains(k.as_str()))
        {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            if !results.contains(&rel) {
                results.push(rel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ContextQuery;

    /// 测试用缺省限额：与内置缺省一致，因此这些用例断言的是改动前的行为。
    const TEST_LIMITS: RetrievalLimits = RetrievalLimits {
        tree_max_depth: 4,
        tree_max_entries: 200,
        rg_timeout_secs: 10,
        max_search_files: 20,
        max_keywords: 16,
    };

    fn provider(root: &Path) -> OnDemandContextProvider {
        OnDemandContextProvider::new(root.to_path_buf())
    }

    fn kw(msg: &str) -> Vec<String> {
        extract_keywords(msg, &[], &TEST_LIMITS)
    }

    #[tokio::test]
    async fn tree_summary_includes_files_skips_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("src/main.rs"), "fn main(){}")
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("target"))
            .await
            .unwrap();
        tokio::fs::write(root.join("target/junk.rs"), "x")
            .await
            .unwrap();
        let p = provider(root);
        let tree = p.tree_summary(&p.limits()).await.unwrap();
        assert!(tree.contains("main.rs"));
        assert!(!tree.contains("junk.rs"), "应忽略 target");
    }

    /// A5：遍历逻辑在非 async 纯函数里，被 async 侧经 spawn_blocking 调用，
    /// 结果与迁移前一致。
    #[test]
    fn collect_tree_lines_is_sync_and_skips_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main(){}").unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("node_modules/junk.rs"), "x").unwrap();
        let lines = collect_tree_lines(root, &TEST_LIMITS);
        let text = lines.join("\n");
        assert!(text.contains("main.rs"), "目录树应含 src/main.rs：{text}");
        assert!(!text.contains("junk.rs"), "应忽略 node_modules：{text}");
    }

    /// A5：降级搜索的同步实现仍返回相对项目根的路径。
    #[test]
    fn collect_matching_files_is_sync_and_returns_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("crates/demo")).unwrap();
        std::fs::write(root.join("crates/demo/lib.rs"), "fn findme() {}").unwrap();
        std::fs::write(root.join("crates/demo/other.rs"), "nothing").unwrap();
        let hits = collect_matching_files(root, &["findme".to_string()], &TEST_LIMITS);
        assert_eq!(hits, vec![PathBuf::from("crates/demo/lib.rs")]);
    }

    #[tokio::test]
    async fn fallback_search_finds_matches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join("a.rs"), "fn findme() {}\n")
            .await
            .unwrap();
        tokio::fs::write(root.join("b.txt"), "no match here")
            .await
            .unwrap();
        let p = provider(root);
        let files = p.fallback_search(&["findme".into()], &p.limits()).await;
        assert!(files.iter().any(|f| f == Path::new("a.rs")));
        assert!(!files.iter().any(|f| f == Path::new("b.txt")));
    }

    #[tokio::test]
    async fn read_file_chunk_truncates_over_budget() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let long = "a".repeat(1000);
        tokio::fs::write(root.join("big.txt"), &long).await.unwrap();
        let p = provider(root);
        // 极小预算
        let chunk = p
            .read_file_chunk(Path::new("big.txt"), 5, ChunkRole::SearchHit, &[])
            .await
            .unwrap();
        assert!(chunk.content.contains("截断"));
        assert!(chunk.token_estimate <= 10);
    }

    #[tokio::test]
    async fn retrieve_returns_tree_and_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("src/main.rs"), "fn findme() {}\n")
            .await
            .unwrap();
        let p = provider(root);
        let q = ContextQuery {
            user_message: "findme 在哪里".into(),
            current_file: None,
            hints: vec![],
            max_tokens: 500,
        };
        let r = p.retrieve(&q).await;
        assert!(r.tree_summary.is_some());
        // 应搜到含 findme 的文件
        assert!(r.chunks.iter().any(|c| c.path.ends_with("main.rs")));
    }

    #[tokio::test]
    async fn retrieve_prioritizes_current_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join("cur.txt"), "current content")
            .await
            .unwrap();
        let p = provider(root);
        let q = ContextQuery {
            user_message: "看这个文件".into(),
            current_file: Some(PathBuf::from("cur.txt")),
            hints: vec![],
            max_tokens: 200,
        };
        let r = p.retrieve(&q).await;
        assert!(
            r.chunks
                .iter()
                .any(|c| c.content.contains("current content"))
        );
    }

    #[tokio::test]
    async fn retrieve_respects_token_budget() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 多个文件，每个较大
        for i in 0..5 {
            tokio::fs::write(root.join(format!("f{i}.txt")), &"word ".repeat(200))
                .await
                .unwrap();
        }
        let p = provider(root);
        let q = ContextQuery {
            user_message: "word".into(),
            current_file: None,
            hints: vec![],
            max_tokens: 100,
        };
        let r = p.retrieve(&q).await;
        assert!(
            r.total_tokens <= 100 + 50,
            "总 token 不应远超预算: {}",
            r.total_tokens
        );
    }

    #[test]
    fn extract_keywords_filters_short() {
        let kws = extract_keywords("fn the hello world abc", &[], &TEST_LIMITS);
        // "the" 是虚词、"fn" 不足 3 字符，"hello"/"world"/"abc" 保留
        assert!(kws.contains(&"hello".to_string()));
        assert!(kws.contains(&"world".to_string()));
        assert!(!kws.iter().any(|k| k == "fn"));
    }

    #[test]
    fn extract_keywords_includes_hints() {
        let kws = extract_keywords("msg", &["hint1".into()], &TEST_LIMITS);
        assert!(kws.contains(&"hint1".to_string()));
    }

    /// A1/A2：无空格中文问句整句作为一个关键词时任何源码都不可能包含它，
    /// 中文检索因此完全失效（R3-1）。切二字组合后每个词都是可命中的片段。
    #[test]
    fn extract_keywords_splits_cjk_into_bigrams() {
        let kws = kw("上下文检索");
        assert_eq!(
            kws,
            vec![
                "上下".to_string(),
                "下文".to_string(),
                "文检".to_string(),
                "检索".to_string()
            ]
        );
        assert!(
            !kws.iter().any(|k| k == "上下文检索"),
            "整句不应再作为一个关键词"
        );
    }

    /// A2：单字太泛，不成关键词。
    #[test]
    fn extract_keywords_drops_single_cjk_char() {
        let kws = kw("检索 图");
        assert!(kws.contains(&"检索".to_string()));
        assert!(!kws.iter().any(|k| k == "图"), "单字应丢弃：{kws:?}");
    }

    /// A5：源码级不变量——`std::fs` 只能出现在非 async 函数里。
    ///
    /// 「遍历不进异步线程」这个约束光靠行为测试测不出来（结果两种写法都一样），
    /// 但一旦有人在 `async fn` 体内直接遍历目录，就会占住 tokio 工作线程（NF-03）。
    /// 这条测试逐行核对：每个含 `std::fs` 的调用点，其所属函数头不得是 `async fn`。
    #[test]
    fn std_fs_calls_never_appear_in_async_fns() {
        let src = include_str!("on_demand.rs");
        let mut current_async = false;
        let mut current_header = String::new();
        let mut fs_calls = 0usize;
        for line in src.lines() {
            let t = line.trim_start();
            for prefix in ["fn ", "pub fn ", "async fn ", "pub async fn "] {
                if t.starts_with(prefix) {
                    current_async = prefix.starts_with("async ") || prefix.contains(" async ");
                    current_header = t.trim_end_matches('{').trim_end().to_string();
                    break;
                }
            }
            if line.contains("std::fs::") {
                fs_calls += 1;
                assert!(
                    !current_async,
                    "std::fs 调用出现在 async 函数内：{current_header}"
                );
            }
        }
        assert!(
            fs_calls > 0,
            "源码应仍有 std::fs 遍历调用，否则本测试失去意义"
        );
    }

    /// A5：async 侧确实经 spawn_blocking 调用这些同步函数。
    ///
    /// 与上条同源检查：只断言「同步函数存在」是不够的，调用点也必须走
    /// blocking pool，否则遍历仍在 tokio 工作线程上。
    #[test]
    fn traversal_helpers_are_awaited_via_spawn_blocking() {
        let src = include_str!("on_demand.rs");
        for helper in ["collect_tree_lines", "collect_matching_files"] {
            let spawned = src
                .lines()
                .any(|l| l.contains("spawn_blocking") && l.contains(helper));
            assert!(spawned, "{helper} 应在 spawn_blocking 闭包内被调用");
        }
    }

    /// A2：跨分隔符不粘连成词片。
    ///
    /// 空格若被「跳过」而非当作边界，"上下文 检索" 会粘成一整段并产出
    /// 假词片 `文检`（源码里永远不会出现 `文检` 与 `索在` 这类跨分隔符组合）。
    /// M7 式变异（只关闭标点切分、不关闭空白切分）逃过其余测试，正是被这条抓住。
    #[test]
    fn no_bigram_spans_a_separator() {
        assert_eq!(
            kw("上下文 检索"),
            vec!["上下".to_string(), "下文".to_string(), "检索".to_string()]
        );
        assert!(!kw("上下文 检索").contains(&"文检".to_string()));
        // 全角标点同样断开
        assert_eq!(kw("上下文。检索"), vec!["上下", "下文", "检索"]);
    }

    /// A5：长度按字符计。旧实现按字节，"bc好d"（6 字节）会整段成为关键词。
    #[test]
    fn extract_keywords_measures_chars_not_bytes() {
        assert!(kw("bc好d").is_empty(), "两段 2 字符与一段单字都应被丢弃");
        assert!(kw("ab").is_empty(), "2 字符 ASCII 应丢弃");
        // 2 字符 / 4 字节：按字节判长度会误留它，按字符应丢弃
        assert!(kw("éé").is_empty(), "按字符长度应丢弃：{:?}", kw("éé"));
    }

    /// A2：中英虚词都不进入关键词。
    #[test]
    fn extract_keywords_drops_function_words() {
        let kws = kw("how what the context window");
        assert!(kws.contains(&"context".to_string()));
        assert!(kws.contains(&"window".to_string()));
        for stop in ["how", "the", "what"] {
            assert!(!kws.iter().any(|k| k == stop), "英文虚词应丢弃：{kws:?}");
        }
        // "use" 一类既是虚词也是语言关键字，不作虚词处理：丢它会让人搜不到
        // `use` 语句本身，而它作为关键词的噪声远低于纯语法词。
        assert!(kw("use the context").contains(&"use".to_string()));

        let cn = kw("的是什么");
        assert!(cn.is_empty(), "纯功能字组成的 bigram 应丢弃：{cn:?}");
        let ru = kw("如何做");
        assert!(
            !ru.iter().any(|k| k == "如何"),
            "纯功能字 bigram 应丢弃：{ru:?}"
        );
    }

    /// A2：上限 16，保持提问顺序、去重。
    #[test]
    fn extract_keywords_caps_at_sixteen_in_order() {
        // 9 个汉字 → 8 个 bigram，再加 ASCII 词，总数超过 16
        let msg = "一二三四五六七八九 alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let kws = kw(msg);
        assert_eq!(kws.len(), TEST_LIMITS.max_keywords);
        assert_eq!(kws[0], "一二");
        assert_eq!(kws[1], "二三");
        assert_eq!(kws[7], "八九");
        assert_eq!(kws[8], "alpha");
        // 去重：重复词只留一次
        let dup = kw("检索 检索 检索");
        assert_eq!(dup, vec!["检索".to_string()]);
    }

    /// A3：`@file:` 线索不作为检索词。
    #[test]
    fn extract_keywords_excludes_file_ref_hint() {
        let kws = extract_keywords(
            "",
            &["@file:src/main.rs".into(), "ContextHub".into()],
            &TEST_LIMITS,
        );
        assert!(kws.contains(&"ContextHub".to_string()));
        assert!(
            !kws.iter().any(|k| k.contains(FILE_REF_PREFIX)),
            "@file: 线索不应成为关键词：{kws:?}"
        );
    }

    #[test]
    fn file_refs_parses_hint_paths() {
        let refs = file_refs(&[
            "@cursor".into(),
            "@file:src/main.rs".into(),
            "@file: crates/xgent_context/src/lib.rs ".into(),
            "@file:".into(),
        ]);
        assert_eq!(
            refs,
            vec![
                "src/main.rs".to_string(),
                "crates/xgent_context/src/lib.rs".to_string()
            ]
        );
    }

    /// A3：被 `@file:` 引用的文件内容进入上下文。
    #[tokio::test]
    async fn retrieve_includes_referenced_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("src/main.rs"), "fn referenced_only_here() {}")
            .await
            .unwrap();
        let p = provider(root);
        let q = ContextQuery {
            user_message: "随便问问".into(),
            current_file: None,
            hints: vec!["@file:src/main.rs".into()],
            max_tokens: 500,
        };
        let r = p.retrieve(&q).await;
        let hit = r
            .chunks
            .iter()
            .find(|c| c.path.ends_with("src/main.rs"))
            .expect("@file 引用的文件应进入上下文");
        assert!(
            hit.relevance.contains("引用"),
            "relevance: {}",
            hit.relevance
        );
    }

    /// A3：引用不存在的文件不是错误，检索照常返回。
    #[tokio::test]
    async fn retrieve_ignores_missing_referenced_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let p = provider(root);
        let q = ContextQuery {
            user_message: "找找看".into(),
            current_file: None,
            hints: vec!["@file:src/absent.rs".into()],
            max_tokens: 200,
        };
        let r = p.retrieve(&q).await;
        assert!(
            r.chunks
                .iter()
                .all(|c| !c.path.to_string_lossy().contains("absent")),
        );
    }

    /// A4：rg 以字面量、逐关键词 `-e` 方式匹配，绝不拼 `|` 正则。
    #[test]
    fn rg_args_matches_each_keyword_literally() {
        let args = rg_args(
            Path::new("/proj"),
            &["a+b".to_string(), "ContextHub".to_string()],
        );
        // 完整参数序列即判据：flags + 每个关键词一对 -e + 路径
        assert_eq!(
            args,
            vec![
                "--files-with-matches".to_string(),
                "-i".to_string(),
                "--fixed-strings".to_string(),
                "-e".to_string(),
                "a+b".to_string(),
                "-e".to_string(),
                "ContextHub".to_string(),
                "/proj".to_string(),
            ]
        );
        assert_eq!(args.iter().filter(|a| a.as_str() == "-e").count(), 2);
        assert_eq!(args.last().map(String::as_str), Some("/proj"));
        assert!(
            !args.iter().any(|a| a.contains('|')),
            "不应再出现拼接的正则交替：{args:?}"
        );
    }

    /// A4：含正则元字符的关键词按字面量命中（降级路径与 rg 语义一致）。
    #[test]
    fn collect_matching_files_treats_metachars_literally() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("lit.rs"), "let x = a+b;").unwrap();
        std::fs::write(root.join("other.rs"), "let x = ab;").unwrap();
        let hits = collect_matching_files(root, &["a+b".to_string()], &TEST_LIMITS);
        assert_eq!(hits, vec![PathBuf::from("lit.rs")]);
    }

    /// A7：relevance 报出内容里真正命中的关键词。
    #[test]
    fn relevance_lists_only_keywords_present_in_content() {
        let kws = vec![
            "findme".to_string(),
            "alpha".to_string(),
            "beta".to_string(),
        ];
        let rel = relevance_for(ChunkRole::SearchHit, "fn findme() { /* alpha */ }\n", &kws);
        assert!(rel.contains("findme") && rel.contains("alpha"), "{rel}");
        assert!(!rel.contains("beta"), "未命中的关键词不应出现：{rel}");
        assert!(
            !rel.contains("匹配用户问题或当前文件"),
            "旧固定文案应消失：{rel}"
        );
    }

    #[test]
    fn relevance_marks_current_file_and_unmatched_hits() {
        let kws = vec!["zzz".to_string()];
        let cur = relevance_for(ChunkRole::CurrentFile, "anything", &kws);
        assert!(cur.starts_with("当前打开文件"), "{cur}");
        let hit = relevance_for(ChunkRole::SearchHit, "anything", &kws);
        assert!(
            hit.contains("未包含任何关键词"),
            "搜索命中但片段里看不到关键词应如实说明：{hit}"
        );
    }

    /// A1：中文问句检索到内容含该词片的文件。
    #[tokio::test]
    async fn retrieve_finds_file_from_chinese_question() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(
            root.join("src/retrieval.rs"),
            "// 上下文检索的实现\nfn impl_here() {}",
        )
        .await
        .unwrap();
        tokio::fs::write(root.join("src/unrelated.rs"), "fn nothing() {}")
            .await
            .unwrap();
        let p = provider(root);
        let q = ContextQuery {
            user_message: "上下文检索在哪里实现".into(),
            current_file: None,
            hints: vec![],
            max_tokens: 500,
        };
        let r = p.retrieve(&q).await;
        assert!(
            r.chunks.iter().any(|c| c.path.ends_with("retrieval.rs")),
            "中文问句应命中含该词片的文件，chunks={:?}",
            r.chunks.iter().map(|c| &c.path).collect::<Vec<_>>()
        );
    }

    /// A1：关键词提不出东西时退化为「只有结构」，不报错。
    #[tokio::test]
    async fn retrieve_degrades_to_tree_when_no_keywords() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join("a.rs"), "fn a() {}")
            .await
            .unwrap();
        let p = provider(root);
        let q = ContextQuery {
            user_message: "的是什么".into(),
            current_file: None,
            hints: vec![],
            max_tokens: 200,
        };
        let r = p.retrieve(&q).await;
        assert!(r.tree_summary.is_some(), "目录树仍应返回");
        assert!(r.chunks.is_empty(), "无关键词时不应有检索片段");
    }

    /// A10：`tree_max_depth` 确实收窄目录树（缺省 4 时更深的文件应出现）。
    #[tokio::test]
    async fn configured_tree_depth_limit_shortens_summary() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("src/sub"))
            .await
            .unwrap();
        tokio::fs::write(root.join("src/top.rs"), "").await.unwrap();
        tokio::fs::write(root.join("src/sub/deep.rs"), "")
            .await
            .unwrap();

        let shallow = OnDemandContextProvider::with_limits(
            root.to_path_buf(),
            RetrievalLimits {
                tree_max_depth: 1,
                ..TEST_LIMITS
            },
        );
        let summary = shallow
            .tree_summary(&shallow.limits())
            .await
            .unwrap_or_default();
        assert!(
            summary.contains("top.rs"),
            "深度 1 仍应列出 src/top.rs: {summary}"
        );
        assert!(
            !summary.contains("deep.rs"),
            "深度 1 不应列出 src/sub/deep.rs: {summary}"
        );

        let full = provider(root);
        let summary = full.tree_summary(&full.limits()).await.unwrap_or_default();
        assert!(
            summary.contains("deep.rs"),
            "缺省深度 4 应列出更深的文件: {summary}"
        );
    }

    /// A10：`tree_max_entries` 收窄时目录树以省略号收尾。
    #[tokio::test]
    async fn configured_tree_entry_limit_stops_early() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..20 {
            tokio::fs::write(root.join(format!("f{i}.txt")), "")
                .await
                .unwrap();
        }
        let p = OnDemandContextProvider::with_limits(
            root.to_path_buf(),
            RetrievalLimits {
                tree_max_entries: 5,
                ..TEST_LIMITS
            },
        );
        let summary = p.tree_summary(&p.limits()).await.unwrap_or_default();
        assert!(
            summary.contains("..."),
            "条目上限命中应有省略标记: {summary}"
        );
        assert!(
            summary.lines().count() <= 6,
            "不应列出全部 20 个文件: {}",
            summary.lines().count()
        );
    }

    /// A10：`max_keywords` 收窄时保留问句前段（顺序不变）。
    #[test]
    fn configured_keyword_cap_keeps_first_in_order() {
        let limits = RetrievalLimits {
            max_keywords: 2,
            ..TEST_LIMITS
        };
        let kws = extract_keywords("上下文 检索 方案", &[], &limits);
        assert_eq!(
            kws,
            vec!["上下".to_string(), "下文".to_string()],
            "上限 2 应取问句最前两个词片（跨分隔符的 文检 不该出现）"
        );
        let dflt = extract_keywords("上下文 检索 方案", &[], &TEST_LIMITS);
        assert_eq!(
            dflt,
            vec!["上下", "下文", "检索", "方案"],
            "缺省上限 16 应保留全部词片"
        );
    }

    /// A10：`max_search_files` 收窄降级遍历返回的文件数。
    #[tokio::test]
    async fn configured_max_search_files_caps_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..5 {
            tokio::fs::write(root.join(format!("f{i}.txt")), "needle\n")
                .await
                .unwrap();
        }
        let p = OnDemandContextProvider::with_limits(
            root.to_path_buf(),
            RetrievalLimits {
                max_search_files: 1,
                ..TEST_LIMITS
            },
        );
        let files = p
            .fallback_search(&["needle".to_string()], &p.limits())
            .await;
        assert_eq!(files.len(), 1, "上限 1 应只返回一个文件: {files:?}");

        let dflt = provider(root);
        let many = dflt
            .fallback_search(&["needle".to_string()], &dflt.limits())
            .await;
        assert_eq!(many.len(), 5, "缺省上限 20 应全部返回: {many:?}");
    }

    /// R3-17：五个限额都必须真的被读取。
    ///
    /// `rg_timeout_secs` 无法用行为测试判别（要一个挂死的 rg），按 NF-03 的
    /// 既有做法改为源码级断言：字段出现在取值处，而不是只存在结构体里没人用。
    #[test]
    fn retrieval_limits_are_actually_read() {
        let src = include_str!("on_demand.rs");
        for field in [
            "tree_max_depth",
            "tree_max_entries",
            "rg_timeout_secs",
            "max_search_files",
            "max_keywords",
        ] {
            let read = format!("limits.{field}");
            assert!(
                src.contains(&read),
                "{field} 应在遍历/搜索/取词处被读取（找 {read}）"
            );
        }
    }
    // ---- R3-13：检索限额走共享句柄，改完下一次检索生效 ----

    /// 把 `[context] tree_max_depth` 从 4 改成 1，**下一次 retrieve** 的目录树就变浅。
    ///
    /// 判据走公开 trait `retrieve`（而不是直接调 tree_summary），因为要证的正是
    /// "运行时读的是句柄当前值"：若限额在构造时被拷成字段，第二次检索仍会给深树。
    #[tokio::test]
    async fn retrieval_limits_written_to_shared_handle_apply_next_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("src/sub"))
            .await
            .unwrap();
        tokio::fs::write(root.join("src/top.rs"), "").await.unwrap();
        tokio::fs::write(root.join("src/sub/deep.rs"), "")
            .await
            .unwrap();

        let limits = Arc::new(parking_lot::RwLock::new(RetrievalLimits {
            tree_max_depth: 4,
            ..TEST_LIMITS
        }));
        let p = OnDemandContextProvider::with_shared_limits(root.to_path_buf(), limits.clone());
        let q = ContextQuery {
            user_message: "无关关键词 zzq".into(),
            current_file: None,
            hints: vec![],
            max_tokens: 2_000,
        };

        let before = p.retrieve(&q).await;
        assert!(
            before
                .tree_summary
                .clone()
                .unwrap_or_default()
                .contains("deep.rs"),
            "缺省深度 4 应列出更深的文件: {:?}",
            before.tree_summary
        );

        limits.write().tree_max_depth = 1;
        let after = p.retrieve(&q).await;
        let summary = after.tree_summary.clone().unwrap_or_default();
        assert!(
            summary.contains("top.rs"),
            "深度 1 仍应列出 src/top.rs: {summary}"
        );
        assert!(
            !summary.contains("deep.rs"),
            "写入深度 1 后下一次检索不应再列出 src/sub/deep.rs: {summary}"
        );
    }

    /// 共享句柄下的 `max_keywords`：收窄到 1 后，第二个关键词的文件检索不到。
    ///
    /// 判据取自检索结果而不是 `limits()` 的回读——后者只证明句柄可写，
    /// 不证明检索读它。
    #[tokio::test]
    async fn keyword_cap_written_to_shared_handle_applies_next_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join("alpha.txt"), "needle_alpha 内容")
            .await
            .unwrap();
        tokio::fs::write(root.join("beta.txt"), "needle_beta 内容")
            .await
            .unwrap();
        let limits = Arc::new(parking_lot::RwLock::new(RetrievalLimits {
            max_keywords: 4,
            ..TEST_LIMITS
        }));
        let p = OnDemandContextProvider::with_shared_limits(root.to_path_buf(), limits.clone());
        let q = ContextQuery {
            user_message: "needle_alpha needle_beta".into(),
            current_file: None,
            hints: vec![],
            max_tokens: 2_000,
        };

        let paths = |r: &ContextResult| -> Vec<String> {
            r.chunks
                .iter()
                .map(|c| c.path.display().to_string())
                .collect()
        };
        let before = p.retrieve(&q).await;
        assert!(
            paths(&before).iter().any(|p| p.contains("beta.txt")),
            "两个关键词都参与检索时 beta.txt 应命中: {:?}",
            paths(&before)
        );

        limits.write().max_keywords = 1;
        let after = p.retrieve(&q).await;
        assert!(
            paths(&after).iter().any(|p| p.contains("alpha.txt")),
            "第一个关键词仍应命中 alpha.txt: {:?}",
            paths(&after)
        );
        assert!(
            !paths(&after).iter().any(|p| p.contains("beta.txt")),
            "上限收到 1 后第二个关键词不该再参与检索: {:?}",
            paths(&after)
        );
    }
}
