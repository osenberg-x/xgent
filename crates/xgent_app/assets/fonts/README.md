# 字体资产

随包分发的字体（均为 **SIL Open Font License 1.1**，许可全文见同目录 `OFL.txt`）：

| 文件 | 家族 | 来源 | 用途 |
|:---|:---|:---|:---|
| `Inter-Variable.ttf` | Inter Variable | https://github.com/rsms/inter（OFL 1.1） | 界面拉丁文本的**全局默认**（覆盖 `Assets<Font>` 的默认 `AssetId`） |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono | https://github.com/JetBrains/JetBrainsMono（OFL 1.1） | `mono` 槽位的**内嵌兜底**（系统没装等宽字体时用） |

## 装配规则（v8 M4-T1）

字体按**槽位**装配，候选栈顺序按平台不同，但平台差异只存在于**数据**里（`xgent_ui::fonts::font_specs()`），代码不分叉：

| 槽位 | 系统候选（按序） | 内嵌兜底 |
|:---|:---|:---|
| `ui` | —（内嵌 Inter Variable 作默认字体资产） | Inter Variable |
| `ui_cjk` | Windows `Microsoft YaHei UI` → `Microsoft YaHei` → `SimHei`；macOS `PingFang SC` → `Hiragino Sans GB` → `Heiti SC`；Linux `Noto Sans CJK SC` → `Source Han Sans SC` → `WenQuanYi Micro Hei` | 系统发现 |
| `mono` | Windows `Cascadia Mono` → `Consolas` → `Cascadia Code`；macOS `SF Mono` → `Menlo` → `Monaco`；Linux `JetBrains Mono` → `DejaVu Sans Mono` → `Liberation Mono` | **JetBrains Mono** |
| `mono_cjk` | `Sarasa Mono SC` → `Noto Sans Mono CJK SC` | 系统发现 |

- 候选栈全缺 → 用内嵌兜底；`mono` **永不**回退到 `ui` 槽位的比例字体（设计系统 C-FONT-1）。
- 解析结果写进启动日志（`字体槽位 mono → Cascadia Mono（来源 system）`），三平台可核对（C-FONT-2）。
- 解析失败是**警告**不是静默。

## 换字体时注意

- 两个字体都必须保留 **OFL 1.1** 许可文件与来源说明。
- 内嵌等宽字体只提供 Regular 字面；`mono_text_weighted` 的 510/590 字重由 parley 就近选面（无对应字面时回落 Regular）。
