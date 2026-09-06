//! 主题：Linear 视觉基准（v7）——近黑画布、四级灰阶文字、单一靛紫强调，
//! 层级用「背景明度阶梯 + 半透明白细边框」表达，非浮层元素零投影。
//!
//! 色值即规范表：ADR-0014 与 `doc/plans/ui-v7-migration.md` §4.2。
//! 状态色为 Radix 深色阶外推、陪伴暖色为唯一例外——均见 ADR 裁剪清单。
//! MVP 仅暗色预设（K-01 亮色留待 P1；半透明叠加组换黑色系即可）。

use bevy::prelude::*;

/// 暗色主题（v7，Linear 基准）。
#[derive(Resource, Debug, Clone, Copy)]
pub struct Theme {
    // ===== 表面：明度阶梯 =====
    /// L0 会话画布（最深层）#08090A
    pub bg: Color,
    /// L1 顶栏/图标轨/上下文面板 #0F1011
    pub surface: Color,
    /// L2 浮层/下拉/命令面板 #191A1B
    pub elevated: Color,
    /// 代码/终端/预览底 #0D0E10
    pub code_bg: Color,

    // ===== 交互面（半透明白叠加；亮色主题时换黑色系）=====
    /// 卡片/工具卡底 white@0.02
    pub subtle: Color,
    /// 输入类底 white@0.02
    pub input_bg: Color,
    /// 悬停步进 white@0.05
    pub hover: Color,
    /// 按下/选中步进 white@0.08
    pub active: Color,
    /// 图标底/小徽标 white@0.06
    pub icon_bg: Color,

    // ===== 边框：半透明白丝线 =====
    /// 弱分隔（面板边界）white@0.05
    pub line: Color,
    /// 标准边框 white@0.08
    pub border: Color,
    /// 悬停边框 white@0.14
    pub border_hover: Color,

    // ===== 文字：四级灰阶 =====
    /// 标题/主文字 #F7F8F8（禁用纯白）
    pub text: Color,
    /// 正文/次要 #D0D6E0
    pub text_dim: Color,
    /// 弱化/placeholder #8A8F98
    pub text_muted: Color,
    /// 最弱（时间戳/行号/禁用）#62666D
    pub text_faint: Color,

    // ===== 强调色：唯一彩色系统（靛紫）=====
    /// 主色（CTA/主按钮/品牌块）#5E6AD2
    pub accent: Color,
    /// 交互强调（链接/active/选中）#7170FF
    pub accent_interactive: Color,
    /// 强调悬停 #828FFF
    pub accent_hover: Color,
    /// 强调薄底 rgba(94,106,210,0.14)
    pub accent_bg: Color,
    /// 强调辉光（focus 环/拖拽条高亮）rgba(113,112,255,0.16)
    pub accent_glow: Color,
    /// 强调色上的文字 #FFFFFF
    pub accent_text: Color,

    // ===== 功能状态色（Radix 深色阶，外推值见 ADR-0014 裁剪#1）=====
    /// 待确认 #FFB224
    pub st_pending: Color,
    /// 执行中 #7170FF
    pub st_running: Color,
    /// 成功 #30A46C
    pub st_ok: Color,
    /// 失败 #E5484D
    pub st_fail: Color,
    /// 已拒绝（与 fail 同色，可调）
    pub st_deny: Color,
    /// 成功薄底 rgba(48,164,108,0.12)
    pub st_ok_bg: Color,
    /// 待确认薄底 rgba(255,178,36,0.12)
    pub st_pending_bg: Color,
    /// 失败薄底 rgba(229,72,77,0.12)
    pub st_fail_bg: Color,
    /// 信息 #0091FF
    pub st_info: Color,
    /// 信息薄底 rgba(0,145,255,0.12)
    pub st_info_bg: Color,

    // ===== 陪伴暖色（全界面唯一暖色例外，ADR-0014 裁剪#3）=====
    /// 陪伴主色 #FFB224
    pub warm: Color,
    /// 陪伴薄底/激活环 rgba(255,178,36,0.15)
    pub warm_bg: Color,

    // ===== 反色浮层（tooltip/toast，两主题恒为暗底）=====
    /// tooltip/toast 底 #28282C
    pub tooltip_bg: Color,
    /// tooltip/toast 文字 #F7F8F8
    pub tooltip_text: Color,

    // ===== 半透明覆盖 =====
    /// 抽屉/弹窗遮罩 black@0.5
    pub overlay: Color,

    // ===== 代码语法（冷调低饱和，与靛紫体系一致）=====
    /// 代码正文 #D0D6E0
    pub code_text: Color,
    /// 关键字 #B3A5FF
    pub kw: Color,
    /// 函数名 #7FB3FF
    pub fn_: Color,
    /// 字符串 #56C08D
    pub str_: Color,
    /// 数字 #E2A35C
    pub num: Color,
    /// 类型名 #6FD3C7
    pub ty: Color,
    /// 注释 #62666D
    pub com: Color,
    /// 标点 #8A8F98（编辑器高亮链消费，勿删——方案 §4.1）
    pub punc: Color,

