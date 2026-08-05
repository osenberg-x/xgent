//! 顶栏：品牌 logo + provider/model pill + 新建会话 + 命令面板 + 设置。
//!
//! v2 重构：精简顶栏，导航入口移至活动栏。顶栏只保留品牌、provider 标签、
//! 全局操作按钮。

use bevy::prelude::*;
use xgent_agent::ProviderInfo;
use xgent_settings::Localizer;
use xui::command_palette::CommandPaletteState;

use crate::i18n::tr;
use crate::layout::TopBarMarker;
use crate::theme::{Theme, space};

/// 顶栏 provider/model 标签节点标记。
#[derive(Component, Default)]
pub struct ProviderLabelMarker;

/// 新建会话按钮标记。
#[derive(Component, Default)]
pub struct NewSessionButtonMarker;

/// 历史会话按钮标记。
#[derive(Component, Default)]
pub struct HistoryButtonMarker;

/// 顶栏 provider 标签按钮标记（点击打开设置面板切换 provider）。
#[derive(Component, Default)]
pub struct ProviderButtonMarker;

/// 顶栏命令面板按钮标记。
#[derive(Component, Default)]
pub struct PaletteButtonMarker;

/// 顶栏设置按钮标记。
#[derive(Component, Default)]
pub struct SettingsButtonMarker;

/// 顶栏插件。
pub struct TopBarPlugin;

impl Plugin for TopBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_top_bar.after(crate::layout::spawn_layout))
            .add_systems(
                Update,
                (update_provider_label, handle_top_bar_buttons)
                    .after(crate::command_palette::handle_palette_triggers),
            );
    }
}

/// 启动时在顶栏内 spawn：品牌 logo + provider pill + spacer + 新建会话 + 命令面板 + 设置。
fn spawn_top_bar(
    mut commands: Commands,
    q_bar: Query<Entity, With<TopBarMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
) {
    let Ok(bar) = q_bar.single() else {
        return;
    };
    let font = theme.font_size;
    let font_size = FontSize::Px(font);

    commands.entity(bar).with_children(|p| {
        // 品牌：logo 方块 + "XGent" 文字
        p.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::SM),
                padding: UiRect::right(px(space::MD)),
                ..default()
            },
        ))
        .with_children(|brand| {
            // logo 方块（渐变色 — 用 elevated 底 + accent 文字模拟）
            brand.spawn((
                Node {
                    width: px(26.0),
                    height: px(26.0),
                    border_radius: BorderRadius::all(px(6.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(theme.accent),
                Text::new("✦"),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(theme.bg),
            ));
            brand.spawn((
                Text::new("XGent"),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(theme.text),
            ));
        });

        // 分隔线
        p.spawn((
            Node {
                width: px(1.0),
                height: px(20.0),
                ..default()
            },
            BackgroundColor(theme.border),
        ));

        // provider/model pill（elevated 底 + 边框 + 绿色状态点）
        p.spawn((
            Button,
            Node {
                padding: UiRect::horizontal(px(10.0)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::SM),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            BackgroundColor(theme.elevated),
            BorderColor::all(theme.border),
            Text::new(String::new()),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text),
            ProviderLabelMarker,
            ProviderButtonMarker,
        ))
        .with_children(|pill| {
            // 绿色状态点
            pill.spawn((
                Node {
                    width: px(6.0),
                    height: px(6.0),
                    border_radius: BorderRadius::all(px(3.0)),
                    ..default()
                },
                BackgroundColor(theme.st_ok),
            ));
        });

        // spacer
        p.spawn((Node {
            flex_grow: 1.0,
            ..default()
        },));

        // ＋ 新建会话按钮
        p.spawn((
            Button,
            Node {
                padding: UiRect::horizontal(px(space::MD)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::XS),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            BackgroundColor(theme.elevated),
            BorderColor::all(theme.border),
            Text::new(format!("+ {}", tr(&loc, "topbar-new-session"))),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text),
            NewSessionButtonMarker,
        ));

        // 🕐 历史会话按钮
        p.spawn((
            Button,
            Node {
                width: px(30.0),
                height: px(30.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            BackgroundColor(theme.elevated),
            BorderColor::all(theme.border),
            Text::new("🕐"),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(theme.text_dim),
            HistoryButtonMarker,
        ));

        // 🔍 命令面板按钮
        p.spawn((
            Button,
            Node {
                width: px(30.0),
                height: px(30.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            Text::new("🔍"),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(theme.text_dim),
            PaletteButtonMarker,
        ));

        // ⚙ 设置按钮
        p.spawn((
            Button,
            Node {
                width: px(30.0),
                height: px(30.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            Text::new("⚙"),
            TextFont {
                font_size: FontSize::Px(15.0),
                ..default()
            },
            TextColor(theme.text_dim),
            SettingsButtonMarker,
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

/// 处理顶栏按钮点击。
fn handle_top_bar_buttons(
    q_new: Query<&Interaction, (With<NewSessionButtonMarker>, Changed<Interaction>)>,
    q_history: Query<&Interaction, (With<HistoryButtonMarker>, Changed<Interaction>)>,
    q_palette: Query<&Interaction, (With<PaletteButtonMarker>, Changed<Interaction>)>,
    q_settings: Query<&Interaction, (With<SettingsButtonMarker>, Changed<Interaction>)>,
    q_provider: Query<&Interaction, (With<ProviderButtonMarker>, Changed<Interaction>)>,
    mut palette: ResMut<CommandPaletteState>,
    mut settings_state: ResMut<crate::settings_panel::SettingsPanelState>,
    mut history_state: ResMut<crate::session_history::SessionHistoryState>,
    mut new_session: MessageWriter<xgent_agent::NewSessionMessage>,
) {
    for i in q_new.iter() {
        if *i == Interaction::Pressed {
            new_session.write(xgent_agent::NewSessionMessage);
        }
    }
    // 🕐 历史会话按钮
    for i in q_history.iter() {
        if *i == Interaction::Pressed {
            history_state.open = true;
        }
    }
    // 🔍 命令面板按钮
    for i in q_palette.iter() {
        if *i == Interaction::Pressed {
            palette.open();
        }
    }
    // ⚙ 设置按钮
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
