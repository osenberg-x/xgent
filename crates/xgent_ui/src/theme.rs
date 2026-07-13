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
    pub const FILE_PANEL_W: f32 = 260.0;
}
