//! UI 通用小件库（v7）——依赖 Theme/UiFonts，业务层专用。
//!
//! 分层约定：xui 保持「纯 bevy + xui_i18n」不动，视觉组件统一落在本模块
//! （方案 §9）。图标为白描边 PNG（导出管线 `docs/design/icons/export_png.py`），
//! 经 `ImageNode.color` 乘法染色。hover 一律走 [`HoverTint`] 数据驱动单系统，
//! 禁止逐组件写专用 hover 系统（方案 §7.1）。

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{FontSize, FontWeight, LetterSpacing, LineHeight};
use bevy::ui::Pressed;
use bevy::ui_widgets::{Button, observe};
use std::collections::HashMap;

use crate::fonts::{UiFonts, mono_text, ui_text};
use crate::theme::{Theme, radius, space, type_scale};

// ===== 图标 =====

/// 图标名清单（`assets/icons/{name}@2x.png`，导出管线见 export_png.py）。
const ICON_NAMES: &[&str] = &[
    "chat",
    "folder",
    "clock",
    "terminal",
    "star",
    "plus",
    "x",
    "check",
    "copy",
    "retry",
    "refresh",
    "send",
    "command",
    "panel-right",
    "chevron-down",
    "chevron-right",
    "info",
    "file",
    "diff",
    "dollar",
    "gear",
    "alert-triangle",
];

/// 图标句柄表（启动经 AssetServer 加载；资产根见 xgent_app 的 AssetPlugin 配置）。
#[derive(Resource, Debug, Clone, Default)]
pub struct IconAssets {
    map: HashMap<&'static str, Handle<Image>>,
}

impl IconAssets {
    /// 经 AssetServer 加载全部清单图标。
    pub fn load(server: &AssetServer) -> Self {
        let map = ICON_NAMES
            .iter()
            .map(|n| (*n, server.load(icon_path(n))))
            .collect();
        Self { map }
    }

    /// 取图标句柄；未知名称回退默认句柄（渲染为空，不 panic）。
    pub fn get(&self, name: &str) -> Handle<Image> {
        self.map.get(name).cloned().unwrap_or_default()
    }
}

fn icon_path(name: &str) -> String {
    format!("icons/{name}@2x.png")
}

/// 单色图标节点（48px 白描边 PNG 按 `px_size` 逻辑像素显示，乘法染色）。
pub fn icon(icons: &IconAssets, name: &str, px_size: f32, color: Color) -> impl Bundle {
    (
        Node {
            width: Val::Px(px_size),
            height: Val::Px(px_size),
            flex_shrink: 0.0,
            ..default()
        },
        ImageNode {
            color,
            image: icons.get(name),
            ..default()
        },
    )
}

// ===== hover =====

/// 悬停/按下自动换色（数据驱动；单一全局系统处理全部挂载节点）。
#[derive(Component, Debug, Clone)]
pub struct HoverTint {
    pub base_bg: Color,
    pub hover_bg: Color,
    pub base_border: Color,
    pub hover_border: Color,
}

impl HoverTint {
    /// 常规交互件：`subtle` 底 + `border` 边，hover 步进。
    pub fn standard(theme: &Theme) -> Self {
        Self {
            base_bg: theme.subtle,
            hover_bg: theme.hover,
            base_border: theme.border,
            hover_border: theme.border_hover,
        }
    }

    /// 透明底变体（chips/分段/图标钮）。
    pub fn ghost(theme: &Theme) -> Self {
        Self {
            base_bg: Color::NONE,
            hover_bg: theme.hover,
            base_border: Color::NONE,
            hover_border: theme.border_hover,
        }
    }
}

