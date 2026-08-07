#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Extract every inline <svg> icon from the XGent v3 single-file prototype,
normalize to token-conformant format, dedupe by geometry, and emit:
  - one icon-xxx.svg per unique icon
  - README.md index (filename / semantic name / size tier / usage / token)
"""
import os, re, html, collections

SRC = r"E:\ws\xgent\doc\design\ui-prototype-v3.html"
OUT = r"E:\ws\xgent\doc\design\ui-v3-icons"

os.makedirs(OUT, exist_ok=True)

with open(SRC, "r", encoding="utf-8") as f:
    src = f.read()

# Grab every <svg ...>...</svg> block: markup + JS string literals.
# JS literals use single-quoted strings containing double-quoted attributes,
# so a single non-greedy DOTALL pattern captures both forms.
blocks = re.findall(r"<svg\b.*?</svg>", src, flags=re.S)
print(f"raw <svg> occurrences found: {len(blocks)}")

def normalize(inner: str) -> str:
    """Wrap inner geometry into the canonical, token-conformant <svg>."""
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" '
        'fill="none" stroke="currentColor" stroke-width="1.75" '
        'stroke-linecap="round" stroke-linejoin="round">'
        f"{inner}</svg>"
    )

def inner_of(block: str) -> str:
    """Extract inner markup between the opening tag's > and </svg>;
    drop any width/height on the root <svg> so icons scale via currentColor."""
    m = re.match(r"<svg\b[^>]*>(.*)</svg>", block, flags=re.S)
    return m.group(1).strip() if m else ""

# Canonicalize inner for dedup: strip whitespace between tags, unify quotes.
def canon(inner: str) -> str:
    s = html.unescape(inner)
    s = re.sub(r"\s+", " ", s).strip()
    s = re.sub(r">\s+<", "><", s)
    return s

# ---- curated metadata keyed by canonical inner geometry ----
# (file, zh-name, category, size-tier, usage, token)
META = {
 # logo
 '<path d="M12 2 2 7l10 5 10-5-10-5z"></path><path d="M2 17l10 5 10-5"></path><path d="M2 12l10 5 10-5"></path>':
   ("icon-logo.svg","品牌标识（层叠）","品牌","lg · 24px","顶栏品牌 logo（L585）","--brand 品牌色"),
 # chevron-down
 '<path d="m6 9 6 6 6-6"></path>':
   ("icon-chevron-down.svg","下箭头","导航/折叠","sm/md · 12–14px","提供方按钮、文件树展开 I.chevD（L1329）","currentColor（继承 --t2）"),
 # chevron-left
 '<path d="m9 18 6-6-6-6"></path>':
   ("icon-chevron-left.svg","左箭头","导航/折叠","14px","时间线折叠 tl-chev（L676）、侧栏折叠（L637）、I.chevR","currentColor（继承 --t3）"),
 # chevron-right / terminal
 '<path d="m4 17 6-6-6-6"></path><path d="M12 19h8"></path>':
   ("icon-chevron-right.svg","右箭头 / 终端","导航/视图","sm/md","活动栏终端（L621）、时间线（L729）、I.term","currentColor（继承 --t1）"),
 # plus
 '<path d="M12 5v14M5 12h14"></path>':
   ("icon-plus.svg","加号","操作","sm/md","新建文件（L857）、新建终端、I.plus、命令新建会话","currentColor（继承 --t1）"),
 # x / close
 '<path d="M18 6 6 18M6 6l12 12"></path>':
   ("icon-x.svg","关闭 / 叉","操作","sm","弹窗关闭（L986）、标签关闭 I.x、拒绝（L1181）","currentColor（继承 --t2）"),
 # search
 '<circle cx="11" cy="11" r="7.5"></circle><path d="m21 21-4.3-4.3"></path>':
   ("icon-search.svg","搜索","操作","md · 20px","命令面板（L602）、活动栏搜索（L615）、命令输入（L1021）","currentColor（继承 --t2）"),
 # check / done
 '<path d="M20 6 9 17l-5-5"></path>':
   ("icon-check.svg","完成勾","状态","sm","时间线完成态 tl-icon（L672）、允许执行（L1174）","--ok 成功绿"),
 # gear / settings
 '<circle cx="12" cy="12" r="3"></circle><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"></path>':
   ("icon-gear.svg","设置（齿轮）","操作","md · 20px","顶栏设置（L605）、活动栏设置（L626）、I.gear","currentColor（继承 --t2）"),
 # play
 '<path d="M6 4.5v15l13-7.5z"></path>':
   ("icon-play.svg","播放 / 重播","演示","sm","I.play、重播流式输出","currentColor（继承 --t1）"),
 # pause
 '<rect x="6" y="4" width="4" height="16" rx="1.2"></rect><rect x="14" y="4" width="4" height="16" rx="1.2"></rect>':
   ("icon-pause.svg","暂停","状态","sm","时间线执行中 tl-icon（L795）","currentColor（继承 --t1）"),
 # eye
 '<path d="M1.8 12S5.6 4.8 12 4.8 22.2 12 22.2 12 18.4 19.2 12 19.2 1.8 12 1.8 12z"></path><circle cx="12" cy="12" r="2.8"></circle>':
   ("icon-eye.svg","预览 / 查看","演示","sm","I.eye、查看差异","currentColor（继承 --t1）"),
 # globe
 '<circle cx="12" cy="12" r="9"></circle><path d="M3 12h18M12 3a15 15 0 0 1 0 18 15 15 0 0 1 0-18z"></path>':
   ("icon-globe.svg","语言 / 全球","设置","sm","I.globe、切换语言","currentColor（继承 --t1）"),
 # split
 '<rect x="3" y="3" width="18" height="18" rx="2"></rect><path d="M12 3v18"></path>':
   ("icon-split.svg","分屏","视图","sm","I.split、切换编辑器分屏","currentColor（继承 --t1）"),
 # folder
 '<path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>':
   ("icon-folder.svg","文件夹","文件","md · 20px","活动栏文件（L612）、I.folder、文件树","currentColor（继承 --t1）"),
 # folder-open
 '<path d="m6 14 1.45-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.55 6A2 2 0 0 1 18.45 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.93a2 2 0 0 1 1.66.9l.82 1.2a2 2 0 0 0 1.66.9H18a2 2 0 0 1 2 2v2"></path>':
   ("icon-folder-open.svg","文件夹（展开）","文件","md · 20px","I.folderOpen、文件树展开","currentColor（继承 --t1）"),
 # file
 '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><path d="M14 2v6h6"></path>':
   ("icon-file.svg","文件","文件","md · 20px","编辑器标签（L876）、I.file、文件树","currentColor（继承 --t1）"),
 # file-new
 '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><path d="M14 2v6h6M12 18v-6M9 15h6"></path>':
   ("icon-file-new.svg","新建文件","文件","sm","侧栏新建文件（L635）","currentColor（继承 --t1）"),
 # box / provider cube
 '<path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"></path><path d="m3.27 6.96 8.73 5.05 8.73-5.05"></path><path d="M12 22.08V12"></path>':
   ("icon-box.svg","provider / 立方体","会话","sm","I.box、切换 provider","currentColor（继承 --t1）"),
 # edit / pencil
 '<path d="M12 20h9"></path><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z"></path>':
   ("icon-edit.svg","编辑 / 铅笔","视图","md · 20px","活动栏编辑器（L618）","currentColor（继承 --t1）"),
 # refresh
 '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"></path><path d="M21 3v5h-5"></path><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"></path><path d="M3 21v-5h5"></path>':
   ("icon-refresh.svg","刷新","操作","sm","侧栏刷新（L636）","currentColor（继承 --t1）"),
 # clear / trash
 '<path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"></path>':
   ("icon-clear.svg","清除 / 垃圾桶","操作","sm","终端清屏（L859）","currentColor（继承 --t1）"),
 # send / paper-plane
 '<path d="M22 2 11 13"></path><path d="m22 2-7 20-4-9-9-4z"></path>':
   ("icon-send.svg","发送 / 纸飞机","操作","md · 20px","输入卡发送键（L845）","--brand 冷电蓝"),
 # info / pending (also cp-pend)
 '<circle cx="12" cy="12" r="9"></circle><path d="M12 7.5v5.5"></path><path d="M12 16.4h.01"></path>':
   ("icon-info.svg","信息 / 待确认","状态","lg · 24px","弹窗待确认标记 mh-mark（L978）、I.cpPend","--pend 暖琥珀"),
 # pet / window toggle (half-circle)
 '<path d="M12 3a9 9 0 1 0 9 9"></path><circle cx="12" cy="12" r="3.2"></circle>':
   ("icon-pet.svg","宠物窗口（半圆）","状态","sm","状态栏宠物开关（L959）","currentColor（继承 --t1）"),
 # sliders / token meter
 '<path d="M4 7V5h16v2M9 19h6M12 5v14"></path>':
   ("icon-sliders.svg","token 用量（滑杆）","仪表","sm","状态栏 token 仪表（L948）","currentColor（继承 --t2）"),
 # cp-idle
 '<circle cx="12" cy="12" r="3"></circle><path d="M4.9 4.9a10 10 0 0 0 0 14.2M19.1 4.9a10 10 0 0 1 0 14.2" opacity=".55"></path>':
   ("icon-cp-idle.svg","陪伴锚点-待命","陪伴锚点五态","md · 20px","陪伴锚点 idle 态 cpGlyph / I.cpIdle","--brand 冷电蓝"),
 # cp-think
 '<circle cx="6" cy="12" r="1.3"></circle><circle cx="12" cy="12" r="1.3"></circle><circle cx="18" cy="12" r="1.3"></circle><path d="M3 6.5a10 10 0 0 1 18 0M3 17.5a10 10 0 0 0 18 0" opacity=".45"></path>':
   ("icon-cp-think.svg","陪伴锚点-思考","陪伴锚点五态","md · 20px","陪伴锚点 思考态 / I.cpThink","--brand 冷电蓝"),
 # cp-tool
 '<path d="M14.7 6.3a4 4 0 0 0 5.3 5.3L21 10l-7 7-3-3 7-7z"></path><path d="m9 15-5.5 5.5"></path>':
   ("icon-cp-tool.svg","陪伴锚点-执行工具","陪伴锚点五态","md · 20px","陪伴锚点 执行工具态 / I.cpTool","--brand 冷电蓝"),
 # cp-fail
 '<path d="M10.3 3.9 1.9 18a2 2 0 0 0 1.7 3h16.8a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"></path><path d="M12 9v4M12 17h.01"></path>':
   ("icon-cp-fail.svg","陪伴锚点-出错","陪伴锚点五态","md · 20px","陪伴锚点 出错态 / I.cpFail","--fail 错误红"),
 # cp-off / power
 '<circle cx="12" cy="12" r="7.5" opacity=".8"></circle><circle cx="12" cy="12" r="2.6"></circle>':
   ("icon-cp-off.svg","陪伴锚点-关闭（电源）","陪伴锚点五态","md · 20px","宠物关闭/降级态 / I.cpOff","--t3 降饱和"),
 # clock (stop-boundary polling)
 '<path d="M12 8v4l3 3"></path><circle cx="12" cy="12" r="9"></circle>':
   ("icon-clock.svg","停止边界轮询（时钟）","时间线","sm","时间线外层 run_agent_loop 标签（L747）","currentColor（继承 --t1）"),
}

# ---- dedupe by canonical geometry, count occurrences ----
seen = collections.OrderedDict()   # canon -> (inner, count)
for b in blocks:
    inner = inner_of(b)
    if not inner:
        continue
    c = canon(inner)
    if c in seen:
        seen[c] = (seen[c][0], seen[c][1] + 1)
    else:
        seen[c] = (inner, 1)

print(f"unique geometries: {len(seen)}")

missing = [c for c in seen if c not in META]
if missing:
    print("WARNING: geometries without curated metadata:")
    for c in missing:
        print("  ", c[:80])

# ---- write files ----
written = []
for c, (inner, cnt) in seen.items():
    if c not in META:
        continue
    fname, zh, cat, size, usage, token = META[c]
    svg = normalize(inner)
    path = os.path.join(OUT, fname)
    with open(path, "w", encoding="utf-8") as f:
        f.write(svg + "\n")
    written.append((fname, zh, cat, size, usage, token, cnt))

print(f"wrote {len(written)} icon files")

# ---- README ----
total = len(written)
readme = []
readme.append("# XGent UI v3 — 图标库（ui-v3-icons）")
readme.append("")
readme.append(f"> 从 `ui-prototype-v3.html` 切出的全部内联 SVG 图标，共 **{total}** 个唯一图标")
readme.append(">（原型内含 55 处 `<svg>` 引用，按几何去重后为 32 个唯一图标）。")
readme.append("")
readme.append("## 规范（与 v3 设计令牌一致）")
readme.append("")
readme.append("- 几何线性：**1.75px** 描边（令牌 `--icon-stroke`）")
readme.append("- 着色：**`stroke=\"currentColor\"`** —— 全部图标可由父级文字色/品牌色直接着色，**无需改源文件**")
readme.append("- 填充：`fill=\"none\"`（除语义状态色由 `currentColor` 继承）")
readme.append("- 端点：`stroke-linecap=\"round\"` `stroke-linejoin=\"round\"`")
readme.append("- 视口：`viewBox=\"0 0 24 24\"` + `xmlns`；**不含 width/height**，由使用处 CSS 控制尺寸")
readme.append("- 尺寸档令牌：`--icon-sm:16px` / `--icon-md:20px` / `--icon-lg:24px`")
readme.append("")
readme.append("## 索引")
readme.append("")
readme.append("| # | 文件名 | 语义名 | 分类 | 尺寸档 | 使用位置（原型行号） | 对应令牌 |")
readme.append("|---|--------|--------|------|--------|----------------------|----------|")
for i, (fname, zh, cat, size, usage, token, cnt) in enumerate(written, 1):
    readme.append(f"| {i} | `{fname}` | {zh} | {cat} | {size} | {usage} | {token} |")
readme.append("")
readme.append("## 使用方式")
readme.append("")
readme.append("```html")
readme.append('<svg class="ico"><use href="icon-search.svg"/></svg>')
readme.append('<!-- 或直接内联；颜色继承父级 currentColor -->')
readme.append('<img src="icon-send.svg" style="width:var(--icon-md);color:var(--brand)">')
readme.append("```")
readme.append("")
readme.append("> 注：所有图标均为 `currentColor` 着色，放到任意 `color:` 容器即可换色；")
readme.append("> `--brand`（冷电蓝）用于发送键与陪伴锚点常规态，`--pend`（暖琥珀）用于待确认，")
readme.append("> `--ok`（绿）用于完成勾，`--fail`（红）用于出错态。")
readme.append("")

with open(os.path.join(OUT, "README.md"), "w", encoding="utf-8") as f:
    f.write("\n".join(readme) + "\n")

print("wrote README.md")
print("DONE")
