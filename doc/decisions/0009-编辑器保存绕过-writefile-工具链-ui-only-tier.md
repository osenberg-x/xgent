# 编辑器保存与 UI-only Tier 绕过 WriteFile 工具链

F-11 编辑器（P1）引入后，用户在编辑器按 `Cmd+S` 保存文件**直接 `fs::write` 落盘，不经 WriteFile 工具、不经 NeedsConfirmation 确认**；落盘后经 daemon 广播 `peer.fileChanged`。agent 驱动编辑器动作（打开文件/跳转/选区/滚动）走新增的 `ToolTier::UiOnly`，默认 `Approved`，同样不走确认。`WriteFile` 工具（agent 调用）仍走 `Write` tier / `NeedsConfirmation`。

**理由**：用户主动编辑保存是常规编辑器 UX，每次保存都弹确认框不可用；且用户对自己行为默认信任，无需确认。agent 调 WriteFile 是机器行为，默认需确认——这是安全模型的核心约束。两条路径语义不同：用户保存是"用户对本地 buffer 的承诺"，agent WriteFile 是"机器对 workspace 的修改"。

**权衡**：绕过 WriteFile 工具链意味着编辑器保存不进统一工具可观测性（不记入工具调用历史、不进成本统计 F-12 的工具调用计数）。备选方案"走 WriteFile 但用户触发默认 Approved"保留可观测性但需扩展 SecurityPolicy 决策路径（区分"用户触发"与"agent 触发"），增加复杂度。选直接落盘以保 UX 简洁，可观测性损失由后续 F-12 成本统计单独设计编辑器保存计数补回。

**新增 `ToolTier::UiOnly` 变体**（与 `Read`/`Write`/`Exec` 并列）：标记只修改 UI 元数据不修改 workspace 状态的动作。agent 经 EditorTool（UI-only Tier）驱动编辑器，避免每次跳转/选区都弹确认框。`resolve_policy` 对 `UiOnly` 默认返回 `Approved`。详见 `doc/design/editor-design.md` 3.2 / 6.4。
