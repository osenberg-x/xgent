# 0008-会话存储 JSONL 决策

## 背景

架构设计文档 `doc/design/architecture.md` §6.4 规定会话历史用 SQLite（存平台路径下 `sessions.db`）。`xgent_settings_core` 已定义 `sessions_db_path()` 但未使用。优化文档 O5 与借鉴分析 `doc/notes/oh-my-pi-borrowing-analysis.md` §3.3 建议改 JSONL append-only，理由：更简单（少一个依赖）、崩溃只丢最后一行（SQLite 崩溃可能损坏整个库）、天然 branching（树形 id/parentId）、可读性（cat/grep 调试）、omp 实践验证可行。

## 决策

**会话历史用 JSONL append-only（主存储），SQLite 留给元数据索引（P1）。**

具体契约：

1. 会话文件路径：`<platform_path>/xgent/sessions/<dir_encoded>/<timestamp>_<id>.jsonl`，每行一个 JSON entry。
2. `SessionEntry` 枚举（`#[serde(tag = "type")]`）：`Header(SessionHeader)` / `Message(SessionMessage)` / `ModelChange(ModelChangeEntry)`。MVP 只这 3 种，Compaction 等留 P1。
3. `SessionStore`（`xgent_agent/src/session_store.rs`）：`open(path)` / `append(&SessionEntry) -> io::Result<()>`（同步 append，返回即持久化）/ `load_all() -> io::Result<Vec<SessionEntry>>`。
4. `SessionMessage` 含 `id` / `parent_id: Option<String>`（树形结构预留，MVP 全 None 即线性）/ `timestamp` / `message: AgentMessage`。
5. MVP 持久化时机：每次 assistant 消息完成（ChatEvent::Done）时 append Message entry；会话开始时 append Header entry。

## 备选方案

### 方案 B：维持架构文档原案，用 SQLite 存会话历史

否决：MVP 阶段会话恢复明确不做（优化文档 §6.4），SQLite 的索引/查询能力在 MVP 无消费者——MVP 只需 append-only 写。引入 SQLite 依赖（rusqlite + libsqlite3）换来的查询能力 MVP 用不到，崩损坏库风险反而增加。JSONL 在 MVP 阶段是更 boring 的选择。

### 方案 C：MVP 不做会话持久化，O5 整体推 P1

承认 MVP 既不恢复也无消费者，持久化是"死存储"，整体推迟。

否决：即便 MVP 不恢复，append-only 持久化也有价值——崩溃后用户可手动 cat JSONL 找回对话内容；为 P1 恢复功能预留存储格式（届时只需加读取逻辑，不需迁移格式）。且实现成本极低（一个 SessionStore 结构 + writeln）。但 MVP 阶段 O5 优先级确为 P1（非 P0），不阻塞 agent 可用性。

## 结论与后果

- **架构文档 §6.4 需更新**：会话存储从 SQLite 改为 JSONL；SQLite 保留用于元数据索引/prompt 历史/模型使用统计（P1）。
- **MVP 不实现会话恢复**：`load_all` 方法定义但 agent loop 不调用。恢复/重放逻辑（buildSessionContext）留 P1。
- **MVP 不实现 Compaction entry**：CompactionProvider trait（O9）P2 预留，SessionEntry 暂无 Compaction 变体，P1 实现压缩时再加。
- **树形结构字段预留**：`parent_id` 字段 MVP 全 None（线性写入），P1 实现 session forking 时启用。预留字段避免后续格式迁移。
- **此 ADR 落实优化文档 §12.4 的决策**：JSONL 主存储 + SQLite 元数据索引，两套存储各司其职。
