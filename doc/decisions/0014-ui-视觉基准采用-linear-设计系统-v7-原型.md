# 0014-ui-视觉基准采用-linear-设计系统-v7-原型

## 背景

v6 及之前的 UI 原型（`doc/design/ui-prototype-v6.html`）视觉体系是「暖石灰 + 紫罗兰渐变 + 彩色辉光 + 投影分层」：亮色默认、暗色主题偏浑浊、层级靠 box-shadow 表达、彩色系统多源头（紫渐变品牌 + 橙色陪伴 + Catppuccin 代码色 + 四组状态色）。问题：渐变与彩色辉光在 bevy_ui 无对应物；投影分层在深色界面几乎不可见；暖石灰暗色与开发者工具气质不符。

项目已安装 design-md skill（`.agents/skills/design-md/`，75 套 Stitch 格式设计系统文档，同步自 awesome-design-md-cn@87d0cab）。按 skill 流程为 UI 重构选型。

### 候选对比（skill 推荐清单）

| 候选 | 气质 | 结论 |
|---|---|---|
| **linear.app** | 暗色原生、消色差灰阶、单一靛紫强调、明度阶梯表达层级 | **选定**。与 XGent 深色开发者工具定位及既有紫色品牌延续性最好；「明度阶梯 + 细半透明边框」的层级哲学恰好是 bevy_ui 可直接表达的（无阴影依赖） |
| cursor | 深色 + 渐变强调 | 渐变强调在 bevy_ui 需图片/着色器近似，成本高；品牌记忆点与既有紫色不接续 |
| raycast | 深色 chrome + 高彩渐变 | 装饰性强，与「实用优先」支柱冲突 |
| claude | 暖陶土、浅编辑式 | 气质偏文档/陪伴，工具密度场景不合适；可作为未来浅色主题再参考 |

## 决策

**采用 Linear 设计系统作为 XGent UI 视觉基准**，v7 原型（`doc/design/ui-prototype-v7.html`）为首个落地物。规范蓝本：`.agents/skills/design-md/library/linear.app/DESIGN.md`。当前状态为**提案**（原型 + 浏览器逐状态截图验证通过），Bevy 侧实现后转为接受。

### 核心规范要点

- 暗色优先：画布 `#08090A`，表面明度阶梯 `#0F1011`（面板）→ `#191A1B`（浮层），层级用「背景明度 + rgba(255,255,255,0.05/0.08) 细边框」表达，非悬浮元素零投影。
- 单一彩色系统：靛紫 `#5E6AD2`（主色）/ `#7170FF`（交互）/ `#828FFF`（悬停），其余界面消色差。
- 排版：Inter（cv01/ss03），字重体系 400/510/590，负字距仅用于展示级字号；代码 ui-monospace/SF Mono/Menlo。
- 组件：按钮 6px、胶囊 9999px、卡片 8px、浮层 12px；命令面板选中态用中性灰。

### 对 Linear 规范的有意裁剪（非走样，实现时勿「修正」）

1. **状态色为外推值**：DESIGN.md 只定义 success 绿；v7 按 Radix 深色阶外推 success `#30A46C` / warning `#FFB224` / error `#E5484D` / info `#0091FF`（亮色对应 Radix 亮阶）。依据：Linear 实际基于 Radix 色板。
2. **遮罩取 0.5/0.55 而非规范的 0.85**：规范值是营销页弹窗语境，工具型应用过重。
3. **陪伴按钮为全界面唯一暖色例外**（`--warm`，琥珀）：XGent「轻量陪伴感」品牌差异点。
4. **工具栏图标按钮用 6px 方圆角而非规范的 50% 圆形**：规范语境是营销站导航，应用工具栏惯例为方圆角。
5. **亮色主题的 hover/active 用半透明黑**（rgba(0,0,0,0.045/0.07)）：规范未覆盖亮色交互态，按同构思路自延。

### Bevy 落地可行性（已核对 ../bevy 0.19 源码）

- `LetterSpacing` 组件存在（`bevy_text/src/text_access.rs`）→ 负字距可移植。
- `FontFeatures`（`text.rs:406`）→ cv01/ss03 可移植。
- `FontVariations`（`text.rs:408`）→ 510/590 任意字重可移植，需内嵌 Inter 可变字体资产（OFL 许可）。
- 浮层环形阴影（`--shadow-lg/xl`）需嵌套节点/描边近似；pulse/blink/ring 动画与 350ms 过渡需手写动画系统。
- `Theme`（`crates/xgent_ui/src/theme.rs`）需新增槽位：`surface_accent`(bg-hover)、`active`(bg-active)、`icon_bg`、`tooltip_bg/text`、`accent_interactive`、`border_hover`、语义化的 code/mono 常量。色值一律收敛进 `Theme`，遵守 ECS 通信与 i18n 既有约定。

### v7.1 增量

会话区与预览/差异/终端面板之间为 6px 可拖拽分隔条（默认 720px，双击复位，拖拽保持会话区 ≥520px）。

## 备选方案

- **维持 v6 体系迭代**：渐变/辉光/投影在 bevy_ui 均需额外实现且深色下效果差，放弃。
- **cursor 风格**：见候选对比，渐变成本与品牌延续性问题，放弃。
- **自研体系**：无权威蓝本时 AI 生成 UI 一致性差（skill 存在的理由），放弃。

## 后续

1. Bevy 侧按本 ADR + skill 映射（`.agents/skills/design-md/SKILL.md`）实现主题，更新 `doc/dev-tutorial.md`。
2. 若采纳为正式规范，将裁剪后的 Linear 蓝本落到 `doc/design/DESIGN.md`（本 ADR 第「裁剪」节即差异清单）。
3. 亮色主题为 secondary，优先级低于暗色打磨。
