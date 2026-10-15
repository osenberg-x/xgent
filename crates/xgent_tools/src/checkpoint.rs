//! agent 文件改动的检查点存储与撤销（缺口 R3-11）。
//!
//! 写工具落盘**之前**把改前内容记进 `<project>/.xgent/checkpoints/`，按「会话 + 轮次」
//! 组织成清单；用户可撤销最近一轮的文件改动。设计约束（见正式 Spec）：
//!
//! - 快照先于写入持久化：写入中途失败也留得下改前状态。
//! - 撤销只恢复「当前内容确实等于 agent 当时写入内容」的文件——哈希对不上就跳过并上报，
//!   绝不覆盖用户后来的编辑，也不重建用户删掉的文件。
//! - 记不下快照时**不阻塞写入**（撤销机制不能反过来弄坏主功能），但该轮会被标成
//!   「有文件没有检查点」，撤销汇报如实说明。
//!
//! 哈希只用来判断「内容是否还是我写的那份」，不是安全摘要（见 [`fnv1a64`]）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

use crate::atomic::atomic_write_bytes;
use crate::tool::ToolCtx;

/// 每会话保留的轮次上限（超出即淘汰最旧清单与其 blob）。
pub const DEFAULT_MAX_TURNS: usize = 20;

/// 检查点根目录名（位于 `<project>/.xgent/` 下，检索层本就忽略 `.xgent`）。
pub const CHECKPOINT_DIR: &str = "checkpoints";

/// 一轮的标识：会话号 + 轮号。
///
/// 会话号取 `xgent_core::ids::SessionId` 的**数值**，不得由 `session:N` 字符串反解
/// （既有 `restore()` 会把 `session:123` 解析成 0，这个坑不带进检查点键）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TurnKey {
    /// 会话号
    pub session: u64,
    /// 轮号（每会话从 1 起，单调递增）
    pub turn: u64,
}

/// 条目的改前状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// 写入前文件不存在（撤销 = 删除）
    Created,
    /// 写入前文件已存在（撤销 = 恢复 blob）
    Modified,
}

/// 一次写入的检查点条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointEntry {
    /// 相对项目根的路径，`/` 分隔
    pub path: String,
    /// 改前状态
    pub kind: EntryKind,
    /// 改前内容哈希（`Created` 为 None）
    #[serde(default)]
    pub pre_hash: Option<String>,
    /// agent 写入后应得的内容哈希；写入成功回填后才有值，缺失即不可验证
    #[serde(default)]
    pub post_hash: Option<String>,
    /// 改前内容 blob（相对检查点根目录）；`Created` 为 None
    #[serde(default)]
    pub blob: Option<String>,
}

/// 一轮的清单。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TurnManifest {
    /// 会话号
    pub session: u64,
    /// 轮号
    pub turn: u64,
    /// 条目（按写入顺序）
    #[serde(default)]
    pub entries: Vec<CheckpointEntry>,
    /// 本轮没能记下检查点的文件（相对路径）
    #[serde(default)]
    pub uncovered: Vec<String>,
}

/// 撤销时被跳过的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// 当前内容不是 agent 写进去的那份（用户或构建改过）
    ExternallyModified,
    /// 文件已被删除，不重建
    DeletedByUser,
    /// 改前内容 blob 丢失
    BlobLost,
    /// 没有写入后校验值，无法判断这份内容是谁写的
    Unverified,
}

/// 一次撤销的结果。
#[derive(Debug, Clone, Default)]
pub struct UndoReport {
    /// 被撤销的轮次；None 表示没有可撤销的记录
    pub turn: Option<TurnKey>,
    /// 恢复为改前内容的文件
    pub restored: Vec<String>,
    /// 删除的（本轮由 agent 新建的）文件
    pub removed: Vec<String>,
    /// agent 新建、用户已自行删除：无需动作
    pub already_absent: Vec<String>,
    /// 跳过的文件与原因
    pub skipped: Vec<(String, SkipReason)>,
    /// 本轮没有检查点的文件
    pub uncovered: Vec<String>,
    /// 撤销过程中写盘失败的文件
    pub failed: Vec<String>,
}

impl UndoReport {
    /// 是否什么都没发生（用于「没有可撤销的改动」提示）。
    pub fn did_nothing(&self) -> bool {
        self.turn.is_none()
            && self.restored.is_empty()
            && self.removed.is_empty()
            && self.already_absent.is_empty()
            && self.skipped.is_empty()
            && self.failed.is_empty()
    }
}

