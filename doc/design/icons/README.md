# XGent UI 图标资源 (v6)

从 `ui-prototype-v6.html` 提取的 SVG 矢量图标，统一 24×24 viewBox，使用 `currentColor` 继承主题色。

## 图标清单

| 文件 | 名称 | 用途位置 | 描边/填充 |
|:---|:---|:---|:---|
| `chat.svg` | 对话 | 图标轨·对话、命令面板 | stroke |
| `folder.svg` | 文件夹 | 项目切换、图标轨·文件、上下文 chip、文件抽屉 | stroke |
| `file.svg` | 文件 | 上下文 chip、工具·读取文件、预览 tab | stroke |
| `search.svg` | 搜索 | 图标轨·搜索、工具·搜索代码、搜索抽屉 | stroke |
| `git.svg` | Git 分支 | 图标轨·Git、Git 抽屉 | stroke |
| `clock.svg` | 时钟/历史 | 图标轨·历史、历史抽屉 | stroke |
| `terminal.svg` | 终端 | 图标轨·终端、上下文 tab·终端、终端抽屉 | stroke |
| `puzzle.svg` | 拼图/插件 | 图标轨·插件 | stroke |
| `command.svg` | 命令面板 | 顶栏·命令面板按钮 | stroke |
| `plus.svg` | 加号 | 新建会话、添加上下文、暂存文件 | stroke |
| `x.svg` | 关闭 | 上下文 chip 删除、抽屉关闭、取消暂存 | stroke |
| `chevron-down.svg` | 下箭头 | 模型选择、滚动到底部 | stroke |
| `sun.svg` | 太阳 | 亮色主题图标 | stroke |
| `moon.svg` | 月亮 | 暗色主题图标 | stroke |
| `star.svg` | 星星 | 陪伴开关（填充） | fill |
| `panel-right.svg` | 右侧面板 | 折叠/展开上下文面板 | stroke |
| `info.svg` | 信息圆 | 快捷操作·解释代码 | stroke |
| `code.svg` | 代码箭头 | 快捷操作·重构代码 | stroke |
| `flask.svg` | 烧瓶 | 快捷操作·生成测试 | stroke |
| `wrench.svg` | 扳手 | 快捷操作·修复问题 | stroke |
| `check-circle.svg` | 勾选圆 | 快捷操作·代码审查 | stroke |
| `check.svg` | 对勾 | 工具状态成功、提交成功、Toast | stroke |
| `copy.svg` | 复制 | 消息操作、代码块复制、文件复制 | stroke |
| `refresh.svg` | 刷新/重新生成 | 消息操作·重新生成 | stroke |
| `retry.svg` | 重试 | 消息操作·重试 | stroke |
| `diff.svg` | 差异网格 | 上下文 tab·差异、查看差异 | stroke |
| `send.svg` | 发送 | 输入框·发送按钮 | stroke |
| `mic.svg` | 麦克风 | 状态栏·模型/token | stroke |
| `shield.svg` | 盾牌 | 输入区·安全提示 | stroke |
| `shield-alert.svg` | 盾牌警告 | 确认弹窗图标 | stroke |
| `dollar.svg` | 美元 | 状态栏·成本统计 | stroke |

## 规范

- **尺寸**：统一 `24×24` viewBox，通过 CSS `width`/`height` 控制实际显示大小
- **颜色**：使用 `stroke="currentColor"` 或 `fill="currentColor"`，继承父元素 `color`
- **描边宽度**：`stroke-width="2"`（统一线宽）
- **线条端点**：`stroke-linecap="round"`、`stroke-linejoin="round"`
- **无固定颜色**：图标本身不包含颜色信息，完全由主题 CSS 变量驱动

## 命名约定

- 全小写 + 连字符 (kebab-case)
- 语义优先，不按位置命名（如 `folder.svg` 而非 `rail-files.svg`）
- 同义图标加修饰词区分（如 `shield.svg` vs `shield-alert.svg`）

## 使用示例

```html
<!-- 作为内联 SVG -->
<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
  <use href="icons/chat.svg#icon"></use>
</svg>

<!-- 或直接内嵌路径 -->
<img src="icons/chat.svg" alt="对话" width="18" height="18">
```

## 后续迁移

Bevy UI 实现时，这些 SVG 路径将转为 `xui` crate 中的图标常量或 Bevy `Image` 资源。
