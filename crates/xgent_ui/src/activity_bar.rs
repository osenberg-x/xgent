//! 活动栏：左侧 48px 窄条，图标导航入口。
//!
//! v2 重构：将文件面板/编辑器/终端/设置入口抽象为图标按钮，
//! active 项左侧带 2px 绿色竖条指示器，减少顶栏负担。

use bevy::prelude::*;

use crate::layout::ActivityBarMarker;
use crate::theme::Theme;

/// 活动栏项类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivityKind {
    #[default]
    Files,
    Editor,
    Terminal,
    Settings,
}

/// 活动栏项标记。
#[derive(Component, Default)]
pub struct ActivityItemMarker {
    pub kind: ActivityKind,
}

/// 当前活跃的活动栏项。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActiveActivity(pub ActivityKind);

/// 活动栏插件。
pub struct ActivityBarPlugin;

impl Plugin for ActivityBarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveActivity>()
            .add_systems(
                Startup,
                spawn_activity_bar.after(crate::layout::spawn_layout),
            )
            .add_systems(Update, (handle_activity_click, update_activity_indicators));
    }
}

/// 启动时在活动栏内 spawn 图标按钮。
fn spawn_activity_bar(
    mut commands: Commands,
    q_bar: Query<Entity, With<ActivityBarMarker>>,
    theme: Res<Theme>,
) {
    let Ok(bar) = q_bar.single() else {
        return;
    };
    let icon_color = theme.text_dim;
    let border = BorderColor::all(Color::NONE);

    commands.entity(bar).with_children(|p| {
        // 文件
        p.spawn((
            Button,
            Node {
                width: px(36.0),
                height: px(36.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            Text::new("📁"),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(icon_color),
            border,
            ActivityItemMarker {
                kind: ActivityKind::Files,
            },
        ));
        // 编辑器
        p.spawn((
            Button,
            Node {
                width: px(36.0),
                height: px(36.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            Text::new("📝"),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(icon_color),
            border,
            ActivityItemMarker {
                kind: ActivityKind::Editor,
            },
        ));
        // 终端
        p.spawn((
            Button,
            Node {
                width: px(36.0),
                height: px(36.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            Text::new("🖥"),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(icon_color),
            border,
            ActivityItemMarker {
                kind: ActivityKind::Terminal,
            },
        ));
        // spacer
        p.spawn((Node {
            flex_grow: 1.0,
            ..default()
        },));
        // 设置
        p.spawn((
            Button,
            Node {
                width: px(36.0),
                height: px(36.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            Text::new("⚙"),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(icon_color),
            border,
            ActivityItemMarker {
                kind: ActivityKind::Settings,
            },
        ));
    });
}

/// 处理活动栏点击：切换面板/视图。
fn handle_activity_click(
    q_items: Query<(&Interaction, &ActivityItemMarker), Changed<Interaction>>,
    mut active: ResMut<ActiveActivity>,
    mut file_collapsed: ResMut<crate::layout::FilePanelCollapsed>,
    mut side_collapsed: ResMut<crate::layout::SideViewCollapsed>,
    mut content: ResMut<crate::editor::SideViewContent>,
    mut settings_state: ResMut<crate::settings_panel::SettingsPanelState>,
    terminal_tabs: Res<crate::terminal::TerminalTabs>,
    mut terminal_spawn: MessageWriter<crate::terminal::tabs::SpawnTabRequest>,
    project_root: Option<Res<crate::file_panel::ProjectRoot>>,
) {
    for (interaction, item) in q_items.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match item.kind {
            ActivityKind::Files => {
                file_collapsed.0 = !file_collapsed.0;
                active.0 = ActivityKind::Files;
            }
            ActivityKind::Editor => {
                if side_collapsed.0 || *content != crate::editor::SideViewContent::Editor {
                    side_collapsed.0 = false;
                    *content = crate::editor::SideViewContent::Editor;
                    active.0 = ActivityKind::Editor;
                } else {
                    side_collapsed.0 = true;
                    *content = crate::editor::SideViewContent::None;
                }
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
            ActivityKind::Settings => {
                settings_state.open = !settings_state.open;
            }
        }
    }
}

/// 更新活动栏项视觉：active 项高亮文字色 + 左侧竖条背景。
fn update_activity_indicators(
    active: Res<ActiveActivity>,
    theme: Res<Theme>,
    mut q: Query<(&ActivityItemMarker, &mut TextColor, &mut Node, &mut BorderColor)>,
) {
    if !active.is_changed() && !theme.is_changed() {
        return;
    }
    for (item, mut color, mut node, mut border_color) in q.iter_mut() {
        let is_active = item.kind == active.0;
        color.0 = if is_active { theme.text } else { theme.text_dim };
        // active 项左侧加 2px 边框作为竖条指示器
        node.border = if is_active {
            UiRect {
                left: px(2.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
            }
        } else {
            UiRect::all(Val::Px(0.0))
        };
        border_color.set_all(if is_active { theme.accent } else { Color::NONE });
    }
}
