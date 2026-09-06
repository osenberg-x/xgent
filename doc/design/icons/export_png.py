#!/usr/bin/env python3
"""SVG 图标 → 2x PNG 导出（v7 M3-T1）。

用法：
    pip3 install --user cairosvg
    python3 export_png.py [输出目录]   # 默认 ../../crates/xgent_app/assets/icons

关键约束（方案 §6.1）：SVG 源用 stroke="currentColor"（渲染器默认解析为**黑**），
而 bevy ImageNode 染色是**乘法**——黑像素乘任何色仍为黑。导出前必须替换为
#FFFFFF 白描边，运行时 ImageNode.color 才能染出任意颜色。
"""
import pathlib
import re
import sys

import cairosvg

SRC = pathlib.Path(__file__).parent
OUT_SIZE = 48  # 2x（viewBox 24，UI 按 12-22px 逻辑像素显示）

# M3 需求清单（任务台账 M3-T1）：现成 19 枚 + 补画 3 枚
ICONS = [
    "chat", "folder", "clock", "terminal", "star", "plus", "x", "check",
    "copy", "retry", "refresh", "send", "command", "panel-right",
    "chevron-down", "info", "file", "diff", "dollar",
    "gear", "chevron-right", "alert-triangle",
]


def main() -> int:
    out = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else (
        SRC / ".." / ".." / ".." / "crates" / "xgent_app" / "assets" / "icons"
    )
    out = out.resolve()
    out.mkdir(parents=True, exist_ok=True)

    ok, missing = [], []
    for name in ICONS:
        src = SRC / f"{name}.svg"
        if not src.exists():
            missing.append(name)
            continue
        svg = src.read_text(encoding="utf-8")
        white = re.sub(r'(stroke|fill)="currentColor"', r'\1="#FFFFFF"', svg)
        if "#FFFFFF" not in white:
            print(f"警告: {name}.svg 无 currentColor，未替换（检查源文件）")
        png = out / f"{name}@2x.png"
        cairosvg.svg2png(
            bytestring=white.encode("utf-8"),
            write_to=str(png),
            output_width=OUT_SIZE,
            output_height=OUT_SIZE,
        )
        ok.append(name)

    print(f"导出 {len(ok)} 枚 → {out}")
    if missing:
        print(f"缺失 {len(missing)} 枚: {', '.join(missing)}")
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