/// 检查点存储。
///
/// 单进程内串行使用（写工具是 `Exclusive`，一轮一个对话），内部一把异步锁覆盖
/// 「读清单 → 改 → 原子回写」，避免同轮多次写入互相覆盖清单。
pub struct CheckpointStore {
    /// 项目根（绝对路径）
    project_root: PathBuf,
    /// 检查点根目录（绝对路径）
    root: PathBuf,
    /// 保留轮数上限
    max_turns: usize,
    /// 可变状态
    state: Mutex<StoreState>,
}

#[derive(Debug, Default)]
struct StoreState {
    /// 当前轮（None = 未开始任何轮）
    current: Option<TurnKey>,
    /// 每会话下一个轮号计数（轮号只用于区分清单，不要求连续）
    next: HashMap<u64, u64>,
    /// 各轮次中没记上检查点的文件（清单写不下去时的兜底记录，按轮存）
    uncovered: HashMap<TurnKey, Vec<String>>,
}

impl CheckpointStore {
    /// 按项目根构造（检查点落在 `<project>/.xgent/checkpoints/`）。
    pub fn new(project_root: PathBuf) -> Self {
        let root = project_root.join(".xgent").join(CHECKPOINT_DIR);
        Self::with_root(project_root, root)
    }

    /// 指定检查点根构造（测试注入临时目录用）。
    pub fn with_root(project_root: PathBuf, root: PathBuf) -> Self {
        Self {
            project_root,
            root,
            max_turns: DEFAULT_MAX_TURNS,
            state: Mutex::new(StoreState::default()),
        }
    }

    /// 项目根。
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// 检查点根。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 开始新的一轮：设定当前轮号并做保留淘汰。
    ///
    /// **只有真正记到东西的轮次才成为「可撤销的一轮」**——清单是首次记录条目时
    /// 才创建的，本轮若一次写入都没发生（纯问答、只读工具），撤销指针不会停在
    /// 这一轮上，用户按撤销就不会出现「点了没反应」。
    pub async fn begin_turn(&self, session: u64) -> TurnKey {
        // 轮号从已有清单里探测一次（**在锁外**做 IO，别持锁跨 await），
        // 避免进程重启后从 1 重新计数、覆盖上一进程留下的轮次清单。
        if !self.state.lock().await.next.contains_key(&session) {
            let existing = self.max_turn_on_disk(session).await;
            self.state.lock().await.next.insert(session, existing);
        }
        let mut st = self.state.lock().await;
        let base = st.next.get(&session).copied().unwrap_or(0);
        let turn = base + 1;
        st.next.insert(session, turn);
        let key = TurnKey { session, turn };
        st.current = Some(key);
        drop(st);
        key
    }

    /// 当前轮（未开始时为 None）。
    pub async fn current_turn(&self) -> Option<TurnKey> {
        self.state.lock().await.current
    }

    /// 本轮是否有过「记不上检查点」的写入（用于撤销汇报如实说明）。
    pub async fn uncovered_count(&self) -> usize {
        let st = self.state.lock().await;
        match st.current {
            Some(k) => st.uncovered.get(&k).map(|v| v.len()).unwrap_or(0),
            None => 0,
        }
    }

