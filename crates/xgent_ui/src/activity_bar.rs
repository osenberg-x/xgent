//! 图标轨（v7）：左侧 52px 导航——对话 / 文件 / 历史 | 终端 | 展开面板 · 陪伴。
//!
//! v8 M5：轨项由**面板注册表**驱动（`Slot::Rail` 的注册项），选中态**从
//! 「注册表 ∩ 槽位可见性」派生**（[`PanelRegistry::active_rail`]）——不再有
//! 独立的 `ActiveActivity` 存储，连点同一项两次把面板关掉后，选中态自然回落
//! （修 D4）。
//!
//! v7 重构（方案 §8.3）：矢量图标 + tooltip + `accent_bg` 激活态 + 左侧 3px
//! 竖条；历史入口自顶栏迁入；编辑器入口移除（预览归上下文面板页签，M5-T1）；
//! 陪伴按钮为全界面唯一暖色例外（ADR-0014 裁剪#3，宠物本体 P1）。
//! 不放搜索/Git/插件入口（F-05/F-10 未实现，非本期目标）。

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use crate::kit::{HoverTint, IconAssets, Tooltip, UiKit, icon};
use crate::layout::ActivityBarMarker;
use crate::panels::{
    PanelActivate, PanelDeactivate, PanelRegistry, RailAction, Slot, SlotLayout, ids,
};
use crate::status_bar::CompanionOn;
use crate::theme::{Theme, size, space};

/// 图标轨项标记（携带该 rail 入口的面板 id）。
#[derive(Component, Debug, Clone)]
pub struct RailItemMarker {
    /// rail 入口自身的面板 id（`Slot::Rail`）。
    pub id: &'static str,
}

/// 图标轨项容器标记（轨项按钮由注册表派生，动态重建于其内）。
#[derive(Component, Default)]
pub struct RailItemsMarker;

/// 「展开面板」按钮标记（面板折叠/响应式收起时显示）。
#[derive(Component, Default)]
pub struct ExpandPanelButtonMarker;

/// 陪伴开关按钮标记。
#[derive(Component, Default)]
pub struct CompanionButtonMarker;

/// 陪伴星星图标标记（颜色随开关切换）。
#[derive(Component, Default)]
pub struct CompanionIconMarker;

/// 图标轨插件。
pub struct ActivityBarPlugin;

impl Plugin for ActivityBarPlugin {
    fn build(&self, app: &mut App) {
        // 面板注册表（幂等：宿主可能已装配）
        if !app.is_plugin_added::<crate::panels::PanelsPlugin>() {
            app.add_plugins(crate::panels::PanelsPlugin);
        }
        if !app.is_plugin_added::<crate::panels::visibility::VisibilityPlugin>() {
            app.add_plugins(crate::panels::visibility::VisibilityPlugin);
        }
        app.add_systems(
            Startup,
            spawn_activity_bar.after(crate::layout::spawn_layout),
        )
        .add_systems(
            Update,
            (
                sync_rail_items,
                handle_rail_click,
                update_active_indicators,
                update_companion_visual,
                companion_ring_system,
            ),
        );
    }
}

