//! F-12 成本面板：当前会话、历史累计与按模型分解。

use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use xgent_settings::GlobalConfigRes;
use xgent_settings::Localizer;
use xgent_settings_core::paths::sessions_dir;

use crate::cost::{CostHistoryState, CostPanelState, CostStats, format_cost};
use crate::fonts::ui_text;
use crate::i18n::{tr, tr_with};
use crate::theme::{Theme, space, type_scale};

/// 成本 overlay 根。
#[derive(Component, Default)]
pub struct CostPanelRoot;

/// 关闭按钮。
#[derive(Component, Default)]
pub struct CostPanelClose;

/// 当前会话文本。
#[derive(Component, Default)]
pub struct CostCurrentText;

/// 历史累计文本。
#[derive(Component, Default)]
pub struct CostHistoryText;

/// 按模型文本。
#[derive(Component, Default)]
pub struct CostModelsText;

/// 成本面板插件。
pub struct CostPanelPlugin;

impl Plugin for CostPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_cost_panel.after(crate::layout::spawn_layout))
            .add_systems(
                Update,
                (
                    sync_cost_panel,
                    poll_cost_history,
                    close_cost_panel,
                    update_cost_texts,
                )
                    .chain(),
            );
    }
}

fn spawn_cost_panel(
    mut commands: Commands,
    q_root: Query<Entity, With<crate::layout::UiRoot>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
) {
    let Ok(root) = q_root.single() else {
        return;
    };
    let overlay = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.overlay),
            Visibility::Hidden,
            crate::kit::drawer_z(),
            CostPanelRoot,
        ))
        .id();
    commands.entity(root).add_child(overlay);

    let card = commands
        .spawn((
            Node {
                width: Val::Px(560.0),
                max_height: Val::Percent(88.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(space::LG)),
                row_gap: Val::Px(space::SM),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.elevated),
            BorderColor::all(theme.border),
            crate::kit::block_press_bubbling(),
            Pickable::default(),
            Button,
            Hovered::default(),
        ))
        .id();
    commands.entity(overlay).add_child(card);

    commands.entity(card).with_children(|card| {
        card.spawn((Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },))
            .with_children(|head| {
                head.spawn(ui_text(
                    tr(&loc, "cost-title"),
                    type_scale::H3,
                    590,
                    theme.text,
                    type_scale::line_height::UI,
                ));
                head.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        padding: UiRect::all(Val::Px(space::SM)),
                        ..default()
                    },
                    ui_text(
                        tr(&loc, "cost-close"),
                        type_scale::BODY,
                        400,
                        theme.text_dim,
                        type_scale::line_height::UI,
                    ),
                    CostPanelClose,
                ));
            });

        card.spawn(ui_text(
            String::new(),
            type_scale::BODY,
            400,
            theme.text,
            type_scale::line_height::UI,
        ))
        .insert(CostCurrentText);

        card.spawn(ui_text(
            String::new(),
            type_scale::BODY_SM,
            400,
            theme.text_dim,
            type_scale::line_height::UI,
        ))
        .insert(CostHistoryText);

        card.spawn(ui_text(
            String::new(),
            type_scale::MICRO,
            400,
            theme.text_muted,
            type_scale::line_height::UI,
        ))
        .insert(CostModelsText);
    });
}

fn sync_cost_panel(
    state: Res<CostPanelState>,
    modal: Option<Res<crate::modal::ModalStack>>,
    q_root: Query<Entity, With<CostPanelRoot>>,
    mut q_vis: Query<&mut Visibility, With<CostPanelRoot>>,
    mut push: MessageWriter<crate::modal::ModalPush>,
    mut pop: MessageWriter<crate::modal::ModalPop>,
    mut history: ResMut<CostHistoryState>,
    global: Res<GlobalConfigRes>,
) {
    if !state.is_changed() {
        return;
    }
    let Ok(entity) = q_root.single() else {
        return;
    };
    if let Ok(mut visibility) = q_vis.single_mut() {
        *visibility = if state.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    let contains = modal.as_deref().is_some_and(|stack| stack.contains("cost"));
    if state.open && !contains {
        push.write(crate::modal::ModalPush {
            id: "cost",
            entity,
            dismiss: crate::modal::Dismiss::EscOnly,
            opener_focus: None,
        });
        history.start_scan(sessions_dir(), global.0.pricing.clone());
    } else if !state.open && contains {
        pop.write(crate::modal::ModalPop { id: Some("cost") });
    }
}

fn poll_cost_history(mut history: ResMut<CostHistoryState>) {
    history.poll();
}

fn close_cost_panel(
    q_close: Query<(), (With<CostPanelClose>, Added<Pressed>)>,
    mut dismiss: MessageReader<crate::modal::ModalDismissRequested>,
    mut state: ResMut<CostPanelState>,
) {
    for _ in q_close.iter() {
        state.open = false;
    }
    for request in dismiss.read() {
        if request.id == "cost" {
            state.open = false;
        }
    }
}

fn update_cost_texts(
    state: Res<CostPanelState>,
    stats: Res<CostStats>,
    history: Res<CostHistoryState>,
    loc: Res<Localizer>,
    mut current: Query<&mut Text, With<CostCurrentText>>,
    mut history_text: Query<
        &mut Text,
        (
            With<CostHistoryText>,
            Without<CostCurrentText>,
            Without<CostModelsText>,
        ),
    >,
    mut models_text: Query<
        &mut Text,
        (
            With<CostModelsText>,
            Without<CostCurrentText>,
            Without<CostHistoryText>,
        ),
    >,
) {
    if !state.open {
        return;
    }
    if let Ok(mut text) = current.single_mut() {
        let next = tr_with(
            &loc,
            "cost-current",
            &[
                ("prompt", stats.prompt.to_string()),
                ("completion", stats.completion.to_string()),
                ("cost", format_cost(stats.cost)),
            ],
        );
        if text.0 != next {
            text.0 = next;
        }
    }
    if let Ok(mut text) = history_text.single_mut() {
        let next = if history.loading {
            tr(&loc, "cost-history-loading").to_string()
        } else if let Some(error) = &history.error {
            tr_with(&loc, "cost-history-error", &[("error", error.clone())])
        } else if let Some(result) = &history.result {
            if result.sessions == 0 {
                tr(&loc, "cost-history-empty").to_string()
            } else {
                tr_with(
                    &loc,
                    "cost-history",
                    &[
                        ("sessions", result.sessions.to_string()),
                        ("prompt", result.prompt.to_string()),
                        ("completion", result.completion.to_string()),
                        ("cost", format_cost(result.cost)),
                    ],
                )
            }
        } else {
            tr(&loc, "cost-history-loading").to_string()
        };
        if text.0 != next {
            text.0 = next;
        }
    }
    if let Ok(mut text) = models_text.single_mut() {
        let next = match &history.result {
            Some(result) if !result.models.is_empty() => {
                let models = result.models_by_usage();
                let shown = models.iter().take(20);
                let mut lines: Vec<String> = shown
                    .map(|model| {
                        tr_with(
                            &loc,
                            "cost-model",
                            &[
                                ("model", model.model.clone()),
                                ("prompt", model.prompt.to_string()),
                                ("completion", model.completion.to_string()),
                                ("cost", format_cost(model.cost)),
                            ],
                        )
                    })
                    .collect();
                if models.len() > 20 {
                    lines.push(tr_with(
                        &loc,
                        "cost-models-others",
                        &[("count", (models.len() - 20).to_string())],
                    ));
                }
                lines.join("\n")
            }
            _ => String::new(),
        };
        if text.0 != next {
            text.0 = next;
        }
    }
}