    /// 在写入前记录改前状态。
    ///
    /// `targets` 由调用方（执行器）从 `Tool::write_targets` 取得；项目根之外的路径
    /// 一律忽略。记录失败不返回错误：写入照常进行，失败路径进本轮的 uncovered 清单。
    pub async fn snapshot_before_write(&self, targets: &[PathBuf], ctx: &ToolCtx) {
        if targets.is_empty() {
            return;
        }
        let key = {
            let st = self.state.lock().await;
            match st.current {
                Some(k) => k,
                // 没有开始的轮（独立使用执行器的测试等）：记到 0 号会话 0 号轮，
                // 仍然可撤销，不会因为「没调 begin_turn」就丢掉保护。
                None => TurnKey {
                    session: 0,
                    turn: 0,
                },
            }
        };
        for target in targets {
            let Some(rel) = self.relative(target, &ctx.project_root) else {
                continue;
            };
            match self.record_entry(key, &rel, target).await {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!(path = %rel, error = %e, "检查点记录失败，本轮该文件不可撤销");
                    // 清单能写就写进清单；清单也写不了时至少留在内存，
                    // 否则撤销会误报「没有可撤销的改动」
                    let mut st = self.state.lock().await;
                    let list = st.uncovered.entry(key).or_default();
                    if !list.contains(&rel) {
                        list.push(rel.clone());
                    }
                    drop(st);
                    let _ = self.mark_uncovered(key, &rel).await;
                }
            }
        }
    }

    /// 写入成功后回填「agent 写进去的那份」哈希，作为撤销时的比对凭据。
    pub async fn finalize_after_write(&self, targets: &[PathBuf], ctx: &ToolCtx) {
        if targets.is_empty() {
            return;
        }
        let key = self.state.lock().await.current;
        let Some(key) = key else { return };
        for target in targets {
            let Some(rel) = self.relative(target, &ctx.project_root) else {
                continue;
            };
            let Ok(bytes) = tokio::fs::read(target).await else {
                continue;
            };
            let hash = Some(fnv1a64(&bytes));
            let mut manifest = match self.load_manifest(key).await {
                Ok(Some(m)) => m,
                _ => continue,
            };
            let mut touched = false;
            for entry in manifest.entries.iter_mut().rev() {
                if entry.path == rel && entry.post_hash.is_none() {
                    entry.post_hash = hash.clone();
                    touched = true;
                    break;
                }
            }
            if touched {
                let _ = self.write_manifest(&manifest).await;
            }
        }
    }

    /// 撤销指定会话最近的一轮。
    pub async fn undo_last_turn(&self, session: u64) -> UndoReport {
        // 最新的「有记录的轮」= 磁盘清单与内存 uncovered 的较大轮号
        let disk_newest = self.newest_on_disk(session).await;
        let mem_newest = {
            let mut st = self.state.lock().await;
            let best = st
                .uncovered
                .iter()
                .filter(|(k, v)| k.session == session && !v.is_empty())
                .map(|(k, _)| k.turn)
                .max();
            let key = match (disk_newest.map(|d| d.turn), best) {
                (Some(d), Some(m)) => TurnKey {
                    session,
                    turn: d.max(m),
                },
                (Some(d), None) => TurnKey { session, turn: d },
                (None, Some(m)) => TurnKey { session, turn: m },
                (None, None) => return UndoReport::default(),
            };
            (key, st.uncovered.remove(&key).unwrap_or_default())
        };
        let (key, mem_uncovered) = mem_newest;
        self.undo_turn(key, mem_uncovered).await
    }

    /// 该会话是否留有可撤销的轮次。
    pub async fn has_undoable_turn(&self, session: u64) -> bool {
        if self.newest_on_disk(session).await.is_some() {
            return true;
        }
        let st = self.state.lock().await;
        st.uncovered
            .iter()
            .any(|(k, v)| k.session == session && !v.is_empty())
    }

    async fn undo_turn(&self, key: TurnKey, mem_uncovered: Vec<String>) -> UndoReport {
        let mut report = UndoReport {
            turn: Some(key),
            uncovered: mem_uncovered,
            ..Default::default()
        };
        report.turn = Some(key);
        let manifest = match self.load_manifest(key).await {
            Ok(Some(m)) => Some(m),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(error = %e, "读取检查点清单失败");
                None
            }
        };
        let Some(manifest) = manifest else {
            // 只有「没记上」的轮次：如实上报，不去偷偷撤更早的一轮
            report.uncovered.sort();
            report.uncovered.dedup();
            return report;
        };
        if !manifest.uncovered.is_empty() {
            report.uncovered.extend(manifest.uncovered.iter().cloned());
            report.uncovered.sort();
            report.uncovered.dedup();
        }
        // 倒序回放：同一文件被写多次时，最后一条先恢复，逐条回到轮次开始前的状态
        for entry in manifest.entries.iter().rev() {
            self.restore_entry(entry, &mut report).await;
        }
        self.delete_turn(key).await;
        report
    }

    async fn restore_entry(&self, entry: &CheckpointEntry, report: &mut UndoReport) {
        let full = self.project_root.join(&entry.path);
        match entry.kind {
            EntryKind::Created => {
                let Some(cur) = Self::read_if_exists(&full).await else {
                    report.already_absent.push(entry.path.clone());
                    return;
                };
                match &entry.post_hash {
                    Some(h) if fnv1a64(&cur) != *h => {
                        report
                            .skipped
                            .push((entry.path.clone(), SkipReason::ExternallyModified));
                        return;
                    }
                    None => {
                        report
                            .skipped
                            .push((entry.path.clone(), SkipReason::Unverified));
                        return;
                    }
                    _ => {}
                }
                match tokio::fs::remove_file(&full).await {
                    Ok(()) => report.removed.push(entry.path.clone()),
                    Err(e) => {
                        tracing::warn!(path = %entry.path, error = %e, "撤销删除失败");
                        report.failed.push(entry.path.clone());
                    }
                }
            }
            EntryKind::Modified => {
                let cur = match Self::read_if_exists(&full).await {
                    Some(c) => c,
                    None => {
                        report
                            .skipped
                            .push((entry.path.clone(), SkipReason::DeletedByUser));
                        return;
                    }
                };
                match &entry.post_hash {
                    Some(h) if fnv1a64(&cur) == *h => {}
                    Some(_) => {
                        report
                            .skipped
                            .push((entry.path.clone(), SkipReason::ExternallyModified));
                        return;
                    }
                    None => {
                        report
                            .skipped
                            .push((entry.path.clone(), SkipReason::Unverified));
                        return;
                    }
                }
                let Some(blob) = &entry.blob else {
                    report
                        .skipped
                        .push((entry.path.clone(), SkipReason::BlobLost));
                    return;
                };
                let bytes = match tokio::fs::read(self.root.join(blob)).await {
                    Ok(b) => b,
                    Err(_) => {
                        report
                            .skipped
                            .push((entry.path.clone(), SkipReason::BlobLost));
                        return;
                    }
                };
                // blob 自身可能已被改（理论上不该发生）：改前哈希对得上才写回
                if let Some(pre) = &entry.pre_hash
                    && fnv1a64(&bytes) != *pre
                {
                    report
                        .skipped
                        .push((entry.path.clone(), SkipReason::BlobLost));
                    return;
                }
                match atomic_write_bytes(&full, &bytes).await {
                    Ok(()) => report.restored.push(entry.path.clone()),
                    Err(e) => {
                        tracing::warn!(path = %entry.path, error = %e, "撤销恢复失败");
                        report.failed.push(entry.path.clone());
                    }
                }
            }
        }
    }

    /// 记录单个文件条目的改前状态：blob 先落盘，再更新清单。
    async fn record_entry(&self, key: TurnKey, rel: &str, full: &Path) -> std::io::Result<()> {
        let pre = match tokio::fs::read(full).await {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        // 先读清单拿到序号：blob 名里的序号决定「同轮同文件的多次写入」是否互相覆盖
        let mut manifest = self
            .load_manifest(key)
            .await?
            .unwrap_or_else(|| TurnManifest {
                session: key.session,
                turn: key.turn,
                entries: Vec::new(),
                uncovered: Vec::new(),
            });
        let index = manifest.entries.len();
        let (kind, pre_hash, blob_name) = match &pre {
            Some(bytes) => (
                EntryKind::Modified,
                Some(fnv1a64(bytes)),
                Some(self.blob_name(key, rel, index)),
            ),
            None => (EntryKind::Created, None, None),
        };
        // blob 必须先于目标写入落盘：这样即便写入中途失败，改前状态也已持久化
        if let (EntryKind::Modified, Some(bytes), Some(name)) = (kind, pre.as_ref(), &blob_name) {
            self.write_blob(name, bytes).await?;
        }
        let is_new_turn = manifest.entries.is_empty() && manifest.uncovered.is_empty();
        manifest.entries.push(CheckpointEntry {
            path: rel.to_string(),
            kind,
            pre_hash,
            post_hash: None,
            blob: blob_name,
        });
        self.write_manifest(&manifest).await?;
        // 保留淘汰挂在「本轮第一次留下记录」时：一轮一次，不必每次写入都扫目录，
        // 也不会在 begin_turn 时提前把刚要写入的那轮算进窗口
        if is_new_turn {
            self.prune(key.session).await;
        }
        Ok(())
    }

    /// 把某个文件标为本轮「没记上检查点」。
    async fn mark_uncovered(&self, key: TurnKey, rel: &str) -> std::io::Result<()> {
        let mut manifest = self
            .load_manifest(key)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| TurnManifest {
                session: key.session,
                turn: key.turn,
                ..Default::default()
            });
        if !manifest.uncovered.iter().any(|p| p == rel) {
            manifest.uncovered.push(rel.to_string());
        }
        self.write_manifest(&manifest).await
    }

    fn blob_name(&self, key: TurnKey, rel: &str, index: usize) -> String {
        // 序号必须进名字：同一轮里同一文件被写两次时，两条改前内容不同，
        // 只用路径哈希会让后写的 blob 覆盖前一条，撤销就只能退到倒数第二个状态。
        format!(
            "blobs/{}/{}-{}-{}.bin",
            key.session,
            key.turn,
            index,
            fnv1a64(rel.as_bytes())
        )
    }

    async fn write_blob(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        atomic_write_bytes(&path, bytes).await
    }

    fn manifest_path(&self, key: TurnKey) -> PathBuf {
        self.root
            .join("turns")
            .join(key.session.to_string())
            .join(format!("{}.json", key.turn))
    }

    async fn load_manifest(&self, key: TurnKey) -> std::io::Result<Option<TurnManifest>> {
        match tokio::fs::read(self.manifest_path(key)).await {
            Ok(b) => match serde_json::from_slice::<TurnManifest>(&b) {
                Ok(m) => Ok(Some(m)),
                Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn write_manifest(&self, manifest: &TurnManifest) -> std::io::Result<()> {
        let key = TurnKey {
            session: manifest.session,
            turn: manifest.turn,
        };
        let path = self.manifest_path(key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_vec_pretty(manifest)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        atomic_write_bytes(&path, &json).await
    }

    /// 删除某轮的清单与 blob（撤销后或淘汰时）。
    async fn delete_turn(&self, key: TurnKey) {
        if let Ok(Some(m)) = self.load_manifest(key).await {
            for entry in &m.entries {
                if let Some(blob) = &entry.blob {
                    let _ = tokio::fs::remove_file(self.root.join(blob)).await;
                }
            }
        }
        let _ = tokio::fs::remove_file(self.manifest_path(key)).await;
    }

    /// 保留淘汰：只留最近 `max_turns` 轮。
    async fn prune(&self, session: u64) {
        let turns = self.turns_on_disk(session).await;
        if turns.len() <= self.max_turns {
            return;
        }
        let keep = turns.len() - self.max_turns;
        for turn in turns.into_iter().take(keep) {
            self.delete_turn(TurnKey { session, turn }).await;
        }
    }

    /// 磁盘上该会话的轮号（升序）。
    async fn turns_on_disk(&self, session: u64) -> Vec<u64> {
        let dir = self.root.join("turns").join(session.to_string());
        let Ok(mut rd) = tokio::fs::read_dir(&dir).await else {
            return Vec::new();
        };
        let mut turns = Vec::new();
        while let Ok(Some(e)) = rd.next_entry().await {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(stem) = name.strip_suffix(".json")
                && let Ok(t) = stem.parse::<u64>()
            {
                turns.push(t);
            }
        }
        turns.sort_unstable();
        turns
    }

    async fn max_turn_on_disk(&self, session: u64) -> u64 {
        self.turns_on_disk(session)
            .await
            .last()
            .copied()
            .unwrap_or(0)
    }

    async fn newest_on_disk(&self, session: u64) -> Option<TurnKey> {
        self.turns_on_disk(session)
            .await
            .last()
            .copied()
            .map(|turn| TurnKey { session, turn })
    }

    /// 相对项目根的路径（`/` 分隔）；项目根之外返回 None。
    fn relative(&self, target: &Path, project_root: &Path) -> Option<String> {
        // 缺省回落到自身的 project_root；不能对临时值取引用，故克隆成拥有者
        let owned;
        let root: &Path = if project_root.as_os_str().is_empty() {
            &self.project_root
        } else {
            owned = project_root.to_path_buf();
            &owned
        };
        let rel = target.strip_prefix(root).ok()?;
        if rel.as_os_str().is_empty() || rel.starts_with("..") {
            return None;
        }
        Some(rel.to_string_lossy().replace('\\', "/"))
    }

    async fn read_if_exists(path: &Path) -> Option<Vec<u8>> {
        tokio::fs::read(path).await.ok()
    }
}

/// FNV-1a 64 位散列（十六进制字符串）。
///
/// 用途是判断「文件内容是否还是 agent 当时写的那份」，不是抗碰撞的安全摘要；
/// 项目里为它引一个新依赖不值得（见本轮正式 Spec 的非目标）。
pub fn fnv1a64(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    /// 构造指向临时目录的存储（不碰真实项目与 `~/.xgent`）。
    fn store(dir: &Path) -> CheckpointStore {
        CheckpointStore::new(dir.to_path_buf())
    }

    async fn write(path: &Path, s: &str) {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(path, s).await.unwrap();
    }

    async fn read(path: &Path) -> String {
        tokio::fs::read_to_string(path).await.unwrap_or_default()
    }

    fn ctx(dir: &Path) -> ToolCtx {
        ToolCtx {
            project_root: dir.to_path_buf(),
            tool_policy: Default::default(),
        }
    }

    #[test]
    fn fnv1a_is_stable_and_distinguishing() {
        assert_eq!(fnv1a64(b"abc"), fnv1a64(b"abc"));
        assert_ne!(fnv1a64(b"abc"), fnv1a64(b"abd"));
        assert_eq!(fnv1a64(b"").len(), 16);
    }

    #[tokio::test]
    async fn snapshot_records_pre_state_and_no_backup_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("a.rs"), "one").await;
        let st = store(root);
        st.begin_turn(7).await;
        st.snapshot_before_write(&[root.join("a.rs")], &ctx(root))
            .await;
        // 改前内容已落盘（此时还没执行写入）
        let key = TurnKey {
            session: 7,
            turn: 1,
        };
        let m = st.load_manifest(key).await.unwrap().unwrap();
        assert_eq!(m.entries.len(), 1);
        assert_eq!(m.entries[0].path, "a.rs");
        assert_eq!(m.entries[0].kind, EntryKind::Modified);
        assert_eq!(m.entries[0].post_hash, None, "写入前不应有 post_hash");
        let blob = m.entries[0].blob.clone().unwrap();
        assert_eq!(read(&st.root().join(&blob)).await, "one");
        // 不再产生 .xgent-bak
        let mut rd = tokio::fs::read_dir(root).await.unwrap();
        while let Some(e) = rd.next_entry().await.unwrap() {
            let n = e.file_name().to_string_lossy().to_string();
            assert!(!n.ends_with(".xgent-bak"), "不该留备份文件: {n}");
        }
    }

    #[tokio::test]
    async fn created_file_is_marked_created_and_has_no_blob() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let st = store(root);
        st.begin_turn(1).await;
        st.snapshot_before_write(&[root.join("new.rs")], &ctx(root))
            .await;
        let m = st
            .load_manifest(TurnKey {
                session: 1,
                turn: 1,
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(m.entries[0].kind, EntryKind::Created);
        assert!(m.entries[0].blob.is_none());
        assert!(m.entries[0].pre_hash.is_none());
    }

    #[tokio::test]
    async fn three_writes_in_a_single_turn_undo_to_pre_turn_content() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("a.rs");
        write(&file, "c0").await;
        let st = store(root);
        let c = ctx(root);
        st.begin_turn(1).await;
        for next in ["c1", "c2", "c3"] {
            st.snapshot_before_write(std::slice::from_ref(&file), &c)
                .await;
            write(&file, next).await;
            st.finalize_after_write(std::slice::from_ref(&file), &c)
                .await;
        }
        let report = st.undo_last_turn(1).await;
        // 同轮三次写入 = 三次恢复（倒序逐条回退），最终回到轮次开始前的 c0
        assert_eq!(report.restored, vec!["a.rs"; 3], "每个条目各恢复一次");
        assert_eq!(read(&file).await, "c0", "倒序回放到轮次开始前的内容");
        // 清单与 blob 已被清掉
        assert!(
            st.load_manifest(TurnKey {
                session: 1,
                turn: 1
            })
            .await
            .unwrap()
            .is_none()
        );
        assert!(!st.has_undoable_turn(1).await);
    }

    #[tokio::test]
    async fn externally_modified_file_is_skipped_and_others_still_restore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a = root.join("a.rs");
        let b = root.join("b.rs");
        write(&a, "a0").await;
        write(&b, "b0").await;
        let st = store(root);
        let c = ctx(root);
        st.begin_turn(1).await;
        st.snapshot_before_write(&[a.clone(), b.clone()], &c).await;
        write(&a, "agent").await;
        write(&b, "agent").await;
        st.finalize_after_write(&[a.clone(), b.clone()], &c).await;
        // 用户只改了 a
        write(&a, "user").await;

        let report = st.undo_last_turn(1).await;
        assert_eq!(
            report.restored,
            vec!["b.rs".to_string()],
            "其余文件照常恢复"
        );
        assert_eq!(
            report.skipped,
            vec![("a.rs".to_string(), SkipReason::ExternallyModified)]
        );
        assert_eq!(read(&a).await, "user", "不得覆盖用户编辑");
        assert_eq!(read(&b).await, "b0");
    }

    #[tokio::test]
    async fn user_deleted_file_is_not_resurrected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a = root.join("old.rs");
        write(&a, "old").await;
        let st = store(root);
        let c = ctx(root);
        st.begin_turn(1).await;
        st.snapshot_before_write(std::slice::from_ref(&a), &c).await;
        write(&a, "agent").await;
        st.finalize_after_write(std::slice::from_ref(&a), &c).await;
        tokio::fs::remove_file(&a).await.unwrap();

        let report = st.undo_last_turn(1).await;
        assert_eq!(
            report.skipped,
            vec![("old.rs".to_string(), SkipReason::DeletedByUser)]
        );
        assert_eq!(read(&a).await, "", "不该重建用户删掉的文件");
        assert!(report.restored.is_empty());
    }

    #[tokio::test]
    async fn created_file_is_removed_on_undo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let new_file = root.join("brand.rs");
        let st = store(root);
        let c = ctx(root);
        st.begin_turn(1).await;
        st.snapshot_before_write(std::slice::from_ref(&new_file), &c)
            .await;
        write(&new_file, "hello").await;
        st.finalize_after_write(std::slice::from_ref(&new_file), &c)
            .await;

        let report = st.undo_last_turn(1).await;
        assert_eq!(report.removed, vec!["brand.rs".to_string()]);
        assert!(!new_file.exists());
    }

    #[tokio::test]
    async fn created_then_deleted_by_user_reports_already_absent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let new_file = root.join("n.rs");
        let st = store(root);
        let c = ctx(root);
        st.begin_turn(1).await;
        st.snapshot_before_write(std::slice::from_ref(&new_file), &c)
            .await;
        write(&new_file, "x").await;
        st.finalize_after_write(std::slice::from_ref(&new_file), &c)
            .await;
        tokio::fs::remove_file(&new_file).await.unwrap();

        let report = st.undo_last_turn(1).await;
        assert_eq!(report.already_absent, vec!["n.rs".to_string()]);
        assert!(report.skipped.is_empty());
    }

    #[tokio::test]
    async fn unwritable_checkpoint_dir_degrades_without_blocking_the_write() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 把检查点根占成一个普通文件，任何 create_dir_all 都会失败
        let blocked = root.join("ck");
        tokio::fs::write(&blocked, b"x").await.unwrap();
        let st = CheckpointStore::with_root(root.to_path_buf(), blocked.clone());
        let c = ctx(root);
        st.begin_turn(1).await;
        st.snapshot_before_write(&[root.join("w.rs")], &c).await;
        // 写入本身照常成功（由调用方负责写，这里只验证记不上时进了 uncovered）
        write(&root.join("w.rs"), "content").await;
        assert_eq!(st.uncovered_count().await, 1, "应记下未覆盖文件数");

        let report = st.undo_last_turn(1).await;
        assert_eq!(report.uncovered, vec!["w.rs".to_string()]);
        assert!(!report.did_nothing(), "有 uncovered 就不算「什么都没做」");
        assert_eq!(read(&root.join("w.rs")).await, "content", "不该被动过");
    }

    #[tokio::test]
    async fn retention_keeps_only_the_newest_turns() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("r.rs");
        write(&file, "v0").await;
        let st = store(root);
        let c = ctx(root);
        for i in 0..25u64 {
            st.begin_turn(5).await;
            st.snapshot_before_write(std::slice::from_ref(&file), &c)
                .await;
            write(&file, &format!("v{}", i + 1)).await;
            st.finalize_after_write(std::slice::from_ref(&file), &c)
                .await;
        }
        let turns = st.turns_on_disk(5).await;
        assert_eq!(turns.len(), 20, "只保留最近 20 轮");
        assert_eq!(*turns.first().unwrap(), 6);
        assert_eq!(*turns.last().unwrap(), 25);
        // 撤销最近一轮仍有效
        let report = st.undo_last_turn(5).await;
        assert_eq!(report.restored, vec!["r.rs".to_string()]);
        assert_eq!(read(&file).await, "v24");
    }

    #[tokio::test]
    async fn undo_is_scoped_to_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("s.rs");
        write(&file, "base").await;
        let st = store(root);
        let c = ctx(root);
        // 会话 1 写一轮
        st.begin_turn(1).await;
        st.snapshot_before_write(std::slice::from_ref(&file), &c)
            .await;
        write(&file, "from-session-1").await;
        st.finalize_after_write(std::slice::from_ref(&file), &c)
            .await;
        // 会话 2 什么都没写
        st.begin_turn(2).await;

        let report = st.undo_last_turn(2).await;
        assert!(report.did_nothing(), "换会话不该撤到别人的轮次");
        assert_eq!(read(&file).await, "from-session-1");

        let report = st.undo_last_turn(1).await;
        assert_eq!(report.restored, vec!["s.rs".to_string()]);
        assert_eq!(read(&file).await, "base");
    }

    /// 空轮次之后仍不得复用轮号：否则后一轮的清单会**覆盖**前一分钟的清单，
    /// 用户撤销时才发现有一轮根本撤不回来。
    #[tokio::test]
    async fn empty_turn_does_not_reuse_turn_number() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("e.rs");
        write(&file, "v0").await;
        let st = store(root);
        let c = ctx(root);

        st.begin_turn(21).await;
        st.snapshot_before_write(std::slice::from_ref(&file), &c)
            .await;
        write(&file, "v1").await;
        st.finalize_after_write(std::slice::from_ref(&file), &c)
            .await;

        // 中间一轮什么都没写（纯问答）
        let empty = st.begin_turn(21).await;
        // 再一轮真实写入
        let third = st.begin_turn(21).await;
        assert_eq!(
            (empty.turn, third.turn),
            (2, 3),
            "轮号必须单调，空轮次不能把下一轮压回同一个号"
        );
        st.snapshot_before_write(std::slice::from_ref(&file), &c)
            .await;
        write(&file, "v2").await;
        st.finalize_after_write(std::slice::from_ref(&file), &c)
            .await;

        let turns = st.turns_on_disk(21).await;
        assert_eq!(turns, vec![1, 3], "两轮写入各自一份清单，互不覆盖");
        assert_eq!(read(&file).await, "v2");
        assert_eq!(
            st.undo_last_turn(21).await.turn,
            Some(TurnKey {
                session: 21,
                turn: 3
            })
        );
        assert_eq!(read(&file).await, "v1");
        assert_eq!(
            st.undo_last_turn(21).await.turn,
            Some(TurnKey {
                session: 21,
                turn: 1
            }),
            "空轮次不该占掉一次撤销"
        );
        assert_eq!(read(&file).await, "v0");
    }

    /// write_file 也必须声明写入目标：漏掉就等于它的改动静默不可撤销。
    #[test]
    fn write_file_declares_its_resolved_target() {
        use crate::builtins::WriteFile;
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let t =
            WriteFile.write_targets(&serde_json::json!({"path": "src/a.rs", "content": "x"}), &c);
        assert_eq!(
            t,
            vec![dir.path().join("src/a.rs")],
            "应给出解析后的绝对路径"
        );
        // 越界与缺参都不产生目标
        assert!(
            WriteFile
                .write_targets(
                    &serde_json::json!({"path": "/etc/passwd", "content": "x"}),
                    &c
                )
                .is_empty(),
            "项目外路径不得成为撤销目标"
        );
        assert!(
            WriteFile
                .write_targets(&serde_json::json!({"content": "x"}), &c)
                .is_empty()
        );
    }

    /// 端到端：执行器驱动的 write_file 建文件 → 撤销把文件删掉。
    #[tokio::test]
    async fn executor_driven_write_file_is_undone() {
        use crate::builtins::WriteFile;
        use crate::executor::{ConfirmCallback, ToolExecutor};
        use crate::tool::{ToolCtx as Ctx, ToolResult};
        use async_trait::async_trait;
        use serde_json::json;
        use tokio::sync::oneshot;

        struct Allow;
        #[async_trait]
        impl ConfirmCallback for Allow {
            async fn confirm(
                &self,
                _req: crate::confirm::ConfirmRequest,
            ) -> oneshot::Receiver<crate::confirm::ConfirmDecision> {
                let (tx, rx) = oneshot::channel();
                let _ = tx.send(crate::confirm::ConfirmDecision::Allow);
                rx
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let store = std::sync::Arc::new(CheckpointStore::new(root.clone()));
        let exec = ToolExecutor::new(vec![std::sync::Arc::new(WriteFile)])
            .attach_checkpoints(store.clone());
        store.begin_turn(77).await;
        let c = Ctx {
            project_root: root.clone(),
            tool_policy: Default::default(),
        };
        let r: ToolResult = exec
            .execute(
                "write_file",
                json!({"path": "made.rs", "content": "created by agent"}),
                &c,
                tokio_util::sync::CancellationToken::new(),
                &Allow,
                None,
            )
            .await
            .unwrap();
        assert!(!r.is_error, "写入应成功: {}", r.output);
        assert!(root.join("made.rs").exists());

        let report = store.undo_last_turn(77).await;
        assert_eq!(report.removed, vec!["made.rs".to_string()], "{report:?}");
        assert!(!root.join("made.rs").exists(), "agent 建的文件应被撤销删除");
    }

    #[tokio::test]
    async fn paths_outside_the_project_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let st = store(root);
        let c = ctx(root);
        st.begin_turn(1).await;
        st.snapshot_before_write(
            &[PathBuf::from("/etc/hosts"), root.join("..").join("x")],
            &c,
        )
        .await;
        let m = st
            .load_manifest(TurnKey {
                session: 1,
                turn: 1,
            })
            .await;
        assert!(m.unwrap().is_none(), "项目根之外的路径不得记入检查点");
    }

    #[tokio::test]
    async fn turn_number_continues_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("t.rs");
        write(&file, "t0").await;
        let c = ctx(root);
        {
            let st = store(root);
            st.begin_turn(11).await;
            st.snapshot_before_write(std::slice::from_ref(&file), &c)
                .await;
            write(&file, "t1").await;
            st.finalize_after_write(std::slice::from_ref(&file), &c)
                .await;
        }
        // 新进程（新实例）接着开始轮次，不应覆盖磁盘上已有的 turn 1
        let st = store(root);
        let key = st.begin_turn(11).await;
        assert_eq!(key.turn, 2, "重启后轮号从磁盘续算，避免覆盖既有清单");
        // 新一轮什么都没写，撤销指针仍停在真正记到东西的 turn 1
        let report = st.undo_last_turn(11).await;
        assert_eq!(
            report.turn,
            Some(TurnKey {
                session: 11,
                turn: 1
            })
        );
        assert_eq!(report.restored, vec!["t.rs".to_string()]);
        assert_eq!(read(&file).await, "t0");
        assert!(
            st.undo_last_turn(11).await.did_nothing(),
            "没有更多记录时不该再去动文件"
        );
    }
}
