//! 设置面板：provider 配置（api_base / api_key / model）。
//!
//! 面板由 top_bar settings 按钮或命令面板 `settings.open` 触发开关。
//! 用户填写 provider 配置后点保存，发 [`SaveProviderConfigMessage`]，
//! 由 `xgent_app` 中的系统经 IPC 写入 daemon 全局配置。
//!
//! 使用官方 `EditableText` 处理输入（光标/删除/IME 全由官方 text_input 系统）。

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::text::FontSize;
use xgent_settings::Localizer;

use crate::i18n::tr;
use crate::theme::{Theme, space};

/// 设置面板根节点标记。
#[derive(Component, Default)]
pub struct SettingsPanelMarker;

/// Provider ID 输入框标记。
#[derive(Component, Default)]
pub struct ProviderIdInput;

/// API Base 输入框标记。
#[derive(Component, Default)]
pub struct ApiBaseInput;

/// API Key 输入框标记。
#[derive(Component, Default)]
pub struct ApiKeyInput;

/// Model 输入框标记。
#[derive(Component, Default)]
pub struct ModelInput;

/// 保存按钮标记。
#[derive(Component, Default)]
pub struct SettingsSaveButtonMarker;

/// 关闭按钮标记。
#[derive(Component, Default)]
pub struct SettingsCloseButtonMarker;

/// kind 选择按钮标记（携带它代表的 ProviderKind）。
#[derive(Component)]
pub struct KindButton {
    /// 该按钮代表的 provider 类型
    pub kind: xgent_settings_core::global::ProviderKind,
    /// 是否当前选中
    pub selected: bool,
}

/// kind 选择器状态。
#[derive(Resource, Debug, Clone)]
pub struct KindSelector {
    /// 当前选中的 provider 类型
    pub current: xgent_settings_core::global::ProviderKind,
}

impl Default for KindSelector {
    fn default() -> Self {
        Self {
            current: xgent_settings_core::global::ProviderKind::OpenAiCompat,
        }
    }
}

/// 设置面板开关状态。
#[derive(Resource, Default)]
pub struct SettingsPanelState {
    pub open: bool,
}

/// 保存 provider 配置消息（UI → xgent_app IPC 系统）。
#[derive(Message, Debug, Clone)]
pub struct SaveProviderConfigMessage {
    /// provider id（providers map key），如 "openai"
    pub provider_id: String,
    /// provider 类型
    pub kind: xgent_settings_core::global::ProviderKind,
    /// API base URL
    pub api_base: String,
    /// API key
    pub api_key: String,
    /// 模型名（落 model_overrides["default"]）
    pub model: String,
}

/// 设置面板插件。
pub struct SettingsPanelPlugin;

impl Plugin for SettingsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveProviderConfigMessage>()
            .insert_resource(SettingsPanelState::default())
            .insert_resource(KindSelector::default())
            .add_systems(
                Update,
                (toggle_panel, handle_kind_button, handle_save_button).chain(),
            );
    }
}

/// 根据开关状态 spawn/despawn 面板。
fn toggle_panel(
    state: Res<SettingsPanelState>,
    mut commands: Commands,
    q_panel: Query<Entity, With<SettingsPanelMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
) {
    let panel_exists = q_panel.single().is_ok();

    if state.open && !panel_exists {
        spawn_panel(&mut commands, &theme, &loc);
    } else if !state.open && panel_exists {
        if let Ok(entity) = q_panel.single() {
            commands.entity(entity).despawn();
        }
    }
}

