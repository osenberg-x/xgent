//! `@` 补全接线（M6.5-T3）：触发、文件候选、DROPDOWN 浮层与键盘路由。

use std::hash::{Hash, Hasher};

use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;
use xgent_settings::Localizer;

use crate::chat_panel::{ChatInputCardMarker, ChatInputMarker};
use crate::completion::{
    CompletionCandidate, CompletionKind, CompletionPopupMarker, CompletionState, accept_candidate,
    detect_at_trigger, handle_completion_item_click, spawn_completion_popup,
};
use crate::file_panel::{FileTreeItems, ProjectRoot};
use crate::layout::LayerRoot;
use crate::theme::{Theme, z};

/// 本帧补全消费的按键（发送与 Esc 路由据此让位）。
#[derive(Resource, Default, Debug)]
pub struct CompletionConsumed {
    /// Enter 已被补全接受消费。
    pub enter: bool,
    /// Esc 已被补全关闭消费。
    pub esc: bool,
}

/// 浮层重建签名，避免每帧重建。
#[derive(Resource, Default)]
struct CompletionUiCache {
    signature: Option<u64>,
}

/// 补全 UI 插件。
pub struct CompletionUiPlugin;

impl Plugin for CompletionUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CompletionState>()
            .init_resource::<CompletionConsumed>()
            .init_resource::<CompletionUiCache>()
            .add_systems(
                PreUpdate,
                (reset_completion_consumed, handle_completion_keys).chain(),
            )
            .add_systems(
                Update,
                (sync_completion_state, handle_completion_item_click).chain(),
            );
    }
}

fn reset_completion_consumed(mut consumed: ResMut<CompletionConsumed>) {
    consumed.enter = false;
    consumed.esc = false;
}

fn handle_completion_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<CompletionState>,
    mut consumed: ResMut<CompletionConsumed>,
    mut q_input: Query<&mut bevy::text::EditableText, With<ChatInputMarker>>,
) {
    if !state.active {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        state.select_next();
        return;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        state.select_prev();
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        state.deactivate();
        consumed.esc = true;
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        let Some(candidate) = state.selected().cloned() else {
            return;
        };
        if let Ok(mut editable) = q_input.single_mut() {
            accept_candidate(&mut editable, &state, &candidate);
        }
        state.deactivate();
        consumed.enter = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_completion_state(
    mut commands: Commands,
    theme: Res<Theme>,
    _loc: Res<Localizer>,
    mut state: ResMut<CompletionState>,
    mut cache: ResMut<CompletionUiCache>,
    q_input: Query<&bevy::text::EditableText, With<ChatInputMarker>>,
    q_card: Query<(&ComputedNode, &UiGlobalTransform), With<ChatInputCardMarker>>,
    items: Res<FileTreeItems>,
    root: Res<ProjectRoot>,
    q_popups: Query<Entity, With<CompletionPopupMarker>>,
    q_layers: Query<(Entity, &LayerRoot)>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(editable) = q_input.single() else {
        state.deactivate();
        if cache.signature.take().is_some() {
            for entity in q_popups.iter() {
                commands.entity(entity).despawn();
            }
        }
        return;
    };
    let text = editable.editor().text().to_string();
    let cursor = editable.editor().raw_selection().focus().index();
    let Some(trigger) = detect_at_trigger(&text, cursor) else {
        state.deactivate();
        if cache.signature.take().is_some() {
            for entity in q_popups.iter() {
                commands.entity(entity).despawn();
            }
        }
        return;
    };
    let raw = &text[trigger + 1..cursor.min(text.len())];
    let query = raw.strip_prefix("file:").unwrap_or(raw).to_lowercase();
    let candidates = file_candidates(&items, &root, &query);
    if candidates.is_empty() {
        state.deactivate();
        if cache.signature.take().is_some() {
            for entity in q_popups.iter() {
                commands.entity(entity).despawn();
            }
        }
        return;
    }
    if state.trigger_position != trigger || state.candidates != candidates {
        state.active = true;
        state.candidates = candidates;
        state.trigger_position = trigger;
        if state.selected_index >= state.candidates.len() {
            state.selected_index = 0;
        }
    }

    let signature = signature(&state);
    if cache.signature == Some(signature) {
        return;
    }
    cache.signature = Some(signature);
    for entity in q_popups.iter() {
        commands.entity(entity).despawn();
    }

    let Some((layer, _)) = q_layers.iter().find(|(_, band)| band.0 == z::DROPDOWN) else {
        return;
    };
    let (anchor, input_size) = q_card
        .single()
        .map(|(node, transform)| {
            let (_, _, translation) = transform.to_scale_angle_translation();
            (translation, node.size())
        })
        .unwrap_or((Vec2::new(64.0, 640.0), Vec2::new(640.0, 40.0)));
    let viewport = windows
        .single()
        .map(|window| Vec2::new(window.resolution.width(), window.resolution.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    spawn_completion_popup(
        &mut commands,
        &state,
        &theme,
        layer,
        anchor,
        input_size,
        viewport,
    );
}

fn signature(state: &CompletionState) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    state.active.hash(&mut hasher);
    state.trigger_position.hash(&mut hasher);
    state.selected_index.hash(&mut hasher);
    for candidate in &state.candidates {
        candidate.label.hash(&mut hasher);
        candidate.insert_text.hash(&mut hasher);
    }
    hasher.finish()
}

fn file_candidates(
    items: &FileTreeItems,
    root: &ProjectRoot,
    query: &str,
) -> Vec<CompletionCandidate> {
    let mut out: Vec<(String, String)> = items
        .items
        .iter()
        .filter(|item| !item.is_dir)
        .filter_map(|item| {
            let rel = item
                .path
                .strip_prefix(&root.path)
                .unwrap_or(&item.path)
                .to_string_lossy()
                .replace('\\', "/");
            if rel.to_lowercase().contains(query) {
                Some((rel.clone(), format!("@file:{rel} ")))
            } else {
                None
            }
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.truncate(50);
    out.into_iter()
        .map(|(label, insert_text)| CompletionCandidate {
            label,
            insert_text,
            kind: CompletionKind::File,
        })
        .collect()
}
