//! 主题：三层深度配色体系 + 间距/尺寸常量。
//!
//! v2 重构：采用 ui-ux-pro-max 推荐的 Developer Tool 调色板，
//! 三层视觉深度（bg < panel < elevated），绿色强调色。
//! MVP 仅暗色主题（K-01 主题增强留待 P1）。

use bevy::prelude::*;

/// 暗色主题颜色表（v2 三层深度体系）。
#[derive(Resource, Debug, Clone, Copy)]
pub struct Theme {
    // ===== 三层深度 =====
    /// L0 最深层 — 全局背景
    pub bg: Color,
    /// L0.5 — 对话区背景，略浅于 bg
    pub surface: Color,
    /// L1 面板 — 文件树/侧栏/卡片底
    pub panel: Color,
    /// L2 提升 — hover/active/floating
    pub elevated: Color,
    /// L-1 最深 — 代码块/终端
    pub deep: Color,

    // ===== 边框丝线 =====
    /// 微弱分隔线
    pub line: Color,
    /// 标准边框
    pub border: Color,
    /// 顶栏/状态栏背景（兼容旧代码，= bg 略深）
    pub bar: Color,

    // ===== 文字 =====
    /// 标题/主文字
    pub text: Color,
    /// 次要文字（兼容旧代码 = text_dim）
    pub text_dim: Color,
    /// 弱化文字/placeholder
    pub text_muted: Color,

    // ===== 强调色 =====
    /// 主强调 — 绿色（CTA/运行/成功）
    pub accent: Color,
    /// 用户消息气泡（兼容旧代码）
    pub bubble_user: Color,
    /// 助手消息气泡（兼容旧代码 = panel）
    pub bubble_assistant: Color,

    // ===== 字体 =====
    /// 字体大小（逻辑像素）
    pub font_size: f32,

    // ===== 状态色 =====
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

    // ===== 语法高亮色 =====
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
    /// 暗色主题（v2 Developer Tool 调色板）。
    pub fn dark() -> Self {
        Self {
            // 三层深度 — slate 系列
            bg: Color::srgba(0.043, 0.067, 0.125, 1.0),       // #0B1120
            surface: Color::srgba(0.059, 0.086, 0.137, 1.0),   // #0F1623
            panel: Color::srgba(0.075, 0.102, 0.168, 1.0),     // #131A2B
            elevated: Color::srgba(0.110, 0.149, 0.251, 1.0),  // #1C2640
            deep: Color::srgba(0.024, 0.039, 0.078, 1.0),      // #060A14

            // 边框丝线
            line: Color::srgba(0.58, 0.64, 0.72, 0.10),       // rgba(148,163,184,0.10)
            border: Color::srgba(0.58, 0.64, 0.72, 0.18),      // rgba(148,163,184,0.18)
            bar: Color::srgba(0.039, 0.059, 0.110, 1.0),       // 略深于 bg

            // 文字
            text: Color::srgba(0.945, 0.961, 0.976, 1.0),      // #F1F5F9
            text_dim: Color::srgba(0.796, 0.835, 0.882, 1.0),  // #CBD5E1
            text_muted: Color::srgba(0.392, 0.455, 0.545, 1.0), // #64748B

            // 强调色 — emerald green
            accent: Color::srgba(0.133, 0.773, 0.369, 1.0),    // #22C55E
            bubble_user: Color::srgba(0.118, 0.227, 0.373, 1.0), // #1E3A5F
            bubble_assistant: Color::srgba(0.075, 0.102, 0.168, 1.0), // = panel

            font_size: 13.5,

            // 状态色
            st_pending: Color::srgba(0.878, 0.702, 0.255, 1.0), // #E0B341
            st_running: Color::srgba(0.231, 0.510, 0.965, 1.0), // #3B82F6
            st_ok: Color::srgba(0.133, 0.773, 0.369, 1.0),     // #22C55E
            st_fail: Color::srgba(0.937, 0.267, 0.267, 1.0),   // #EF4444
            st_deny: Color::srgba(0.392, 0.455, 0.545, 1.0),   // #64748B

            // 语法高亮色
            kw: Color::srgba(0.753, 0.518, 0.988, 1.0),   // #C084FC
            fn_: Color::srgba(0.376, 0.647, 0.980, 1.0),  // #60A5FA
            str_: Color::srgba(0.525, 0.937, 0.675, 1.0), // #86EFAC
            num: Color::srgba(0.984, 0.749, 0.145, 1.0),  // #FBBF24
            ty: Color::srgba(0.988, 0.827, 0.302, 1.0),   // #FCD34D
            com: Color::srgba(0.392, 0.455, 0.545, 1.0),  // #64748B
            punc: Color::srgba(0.796, 0.835, 0.882, 1.0), // #CBD5E1
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// 间距常量（逻辑像素，4px 网格）。
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 20.0;
    pub const XXL: f32 = 24.0;
    pub const XXXL: f32 = 32.0;
}

/// 尺寸常量（逻辑像素）。
pub mod size {
    /// 顶栏高度
    pub const TOP_BAR_H: f32 = 48.0;
    /// 状态栏高度
    pub const STATUS_BAR_H: f32 = 28.0;
    /// 活动栏宽度
    pub const ACTIVITY_BAR_W: f32 = 48.0;
    /// 文件面板宽度
    pub const FILE_PANEL_W: f32 = 240.0;
    /// 对话侧栏（SideView）默认宽度
    pub const CHAT_SIDEBAR_W: f32 = 380.0;
    /// 视图标签条高度
    pub const VIEW_TABS_H: f32 = 36.0;
    /// 编辑器 tab 条高度
    pub const EDITOR_TABS_H: f32 = 32.0;
    /// 终端 tab 条高度
    pub const TERMINAL_TABS_H: f32 = 32.0;
}

/// 便捷：f32 → Val::Px（跨模块共享，避免重复定义）。
pub fn px(v: f32) -> Val {
    Val::Px(v)
}
