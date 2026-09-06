//! 顶栏（v7）：品牌块 + 项目名 + 新建会话 ghost 钮 | spacer | agent pill +
//! provider/model pill + 命令面板钮 + 设置钮。
//!
//! v7 重构（方案 §8.2）：高度 52；近黑层级（底 `surface` + 底边 `line`）；
//! 历史入口迁图标轨（M3-T5）；agent 状态 pill 取代状态栏会话状态文本。
//! 按钮交互标记与既有系统（`handle_top_bar_buttons`）保持兼容。

use bevy::prelude::*;
use bevy::text::FontSize;
use xgent_agent::ProviderInfo;
use xgent_settings::Localizer;
use xui::command_palette::CommandPaletteState;

use crate::fonts::UiFonts;
use crate::fonts::ui_text;
use crate::i18n::tr;
use crate::kit::{HoverTint, IconAssets, UiKit, icon};
use crate::layout::TopBarMarker;
use crate::theme::{space, type_scale};
use crate::theme::{Theme, radius};

use xgent_agent::Conversation;

/// 顶栏 provider/model 标签节点标记。
#[derive(Component, Default)]
pub struct ProviderLabelMarker;

/// 新建会话按钮标记。
#[derive(Component, Default)]
pub struct NewSessionButtonMarker;

/// 顶栏 provider 标签按钮标记（点击打开设置面板切换 provider）。
#[derive(Component, Default)]
pub struct ProviderButtonMarker;

/// 顶栏命令面板按钮标记。
#[derive(Component, Default)]
pub struct PaletteButtonMarker;

/// 顶栏设置按钮标记。
#[derive(Component, Default)]
pub struct SettingsButtonMarker;

// ===== agent 状态 pill（M3-T4）=====

/// agent 状态 pill 容器标记（底色/边框随状态切换）。
#[derive(Component, Default)]
pub struct AgentPillMarker;

/// agent pill 状态点标记（颜色 + 忙时脉冲）。
#[derive(Component, Default)]
pub struct AgentPillDotMarker;

/// agent pill 状态文本标记。
#[derive(Component, Default)]
pub struct AgentPillTextMarker;

/// 顶栏插件。
pub struct TopBarPlugin;

impl Plugin for TopBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_top_bar.after(crate::layout::spawn_layout))
            .add_systems(
                Update,
                (
                    update_provider_label,
                    update_agent_pill,
                    handle_top_bar_buttons,
                )
                    .after(crate::command_palette::handle_palette_triggers),
            );
    }
}

