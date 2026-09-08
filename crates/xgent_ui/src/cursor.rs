//! 鼠标指针样式（CursorStyle）：按 UI 区域显示对应系统指针。
//!
//! 数据驱动 + 单一全局系统（对齐 [`crate::kit::HoverTint`] 的架构约定）：
//! 业务节点挂 [`CursorStyle`] 组件声明期望指针，[`cursor_style_system`]
//! 每帧从 [`UiStack`]（back-to-front）找第一个「光标悬停 + 带 CursorStyle +
//! 可见」的节点，把对应 [`CursorIcon`] 写到主窗实体（winit 侧监听
//! `Changed<CursorIcon>` 应用，无变化帧零开销）。
//!
//! 判定复用 bevy_ui `ui_focus_system` 的产物：节点须挂
//! `RelativeCursorPosition`（bevy_ui 对所有含该组件的节点写 `cursor_over`，
//! 含可见性与裁剪判定），本模块提供 [`CursorHit`] 便捷 bundle。
//!
//! 区域映射（方案对齐 ui-prototype-v7）：
//! - 编辑器/文本输入 → [`SystemCursorIcon::Text`]（竖线 I-beam）
//! - 拖拽手柄（上下文面板分隔条）→ [`SystemCursorIcon::EwResize`]（左右箭头）
//! - 按钮/可点组件 → [`SystemCursorIcon::Pointer`]（手形）
//! - 默认（无匹配）→ 系统默认箭头

use bevy::prelude::*;
use bevy::ui::{RelativeCursorPosition, UiStack};
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

/// 节点期望的鼠标指针样式（挂于 UI 节点，配 [`CursorHit`] 或自带 `RelativeCursorPosition`）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    /// 编辑/文本输入：竖线 I-beam
    Text,
    /// 水平拖拽：左右箭头
    EwResize,
    /// 可点击：手形
    #[default]
    Pointer,
    /// 显式恢复默认箭头（用于覆盖子区域继承的 pointer）
    #[allow(dead_code)]
    Arrow,
}

/// 按钮节点的交互/样式/可见性查询元组。
type ButtonBits<'a> = (
    &'a Interaction,
    Option<&'a CursorStyle>,
    Option<&'a InheritedVisibility>,
);

impl CursorStyle {
    /// 映射到系统指针。
    fn to_icon(self) -> CursorIcon {
        CursorIcon::System(match self {
            Self::Text => SystemCursorIcon::Text,
            Self::EwResize => SystemCursorIcon::EwResize,
            Self::Pointer => SystemCursorIcon::Pointer,
            Self::Arrow => SystemCursorIcon::Default,
        })
    }
}

/// 光标命中判定 bundle：挂于带 [`CursorStyle`] 的节点即可参与判定。
///
/// `ui_focus_system` 对所有含 `RelativeCursorPosition` 的节点写 `cursor_over`
/// （含 `InheritedVisibility` 与祖先裁剪判定），本组件仅是便捷构造。
#[derive(Bundle, Debug, Default)]
pub struct CursorHit {
    /// 命中判定（bevy_ui 每帧写入 cursor_over）
    pub hit: RelativeCursorPosition,
}

/// 每帧据 hover 焦点更新主窗指针。
///
/// 遍历 [`UiStack`]（back-to-front，最上层优先），找第一个
/// 「`RelativeCursorPosition::cursor_over == true` + 可见 + 带 [`CursorStyle`]」
/// 的节点；找不到（光标在 UI 之外/空白画布）回退系统默认箭头。
pub fn cursor_style_system(
    stack: Res<UiStack>,
    windows: Query<Entity, With<PrimaryWindow>>,
    // Button 节点自带 RelativeCursorPosition 吗？不一定——交互判定只要求
    // Interaction；为此对 Button 单独查询（bevy_ui 对所有含 RelativeCursorPosition
    // 的节点写 cursor_over，Button 若无该组件则用 Interaction::Hovered 判定）
    styled: Query<(
        &RelativeCursorPosition,
        &CursorStyle,
        Option<&InheritedVisibility>,
    )>,
    buttons: Query<ButtonBits, With<Button>>,
    mut commands: Commands,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let visible = |vis: Option<&InheritedVisibility>| vis.is_none_or(|v| v.get());

    // UiStack 为 back-to-front：倒序遍历，第一个命中的即视觉最上层。
    // 优先级：显式 CursorStyle（Text/EwResize 等特化）> Button（Pointer）。
    let want: Option<CursorStyle> = stack.uinodes.iter().rev().find_map(|e| {
        if let Ok((hit, style, vis)) = styled.get(*e) {
            if hit.cursor_over && visible(vis) {
                return Some(*style);
            }
            return None;
        }
        if let Ok((interaction, style, vis)) = buttons.get(*e)
            && *interaction != Interaction::None
            && visible(vis)
        {
            return Some(style.copied().unwrap_or(CursorStyle::Pointer));
        }
        None
    });

    let icon = want.map(|s| s.to_icon()).unwrap_or_default();
    // winit 侧以 Changed<CursorIcon> 过滤，值不变帧无副作用
    commands.entity(window).insert(icon);
}

/// 指针样式插件。
pub struct CursorStylePlugin;

impl Plugin for CursorStylePlugin {
    fn build(&self, app: &mut App) {
        // 须在 ui_focus_system 写完 RelativeCursorPosition/Interaction 之后跑
        app.add_systems(
            Update,
            cursor_style_system.after(bevy::ui::UiSystems::Focus),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CursorStyle → 系统指针映射契约。
    #[test]
    fn style_maps_to_system_icon() {
        assert_eq!(
            CursorStyle::Text.to_icon(),
            CursorIcon::System(SystemCursorIcon::Text)
        );
        assert_eq!(
            CursorStyle::EwResize.to_icon(),
            CursorIcon::System(SystemCursorIcon::EwResize)
        );
        assert_eq!(
            CursorStyle::Pointer.to_icon(),
            CursorIcon::System(SystemCursorIcon::Pointer)
        );
        assert_eq!(
            CursorStyle::Arrow.to_icon(),
            CursorIcon::System(SystemCursorIcon::Default)
        );
    }

    /// 无匹配时回退系统默认（CursorIcon::default() == Default 箭头）。
    #[test]
    fn fallback_is_system_default() {
        assert_eq!(
            CursorIcon::default(),
            CursorIcon::System(SystemCursorIcon::Default)
        );
    }
}
