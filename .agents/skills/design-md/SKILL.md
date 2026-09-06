---
name: design-md
description: XGent 的 UI 视觉风格参考库。在做 UI 重构、新增面板/界面、调整视觉风格、xui 组件样式、主题配色（xgent_ui::theme）等设计工作时使用：从 75 套 DESIGN.md 设计系统（Stitch 格式，来自 awesome-design-md-cn）中选定风格，读取其设计规范并映射到 Bevy UI 实现。
---

# design-md：设计系统参考库

一份 DESIGN.md 是给 AI Agent 读取的纯文本设计系统文档（Google Stitch 提出的格式），定义视觉气质、语义色、排版层级、组件样式、间距/阴影体系与 Do's & Don'ts。让 Agent 生成风格一致的 UI，比口头描述「要高级感」稳定得多。

- 来源：[fchangjun/awesome-design-md-cn](https://github.com/fchangjun/awesome-design-md-cn)（基于 [VoltAgent/awesome-design-md](https://github.com/VoltAgent/awesome-design-md) 的中文整理版），同步于上游 commit `87d0cab`（2026-07-07），共 75 套。
- 本地库：`.agents/skills/design-md/library/<风格目录>/DESIGN.md`（仅保留 DESIGN.md，预览页见[在线站点](https://fchangjun.github.io/awesome-design-md-cn/)）。

## 使用流程

1. **确定目标气质**。用户已指定风格则直接进入下一步；未指定时，结合 XGent 定位（桌面端 AI 编码工具、深色、开发者向）从下方索引推荐 2~3 个候选（可用「本项目适配建议」），让用户选定。
2. **完整阅读选中的** `library/<风格>/DESIGN.md`。重点是第 1（视觉气质）、2（语义色）、3（排版层级）、4（组件样式）、5（间距）、6（阴影/层级）、7（Do's and Don'ts）节；第 8 节响应式按桌面窗口场景取舍；第 9 节示例提示词是 Web 语境，只作意图参考。
3. **按下方映射规则落地到 Bevy UI**，不逐字照搬 Web 细节。
4. **若该风格被确定为项目长期视觉语言**：更新 `crates/xgent_ui/src/theme.rs` 为新主题，把该 DESIGN.md 复制到 `doc/design/DESIGN.md` 作为项目视觉蓝本（注明对原版的裁剪与调整），并在 `doc/decisions/` 记一条简短 ADR。临时参考则不动这些文件。

## Web → Bevy UI 映射要点

DESIGN.md 按 Web/CSS 语境撰写，落地到本项目（bevy_ui + xui）时按此转译：

| Web 概念 | Bevy/本项目落地 |
|---|---|
| 语义色 + hex（第 2 节） | `crates/xgent_ui/src/theme.rs` 的 `Theme` Resource（三层深度 bg/panel/elevated/deep、line/border、text 三级、accent、状态色、语法高亮色）；rgba 半透明用 `Color::srgba` 的 alpha 表达 |
| px 尺寸与间距（第 5 节） | `Val::Px`；间距体系归入 theme 的间距常量，不要在组件里散落魔法数 |
| 圆角 | `BorderRadius` |
| box-shadow / 多层阴影 | bevy_ui 无阴影：按所选风格阴影哲学用「背景明度阶梯 + 细半透明边框」表达层级（深色系风格如 Linear 正是如此）；确需强立体感用嵌套节点叠色 |
| 字体栈（Inter/Geist 等） | 需有真实字体资产（`.ttf` 加载进 `Assets<Font>`）才生效；字号/字重层级表转成 theme 的字号常量，字重受字体资产实际包含的 weight 限制 |
| hover / active / focus 状态 | `Interaction`（及焦点状态）驱动的颜色切换，参照 theme 中 elevated/hover 的现有用法 |
| 断点与触控目标（第 8 节） | 桌面应用：断点映射为窗口尺寸下的面板布局规则，触控目标忽略 |
| Do's and Don'ts（第 7 节） | 基本与实现技术无关，全部照常遵守 |

改动通用组件观感在 `xui`，业务界面在 `xgent_ui`；色值一律收敛进 `theme.rs`，遵守项目「子系统经 ECS 通信」「用户可见字符串走 i18n」等既有约定。

## 本项目适配建议

XGent 是深色、开发者向、主打「轻量陪伴感」的编码工具：

- **贴近产品定位**：cursor、linear.app、raycast、warp、superhuman、vercel、opencode.ai、supabase、resend、voltagent、ollama
- **偏个性/陪伴感**（区别于同类工具的设计支柱）：spotify、sentry、posthog、elevenlabs、claude
- **浅色/编辑式路线**：notion、mintlify、apple、cal

## 风格索引（75 套）

目录名即 `library/<目录名>/`。

**自定义案例**

| 目录 | 风格 |
|---|---|
| awesome-design-md-cn | 冷灰科技风、低疲劳浏览、左侧筛选、紧凑目录卡片 |

**AI 与机器学习**

| 目录 | 风格 |
|---|---|
| claude | Anthropic 的 AI 助手。暖陶土色点缀，干净的编辑式排版 |
| cohere | 企业 AI 平台。高饱和渐变、数据密集型控制台气质 |
| elevenlabs | AI 语音平台。深色电影感界面，音频波形视觉 |
| minimax | AI 模型提供方。大胆深色界面与霓虹强调 |
| mistral.ai | 开放权重 LLM 提供方。法式极简，偏紫色调 |
| ollama | 本地运行 LLM。终端优先，黑白极简 |
| opencode.ai | AI 编码平台。开发者导向的深色主题 |
| replicate | 通过 API 运行模型。清爽白底，偏代码导向 |
| runwayml | AI 视频生成。电影感深色 UI，媒体展示导向 |
| together.ai | 开源 AI 基础设施。技术蓝图风格 |
| voltagent | AI Agent 框架。深黑底、祖母绿强调、终端原生气质 |
| x.ai | xAI 实验室。极简黑白、未来感强 |

**开发工具与平台**

| 目录 | 风格 |
|---|---|
| cursor | AI 代码编辑器。深色界面与渐变强调 |
| expo | React Native 平台。深色主题、紧凑字距、代码导向 |
| linear.app | 面向工程师的项目管理。极简、精确、紫色点缀 |
| lovable | AI 全栈构建工具。playful 渐变、友好的开发者气质 |
| mintlify | 文档平台。干净、绿色点缀、适合长阅读 |
| posthog | 产品分析工具。品牌个性强、面向开发者的深色 UI |
| raycast | 效率启动器。深色 chrome 与高彩渐变强调 |
| resend | 邮件 API。极简深色主题与等宽字点缀 |
| sentry | 错误监控。深色数据台，粉紫强调 |
| supabase | 开源 Firebase 替代方案。深色祖母绿，代码优先 |
| superhuman | 高速邮件客户端。高级深色 UI，键盘优先 |
| vercel | 前端部署平台。黑白精确感，Geist 字体 |
| warp | 现代终端。类 IDE 深色界面，块状命令 UI |
| zapier | 自动化平台。暖橙色，友好的插画驱动风格 |

**基础设施与云**

| 目录 | 风格 |
|---|---|
| clickhouse | 高性能分析数据库。黄色点缀，技术文档风 |
| composio | 工具集成平台。现代深色与多彩集成图标 |
| hashicorp | 基础设施自动化。企业级干净黑白风格 |
| mongodb | 文档数据库。绿色品牌识别，开发者文档导向 |
| sanity | Headless CMS。红色强调，内容优先的编辑式布局 |
| stripe | 支付基础设施。标志性紫色渐变，轻字重优雅风格 |

**设计与效率**

| 目录 | 风格 |
|---|---|
| airtable | 表格与数据库混合工具。多彩、友好、结构化数据气质 |
| cal | 开源日程安排工具。中性干净，面向开发者 |
| clay | 创意工作室风格。自然形态、柔和渐变、强 art direction |
| figma | 协作设计工具。多彩、活泼，但仍保持专业 |
| framer | 建站工具。黑蓝高对比，强调动效与设计感 |
| intercom | 客服消息平台。友好的蓝色系统与对话式 UI |
| miro | 视觉协作工具。亮黄色点缀，无限画布气质 |
| notion | 一体化工作空间。温和极简、衬线标题、柔和表面 |
| pinterest | 图片发现平台。红色强调、瀑布流、图片优先 |
| slack | 团队协作消息工具。紫色品牌底、柔和渐变首屏、产品界面组合展示 |
| webflow | 可视化建站。蓝色点缀、polished marketing site 风格 |

**金融科技与加密**

| 目录 | 风格 |
|---|---|
| binance | 加密交易平台。黑黄高对比、交易数据紧迫感、深色金融界面 |
| coinbase | 加密交易所。干净蓝色识别，强调可信与机构感 |
| kraken | 加密交易平台。紫色深色界面，数据密集型 dashboard |
| mastercard | 全球支付网络。暖米色画布、胶囊圆角、圆形轨道叙事 |
| revolut | 数字银行。精致深色界面，渐变卡片，金融科技精确感 |
| wise | 国际转账。明亮绿色点缀，清晰且友好 |

**企业与消费品牌**

| 目录 | 风格 |
|---|---|
| airbnb | 旅行平台。暖珊瑚色点缀，强摄影感，圆润 UI |
| apple | 消费电子。高级留白，SF Pro，电影感视觉 |
| dell-1996 | 1996 年 Dell 官网复古目录风。黑色页面框、彩色丝带卡片、粗重标题和手工 GIF 贴纸感 |
| hp | PC 与打印机品牌。纯白画布、HP 电光蓝 CTA、几何无衬线和蓝色斜角装饰 |
| ibm | 企业技术品牌。Carbon 设计系统，结构化蓝色体系 |
| meta | 消费硬件商店。摄影优先、黑白双 CTA、Meta 蓝购买动作 |
| nike | 运动零售品牌。黑白高对比、超大写标题、全幅运动摄影 |
| nintendo-2001 | 复古游戏网页。金属斜面面板、琥珀导航、Y2K 主机外壳式界面 |
| nvidia | GPU 计算。绿色与黑色能量感，技术力量气质 |
| playstation | 游戏主机零售与内容平台。蓝色 CTA、三段式明暗画布、游戏视觉主导 |
| shopify | 电商平台。深色电影感营销与浅色交易界面并行，绿色商业强调 |
| spacex | 航天科技。黑白强对比，全幅影像，未来感 |
| spotify | 音乐流媒体。深色底上的高亮绿色，大胆排版 |
| starbucks | 咖啡零售品牌。暖奶油底、四层绿色系统、圆润门店标识感 |
| theverge | 科技媒体。近黑编辑画布、酸性薄荷与紫色强调、杂志化高密度信息流 |
| uber | 出行平台。粗黑白对比、紧凑字距、都市感强 |
| vodafone | 全球电信品牌。沃达丰红 CTA、巨大大写标题、影像分段叙事 |
| wired | 科技杂志。纸白编辑版式、黑色字标、窄高衬线标题与高密度文章排版 |

**汽车品牌**

| 目录 | 风格 |
|---|---|
| bmw | 豪华汽车品牌。深色高级表面，精确的德系工程感 |
| bmw-m | BMW 高性能子品牌。赛车运动三色强调、黑底摄影、工程感排版 |
| bugatti | 超豪华跑车品牌。纯黑电影感、克制单色、纪念碑式大标题 |
| ferrari | 豪华汽车品牌。黑白编辑感极强，法拉利红极少量点缀 |
| lamborghini | 豪华汽车品牌。纯黑舞台感、金色强调、张力极强 |
| renault | 法国汽车品牌。高能渐变、几何秩序、现代车展感 |
| tesla | 电动车品牌。极简克制、摄影驱动、产品展示优先 |
