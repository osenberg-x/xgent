//! 主题：颜色、字体、间距常量。
//!
//! MVP 仅暗色主题（K-01 主题增强留待 P1）。

use bevy::color::palettes::css;
use bevy::prelude::*;

/// 暗色主题颜色表。
#[derive(Resource, Debug, Clone, Copy)]
pub struct Theme {
    /// 背景底色
    pub bg: Color,
    /// 面板背景
    pub panel: Color,
    /// 顶栏/状态栏背景
    pub bar: Color,
    /// 边框
    pub border: Color,
    /// 主文本
    pub text: Color,
    /// 次要文本
    pub text_dim: Color,
    /// 强调色（按钮、链接）
    pub accent: Color,
    /// 用户消息气泡
    pub bubble_user: Color,
    /// 助手消息气泡
    pub bubble_assistant: Color,
    /// 字体大小（逻辑像素）
    pub font_size: f32,
    // ===== 状态色（§4.2，对齐 ui-prototype.html --st_*） =====
    /// 待确认（pending）
    pub st_pending: Color,
    /// 执行中（running）
    pub st_running: Color,
    /// 完成（ok）
    pub st_ok: Color,
    /// 失败（fail）
    pub st_fail: Color,
    /// 已拒绝（deny）
    pub st_deny: Color,
    // ===== 语法高亮色（tree-sitter 风格，对齐 --kw/--fn/...） =====
    /// 关键字
    pub kw: Color,
    /// 函数名
    pub fn_: Color,
    /// 字符串
    pub str_: Color,
    /// 数字
    pub num: Color,
    /// 类型名
    pub ty: Color,
    /// 注释
    pub com: Color,
    /// 标点
    pub punc: Color,
}

impl Theme {
    /// 暗色主题。
    pub fn dark() -> Self {
        Self {
            bg: Color::srgba(0.10, 0.11, 0.13, 1.0),
            panel: Color::srgba(0.13, 0.14, 0.17, 1.0),
            bar: Color::srgba(0.08, 0.09, 0.11, 1.0),
            border: Color::srgba(0.25, 0.26, 0.30, 1.0),
            text: css::WHITE.into(),
            text_dim: Color::srgba(0.62, 0.64, 0.68, 1.0),
            accent: Color::srgba(0.36, 0.62, 0.92, 1.0),
            bubble_user: Color::srgba(0.20, 0.36, 0.56, 1.0),
            bubble_assistant: Color::srgba(0.18, 0.19, 0.22, 1.0),
            font_size: 14.0,
            // 状态色（对齐 ui-prototype.html --st_*）
            st_pending: Color::srgba(0.88, 0.70, 0.25, 1.0), // #e0b341
            st_running: Color::srgba(0.48, 0.64, 0.97, 1.0), // #7aa2f7
            st_ok: Color::srgba(0.31, 0.78, 0.47, 1.0),      // #50c878
            st_fail: Color::srgba(0.88, 0.34, 0.34, 1.0),    // #e05656
            st_deny: Color::srgba(0.53, 0.53, 0.53, 1.0),    // #888
            // 语法高亮色（对齐 --kw/--fn/...）
            kw: Color::srgba(0.78, 0.47, 0.87, 1.0), // #c678dd
            fn_: Color::srgba(0.38, 0.69, 0.94, 1.0), // #61afef
            str_: Color::srgba(0.60, 0.76, 0.47, 1.0), // #98c379
            num: Color::srgba(0.82, 0.60, 0.40, 1.0), // #d19a66
            ty: Color::srgba(0.90, 0.75, 0.48, 1.0), // #e5c07b
            com: Color::srgba(0.36, 0.39, 0.44, 1.0), // #5c6370
            punc: Color::srgba(0.67, 0.70, 0.75, 1.0), // #abb2bf
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// 间距常量（逻辑像素）。
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// 尺寸常量（逻辑像素）。
pub mod size {
    pub const TOP_BAR_H: f32 = 40.0;
    pub const STATUS_BAR_H: f32 = 24.0;
    pub const CHAT_SIDEBAR_W: f32 = 380.0;
    pub const FILE_PANEL_W: f32 = 240.0;
}