/// hover/按下换色（单系统遍历全部 `HoverTint` 节点；无过滤全量轮询，
/// 写入前比较避免无谓的组件变更——对齐官方 widgets 示例的每帧刷新模式）。
fn hover_tint_system(
    theme: Res<Theme>,
    mut q: Query<
        (
            &Hovered,
            Has<Pressed>,
            &HoverTint,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<HoverTint>,
    >,
) {
    for (hovered, pressed, tint, mut bg, mut border) in q.iter_mut() {
        let (bg_want, line_want) = if pressed {
            (theme.active, tint.hover_border)
        } else if hovered.get() {
            (tint.hover_bg, tint.hover_border)
        } else {
            (tint.base_bg, tint.base_border)
        };
        if bg.0 != bg_want {
            bg.0 = bg_want;
        }
        if (border.top != line_want)
            || (border.right != line_want)
            || (border.bottom != line_want)
            || (border.left != line_want)
        {
            border.set_all(line_want);
        }
    }
}

// ===== 面板按压防护 =====

/// 「面板压遮罩」防护：0.20 的指针事件会从子实体向 Button 祖先冒泡
/// （ButtonPlugin 全局 observer 逐级运行），遮罩 Button 上的普通面板被点击时
/// 事件会冒泡到遮罩 → 误触发"点面板外关闭"。把本 bundle 挂到面板实体上，
/// 在面板处消费 `PointerPress`，等价旧 `FocusPolicy::Block` 的阻断语义。
///
/// 实现注意：必须经 [`observe`] 助手挂**实体作用域** observer（其 bundle effect
/// 调用 `entity.observe()`）；直接插裸 `Observer` 组件是全局观察者，会消费
/// 全世界的 PointerPress。
///
/// 面板内的按钮不受影响：内层 Button 的事件由 ButtonPlugin 在自身消费
/// （propagate(false)），不会到达面板这一层。
pub fn block_press_bubbling() -> impl Bundle {
    observe(|mut press: On<bevy::picking::events::PointerPress>| {
        press.propagate(false);
    })
}

// ===== 层根 =====

/// 静态层根 bundle：全屏绝对定位、透明、**不带 `Pickable`**（`require_markers = true`
/// 下对拾取完全惰性）。业务模块只向对应层根挂子树。
pub fn layer_root(band: i32) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            ..default()
        },
        GlobalZIndex(band),
        BackgroundColor(Color::NONE),
        crate::layout::LayerRoot(band),
    )
}

/// 抽屉层 z。数值只在 `theme::z` 定义；抽屉模块经此挂载，不裸写数字。
pub fn drawer_z() -> GlobalZIndex {
    GlobalZIndex(crate::theme::z::DRAWER)
}

/// 抽屉层内的相对 z（例如面板压在遮罩之上）。
pub fn drawer_z_offset(offset: i32) -> GlobalZIndex {
    GlobalZIndex(crate::theme::z::DRAWER + offset)
}

// ===== tooltip =====

/// tooltip 内容（挂在带 `Button` 的节点上；持续悬停 500ms 后浮现）。
#[derive(Component, Debug, Clone)]
pub struct Tooltip {
    pub text: String,
}

/// tooltip 悬停计时（挂于宿主按钮）。
#[derive(Component, Debug, Clone, Copy)]
struct TooltipPending {
    elapsed: f32,
}

/// 已浮现的 tooltip 面板（挂在 `TOOLTIP` 层根下，记录宿主以便定位与清理）。
#[derive(Component, Debug, Clone, Copy)]
pub struct TooltipPanel {
    /// 宿主实体。
    pub host: Entity,
}

/// tooltip 系统：悬停计时 500ms → 在 `TOOLTIP` 层根下 spawn 面板；离开即清。
///
/// v8 M1：面板不再作为宿主子节点，因此不会被 `MainArea`/`ChatPanel` 的
/// `Overflow::clip()` 裁掉（审计 B3）。
fn tooltip_system(
    mut commands: Commands,
    time: Res<Time>,
    theme: Res<Theme>,
    fonts: Res<UiFonts>,
    mut q: Query<(Entity, &Hovered, &Tooltip, Option<&mut TooltipPending>), With<Button>>,
    panels: Query<(Entity, &TooltipPanel)>,
    hosts: Query<Entity, With<Tooltip>>,
    layer: Query<Entity, With<crate::layout::TooltipLayer>>,
) {
    let layer_entity = layer.single().ok();

    for (entity, hovered, tip, pending) in q.iter_mut() {
        let panel = panels
            .iter()
            .find(|(_, panel)| panel.host == entity)
            .map(|(panel_entity, _)| panel_entity);

        if hovered.get() {
            if panel.is_some() {
                continue;
            }
            match pending {
                Some(mut p) => {
                    p.elapsed += time.delta_secs();
                    if p.elapsed >= 0.5 {
                        commands.entity(entity).remove::<TooltipPending>();
                        spawn_tooltip_panel(
                            &mut commands,
                            layer_entity,
                            entity,
                            &tip.text,
                            &theme,
                            &fonts,
                        );
                    }
                }
                None => {
                    commands
                        .entity(entity)
                        .insert(TooltipPending { elapsed: 0.0 });
                }
            }
        } else {
            if pending.is_some() {
                commands.entity(entity).remove::<TooltipPending>();
            }
            if let Some(panel) = panel {
                commands.entity(panel).despawn();
            }
        }
    }

    // 宿主已消失的面板一并清理（宿主被 despawn 时不会触发 hovered=false）。
    for (panel_entity, panel) in &panels {
        if hosts.get(panel.host).is_err() {
            commands.entity(panel_entity).despawn();
        }
    }
}