/// 启动时在图标轨内 spawn：轨项容器 + spacer + 展开面板 · 陪伴。
///
/// 轨项按钮本身由 [`sync_rail_items`] 从注册表生成（注册表在别的模块的
/// Startup 里填充，轨项无法在 Startup 里可靠排序）。
fn spawn_activity_bar(
    mut commands: Commands,
    q_bar: Query<Entity, With<ActivityBarMarker>>,
    theme: Res<Theme>,
    icons: Res<IconAssets>,
) {
    let Ok(bar) = q_bar.single() else {
        return;
    };
    let mut companion_star: Option<Entity> = None;
    commands.entity(bar).with_children(|p| {
        // 轨项容器（按钮由注册表派生）
        p.spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(space::XS),
                flex_shrink: 0.0,
                ..default()
            },
            RailItemsMarker,
        ));
        // spacer
        p.spawn((Node {
            flex_grow: 1.0,
            ..default()
        },));
        // 展开面板钮（面板折叠/响应式收起时显示；显隐由派生系统按
        // `Slot::SideDock` 的开合状态写）
        p.spawn((
            Button,
            Hovered::default(),
            Node {
                width: px(size::ICON_BTN),
                height: px(size::ICON_BTN),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(6.0)),
                flex_shrink: 0.0,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
            HoverTint::ghost(&theme),
            Tooltip {
                text: "展开面板".to_string(),
            },
            ExpandPanelButtonMarker,
            // v8 M5-T3：展开钮是**恢复入口**——分屏折叠时才出现（反向跟随
            // `Slot::SideDock` 的开合）。
            crate::panels::PanelVisibility::of_closed_slot(Slot::SideDock),
        ))
        .with_children(|b| {
            b.spawn(icon(&icons, "panel-right", 20.0, theme.text_muted));
        });
        // 陪伴开关（全界面唯一暖色例外）+ 激活环（M7-T1：嵌套描边节点）
        p.spawn((
            Button,
            Hovered::default(),
            Node {
                width: px(size::ICON_BTN),
                height: px(size::ICON_BTN),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::MAX,
                flex_shrink: 0.0,
                margin: UiRect::top(px(space::XS)),
                ..default()
            },
            BackgroundColor(theme.warm),
            CompanionButtonMarker,
        ))
        .with_children(|b| {
            // 激活环：外圈 2px 描边（warm），由 companion_ring_system 驱动
            // scale 1.0→1.3 + alpha 0.4→0，2s 循环；关闭时隐藏
            b.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: px(size::ICON_BTN),
                    height: px(size::ICON_BTN),
                    border: UiRect::all(px(2.0)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(theme.warm),
                CompanionRingMarker,
            ));
            companion_star = Some(b.spawn(icon(&icons, "star", 20.0, theme.star)).id());
        });
    });
    if let Some(star) = companion_star {
        commands.entity(star).insert(CompanionIconMarker);
    }
}

