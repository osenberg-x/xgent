# 验证报告：f20-s3-timeline

- 变更：F-20 的 S3 3D 时间轴依赖（回合/提交锚点、只读回放、HUD scrubber）
- 分支：`comet/f20-s3-timeline`；基线 `2769684`；实现 `6da1f5c`、复审修复 `dcb1184`、`683da3e`
- 验证模式：full（任务 9 > 3，delta spec 1）
- 语言：zh-CN

## 摘要

| 维度 | 结果 |
|:---|:---|
| 完整性 | 9/9 任务完成；6/6 需求覆盖 |
| 正确性 | 6/6 ADDED 需求实现；16/16 场景覆盖 |
| 一致性 | 遵循 `design.md` D1–D6；OpenSpec design 为唯一设计文档（Superpowers spec 不适用） |

## 需求与实现映射

| 需求（delta spec） | 实现 | 场景证据 |
|:---|:---|:---|
| 时间轴数据源 | `s3.rs:145-397` `TimelineData`/`merge_anchors`/`load_timeline`；git 不可用仅回合 | `merge_orders_by_time_and_commit_first`、`merge_keeps_most_recent_turns`、`merge_handles_empty`、`git_timeline_reads_tree_and_degrades_without_git` |
| 时间轴 scrubber | `s3.rs:624-820` HUD rail/标记/标签/回到现在/历史边框；点击/拖动观察者 `s3.rs:658-820`；键盘 `s3.rs:996-1035` | `slot_mapping_rounds_to_nearest_endpoint`、`arrow_keys_step_history_only_when_space_visible`、`return_button_restores_selection`、`ui_shows_empty_then_history_label_and_border` |
| 历史状态回放 | `s3.rs:408-490` 文件集与计划；`s3.rs:1037-1102` 文件/目录/边可见性；只回放当前图节点 | `plan_intersects_current_graph_and_filters_dirs`、`turn_visible_files_use_commit_prefix_plus_created`、`no_git_turn_visible_removes_later_created`、`visibility_system_replays_nodes_edges_and_keeps_camera` |
| 相机与现在状态不变 | 回放不写相机；`go_to_slot`/`return_now` 保存并恢复选中；git 只读 | `visibility_system_replays_nodes_edges_and_keeps_camera`（断言 yaw/pitch/dist/target）、`state_saves_and_restores_selection`、`step_walks_anchors_and_returns_now` |
| 后台加载与失败降级 | `s3.rs:868-994` 后台线程 + crossbeam + 签名/排队/断连保护；检查点损坏 warning；git 失败降级 | `poll_loader_applies_then_clears_loading`、`poll_loader_reports_thread_failure`、`corrupt_checkpoint_degrades_with_warning` |
| 时间轴 UI 与 i18n | `s3.rs:624-820`、`s3.rs:1144-1235`；`zh-CN/en-US/main.ftl` 时间轴键 | `spawns_timeline_structure_and_i18n`、`ui_shows_empty_then_history_label_and_border` |

## 验证中发现并修复的问题

| 级别 | 问题 | 修复 |
|:---|:---|:---|
| CRITICAL | 扫描节点是绝对路径，检查点/git 是项目相对路径，交集为空 → 历史视图把所有节点隐藏 | `current_paths`/可见性统一经 `relative_path` 剥项目根（`s3.rs:277-317`、`1037-1102`）；根目录归一为空串；测试改用绝对路径并新增 `current_paths_strip_project_root` |
| IMPORTANT | 空间面板隐藏（经典布局）时 Esc 仍消费历史退出，吞掉对话/编辑器快捷键 | Esc 历史分支加 `space_visible` 门控（`s2.rs:863-871`）；新增 `escape_does_not_steal_shortcuts_when_space_hidden` |
| IMPORTANT | 加载线程断连后每帧重启、错误反复闪；旧项目/会话结果可能短暂套到新目标 | 断连记录已尝试签名（`s3.rs:887-893`）；根/会话变化时丢弃旧结果（`s3.rs:866-879`） |
| WARNING（已修） | 可见性/搜索/HUD 查询新增 `&Visibility` 后与 `&mut Visibility` 冲突（B0001，`space_panel` 集成测试全挂） | `update_space_hud` 星体加 `Without<SpaceHudLabel>`；搜索空态加 `Without<SpatialStar>`（`view.rs:1253`、`s2.rs:320`） |
| WARNING（已修） | git 树保留项目前缀外文件；同一秒提交顺序反了；非 ASCII 路径被转义 | `git_tree` 只收前缀内路径、`core.quotePath=false`；git log 结果反转后再合并（`s3.rs:390-397`、`562-591`） |
| WARNING（已修） | 回到现在只认 `Pressed`，键盘激活走 `Activate` | 改为 `Activate` 观察者（`s3.rs:789-803`）；UI 测试触发真实观察者 |
| WARNING（已修） | 线程错误硬编码中文，英文界面漏翻译 | `TimelineError::{ThreadExited, SpawnFailed}` + 两语言 FTL 键 |
| WARNING（已修） | 无后台任务时 `poll_timeline_job` 每帧 `ResMut` 触发可见性重算 | `rx.is_none()` 提前返回（`s3.rs:856-865`）；根路径归一化移出循环 |

## 独立复审

- 首轮独立复审发现上述 CRITICAL 路径域错配与 Esc 门控问题；均在 `dcb1184` 修复。
- 第二轮独立复审确认两项修复有效；其提出的「`git -C` 不接受 canonical `\\?\` 路径」经本机 Rust `Command` 实测与新增回归 `git_accepts_canonical_project_root` 证伪（git 2.51.0.windows.1，exit 0）。
- 复审后新增 B0001 冲突由完整 `cargo test -p xgent_ui` 暴露并在 `683da3e` 修复。

## 已知限制（非阻塞）

- 回合锚点按「当前会话第 N 个用户消息 = 检查点第 N 轮」映射（`design.md` D3 明确假设）。agent 执行中的 steering 插话不新开检查点轮次，极端情况下会把该轮 `Created` 归到插话锚点；正常单轮会话与恢复会话不受影响。完整 F-20 持久化真实 `TurnKey` 时再收紧。
- 最多 71 个标记（20 回合 + 50 提交 + 现在端点）在窄 rail 上可能拥挤；当前固定 14px 外框等距排布。
- `plan.missing`（历史存在但当前图缺失的文件数）只写 debug 日志，未在标签上展示；符合 spec 的「面板/日志」二选一。
- 后台 git 线程无超时；git 挂起时 `loading` 会一直保持，但不阻塞 UI。

## 验证命令

| 命令 | 结果 | 证据 |
|:---|:---|:---|
| `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p xgent_ui -p xgent_tools -p xgent_spatial -j 2` | exit 0 | `docs/openspec/changes/f20-s3-timeline/.comet/checks/8f2c79cb-a630-4a5a-b631-6382c493ee48.log` |
| `cargo check --workspace` | exit 0 | `docs/openspec/changes/f20-s3-timeline/.comet/checks/8efe56af-8c51-4e66-acdc-eb9be43929aa.log` |
| `cargo build` | exit 0 | `docs/openspec/changes/f20-s3-timeline/.comet/checks/f32cf3b2-ee84-4c5d-ba1a-1334ba77c7ca.log` |

## 结论

无 CRITICAL / IMPORTANT 未决问题。建议：后续补一次直接模拟 pointer 事件的 rail 点击测试；为 rail 按钮补无障碍名称；git 子进程加超时。可以进入归档。
