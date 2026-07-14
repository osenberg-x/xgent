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
            .add_systems(Update, (update_provider_label, handle_top_bar_buttons).after(crate::command_palette::handle_palette_triggers));
    }
}

/// 启动时在顶栏内 spawn provider/model 标签 + 按钮。
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
    commands.entity(bar).with_children(|p| {
        // 右侧弹性间距
        p.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });

        // provider/model 标签
        p.spawn((
            Node {
                padding: UiRect::horizontal(px(space::SM)),
                ..default()
            },
            Text::new(String::new()),
            TextFont {
                font_size: FontSize::Px(font),
                ..default()
            },
            TextColor(theme.text_dim),
            ProviderLabelMarker,
        ));

        // 新建会话按钮
        p.spawn((
            Button,
            Node {
                padding: UiRect::horizontal(px(space::SM)),
                ..default()
            },
            Text::new(tr(&loc, "topbar-new-session")),
            TextFont {
                font_size: FontSize::Px(font),
                ..default()
            },
            TextColor(theme.text_dim),
            NewSessionButtonMarker,
        ));

        // 设置按钮
        p.spawn((
            Button,
            Node {
                padding: UiRect::horizontal(px(space::SM)),
                ..default()
            },
            Text::new(tr(&loc, "topbar-settings")),
            TextFont {
                font_size: FontSize::Px(font),
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
    q_settings: Query<&Interaction, (With<SettingsButtonMarker>, Changed<Interaction>)>,
    mut palette: ResMut<CommandPaletteState>,
) {
    for i in q_new.iter() {
        if *i == Interaction::Pressed {
            // MVP：直接打开命令面板触发 session.new
            palette.open();
        }
    }
    for i in q_settings.iter() {
        if *i == Interaction::Pressed {
            palette.open();
        }
    }
}