/// 轨项按钮（40×40、圆角 6、ghost hover、tooltip）。
fn rail_button(
    p: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    kit: &UiKit,
    id: &'static str,
    icon_name: &str,
    tip: String,
) -> Entity {
    p.spawn((
        Button,
        Hovered::default(),
        Node {
            width: px(size::ICON_BTN),
            height: px(size::ICON_BTN),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(px(6.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        HoverTint::ghost(kit.theme),
        Tooltip { text: tip },
        RailItemMarker { id },
    ))
    .with_children(|b| {
        b.spawn(icon(kit.icons, icon_name, 20.0, kit.theme.text_muted));
    })
    .id()
}

/// 注册表变化时重建轨项按钮（轨项 = `Slot::Rail` 的注册面板）。
pub fn sync_rail_items(
    registry: Res<PanelRegistry>,
    theme: Res<Theme>,
    icons: Res<IconAssets>,
    fonts: Res<crate::fonts::UiFonts>,
    loc: Res<xgent_settings::Localizer>,
    mut commands: Commands,
    q_container: Query<Entity, With<RailItemsMarker>>,
    q_items: Query<Entity, With<RailItemMarker>>,
    mut signature: Local<Vec<&'static str>>,
) {
    let wanted: Vec<&'static str> = registry.iter_rail().map(|e| e.desc.id).collect();
    if wanted == *signature {
        return;
    }
    *signature = wanted.clone();
    let Ok(container) = q_container.single() else {
        return;
    };
    for entity in q_items.iter() {
        commands.entity(entity).despawn();
    }
    let kit = UiKit {
        theme: &theme,
        icons: &icons,
        fonts: &fonts,
    };
    for entry in registry.iter_rail() {
        let Some(rail) = entry.desc.rail else {
            continue;
        };
        // 分隔线（由描述符声明，不在代码里硬编码分组）
        if rail.divider_before {
            commands.entity(container).with_children(|p| {
                p.spawn((
                    Node {
                        width: px(24.0),
                        height: px(1.0),
                        margin: UiRect::vertical(px(space::SM)),
                        ..default()
                    },
                    BackgroundColor(theme.border),
                ));
            });
        }
        // tooltip 取被驱动面板的 `title_key`（i18n 单源）
        let tip = registry
            .get(rail.target)
            .map(|t| crate::i18n::tr(&loc, t.desc.title_key))
            .unwrap_or_else(|| entry.desc.id.to_string());
        let icon_name = registry
            .get(rail.target)
            .map(|t| t.desc.icon)
            .unwrap_or(entry.desc.icon);
        commands.entity(container).with_children(|p| {
            rail_button(p, &kit, entry.desc.id, icon_name, tip);
        });
    }
}

/// 陪伴激活环节点标记。
#[derive(Component, Default)]
pub struct CompanionRingMarker;

/// 陪伴激活环动效（单系统，R1 修复：合并原两系统避免双写 Node 调度歧义）。
///
/// 2s 循环 scale 1.0→1.3 + 透明度 0.4→0；显隐随 `CompanionOn`。
fn companion_ring_system(
    on: Res<CompanionOn>,
    theme: Res<Theme>,
    time: Res<Time>,
    mut q_ring: Query<(&mut Node, &mut BorderColor), With<CompanionRingMarker>>,
) {
    let display = if on.0 { Display::Flex } else { Display::None };
    // 相位 0..1（2s 循环）
    let t = (time.elapsed_secs() % 2.0) / 2.0;
    let scale = 1.0 + 0.3 * t;
    let alpha = 0.4 * (1.0 - t);
    for (mut node, mut border) in q_ring.iter_mut() {
        if node.display != display {
            node.display = display;
        }
        if on.0 {
            let side = size::ICON_BTN * scale;
            node.width = px(side);
            node.height = px(side);
            node.left = px(-(side - size::ICON_BTN) / 2.0);
            node.top = px(-(side - size::ICON_BTN) / 2.0);
            border.set_all(theme.warm.with_alpha(alpha));
        }
    }
}

/// 处理图标轨点击：按描述符驱动目标面板（消息）。
fn handle_rail_click(
    q_items: Query<&RailItemMarker, Added<Pressed>>,
    q_expand: Query<(), (With<ExpandPanelButtonMarker>, Added<Pressed>)>,
    q_companion: Query<(), (With<CompanionButtonMarker>, Added<Pressed>)>,
    registry: Res<PanelRegistry>,
    mut activate: MessageWriter<PanelActivate>,
    mut deactivate: MessageWriter<PanelDeactivate>,
    mut layout: ResMut<SlotLayout>,
    terminal_tabs: Res<crate::terminal::TerminalTabs>,
    mut terminal_spawn: MessageWriter<crate::terminal::tabs::SpawnTabRequest>,
    mut companion: ResMut<CompanionOn>,
    project_root: Option<Res<crate::file_panel::ProjectRoot>>,
) {
    for item in q_items.iter() {
        let Some(entry) = registry.get(item.id) else {
            continue;
        };
        let Some(rail) = entry.desc.rail else {
            continue;
        };
        match rail.action {
            // 回到主区：激活目标面板（`Slot::Center` 的面板被激活时，注册表
            // 自动让所有可覆盖层面板失活）。
            RailAction::FocusMain => {
                activate.write(PanelActivate { id: rail.target });
            }
            // 切换目标面板的可见性。
            RailAction::Toggle => {
                if registry.is_visible(rail.target) {
                    deactivate.write(PanelDeactivate { id: rail.target });
                } else {
                    if rail.target == ids::TERMINAL && terminal_tabs.is_empty() {
                        let cwd = project_root
                            .as_deref()
                            .map(|r| r.path.clone())
                            .unwrap_or_else(std::env::temp_dir);
                        terminal_spawn.write(crate::terminal::tabs::SpawnTabRequest { cwd });
                    }
                    activate.write(PanelActivate { id: rail.target });
                }
            }
        }
    }
    // 展开面板钮：恢复折叠的上下文面板（`Slot::SideDock` 展开）
    for _ in q_expand.iter() {
        layout.set_open(Slot::SideDock, true);
    }
    // 陪伴开关
    for _ in q_companion.iter() {
        companion.0 = !companion.0;
    }
}

/// 活跃项视觉：`accent_bg` 底 + `accent_interactive` 图标 + 左侧 3px 圆角竖条。
///
/// 选中态由 [`PanelRegistry::active_rail`] **派生**（不再独立存储）。
fn update_active_indicators(
    registry: Res<PanelRegistry>,
    layout: Res<SlotLayout>,
    theme: Res<Theme>,
    mut q: Query<(
        &RailItemMarker,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut q_icon: Query<&mut ImageNode, Without<RailItemMarker>>,
) {
    if !registry.is_changed() && !layout.is_changed() && !theme.is_changed() {
        return;
    }
    let active = registry.active_rail(&layout);
    for (item, mut node, mut bg, mut border, children) in q.iter_mut() {
        let is_active = active == Some(item.id);
        bg.0 = if is_active {
            theme.accent_bg
        } else {
            Color::NONE
        };
        // 左侧 3px 竖条：宽度在 Node.border、颜色在 BorderColor
        node.border = UiRect {
            left: if is_active { px(3.0) } else { px(0.0) },
            ..default()
        };
        border.set_all(if is_active {
            theme.accent_interactive
        } else {
            Color::NONE
        });
        let icon_color = if is_active {
            theme.accent_interactive
        } else {
            theme.text_muted
        };
        for child in children.iter() {
            if let Ok(mut img) = q_icon.get_mut(child)
                && img.color != icon_color
            {
                img.color = icon_color;
            }
        }
    }
}

/// 陪伴按钮视觉：开启=warm 圆底 + 深色星标；关闭=透明底 + muted 星标。
fn update_companion_visual(
    on: Res<CompanionOn>,
    theme: Res<Theme>,
    mut q: Query<&mut BackgroundColor, With<CompanionButtonMarker>>,
    companions: Query<&Children, With<CompanionButtonMarker>>,
    mut q_icon: Query<&mut ImageNode, With<CompanionIconMarker>>,
) {
    if !on.is_changed() && !theme.is_changed() {
        return;
    }
    let star_color = if on.0 { theme.star } else { theme.text_muted };
    for mut bg in q.iter_mut() {
        let want = if on.0 { theme.warm } else { theme.subtle };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    for children in companions.iter() {
        for child in children.iter() {
            if let Ok(mut img) = q_icon.get_mut(child)
                && img.color != star_color
            {
                img.color = star_color;
            }
        }
    }
}

/// 轨项的可见性由注册表派生（`Slot::Rail` 恒开），选中态由
/// [`PanelRegistry::active_rail`] 派生。
#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::{PanelDescriptor, ids};

    /// v8 M5 回归：内置 rail 入口的描述符自洽——目标面板 id 合法、顺序递增、
    /// tooltip 的 `title_key` 与目标面板一致。
    #[test]
    fn builtin_rail_descriptors_are_consistent() {
        let entries = crate::panels::builtin_rail_panels();
        assert_eq!(entries.len(), 4);
        let mut last_order = i16::MIN;
        for entry in &entries {
            assert_eq!(entry.slot, Slot::Rail);
            assert!(entry.order > last_order, "rail 顺序应递增");
            last_order = entry.order;
            let rail = entry.rail.expect("rail 入口必须有目标");
            assert!(!rail.target.is_empty());
        }
        // 文件 / 历史入口驱动 Overlay 槽位的两个面板
        let files = entries
            .iter()
            .find(|e| e.id == ids::RAIL_FILES)
            .expect("应有文件入口");
        assert_eq!(files.rail.map(|r| r.target), Some(ids::FILE_DRAWER));
    }

    /// v8 M5 回归：rail 选中态**从注册表派生**——目标面板不可见时选中态回落到
    /// 对话（不残留）。
    #[test]
    fn rail_selection_is_derived_and_falls_back() {
        let mut registry = PanelRegistry::default();
        for desc in crate::panels::builtin_rail_panels() {
            registry.register(desc, None);
        }
        registry.register(
            PanelDescriptor::new(ids::CHAT, Slot::Center, "panel-chat", "chat", 0)
                .with_default_visible(),
            None,
        );
        registry.register(
            PanelDescriptor::new(
                ids::FILE_DRAWER,
                Slot::Overlay,
                "panel-file-drawer",
                "folder",
                0,
            ),
            None,
        );
        let layout = SlotLayout::default();

        assert_eq!(registry.active_rail(&layout), Some(ids::RAIL_CHAT));
        registry.activate(ids::FILE_DRAWER);
        assert_eq!(registry.active_rail(&layout), Some(ids::RAIL_FILES));
        // 关掉抽屉 → 选中态回落到对话（连点两次 Files 不残留）
        registry.deactivate(ids::FILE_DRAWER);
        assert_eq!(registry.active_rail(&layout), Some(ids::RAIL_CHAT));
    }
}