fn spawn_tooltip_panel(
    commands: &mut Commands,
    layer: Option<Entity>,
    host: Entity,
    text: &str,
    theme: &Theme,
    fonts: &UiFonts,
) {
    let mut spawn = |parent: Option<Entity>| {
        let bundle = (
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(0.0),
                padding: UiRect::all(px(space::XS)),
                border_radius: BorderRadius::all(px(radius::SMALL)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.tooltip_bg),
            BorderColor::all(theme.border),
            // 穿透装饰层：不吃点击、不参与 hover。
            Pickable {
                should_block_lower: false,
                is_hoverable: false,
            },
            TooltipPanel { host },
        );
        let mut entity = commands.spawn(bundle);
        if let Some(parent) = parent {
            entity.insert(ChildOf(parent));
        }
        entity.with_children(|tip_node| {
            tip_node.spawn(mono_text(
                fonts,
                text.to_string(),
                type_scale::MICRO,
                theme.tooltip_text,
                type_scale::line_height::UI,
            ));
        });
    };

    if let Some(layer) = layer {
        spawn(Some(layer));
    } else {
        spawn(None);
    }
}

/// 计算 tooltip 的左边界：默认贴宿主右侧；右缘空间不足时翻转到左侧。
pub fn tooltip_left(
    host_left: f32,
    host_right: f32,
    tip_width: f32,
    viewport_width: f32,
    gap: f32,
) -> f32 {
    if host_right + gap + tip_width <= viewport_width {
        host_right + gap
    } else {
        (host_left - gap - tip_width).max(0.0)
    }
}

/// tooltip 定位：按宿主 `ComputedNode` 与 `GlobalTransform` 计算屏幕位置；
/// 右缘不足时翻转到宿主左侧。每帧跟随（窗口缩放、滚动、布局变化都能跟上）。
fn position_tooltips(
    windows: Query<&Window>,
    mut tips: Query<(&TooltipPanel, &mut Node, &ComputedNode)>,
    hosts: Query<(&ComputedNode, &GlobalTransform), Without<TooltipPanel>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let scale = window.scale_factor().max(0.0001);
    let viewport_width = window.width();

    for (panel, mut node, tip_computed) in &mut tips {
        let Ok((host_computed, host_transform)) = hosts.get(panel.host) else {
            continue;
        };
        let host_size = host_computed.size() / scale;
        let host_center = host_transform.translation().truncate() / scale;
        let host_left = host_center.x - host_size.x / 2.0;
        let host_top = host_center.y - host_size.y / 2.0;
        let host_right = host_center.x + host_size.x / 2.0;

        let tip_width = tip_computed.size().x / scale;
        let gap = space::SM;

        let left = tooltip_left(host_left, host_right, tip_width, viewport_width, gap);
        node.left = px(left);
        node.top = px(host_top);
    }
}

// ===== 组件生成器 =====

/// 生成器视图：把 Theme/IconAssets/UiFonts 打包传给各 spawn 系统，
/// 免去逐系统三连 Res 注入。
pub struct UiKit<'a> {
    pub theme: &'a Theme,
    pub icons: &'a IconAssets,
    pub fonts: &'a UiFonts,
}

