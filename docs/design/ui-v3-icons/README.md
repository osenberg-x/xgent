# XGent UI v3 — 图标库（ui-v3-icons）

> 从 `ui-prototype-v3.html` 切出的全部内联 SVG 图标，共 **32** 个唯一图标
> （原型内含 55 处 `<svg>` 引用，按几何去重后为 32 个唯一图标）。

## 规范（与 v3 设计令牌一致）

- 几何线性：**1.75px** 描边（令牌 `--icon-stroke`）
- 着色：**`stroke="currentColor"`** —— 全部图标可由父级文字色 / 品牌色直接着色，**无需改源文件**
- 填充：`fill="none"`（仅几何描边，无填充色块）
- 端点：`stroke-linecap="round"` `stroke-linejoin="round"`
- 视口：`viewBox="0 0 24 24"` + `xmlns`；**不含 width/height**，由使用处 CSS 控制尺寸
- 尺寸档令牌：`--icon-sm:16px` / `--icon-md:20px` / `--icon-lg:24px`

## 索引

| # | 文件名 | 语义名 | 分类 | 尺寸档 | 使用位置（原型行号） | 对应令牌 |
|---|--------|--------|------|--------|----------------------|----------|
| 1 | `icon-logo.svg` | 品牌标识（层叠） | 品牌 | lg · 24px | 顶栏品牌 logo（L585） | `--brand` 品牌色 |
| 2 | `icon-chevron-down.svg` | 下箭头 | 导航/折叠 | sm/md · 12–14px | 提供方按钮、文件树展开 `I.chevD`（L1329） | `currentColor`（继承 `--t2`） |
| 3 | `icon-chevron-left.svg` | 左箭头 | 导航/折叠 | 14px | 时间线折叠 `tl-chev`（L676）、侧栏折叠（L637）、`I.chevR` | `currentColor`（继承 `--t3`） |
| 4 | `icon-chevron-right.svg` | 右箭头 / 终端 | 导航/视图 | sm/md | 活动栏终端（L621）、时间线（L729）、`I.term` | `currentColor`（继承 `--t1`） |
| 5 | `icon-plus.svg` | 加号 | 操作 | sm/md | 新建文件（L857）、新建终端、`I.plus`、命令新建会话 | `currentColor`（继承 `--t1`） |
| 6 | `icon-x.svg` | 关闭 / 叉 | 操作 | sm | 弹窗关闭（L986）、标签关闭 `I.x`、拒绝（L1181） | `currentColor`（继承 `--t2`） |
| 7 | `icon-search.svg` | 搜索 | 操作 | md · 20px | 命令面板（L602）、活动栏搜索（L615）、命令输入（L1021） | `currentColor`（继承 `--t2`） |
| 8 | `icon-check.svg` | 完成勾 | 状态 | sm | 时间线完成态 `tl-icon`（L672）、允许执行（L1174） | `--ok` 成功绿 |
| 9 | `icon-gear.svg` | 设置（齿轮） | 操作 | md · 20px | 顶栏设置（L605）、活动栏设置（L626）、`I.gear` | `currentColor`（继承 `--t2`） |
| 10 | `icon-play.svg` | 播放 / 重播 | 演示 | sm | `I.play`、重播流式输出 | `currentColor`（继承 `--t1`） |
| 11 | `icon-pause.svg` | 暂停 | 状态 | sm | 时间线执行中 `tl-icon`（L795） | `currentColor`（继承 `--t1`） |
| 12 | `icon-eye.svg` | 预览 / 查看 | 演示 | sm | `I.eye`、查看差异 | `currentColor`（继承 `--t1`） |
| 13 | `icon-globe.svg` | 语言 / 全球 | 设置 | sm | `I.globe`、切换语言 | `currentColor`（继承 `--t1`） |
| 14 | `icon-split.svg` | 分屏 | 视图 | sm | `I.split`、切换编辑器分屏 | `currentColor`（继承 `--t1`） |
| 15 | `icon-folder.svg` | 文件夹 | 文件 | md · 20px | 活动栏文件（L612）、`I.folder`、文件树 | `currentColor`（继承 `--t1`） |
| 16 | `icon-folder-open.svg` | 文件夹（展开） | 文件 | md · 20px | `I.folderOpen`、文件树展开 | `currentColor`（继承 `--t1`） |
| 17 | `icon-file.svg` | 文件 | 文件 | md · 20px | 编辑器标签（L876）、`I.file`、文件树 | `currentColor`（继承 `--t1`） |
| 18 | `icon-file-new.svg` | 新建文件 | 文件 | sm | 侧栏新建文件（L635） | `currentColor`（继承 `--t1`） |
| 19 | `icon-box.svg` | provider / 立方体 | 会话 | sm | `I.box`、切换 provider | `currentColor`（继承 `--t1`） |
| 20 | `icon-edit.svg` | 编辑 / 铅笔 | 视图 | md · 20px | 活动栏编辑器（L618） | `currentColor`（继承 `--t1`） |
| 21 | `icon-refresh.svg` | 刷新 | 操作 | sm | 侧栏刷新（L636） | `currentColor`（继承 `--t1`） |
| 22 | `icon-clear.svg` | 清除 / 垃圾桶 | 操作 | sm | 终端清屏（L859） | `currentColor`（继承 `--t1`） |
| 23 | `icon-send.svg` | 发送 / 纸飞机 | 操作 | md · 20px | 输入卡发送键（L845） | `--brand` 冷电蓝 |
| 24 | `icon-info.svg` | 信息 / 待确认 | 状态 | lg · 24px | 弹窗待确认标记 `mh-mark`（L978）、`I.cpPend` | `--pend` 暖琥珀 |
| 25 | `icon-pet.svg` | 宠物窗口（半圆） | 状态 | sm | 状态栏宠物开关（L959） | `currentColor`（继承 `--t1`） |
| 26 | `icon-sliders.svg` | token 用量（滑杆） | 仪表 | sm | 状态栏 token 仪表（L948） | `currentColor`（继承 `--t2`） |
| 27 | `icon-cp-idle.svg` | 陪伴锚点-待命 | 陪伴锚点五态 | md · 20px | 陪伴锚点 idle 态 `cpGlyph` / `I.cpIdle` | `--brand` 冷电蓝 |
| 28 | `icon-cp-think.svg` | 陪伴锚点-思考 | 陪伴锚点五态 | md · 20px | 陪伴锚点 思考态 / `I.cpThink` | `--brand` 冷电蓝 |
| 29 | `icon-cp-tool.svg` | 陪伴锚点-执行工具 | 陪伴锚点五态 | md · 20px | 陪伴锚点 执行工具态 / `I.cpTool` | `--brand` 冷电蓝 |
| 30 | `icon-cp-fail.svg` | 陪伴锚点-出错 | 陪伴锚点五态 | md · 20px | 陪伴锚点 出错态 / `I.cpFail` | `--fail` 错误红 |
| 31 | `icon-cp-off.svg` | 陪伴锚点-关闭（电源） | 陪伴锚点五态 | md · 20px | 宠物关闭/降级态 / `I.cpOff` | `--t3` 降饱和 |
| 32 | `icon-clock.svg` | 停止边界轮询（时钟） | 时间线 | sm | 时间线外层 `run_agent_loop` 标签（L747） | `currentColor`（继承 `--t1`） |

## 使用方式

```html
<!-- 方式 A：<img> 引用，颜色由 color 控制 -->
<img src="icon-send.svg" style="width:var(--icon-md);color:var(--brand)">

<!-- 方式 B：内联 <use>（需同名 sprite 或文件可被引用） -->
<svg class="ico" style="width:var(--icon-sm)"><use href="icon-search.svg"/></svg>
```

> 注：所有图标均为 `currentColor` 着色，放到任意 `color:` 容器即可换色；
> `--brand`（冷电蓝）用于发送键与陪伴锚点常规态，`--pend`（暖琥珀）用于待确认，
> `--ok`（绿）用于完成勾，`--fail`（红）用于出错态。语义状态色由原型在运行时注入，
> 图标源文件本身保持中性、可复用。

## 去重说明

原型中同一几何图标被多处复用（如 `search` 出现 3 次、`x` 4 次、`chevron-left` 6 次、
`gear` 3 次、`plus` 3 次、`folder`/`file` 各 2 次、`info`/`cpPend` 为同一几何复用），
本目录按几何去重，每个唯一图标仅保留一份。重复的引用点已在「使用位置」列合并标注。