/// 启动时在顶栏内 spawn v7 九元素（主题钮随亮色 P1 暂缺，历史钮迁 rail）。
fn spawn_top_bar(
    mut commands: Commands,
    q_bar: Query<Entity, With<TopBarMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    icons: Res<IconAssets>,
    fonts: Res<UiFonts>,
    project: Option<Res<crate::file_panel::ProjectRoot>>,
) {
    let Ok(bar) = q_bar.single() else {
        return;
    };
    let kit = UiKit {
        theme: &theme,
        icons: &icons,
        fonts: &fonts,
    };

    let mut new_session_btn: Option<Entity> = None;
    let mut palette_btn: Option<Entity> = None;
    let mut settings_btn: Option<Entity> = None;
    commands.entity(bar).with_children(|mut p| {
        // ① 品牌块：28×28 `accent` 底白字 "X"（圆角 8）+ "XGent"（H3/590）
        p.spawn((Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(space::SM),
            padding: UiRect::right(px(space::MD)),
            ..default()
        },))
            .with_children(|brand| {
                brand.spawn((
                    Node {
                        width: px(28.0),
                        height: px(28.0),
                        border_radius: BorderRadius::all(px(radius::CARD)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(theme.accent),
                    Text::new("X"),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        weight: FontWeight(590),
                        ..default()
                    },
                    TextColor(theme.accent_text),
                ));
                brand.spawn(ui_text(
                    "XGent",
                    type_scale::H3,
                    590,
                    theme.text,
                    type_scale::line_height::UI,
                ));
            });

        // ② 项目名（静态；项目切换器留待后续）
        let project_name = project
            .as_ref()
            .and_then(|r| r.path.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "xgent".to_string());
        p.spawn((Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(space::XS),
            ..default()
        },))
            .with_children(|crumb| {
                crumb.spawn(icon(&icons, "folder", 14.0, theme.text_muted));
                crumb.spawn(ui_text(
                    &project_name,
                    type_scale::BODY_SM,
                    400,
                    theme.text_muted,
                    type_scale::line_height::UI,
                ));
            });

        // ③ 新建会话 ghost 钮
        new_session_btn = Some(kit.ghost_button(
            &mut p,
            &tr(&loc, "topbar-new-session"),
            "plus",
        ));

        // ④ spacer
        p.spawn((Node {
            flex_grow: 1.0,
            ..default()
        },));

        // ⑤ agent 状态 pill（M3-T4）
        spawn_agent_pill(&mut p, &theme, &loc);

        // ⑥ provider/model pill（点击开设置；chevron 指示下拉）
        p.spawn((
            Button,
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::XS),
                padding: UiRect::horizontal(px(space::SM)),
                border_radius: BorderRadius::MAX,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.subtle),
            BorderColor::all(theme.border),
            HoverTint::standard(&theme),
            ProviderButtonMarker,
        ))
        .with_children(|pill| {
            pill.spawn((Node {
                width: px(16.0),
                height: px(16.0),
                border_radius: BorderRadius::all(px(radius::SMALL)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.accent),
            Text::new("G"),
            TextFont {
                font_size: FontSize::Px(9.0),
                weight: FontWeight(590),
                ..default()
            },
            TextColor(theme.accent_text),
            ));
            pill.spawn((
                Text::new(String::new()),
                TextFont {
                    font_size: FontSize::Px(type_scale::SMALL),
                    weight: FontWeight(510),
                    ..default()
                },
                TextColor(theme.text_dim),
                ProviderLabelMarker,
            ));
            pill.spawn(icon(&icons, "chevron-down", 12.0, theme.text_muted));
        });

        // ⑧ 命令面板图标钮
        palette_btn = Some(kit.icon_button(&mut p, "command", "命令面板"));

        // ⑨ 设置图标钮（常驻入口，rail 不放设置）
        settings_btn = Some(kit.icon_button(&mut p, "gear", "设置"));
    });
    // 标记插入（闭包外，避免与 with_children 的 commands 可变借用冲突）
    if let Some(e) = new_session_btn {
        commands.entity(e).insert(NewSessionButtonMarker);
    }
    if let Some(e) = palette_btn {
        commands.entity(e).insert(PaletteButtonMarker);
    }
    if let Some(e) = settings_btn {
        commands.entity(e).insert(SettingsButtonMarker);
    }
}