    // ===== 排版 =====
    /// 正文基准字号（逻辑像素）
    pub font_size: f32,
}

impl Theme {
    /// 暗色主题（v7，Linear 基准）。
    pub fn dark() -> Self {
        Self {
            // 表面明度阶梯
            bg: Color::srgba(0.0314, 0.0353, 0.0392, 1.0), // #08090A
            surface: Color::srgba(0.0588, 0.0627, 0.0667, 1.0), // #0F1011
            elevated: Color::srgba(0.0980, 0.1020, 0.1059, 1.0), // #191A1B
            code_bg: Color::srgba(0.0510, 0.0549, 0.0627, 1.0), // #0D0E10

            // 交互面（半透明白）
            subtle: Color::srgba(1.0, 1.0, 1.0, 0.02),
            input_bg: Color::srgba(1.0, 1.0, 1.0, 0.02),
            hover: Color::srgba(1.0, 1.0, 1.0, 0.05),
            active: Color::srgba(1.0, 1.0, 1.0, 0.08),
            icon_bg: Color::srgba(1.0, 1.0, 1.0, 0.06),

            // 边框丝线
            line: Color::srgba(1.0, 1.0, 1.0, 0.05),
            border: Color::srgba(1.0, 1.0, 1.0, 0.08),
            border_hover: Color::srgba(1.0, 1.0, 1.0, 0.14),

            // 文字四级灰阶
            text: Color::srgba(0.9686, 0.9725, 0.9725, 1.0), // #F7F8F8
            text_dim: Color::srgba(0.8157, 0.8392, 0.8784, 1.0), // #D0D6E0
            text_muted: Color::srgba(0.5412, 0.5608, 0.5961, 1.0), // #8A8F98
            text_faint: Color::srgba(0.3843, 0.4000, 0.4275, 1.0), // #62666D

            // 强调（靛紫）
            accent: Color::srgba(0.3686, 0.4157, 0.8235, 1.0), // #5E6AD2
            accent_interactive: Color::srgba(0.4431, 0.4392, 1.0, 1.0), // #7170FF
            accent_hover: Color::srgba(0.5098, 0.5608, 1.0, 1.0), // #828FFF
            accent_bg: Color::srgba(0.3686, 0.4157, 0.8235, 0.14),
            accent_glow: Color::srgba(0.4431, 0.4392, 1.0, 0.16),
            accent_text: Color::WHITE,

            // 状态色
            st_pending: Color::srgba(1.0, 0.6980, 0.1412, 1.0), // #FFB224
            st_running: Color::srgba(0.4431, 0.4392, 1.0, 1.0), // #7170FF
            st_ok: Color::srgba(0.1882, 0.6431, 0.4235, 1.0),   // #30A46C
            st_fail: Color::srgba(0.8980, 0.2824, 0.3020, 1.0), // #E5484D
            st_deny: Color::srgba(0.8980, 0.2824, 0.3020, 1.0), // #E5484D
            st_ok_bg: Color::srgba(0.1882, 0.6431, 0.4235, 0.12),
            st_pending_bg: Color::srgba(1.0, 0.6980, 0.1412, 0.12),
            st_fail_bg: Color::srgba(0.8980, 0.2824, 0.3020, 0.12),
            st_info: Color::srgba(0.0, 0.5686, 1.0, 1.0), // #0091FF
            st_info_bg: Color::srgba(0.0, 0.5686, 1.0, 0.12),

            // 陪伴暖色（唯一例外）
            warm: Color::srgba(1.0, 0.6980, 0.1412, 1.0), // #FFB224
            warm_bg: Color::srgba(1.0, 0.6980, 0.1412, 0.15),

            // 反色浮层
            tooltip_bg: Color::srgba(0.1569, 0.1569, 0.1725, 1.0), // #28282C
            tooltip_text: Color::srgba(0.9686, 0.9725, 0.9725, 1.0), // #F7F8F8

            // 遮罩
            overlay: Color::srgba(0.0, 0.0, 0.0, 0.5),

            // 代码语法
            code_text: Color::srgba(0.8157, 0.8392, 0.8784, 1.0), // #D0D6E0
            kw: Color::srgba(0.7020, 0.6471, 1.0, 1.0),           // #B3A5FF
            fn_: Color::srgba(0.4980, 0.7020, 1.0, 1.0),          // #7FB3FF
            str_: Color::srgba(0.3373, 0.7529, 0.5529, 1.0),      // #56C08D
            num: Color::srgba(0.8863, 0.6392, 0.3608, 1.0),       // #E2A35C
            ty: Color::srgba(0.4353, 0.8275, 0.7804, 1.0),        // #6FD3C7
            com: Color::srgba(0.3843, 0.4000, 0.4275, 1.0),       // #62666D
            punc: Color::srgba(0.5412, 0.5608, 0.5961, 1.0),      // #8A8F98

            font_size: 14.0,
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
    pub const TOP_BAR_H: f32 = 52.0;
    /// 状态栏高度
    pub const STATUS_BAR_H: f32 = 32.0;
    /// 图标轨宽度（原活动栏）
    pub const RAIL_W: f32 = 52.0;
    /// 上下文面板默认宽度
    pub const CONTEXT_W_DEFAULT: f32 = 720.0;
    /// 上下文面板最小宽度
    pub const CONTEXT_W_MIN: f32 = 380.0;
    /// 会话主区最小宽度（拖拽钳制）
    pub const CHAT_MIN: f32 = 520.0;
    /// 上下文面板页签条高度
    pub const CONTEXT_TABS_H: f32 = 38.0;
    /// 抽屉宽度
    pub const DRAWER_W: f32 = 320.0;
    /// 拖拽分隔条宽度
    pub const RESIZER_W: f32 = 6.0;
    /// 文件面板宽度（过渡期；M5 抽屉化后删除）
    pub const FILE_PANEL_W: f32 = 240.0;
    /// 旧·会话侧栏默认宽度（M2-T1 删除）
    pub const CHAT_SIDEBAR_W: f32 = 380.0;
    /// 旧·视图标签条高度（M4-T6 删除）
    pub const VIEW_TABS_H: f32 = 36.0;
    /// 编辑器 tab 条高度
    pub const EDITOR_TABS_H: f32 = 32.0;
    /// 终端 tab 条高度
    pub const TERMINAL_TABS_H: f32 = 32.0;
}

/// 排版阶梯（v7，原型唯一字号集合；新界面禁止就地发明字号）。
pub mod type_scale {
    /// welcome 标题（配负字距 -0.29）
    pub const DISPLAY: f32 = 24.0;
    /// 弹窗标题/顶栏品牌
    pub const H3: f32 = 15.0;
    /// 消息正文/输入框
    pub const BODY: f32 = 14.0;
    /// 次要正文/按钮
    pub const BODY_SM: f32 = 13.0;
    /// 工具名/按钮小字/pill
    pub const SMALL: f32 = 12.5;
    /// chips/参数/代码
    pub const CAPTION: f32 = 12.0;
    /// 时间戳/元信息/kbd
    pub const MICRO: f32 = 11.0;
    /// 大写分区标签/徽标
    pub const TINY: f32 = 10.0;
    /// 代码（终端另 -2）
    pub const MONO: f32 = 12.0;

    /// 行高档（配合字号使用；原型正文 1.6、UI 1.4-1.5、代码 1.5）
    pub mod line_height {
        /// 展示级标题
        pub const TIGHT: f32 = 1.2;
        /// 紧凑 UI 文本/时间戳
        pub const UI: f32 = 1.4;
        /// 控件/按钮/代码
        pub const CTRL: f32 = 1.5;
        /// 正文
        pub const BODY: f32 = 1.6;
        /// 终端
        pub const TERM: f32 = 1.6;
    }
}

/// 圆角阶梯（v7，Linear 尺度；胶囊形用 `BorderRadius::MAX`）。
pub mod radius {
    /// 行内徽标/微元素
    pub const MICRO: f32 = 2.0;
    /// 小元素
    pub const SMALL: f32 = 4.0;
    /// 按钮/输入等控件
    pub const CTRL: f32 = 6.0;
    /// 卡片
    pub const CARD: f32 = 8.0;
    /// 浮层/弹窗
    pub const PANEL: f32 = 12.0;
}

/// 便捷：f32 → Val::Px（跨模块共享，避免重复定义）。
pub fn px(v: f32) -> Val {
    Val::Px(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 关键令牌值断言（防误改；规范表见方案 §4.2）。
    #[test]
    fn dark_tokens_match_v7_spec() {
        let t = Theme::dark();
        let bg = t.bg.to_srgba();
        assert!((bg.red - 0.0314).abs() < 0.001, "bg 应为 #08090A");
        assert!((bg.green - 0.0353).abs() < 0.001, "bg 应为 #08090A");

        let surface = t.surface.to_srgba();
        assert!((surface.red - 0.0588).abs() < 0.001, "surface 应为 #0F1011");

        let accent = t.accent.to_srgba();
        assert!((accent.red - 0.3686).abs() < 0.001, "accent 应为 #5E6AD2");
        assert!((accent.blue - 0.8235).abs() < 0.001, "accent 应为 #5E6AD2");
    }

    /// 正文/背景对比度门槛（简化 luma，暗色主题可读性护栏）。
    #[test]
    fn text_contrast_above_threshold() {
        let t = Theme::dark();
        let luma = |c: Color| {
            let s = c.to_srgba();
            0.2126 * s.red + 0.7152 * s.green + 0.0722 * s.blue
        };
        let ratio_main = (luma(t.text) + 0.05) / (luma(t.bg) + 0.05);
        assert!(
            ratio_main > 4.5,
            "主文字/画布对比度 {ratio_main:.2} 应 > 4.5"
        );
        let ratio_dim = (luma(t.text_dim) + 0.05) / (luma(t.bg) + 0.05);
        assert!(
            ratio_dim > 4.5,
            "次级文字/画布对比度 {ratio_dim:.2} 应 > 4.5"
        );
    }
}