/// spawn 设置面板 overlay。
fn spawn_panel(
    commands: &mut Commands,
    theme: &Theme,
    loc: &Localizer,
) {
    let font = theme.font_size;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            SettingsPanelMarker,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    padding: UiRect::all(px(space::LG)),
                    border: UiRect::all(px(1.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(space::MD),
                    min_width: px(420.0),
                    ..default()
                },
                BackgroundColor(theme.panel),
                BorderColor::all(theme.border),
            ))
            .with_children(|card| {
                // 标题
                card.spawn((
                    Text::new(tr(loc, "settings-title")),
                    TextFont {
                        font_size: FontSize::Px(font + 2.0),
                        ..default()
                    },
                    TextColor(theme.text),
                ));

                // Provider ID
                card.spawn((
                    Text::new(tr(loc, "settings-provider-id")),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                ));
                card.spawn(text_input_node(theme, font, ProviderIdInput));

                // Kind 选择（横排按钮组，MVP 暴露 4 变体，隐藏 Custom）
                card.spawn((
                    Text::new(tr(loc, "settings-kind")),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                ));
                card.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(space::SM),
                        ..default()
                    },
                ))
                .with_children(|row| {
                    let kinds = [
                        (
                            xgent_settings_core::global::ProviderKind::OpenAiCompat,
                            tr(loc, "settings-kind-openai-compat"),
                        ),
                        (
                            xgent_settings_core::global::ProviderKind::ResponseApi,
                            tr(loc, "settings-kind-response-api"),
                        ),
                        (
                            xgent_settings_core::global::ProviderKind::Anthropic,
                            tr(loc, "settings-kind-anthropic"),
                        ),
                        (
                            xgent_settings_core::global::ProviderKind::Ollama,
                            tr(loc, "settings-kind-ollama"),
                        ),
                    ];
                    for (kind, label) in kinds {
                        row.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(px(space::SM)),
                                ..default()
                            },
                            BackgroundColor(theme.bar),
                            Text::new(label),
                            TextFont {
                                font_size: FontSize::Px(font),
                                ..default()
                            },
                            TextColor(theme.text),
                            KindButton {
                                kind,
                                selected: false,
                            },
                        ));
                    }
                });

                // API Base
                card.spawn((
                    Text::new(tr(loc, "settings-api-base")),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                ));
                card.spawn(text_input_node(theme, font, ApiBaseInput));

                // API Key
                card.spawn((
                    Text::new(tr(loc, "settings-api-key")),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                ));
                card.spawn(text_input_node(theme, font, ApiKeyInput));

                // Model
                card.spawn((
                    Text::new(tr(loc, "settings-model")),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                ));
                card.spawn(text_input_node(theme, font, ModelInput));

                // 按钮行
                card.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(space::MD),
                    ..default()
                },))
                    .with_children(|btns| {
                        btns.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(px(space::SM)),
                                ..default()
                            },
                            BackgroundColor(theme.accent),
                            Text::new(tr(loc, "settings-save")),
                            TextFont {
                                font_size: FontSize::Px(font),
                                ..default()
                            },
                            TextColor(theme.text),
                            SettingsSaveButtonMarker,
                        ));
                        btns.spawn((
                            Button,
                            Node {
                                padding: UiRect::all(px(space::SM)),
                                ..default()
                            },
                            BackgroundColor(theme.bar),
                            Text::new(tr(loc, "settings-close")),
                            TextFont {
                                font_size: FontSize::Px(font),
                                ..default()
                            },
                            TextColor(theme.text),
                            SettingsCloseButtonMarker,
                        ));
                    });
            });
        });
}

/// 创建一个 EditableText 输入框节点。
fn text_input_node(
    theme: &Theme,
    font: f32,
    marker: impl Component,
) -> impl Bundle {
    (
        Node {
            padding: UiRect::all(px(space::SM)),
            border: UiRect::all(px(1.0)),
            min_height: px(font + 8.0),
            ..default()
        },
        BackgroundColor(theme.bar),
        BorderColor::all(theme.border),
        TextFont {
            font_size: FontSize::Px(font),
            ..default()
        },
        TextColor(theme.text),
        bevy::text::TextCursorStyle::default(),
        marker,
        EditableText::default(),
    )
}

/// px 辅助。
fn px(v: f32) -> Val {
    Val::Px(v)
}

/// 处理 kind 按钮点击：切换当前选中 kind，更新按钮高亮。
fn handle_kind_button(
    q_kind: Query<(&Interaction, Entity, &KindButton), Changed<Interaction>>,
    mut selector: ResMut<KindSelector>,
    mut commands: Commands,
    q_all: Query<(Entity, &KindButton)>,
    theme: Res<Theme>,
) {
    for (interaction, _entity, kb) in q_kind.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        selector.current = kb.kind;
        // 更新所有 kind 按钮的选中状态与背景色
        for (e, kb) in q_all.iter() {
            let selected = kb.kind == selector.current;
            commands.entity(e).insert((
                KindButton {
                    kind: kb.kind,
                    selected,
                },
                BackgroundColor(if selected { theme.accent } else { theme.bar }),
            ));
        }
    }
}

/// 处理保存/关闭按钮点击。
fn handle_save_button(
    q_save: Query<&Interaction, (With<SettingsSaveButtonMarker>, Changed<Interaction>)>,
    q_close: Query<&Interaction, (With<SettingsCloseButtonMarker>, Changed<Interaction>)>,
    q_id: Query<&EditableText, With<ProviderIdInput>>,
    q_base: Query<&EditableText, With<ApiBaseInput>>,
    q_key: Query<&EditableText, With<ApiKeyInput>>,
    q_model: Query<&EditableText, With<ModelInput>>,
    kind_selector: Res<KindSelector>,
    mut state: ResMut<SettingsPanelState>,
    mut writer: MessageWriter<SaveProviderConfigMessage>,
) {
    for interaction in q_save.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let provider_id = q_id.single().map(|e| e.value().to_string()).unwrap_or_default();
        let api_base = q_base.single().map(|e| e.value().to_string()).unwrap_or_default();
        let api_key = q_key.single().map(|e| e.value().to_string()).unwrap_or_default();
        let model = q_model.single().map(|e| e.value().to_string()).unwrap_or_default();

        if !provider_id.is_empty() {
            writer.write(SaveProviderConfigMessage {
                provider_id,
                kind: kind_selector.current,
                api_base,
                api_key,
                model,
            });
        }
        state.open = false;
    }

    for interaction in q_close.iter() {
        if *interaction == Interaction::Pressed {
            state.open = false;
        }
    }
}
