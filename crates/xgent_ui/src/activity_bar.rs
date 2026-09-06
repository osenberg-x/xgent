//! 图标轨（v7）：左侧 52px 导航——对话 / 文件 / 历史 | 终端 | 展开面板 · 陪伴。
//!
//! v7 重构（方案 §8.3）：矢量图标 + tooltip + `accent_bg` 激活态 + 左侧 3px
//! 竖条；历史入口自顶栏迁入；编辑器入口移除（预览归上下文面板页签，M5-T1）；
//! 陪伴按钮为全界面唯一暖色例外（ADR-0014 裁剪#3，宠物本体 P1）。
//! 不放搜索/Git/插件入口（F-05/F-10 未实现，非本期目标）。

use bevy::prelude::*;

use crate::kit::{HoverTint, IconAssets, Tooltip, UiKit, icon};
use crate::layout::{ActivityBarMarker, FilePanelCollapsed, SideViewCollapsed};
use crate::status_bar::CompanionOn;
use crate::theme::{Theme, space};

/// 图标轨项类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivityKind {
    /// 对话（默认视图）
    #[default]
    Chat,
    /// 文件抽屉
    Files,
    /// 会话历史抽屉
    History,
    /// 终端
    Terminal,
}

/// 图标轨项标记。
#[derive(Component, Default)]
pub struct ActivityItemMarker {
    pub kind: ActivityKind,
}

/// 当前活跃的图标轨项。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActiveActivity(pub ActivityKind);

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
        app.init_resource::<ActiveActivity>()
            .add_systems(
                Startup,
                spawn_activity_bar.after(crate::layout::spawn_layout),
            )
            .add_systems(
                Update,
                (
                    handle_rail_click,
                    update_active_indicators,
                    update_expand_visibility,
                    update_companion_visual,
                ),
            );
    }
}

