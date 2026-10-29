//! `@` 补全接线（M6.5-T3）：触发、文件候选、DROPDOWN 浮层与键盘路由。

use std::hash::{Hash, Hasher};

use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;
use xgent_settings::Localizer;

use crate::chat_panel::{ChatInputCardMarker, ChatInputMarker};
use crate::completion::{
    CompletionCandidate, CompletionKind, CompletionPopupMarker, CompletionState, accept_candidate,
    clamp_index, detect_at_trigger, handle_completion_item_click, spawn_completion_popup,
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
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut state: ResMut<CompletionState>,
    mut consumed: ResMut<CompletionConsumed>,
    mut q_input: Query<&mut bevy::text::EditableText, With<ChatInputMarker>>,
) {
    if !state.active {
        return;
    }
    // 输入法组合期间不接管按键：Enter 属于选词提交，方向键属于 IME 候选窗。
    if q_input
        .single()
        .is_ok_and(|editable| editable.is_composing())
    {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        keys.clear_just_pressed(KeyCode::ArrowDown);
        state.select_next();
        return;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        keys.clear_just_pressed(KeyCode::ArrowUp);
        state.select_prev();
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        let text = q_input
            .single()
            .map(|editable| editable.editor().text().to_string())
            .unwrap_or_default();
        state.dismiss(&text);
        consumed.esc = true;
        return;
    }
    // 带任何修饰键的 Enter 不是「接受候选」：多行输入框用主修饰键 + Enter 发送，
    // 若在此误吞会让用户的发送变成「只把候选塞进输入框」。
    if keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]) {
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        let Some(candidate) = state.selected().cloned() else {
            return;
        };
        if let Ok(mut editable) = q_input.single_mut() {
            accept_candidate(&mut editable, &mut state, &candidate);
        } else {
            state.deactivate();
        }
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
        close_popup(&mut commands, &mut cache, &q_popups);
        return;
    };
    let text = editable.editor().text().to_string();
    if editable.is_composing() {
        // IME 组合期整段跳过：索引空间与可见文本长度不一致（`text()` 排除 preedit，
        // 选区索引含 preedit），且用户正在选词，不该重开或关闭浮层。
        return;
    }
    if state.suppress_text.as_deref() == Some(text.as_str()) {
        // 用户已主动关闭（Esc/点击/接受）：不但要停用状态，还要把浮层收掉，
        // 否则它会一直停在 DROPDOWN 层直到文本再次变化。
        state.deactivate();
        close_popup(&mut commands, &mut cache, &q_popups);
        return;
    }
    state.suppress_text = None;
    let cursor = clamp_index(&text, editable.editor().raw_selection().focus().index());
    let Some(trigger) = detect_at_trigger(&text, cursor) else {
        state.deactivate();
        close_popup(&mut commands, &mut cache, &q_popups);
        return;
    };
    let raw = &text[trigger + 1..cursor];
    let query = raw.strip_prefix("file:").unwrap_or(raw).to_lowercase();
    let candidates = file_candidates(&items, &root, &query);
    if candidates.is_empty() {
        state.deactivate();
        close_popup(&mut commands, &mut cache, &q_popups);
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
    // 锚点与尺寸取自 ComputedNode / UiGlobalTransform（**物理**像素），浮层的
    // left/top 是逻辑像素；HiDPI 下不换算会让浮层离输入框偏 (scale-1) 倍距离。
    let (anchor, input_size) = q_card
        .single()
        .map(|(node, transform)| {
            let scale = node.inverse_scale_factor();
            let (_, _, translation) = transform.to_scale_angle_translation();
            (translation * scale, node.size() * scale)
        })
        .unwrap_or((Vec2::new(64.0, 640.0), Vec2::new(640.0, 40.0)));
    let viewport = windows
        .single()
        .map(|window| Vec2::new(window.width(), window.height()))
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

/// 收掉补全浮层并清签名缓存（下一次重建才会重新 spawn）。
fn close_popup(
    commands: &mut Commands,
    cache: &mut CompletionUiCache,
    q_popups: &Query<Entity, With<CompletionPopupMarker>>,
) {
    cache.signature = None;
    for entity in q_popups.iter() {
        commands.entity(entity).despawn();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_panel::ChatInputMarker;
    use crate::completion::{CompletionCandidate, CompletionKind, CompletionState};

    #[test]
    fn enter_is_consumed_and_popup_closes() {
        let mut app = App::new();
        app.init_resource::<CompletionState>()
            .init_resource::<CompletionConsumed>()
            .insert_resource(ButtonInput::<KeyCode>::default())
            .add_systems(
                PreUpdate,
                (reset_completion_consumed, handle_completion_keys).chain(),
            );
        app.world_mut()
            .spawn((bevy::text::EditableText::new("hi @fi"), ChatInputMarker));
        {
            let mut state = app.world_mut().resource_mut::<CompletionState>();
            state.activate(
                vec![CompletionCandidate {
                    label: "src/main.rs".into(),
                    insert_text: "@file:src/main.rs ".into(),
                    kind: CompletionKind::File,
                }],
                3,
            );
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(!app.world().resource::<CompletionState>().active);
        assert!(app.world().resource::<CompletionConsumed>().enter);
    }

    #[test]
    fn modified_enter_is_not_swallowed_by_completion() {
        // 多行输入框用 Ctrl/Cmd+Enter 发送；补全不得把它当「接受候选」。
        let mut app = App::new();
        app.init_resource::<CompletionState>()
            .init_resource::<CompletionConsumed>()
            .insert_resource(ButtonInput::<KeyCode>::default())
            .add_systems(
                Update,
                (reset_completion_consumed, handle_completion_keys).chain(),
            );
        app.world_mut()
            .spawn((bevy::text::EditableText::new("hi @fi"), ChatInputMarker));
        app.world_mut().resource_mut::<CompletionState>().activate(
            vec![CompletionCandidate {
                label: "src/main.rs".into(),
                insert_text: "@file:src/main.rs ".into(),
                kind: CompletionKind::File,
            }],
            3,
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::Enter);
        }
        app.update();
        assert!(
            !app.world().resource::<CompletionConsumed>().enter,
            "带修饰键的 Enter 不能被判为接受候选"
        );
        assert!(
            app.world().resource::<CompletionState>().active,
            "补全状态应保持不变，让发送路径继续"
        );
    }

    #[test]
    fn dismiss_closes_the_visible_popup() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Theme>()
            .init_resource::<Localizer>()
            .init_resource::<CompletionState>()
            .init_resource::<CompletionConsumed>()
            .init_resource::<CompletionUiCache>()
            .insert_resource(FileTreeItems {
                items: Vec::new(),
                generation: 0,
            })
            .insert_resource(ProjectRoot {
                path: std::path::PathBuf::from("."),
            })
            .add_systems(Update, sync_completion_state);
        app.world_mut()
            .spawn((bevy::text::EditableText::new("@Cargo"), ChatInputMarker));
        app.world_mut().spawn((
            ChatInputCardMarker,
            ComputedNode::default(),
            UiGlobalTransform::default(),
        ));
        app.world_mut().spawn(LayerRoot(z::DROPDOWN));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let popup = app.world_mut().spawn(CompletionPopupMarker).id();
        app.world_mut()
            .resource_mut::<CompletionState>()
            .suppress_text = Some("@Cargo".into());
        app.update();
        assert!(
            app.world().get::<CompletionPopupMarker>(popup).is_none(),
            "主动关闭（Esc/点击/接受）后浮层必须从 DROPDOWN 层收掉"
        );
    }
}
