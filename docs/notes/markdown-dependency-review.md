# Markdown 解析依赖评审（M6.5-T2）

> 结论：保留 `pulldown-cmark 0.12`。它是本轮唯一新增的 Markdown 依赖，许可证 MIT，依赖面小，且只用于把文本解析成块模型，不渲染 HTML。

## 候选对比

| 方案 | 许可证 | 依赖面 | 适用性 | 结论 |
|:---|:---|:---|:---|:---|
| `pulldown-cmark 0.12.2` | MIT | `bitflags` / `getopts` / `memchr` / `pulldown-cmark-escape` / `unicase` | 纯 Rust、事件流 API，适合映射到 `MarkdownBlock`；无 HTML 渲染负担 | **采用** |
| `comrak` | BSD-2-Clause | 明显更大（GFM 扩展、HTML 渲染、插件） | 功能过剩；我们只需要六类块 | 不采用 |
| `markdown-rs` | MIT | 中等 | AST 完整，但当前版本迭代节奏与 API 稳定性不如前者 | 备选 |
| 手写解析 | — | 0 | 六类元素可做，但边界（嵌套列表、转义、代码围栏）容易出错 | 不采用 |

## 采用理由

1. **许可证**：MIT，与项目其余依赖兼容。
2. **用途匹配**：只调用 `Parser`/`Event`，输出 `MarkdownBlock` 后再由 Bevy UI 构造节点；不启用 HTML 输出，不存在 XSS 面。
3. **体积**：`Cargo.lock` 新增 5 个传递依赖，均为小体量工具库；相比 `comrak` 的依赖树更可控。
4. **退化方案**：若未来需要 GFM 表格/任务列表，先评估在 `markdown.rs` 内补块类型，仍不引入 HTML 渲染。

## 已知边界

- 当前只覆盖标题、段落、列表、代码块、粗体、链接六类；表格、删除线、任务列表暂不解析。
- 链接只显示文本与 URL tooltip，不打开外部浏览器。
- 流式期间不解析 Markdown（终稿才解析），避免每帧重排。

## 变更记录

- 2026-10-28：新增评审，跟随 M6.5-T2 接线。