/// ⑤ agent 状态 pill：点 + 文本，状态机映射 `ConversationStatus`（方案 §8.2）。
fn spawn_agent_pill(
    p: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    theme: &Theme,
    loc: &Localizer,
) {
    p.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(space::XS),
            padding: UiRect::horizontal(px(space::SM)),
            border_radius: BorderRadius::MAX,
            border: UiRect::all(px(1.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(theme.subtle),
        BorderColor::all(theme.border),
        AgentPillMarker,
    ))
    .with_children(|pill| {
        pill.spawn((
            Node {
                width: px(8.0),
                height: px(8.0),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(theme.st_ok),
            AgentPillDotMarker,
        ));
        pill.spawn((
            Text::new(tr(loc, "status-ready").to_string()),
            TextFont {
                font_size: FontSize::Px(type_scale::SMALL),
                weight: FontWeight(510),
                ..default()
            },
            TextColor(theme.text_dim),
            AgentPillTextMarker,
        ));
    });
}

/// 根据 ProviderInfo 更新 provider/model 标签。
fn update_provider_label(
    info: Res<ProviderInfo>,
    theme: Res<Theme>,
    mut q: Query<&mut Text, With<ProviderLabelMarker>>,
) {
    if !info.is_changed() && !theme.is_changed() {
        return;
    }
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    text.0 = if info.id.is_empty() {
        String::new()
    } else {
        format!("{} / {}", info.id, info.model)
    };
}

/// agent pill 五态机（方案 §8.2，状态源已核实 conversation.rs:298-308）：
/// Idle→就绪(ok)、Thinking/Streaming→强调(accent 脉冲)、ToolRunning/Confirming/
/// Aborting→待确认(warning)、Error→失败。忙时状态点正弦脉冲。
fn update_agent_pill(
    conv: Res<Conversation>,
    time: Res<Time>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    mut q: ParamSet<(
        Query<(&mut BackgroundColor, &mut BorderColor), With<AgentPillMarker>>,
        Query<&mut BackgroundColor, With<AgentPillDotMarker>>,
        Query<(&mut Text, &mut TextColor), With<AgentPillTextMarker>>,
    )>,
) {
    use xgent_agent::ConversationStatus::*;

    let (key, dot, pill_bg, pill_border, text_color, pulsing) = match conv.status {
        Idle => (
            "status-ready",
            theme.st_ok,
            theme.subtle,
            theme.border,
            theme.text_dim,
            false,
        ),
        Thinking => (
            "status-thinking",
            theme.accent_interactive,
            theme.accent_bg,
            Color::NONE,
            theme.accent_interactive,
            true,
        ),
        Streaming => (
            "status-streaming",
            theme.accent_interactive,
            theme.accent_bg,
            Color::NONE,
            theme.accent_interactive,
            true,
        ),
        ToolRunning | Confirming | Aborting => (
            "status-tool-running",
            theme.st_pending,
            theme.st_pending_bg,
            Color::NONE,
            theme.st_pending,
            false,
        ),
        Error => (
            "status-error",
            theme.st_fail,
            theme.st_fail_bg,
            Color::NONE,
            theme.st_fail,
            false,
        ),
    };

    // pill 底/边
    if let Ok((mut bg, mut border)) = q.p0().single_mut() {
        if bg.0 != pill_bg {
            bg.0 = pill_bg;
        }
        border.set_all(pill_border);
    }

    // 状态点（忙时脉冲）
    if let Ok(mut dot_bg) = q.p1().single_mut() {
        let alpha = if pulsing {
            0.4 + 0.6
                * (0.5
                    + 0.5 * (time.elapsed().as_secs_f64() * std::f64::consts::TAU / 1.2).sin())
        } else {
            1.0
        } as f32;
        let srgba = dot.to_srgba();
        let want = Color::srgba(srgba.red, srgba.green, srgba.blue, alpha);
        if dot_bg.0 != want {
            dot_bg.0 = want;
        }
    }

    // 文本（变更检测避免每帧分配）
    if conv.is_changed() || loc.is_changed() {
        if let Ok((mut text, mut color)) = q.p2().single_mut() {
            let label = tr(&loc, key).to_string();
            if text.0 != label {
                text.0 = label;
            }
            if color.0 != text_color {
                color.0 = text_color;
            }
        }
    }
}

/// 处理顶栏按钮点击（历史钮已迁 rail，M3-T5）。
fn handle_top_bar_buttons(
    q_new: Query<&Interaction, (With<NewSessionButtonMarker>, Changed<Interaction>)>,
    q_palette: Query<&Interaction, (With<PaletteButtonMarker>, Changed<Interaction>)>,
    q_settings: Query<&Interaction, (With<SettingsButtonMarker>, Changed<Interaction>)>,
    q_provider: Query<&Interaction, (With<ProviderButtonMarker>, Changed<Interaction>)>,
    mut palette: ResMut<CommandPaletteState>,
    mut settings_state: ResMut<crate::settings_panel::SettingsPanelState>,
    mut new_session: MessageWriter<xgent_agent::NewSessionMessage>,
) {
    for i in q_new.iter() {
        if *i == Interaction::Pressed {
            new_session.write(xgent_agent::NewSessionMessage);
        }
    }
    // 命令面板按钮
    for i in q_palette.iter() {
        if *i == Interaction::Pressed {
            palette.open();
        }
    }
    // 设置按钮
    for i in q_settings.iter() {
        if *i == Interaction::Pressed {
            settings_state.open = !settings_state.open;
        }
    }
    // provider 标签点击 → 打开设置面板
    for i in q_provider.iter() {
        if *i == Interaction::Pressed {
            settings_state.open = true;
        }
    }
}