/// 单个轨上按钮（40×40、圆角 6、ghost hover、tooltip）。
fn rail_button(
    p: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    kit: &UiKit,
    kind: ActivityKind,
    icon_name: &str,
    tip: &str,
) -> Entity {
    p.spawn((
        Button,
        Node {
            width: px(40.0),
            height: px(40.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(px(6.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        HoverTint::ghost(kit.theme),
        Tooltip {
            text: tip.to_string(),
        },
        ActivityItemMarker { kind },
    ))
    .with_children(|b| {
        b.spawn(icon(kit.icons, icon_name, 20.0, kit.theme.text_muted));
    })
    .id()
}

/// 启动时在图标轨内 spawn：对话 / 文件 / 历史 | 终端 | spacer | 展开面板 · 陪伴。
fn spawn_activity_bar(
    mut commands: Commands,
    q_bar: Query<Entity, With<ActivityBarMarker>>,
    theme: Res<Theme>,
    icons: Res<IconAssets>,
    fonts: Res<crate::fonts::UiFonts>,
) {
    let Ok(bar) = q_bar.single() else {
        return;
    };
    let kit = UiKit {
        theme: &theme,
        icons: &icons,
        fonts: &fonts,
    };

    let mut companion_star: Option<Entity> = None;
    commands.entity(bar).with_children(|mut p| {
        rail_button(&mut p, &kit, ActivityKind::Chat, "chat", "对话");
        rail_button(&mut p, &kit, ActivityKind::Files, "folder", "文件");
        rail_button(&mut p, &kit, ActivityKind::History, "clock", "历史会话");
        // 分隔线
        p.spawn((
            Node {
                width: px(24.0),
                height: px(1.0),
                margin: UiRect::vertical(px(space::SM)),
                ..default()
            },
            BackgroundColor(theme.border),
        ));
        rail_button(&mut p, &kit, ActivityKind::Terminal, "terminal", "终端");
        // spacer
        p.spawn((Node {
            flex_grow: 1.0,
            ..default()
        },));
        // 展开面板钮（面板折叠/响应式收起时显示，update_expand_visibility 控制）
        p.spawn((
            Button,
            Node {
                width: px(40.0),
                height: px(40.0),
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
        ))
        .with_children(|b| {
            b.spawn(icon(&icons, "panel-right", 20.0, theme.text_muted));
        });
        // 陪伴开关（全界面唯一暖色例外）
        p.spawn((
            Button,
            Node {
                width: px(40.0),
                height: px(40.0),
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
            companion_star = Some(
                b.spawn(icon(&icons, "star", 20.0, Color::srgb_u8(0x1C, 0x19, 0x17)))
                    .id(),
            );
        });
    });
    if let Some(star) = companion_star {
        commands.entity(star).insert(CompanionIconMarker);
    }
}

/// 处理图标轨点击：切换面板/视图/抽屉。
fn handle_rail_click(
    q_items: Query<(&Interaction, &ActivityItemMarker), Changed<Interaction>>,
    q_expand: Query<&Interaction, (With<ExpandPanelButtonMarker>, Changed<Interaction>)>,
    q_companion: Query<&Interaction, (With<CompanionButtonMarker>, Changed<Interaction>)>,
    mut active: ResMut<ActiveActivity>,
    mut file_collapsed: ResMut<FilePanelCollapsed>,
    mut side_collapsed: ResMut<SideViewCollapsed>,
    mut content: ResMut<crate::editor::SideViewContent>,
    terminal_tabs: Res<crate::terminal::TerminalTabs>,
    mut terminal_spawn: MessageWriter<crate::terminal::tabs::SpawnTabRequest>,
    mut history_state: ResMut<crate::session_history::SessionHistoryState>,
    mut companion: ResMut<CompanionOn>,
    project_root: Option<Res<crate::file_panel::ProjectRoot>>,
) {
    for (interaction, item) in q_items.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match item.kind {
            ActivityKind::Chat => {
                // 对话为默认视图：取消其他活跃项即可（抽屉化后此处负责关抽屉）
                active.0 = ActivityKind::Chat;
            }
            ActivityKind::Files => {
                file_collapsed.0 = !file_collapsed.0;
                active.0 = ActivityKind::Files;
            }
            ActivityKind::History => {
                history_state.open = !history_state.open;
                active.0 = ActivityKind::History;
            }
            ActivityKind::Terminal => {
                if *content == crate::editor::SideViewContent::Terminal {
                    *content = crate::editor::SideViewContent::None;
                    side_collapsed.0 = true;
                } else {
                    *content = crate::editor::SideViewContent::Terminal;
                    side_collapsed.0 = false;
                    if terminal_tabs.is_empty() {
                        let cwd = project_root
                            .as_deref()
                            .map(|r| r.path.clone())
                            .unwrap_or_else(std::env::temp_dir);
                        terminal_spawn.write(crate::terminal::tabs::SpawnTabRequest { cwd });
                    }
                    active.0 = ActivityKind::Terminal;
                }
            }
        }
    }
    // 展开面板钮：恢复折叠的上下文面板
    for interaction in q_expand.iter() {
        if *interaction == Interaction::Pressed {
            side_collapsed.0 = false;
        }
    }
    // 陪伴开关
    for interaction in q_companion.iter() {
        if *interaction == Interaction::Pressed {
            companion.0 = !companion.0;
        }
    }
}

/// 活跃项视觉：`accent_bg` 底 + `accent_interactive` 图标 + 左侧 3px 圆角竖条。
fn update_active_indicators(
    active: Res<ActiveActivity>,
    theme: Res<Theme>,
    mut q: Query<(
        &ActivityItemMarker,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut q_icon: Query<&mut ImageNode, Without<ActivityItemMarker>>,
) {
    if !active.is_changed() && !theme.is_changed() {
        return;
    }
    for (item, mut node, mut bg, mut border, children) in q.iter_mut() {
        let is_active = item.kind == active.0;
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
            if let Ok(mut img) = q_icon.get_mut(child) {
                if img.color != icon_color {
                    img.color = icon_color;
                }
            }
        }
    }
}

/// 「展开面板」钮随面板折叠态显隐。
fn update_expand_visibility(
    side_collapsed: Res<SideViewCollapsed>,
    mut q: Query<&mut Node, With<ExpandPanelButtonMarker>>,
) {
    if !side_collapsed.is_changed() {
        return;
    }
    for mut node in q.iter_mut() {
        node.display = if side_collapsed.0 {
            Display::Flex
        } else {
            Display::None
        };
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
    let star_color = if on.0 {
        Color::srgb_u8(0x1C, 0x19, 0x17)
    } else {
        theme.text_muted
    };
    for mut bg in q.iter_mut() {
        let want = if on.0 { theme.warm } else { theme.subtle };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    for children in companions.iter() {
        for child in children.iter() {
            if let Ok(mut img) = q_icon.get_mut(child) {
                if img.color != star_color {
                    img.color = star_color;
                }
            }
        }
    }
}
