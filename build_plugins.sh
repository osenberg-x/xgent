#!/usr/bin/env bash
# 构建内建插件并打包到 xgent_app/assets/plugins/。
#
# 照设计文档 §13 Step P6。当前内建插件：git。
#
# 用法：./build_plugins.sh [release]
#   release  用 release profile（更小 wasm）

set -euo pipefail

PROFILE="${1:-debug}"
TARGET="wasm32-wasip2"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

build_one() {
    local crate="$1"      # crate 名（crates/<crate>）
    local plugin_id="$2"  # 插件 id（assets/plugins/<id>/）
    echo "→ 构建 $crate (profile=$PROFILE)"
    if [ "$PROFILE" = "release" ]; then
        cargo build -p "$crate" --target "$TARGET" --release
        WASM="$ROOT/target/$TARGET/release/${crate}.wasm"
    else
        cargo build -p "$crate" --target "$TARGET"
        WASM="$ROOT/target/$TARGET/debug/${crate}.wasm"
    fi
    OUT="$ROOT/crates/xgent_app/assets/plugins/$plugin_id"
    mkdir -p "$OUT"
    cp "$WASM" "$OUT/extension.wasm"
    cp "$ROOT/crates/$crate/plugin.toml" "$OUT/plugin.toml"
    echo "  ✓ 打包到 $OUT"
}

build_one "xgent_plugin_git" "git"

echo "完成"