impl UiKit<'_> {
    /// ghost 按钮：`subtle` 底 + `border` + 6px 圆角 + 图标 + 文本（SMALL/510）。
    pub fn ghost_button(
        &self,
        parent: &mut ChildSpawnerCommands,
        label: &str,
        icon_name: &str,
    ) -> Entity {
        parent
            .spawn((
                Button,
                Hovered::default(),
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(space::XS),
                    padding: UiRect::horizontal(px(space::SM + 1.0)),
                    border_radius: BorderRadius::all(px(radius::CTRL)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(self.theme.subtle),
                BorderColor::all(self.theme.border),
                HoverTint::standard(self.theme),
            ))
            .with_children(|b| {
                b.spawn(icon(self.icons, icon_name, 14.0, self.theme.text_dim));
                b.spawn(ui_text(
                    label,
                    type_scale::SMALL,
                    510,
                    self.theme.text_dim,
                    type_scale::line_height::UI,
                ));
            })
            .id()
    }

    /// primary 按钮：`accent` 底白字（hover `accent_hover`）。
    pub fn primary_button(
        &self,
        parent: &mut ChildSpawnerCommands,
        label: &str,
        icon_name: &str,
    ) -> Entity {
        parent
            .spawn((
                Button,
                Hovered::default(),
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(space::XS),
                    padding: UiRect::horizontal(px(space::SM + 1.0)),
                    border_radius: BorderRadius::all(px(radius::CTRL)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(self.theme.accent),
                BorderColor::all(Color::NONE),
            ))
            .with_children(|b| {
                b.spawn(icon(self.icons, icon_name, 14.0, self.theme.accent_text));
                b.spawn(ui_text(
                    label,
                    type_scale::SMALL,
                    510,
                    self.theme.accent_text,
                    type_scale::line_height::UI,
                ));
            })
            .id()
    }

    /// 图标钮（34px 方形圆角 6，透明底；`tip` 为 tooltip 文案）。
    pub fn icon_button(
        &self,
        parent: &mut ChildSpawnerCommands,
        icon_name: &str,
        tip: &str,
    ) -> Entity {
        parent
            .spawn((
                Button,
                Hovered::default(),
                Node {
                    width: px(34.0),
                    height: px(34.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(px(radius::CTRL)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::NONE),
                HoverTint::ghost(self.theme),
                Tooltip {
                    text: tip.to_string(),
                },
            ))
            .with_children(|b| {
                b.spawn(icon(self.icons, icon_name, 18.0, self.theme.text_muted));
            })
            .id()
    }

    /// 胶囊 pill（9999 圆角、透明底 + `border`，SMALL/510）。
    pub fn pill(&self, parent: &mut ChildSpawnerCommands, label: &str) -> Entity {
        parent
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(space::XS),
                    padding: UiRect::horizontal(px(space::SM)),
                    border_radius: BorderRadius::MAX,
                    flex_shrink: 0.0,
                    ..default()
                },
                BorderColor::all(self.theme.border),
            ))
            .with_children(|p| {
                p.spawn(ui_text(
                    label,
                    type_scale::SMALL,
                    510,
                    self.theme.text_dim,
                    type_scale::line_height::UI,
                ));
            })
            .id()
    }

    /// kbd 小徽章（`icon_bg` 底 + `border`，等宽 MICRO）。
    pub fn kbd(&self, parent: &mut ChildSpawnerCommands, label: &str) -> Entity {
        parent
            .spawn((
                Node {
                    padding: UiRect::horizontal(px(space::XS + 1.0)),
                    border_radius: BorderRadius::all(px(radius::MICRO)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(self.theme.icon_bg),
                BorderColor::all(self.theme.border),
            ))
            .with_children(|k| {
                k.spawn(mono_text(
                    self.fonts,
                    label,
                    type_scale::MICRO,
                    self.theme.text_muted,
                    type_scale::line_height::UI,
                ));
            })
            .id()
    }

    /// 大写分区标签（TINY/510 + 正字距）。
    pub fn section_label(&self, parent: &mut ChildSpawnerCommands, label: &str) -> Entity {
        parent
            .spawn((
                Text::new(label.to_string()),
                TextFont {
                    // M6-T6 例外：LetterSpacing
                    font_size: FontSize::Px(type_scale::TINY),
                    weight: FontWeight(510),
                    ..default()
                },
                TextColor(self.theme.text_muted),
                LetterSpacing::Px(0.5),
                LineHeight::RelativeToFont(type_scale::line_height::UI),
            ))
            .id()
    }

    /// 单色图标节点（便捷重导出）。
    pub fn icon(
        &self,
        parent: &mut ChildSpawnerCommands,
        name: &str,
        px_size: f32,
        color: Color,
    ) -> Entity {
        parent.spawn(icon(self.icons, name, px_size, color)).id()
    }
}

// ===== toast =====

/// toast 消息（M6-T1）：任何系统写入即触发底部居中浮层。
///
/// 全局唯一节点**替换式刷新**：已有 toast 在场时新消息替换文案并重置 2.2s
/// 计时（同帧多条只显示最后一条，不排队——原型 §`.toast` 的单浮层语义）。
#[derive(Message, Debug, Clone)]
pub struct ToastMessage {
    /// 展示文案
    pub text: String,
}

/// toast 浮层节点标记。
#[derive(Component, Default)]
pub struct ToastMarker;

/// toast 剩余存活秒数。
#[derive(Component)]
pub struct ToastTtl(f32);

/// toast 存活时长（秒，原型 2.2s）。
const TOAST_TTL_SECS: f32 = 2.2;

/// toast 文本子节点标记（替换文案用）。
#[derive(Component, Default)]
pub struct ToastTextMarker;

/// 展示 toast：写入 [`ToastMessage`] 即出浮层（底部居中，2.2s 自动消失）。
///
/// 全局唯一 toast 节点：已有 toast 在场时，新消息替换文案并重置计时。
pub fn show_toast(
    mut reader: MessageReader<ToastMessage>,
    mut q_ttl: Query<&mut ToastTtl>,
    mut q_text: Query<&mut Text, With<ToastTextMarker>>,
    mut commands: Commands,
    theme: Res<Theme>,
    toast_layer: Query<Entity, With<crate::layout::ToastLayer>>,
) {
    let Some(latest) = reader.read().last() else {
        return;
    };
    // 已有 toast：替换文案 + 重置计时
    if let Ok(mut ttl) = q_ttl.single_mut() {
        ttl.0 = TOAST_TTL_SECS;
        if let Ok(mut t) = q_text.single_mut() {
            t.0 = latest.text.clone();
        }
        return;
    }
    // 原型 .toast：bottom 50px、水平居中；tooltip_bg 恒暗底 + border + 圆角 8
    let mut toast = commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(crate::theme::size::TOAST_BOTTOM),
                left: Val::Percent(50.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect {
                    left: px(space::LG),
                    right: px(space::LG),
                    top: px(space::SM),
                    bottom: px(space::SM),
                },
                border_radius: BorderRadius::all(px(radius::CARD)),
                border: UiRect::all(px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.tooltip_bg),
            BorderColor::all(theme.border),
            GlobalZIndex(crate::theme::z::TOAST),
            ToastMarker,
            ToastTtl(TOAST_TTL_SECS),
        ));
    if let Ok(layer) = toast_layer.single() {
        toast.insert(ChildOf(layer));
    }
    toast.with_children(|t| {
            t.spawn((
                ui_text(
                    latest.text.clone(),
                    type_scale::BODY_SM,
                    510,
                    theme.tooltip_text,
                    type_scale::line_height::UI,
                ),
                ToastTextMarker,
            ));
        });
}

/// toast 计时消隐。
pub fn toast_ttl_system(
    time: Res<Time>,
    mut q: Query<(Entity, &mut ToastTtl)>,
    mut commands: Commands,
) {
    for (entity, mut ttl) in q.iter_mut() {
        ttl.0 -= time.delta_secs();
        if ttl.0 <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// kit 插件：图标资源 + hover/tooltip/toast 全局系统。
pub struct KitPlugin;

impl Plugin for KitPlugin {
    fn build(&self, app: &mut App) {
        let server = app.world().resource::<AssetServer>().clone();
        app.insert_resource(IconAssets::load(&server))
            .add_message::<ToastMessage>()
            .add_systems(
                Update,
                // R1 修复：show_toast（重置 TTL）与 toast_ttl_system（扣减 TTL）
                // 双写 ToastTtl，chain 消除调度歧义
                (show_toast, toast_ttl_system).chain(),
            )
            .add_systems(Update, (hover_tint_system, tooltip_system, position_tooltips));
    }
}

#[cfg(test)]
mod tests {
    /// A5：tooltip 右缘不足时翻转到左侧；空间足够时贴右侧。
    #[test]
    fn tooltip_flips_at_viewport_edge() {
        let gap = 8.0;
        assert_eq!(
            super::tooltip_left(100.0, 140.0, 120.0, 800.0, gap),
            148.0
        );
        assert_eq!(
            super::tooltip_left(700.0, 740.0, 120.0, 800.0, gap),
            572.0
        );
        assert_eq!(super::tooltip_left(10.0, 40.0, 120.0, 100.0, gap), 0.0);
    }
}
