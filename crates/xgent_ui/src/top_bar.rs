//! 顶栏：项目名 · provider/model 标签 · 新建会话 · 设置。
//!
//! MVP 顶栏极简，复杂操作经命令面板入口。

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

/// 设置按钮标记。
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

/// 启动时在顶栏内 spawn：品牌 xgent ▾ + provider ▾ + spacer + 新建会话 btn + ⚙ icon-btn。
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
        // 品牌：xgent ▾（白色加粗 + caret）
        p.spawn((
            Node {
                padding: UiRect::all(px(space::XS)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::XS),
                ..default()
            },
            Text::new("xgent ▾"),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text),
        ));
        // provider/model：📦 {label} ▾（panel 底 + 边框）
        p.spawn((
            Node {
                padding: UiRect::all(px(space::XS)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::XS),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(theme.panel),
            BorderColor::all(theme.border),
            Text::new(String::new()),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text),
            ProviderLabelMarker,
        ));
        // spacer
        p.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        // ＋新建会话按钮（btn 样式：panel 底 + 边框）
        p.spawn((
            Button,
            Node {
                padding: UiRect::horizontal(px(space::MD)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(theme.panel),
            BorderColor::all(theme.border),
            Text::new(format!("＋ {}", tr(&loc, "topbar-new-session"))),
            TextFont {
                font_size,
                ..default()
            },
            TextColor(theme.text),
            NewSessionButtonMarker,
        ));
        // ⚙ 设置 icon-btn（28px 方形，hover panel 底）
        p.spawn((
            Button,
            Node {
                width: px(28.0),
                height: px(28.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            Text::new("⚙"),
            TextFont {
                font_size,
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
        format!("📦 {} / {} ▾", info.id, info.model)
    };
}

/// 处理顶栏按钮点击。
fn handle_top_bar_buttons(
    q_new: Query<&Interaction, (With<NewSessionButtonMarker>, Changed<Interaction>)>,
    q_settings: Query<&Interaction, (With<SettingsButtonMarker>, Changed<Interaction>)>,
    mut palette: ResMut<CommandPaletteState>,
    mut settings_state: ResMut<crate::settings_panel::SettingsPanelState>,
) {
    for i in q_new.iter() {
        if *i == Interaction::Pressed {
            palette.open();
        }
    }
    for i in q_settings.iter() {
        if *i == Interaction::Pressed {
            settings_state.open = !settings_state.open;
        }
    }
}
