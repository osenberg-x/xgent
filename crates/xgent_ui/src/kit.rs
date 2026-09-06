//! UI 通用小件库（v7）——依赖 Theme/UiFonts，业务层专用。
//!
//! 分层约定：xui 保持「纯 bevy + xui_i18n」不动，视觉组件统一落在本模块
//! （方案 §9）。图标为白描边 PNG（导出管线 `doc/design/icons/export_png.py`），
//! 经 `ImageNode.color` 乘法染色。hover 一律走 [`HoverTint`] 数据驱动单系统，
//! 禁止逐组件写专用 hover 系统（方案 §7.1）。

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;
use bevy::text::{FontFeatureTag, FontFeatures, FontSize, FontWeight, LetterSpacing, LineHeight};
use std::collections::HashMap;

use crate::fonts::{UiFonts, mono_text, ui_text};
use crate::theme::{Theme, radius, space, type_scale};

// ===== 图标 =====

/// 图标名清单（`assets/icons/{name}@2x.png`，导出管线见 export_png.py）。
const ICON_NAMES: &[&str] = &[
    "chat", "folder", "clock", "terminal", "star", "plus", "x", "check", "copy", "retry",
    "refresh", "send", "command", "panel-right", "chevron-down", "chevron-right", "info",
    "file", "diff", "dollar", "gear", "alert-triangle",
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

/// hover/按下三态换色（单系统遍历全部 `HoverTint` 节点）。
fn hover_tint_system(
    theme: Res<Theme>,
    mut q: Query<
        (&Interaction, &HoverTint, &mut BackgroundColor, &mut BorderColor),
        Changed<Interaction>,
    >,
) {
    for (interaction, tint, mut bg, mut border) in q.iter_mut() {
        let (bg_want, line_want) = match interaction {
            Interaction::Hovered => (tint.hover_bg, tint.hover_border),
            Interaction::Pressed => (theme.active, tint.hover_border),
            Interaction::None => (tint.base_bg, tint.base_border),
        };
        if bg.0 != bg_want {
            bg.0 = bg_want;
        }
        border.set_all(line_want);
    }
}

// ===== tooltip =====

/// tooltip 内容（挂在带 `Button` 的节点上；持续悬停 500ms 后浮现子面板）。
#[derive(Component, Debug, Clone)]
pub struct Tooltip {
    pub text: String,
}

/// tooltip 悬停计时（挂于宿主按钮）。
#[derive(Component, Debug, Clone, Copy)]
struct TooltipPending {
    elapsed: f32,
}

/// 已浮现的 tooltip 子面板标记。
#[derive(Component, Debug, Clone, Copy)]
struct TooltipPanel;

/// tooltip 系统：悬停计时 500ms → spawn 子面板（宿主右侧）；离开/移出即清。
fn tooltip_system(
    mut commands: Commands,
    time: Res<Time>,
    theme: Res<Theme>,
    fonts: Res<UiFonts>,
    mut q: Query<
        (
            Entity,
            &Interaction,
            &Tooltip,
            Option<&mut TooltipPending>,
            Option<&Children>,
        ),
        With<Button>,
    >,
    panels: Query<(), With<TooltipPanel>>,
) {
    for (entity, interaction, tip, pending, children) in q.iter_mut() {
        let has_panel = children
            .map(|c| c.iter().any(|child| panels.contains(child)))
            .unwrap_or(false);

        match interaction {
            Interaction::Hovered => {
                if has_panel {
                    continue;
                }
                match pending {
                    // 计时中
                    Some(mut p) => {
                        p.elapsed += time.delta_secs();
                        if p.elapsed >= 0.5 {
                            commands.entity(entity).remove::<TooltipPending>();
                            let text = tip.text.clone();
                            commands.entity(entity).with_children(|t| {
                                t.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Percent(100.0),
                                        top: Val::Px(2.0),
                                        margin: UiRect::left(px(space::SM)),
                                        padding: UiRect::all(px(space::XS)),
                                        border_radius: BorderRadius::all(px(radius::SMALL)),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                    BackgroundColor(theme.tooltip_bg),
                                    BorderColor::all(theme.border),
                                    TooltipPanel,
                                ))
                                .with_children(|tip_node| {
                                    tip_node.spawn(mono_text(
                                        &fonts,
                                        text,
                                        type_scale::MICRO,
                                        theme.tooltip_text,
                                        type_scale::line_height::UI,
                                    ));
                                });
                            });
                        }
                    }
                    // 悬停开始：挂计时
                    None => {
                        commands.entity(entity).insert(TooltipPending { elapsed: 0.0 });
                    }
                }
            }
            // 离开：清计时与面板
            _ => {
                if pending.is_some() {
                    commands.entity(entity).remove::<TooltipPending>();
                }
                if let Some(children) = children {
                    for child in children.iter() {
                        if panels.contains(child) {
                            commands.entity(child).despawn();
                        }
                    }
                }
            }
        }
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
    pub fn primary_button(&self, parent: &mut ChildSpawnerCommands, label: &str, icon_name: &str) -> Entity {
        parent
            .spawn((
                Button,
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
    pub fn icon_button(&self, parent: &mut ChildSpawnerCommands, icon_name: &str, tip: &str) -> Entity {
        parent
            .spawn((
                Button,
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
        parent.spawn((
            Text::new(label.to_string()),
            TextFont {
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
    pub fn icon(&self, parent: &mut ChildSpawnerCommands, name: &str, px_size: f32, color: Color) -> Entity {
        parent.spawn(icon(self.icons, name, px_size, color)).id()
    }
}

/// kit 插件：图标资源 + hover/tooltip 全局系统。
pub struct KitPlugin;


impl Plugin for KitPlugin {
    fn build(&self, app: &mut App) {
        let server = app.world().resource::<AssetServer>().clone();
        app.insert_resource(IconAssets::load(&server))
            .add_systems(Update, (hover_tint_system, tooltip_system));
    }
}


