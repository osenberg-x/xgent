//! 上下文条（v7）：会话区顶部的「工作文件」只读 chips（方案 §8.10）。
//!
//! 数据源 = 编辑器已打开 tabs（`EditorTabs` + `EditorBuffer.path`）；**只读展示**、
//! 无删除钮——agent 侧暂无会话上下文文件集概念（`UserInputMessage` 仅携带
//! editor_queries），删 chip 关 tab 会误伤用户。「+ 添加」打开文件面板。
//! chips 挂 `ContextChipMarker`，tab 列表变化时按签名比对整批重建（标签/添加钮保留）。

use crate::fonts::ui_text;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;
use xgent_settings::Localizer;

use crate::context_set::{ContextItem, ContextKind, ContextSet};
use crate::editor::buffer::EditorBuffer;
use crate::editor::tabs::EditorTabs;
use crate::i18n::tr;
use crate::kit::{HoverTint, IconAssets, Tooltip, icon};
use crate::layout::ChatPanelMarker;
use crate::theme::{Theme, space, type_scale};

/// 上下文条容器标记。
#[derive(Component, Default)]
pub struct ContextScopeMarker;

/// 条内固定标签标记（chips 重建时保留）。
#[derive(Component, Default)]
pub struct ContextScopeLabelMarker;

/// 单个文件 chip 标记（重建时批量清除）。
#[derive(Component, Default)]
pub struct ContextChipMarker;

/// 「添加上下文」按钮标记。
#[derive(Component, Default)]
pub struct ContextAddMarker;

/// 从 ContextSet 删除一项。
#[derive(Message)]
pub struct ContextSetRemove {
    /// 上下文项 id。
    pub id: String,
}

/// 上下文条右侧的 token 占比标签。
#[derive(Component)]
pub struct ContextTokenRatioMarker;

/// chip 上的删除钮。
#[derive(Component)]
pub struct ContextChipRemoveMarker {
    /// 对应上下文项 id。
    pub id: String,
}

/// 上下文同步状态：恢复会话后跳过一次 tab 同步，避免把恢复的集合覆盖。
#[derive(Resource, Default)]
pub struct ContextSetUiState {
    skip_tab_sync: bool,
}

/// 上下文条插件。
pub struct ContextScopePlugin;

impl Plugin for ContextScopePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContextSet>()
            .init_resource::<ContextSetUiState>()
            .add_message::<ContextSetRemove>()
            .add_systems(
                Startup,
                spawn_context_scope
                    .after(crate::layout::spawn_layout)
                    .after(crate::chat_panel::spawn_chat_panel),
            )
            .add_systems(
                Update,
                (
                    sync_context_from_tabs,
                    handle_chip_remove_click,
                    apply_context_remove,
                    restore_context_set,
                    rebuild_context_chips,
                    handle_add_click,
                    save_context_on_change,
                )
                    .chain(),
            );
    }
}

/// 启动时在对话面板顶部插入上下文条（index 0，置于消息列表之上）。
fn spawn_context_scope(
    mut commands: Commands,
    q_panel: Query<Entity, With<ChatPanelMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    icons: Res<IconAssets>,
) {
    let Ok(panel) = q_panel.single() else {
        return;
    };
    let row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::XS),
                padding: UiRect::horizontal(px(space::MD)),
                height: px(38.0),
                border: UiRect::bottom(px(1.0)),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.surface),
            BorderColor::all(theme.line),
            ContextScopeMarker,
        ))
        .with_children(|row| {
            row.spawn((
                ui_text(
                    tr(&loc, "context-scope-label").to_string(),
                    type_scale::TINY,
                    510,
                    theme.text_muted,
                    type_scale::line_height::UI,
                ),
                ContextScopeLabelMarker,
            ));
            row.spawn((
                Button,
                Hovered::default(),
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
                BorderColor::all(theme.border_hover),
                HoverTint::ghost(&theme),
                ContextAddMarker,
            ))
            .with_children(|add| {
                add.spawn(icon(&icons, "plus", 12.0, theme.text_muted));
                add.spawn(ui_text(
                    tr(&loc, "context-add").to_string(),
                    type_scale::MICRO,
                    400,
                    theme.text_muted,
                    type_scale::line_height::UI,
                ));
            });
        })
        .id();
    commands.entity(panel).insert_children(0, &[row]);
}

/// 编辑器打开的 tab 同步进 ContextSet（只增不减；用户删除后不会被静默加回）。
fn sync_context_from_tabs(
    tabs: Res<EditorTabs>,
    buffers: Query<&EditorBuffer>,
    mut set: ResMut<ContextSet>,
    mut ui: ResMut<ContextSetUiState>,
) {
    if ui.skip_tab_sync {
        ui.skip_tab_sync = false;
        return;
    }
    for &entity in &tabs.tabs {
        let Ok(buffer) = buffers.get(entity) else {
            continue;
        };
        let id = buffer.path.display().to_string();
        if set.contains(&id) || set.is_removed(&id) {
            continue;
        }
        let label = buffer
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| id.clone());
        let content = buffer.disk_content.clone();
        set.add(ContextItem {
            id,
            kind: ContextKind::File,
            label,
            token_count: (content.chars().count() as u64 / 4).max(1),
            content,
        });
    }
}

/// chip 删除钮点击 → 发删除消息。
fn handle_chip_remove_click(
    q: Query<(&ContextChipRemoveMarker, &Pressed), Changed<Pressed>>,
    mut remove: MessageWriter<ContextSetRemove>,
) {
    for (marker, _) in q.iter() {
        remove.write(ContextSetRemove {
            id: marker.id.clone(),
        });
    }
}

/// 应用删除消息。
fn apply_context_remove(mut reader: MessageReader<ContextSetRemove>, mut set: ResMut<ContextSet>) {
    for ev in reader.read() {
        set.remove(&ev.id);
    }
}

/// 会话恢复时替换 ContextSet。
fn restore_context_set(
    mut reader: MessageReader<xgent_agent::SessionRestoredMessage>,
    mut set: ResMut<ContextSet>,
    mut ui: ResMut<ContextSetUiState>,
) {
    for ev in reader.read() {
        ui.skip_tab_sync = true;
        set.clear();
        for snapshot in &ev.context_set {
            set.add(ContextItem {
                id: snapshot.id.clone(),
                kind: ContextKind::from_persisted(&snapshot.kind),
                label: snapshot.label.clone(),
                token_count: snapshot.token_count,
                content: snapshot.content.clone(),
            });
        }
        for id in &ev.removed_context {
            set.mark_removed(id);
        }
    }
}

/// 把 ContextSet 写入会话 sidecar（会话内持久化）。
fn save_context_on_change(set: Res<ContextSet>, conv: Option<Res<xgent_agent::Conversation>>) {
    if !set.is_changed() {
        return;
    }
    let Some(conv) = conv else {
        return;
    };
    // 测试/未开始会话的占位 id 不落盘（默认 1）。
    if conv.id.0 < 1_000_000_000_000 {
        return;
    }
    let snapshots: Vec<xgent_core::session::ContextSnapshot> = set
        .items()
        .iter()
        .map(|item| xgent_core::session::ContextSnapshot {
            id: item.id.clone(),
            kind: item.kind.as_str().to_string(),
            label: item.label.clone(),
            token_count: item.token_count,
            content: item.content.clone(),
        })
        .collect();
    let removed = set.removed_ids();
    let _ =
        xgent_agent::session_store::save_context_set(&conv.id.0.to_string(), &snapshots, &removed);
}

/// ContextSet 变化时重建 chips（签名比对避免每帧重建；标签/添加钮保留）。
fn rebuild_context_chips(
    mut commands: Commands,
    set: Res<ContextSet>,
    theme: Res<Theme>,
    icons: Res<IconAssets>,
    loc: Res<xgent_settings::Localizer>,
    global: Option<Res<xgent_settings::GlobalConfigRes>>,
    mut cached: Local<String>,
    q_row: Query<Entity, With<ContextScopeMarker>>,
    chips: Query<Entity, With<ContextChipMarker>>,
    ratios: Query<Entity, With<ContextTokenRatioMarker>>,
) {
    let Ok(row) = q_row.single() else {
        return;
    };

    let window = global
        .as_deref()
        .and_then(|global| {
            global
                .providers
                .get(&global.default_provider)
                .map(|provider| provider.resolve_context_window(&global.default_model).0)
        })
        .unwrap_or(xgent_settings_core::global::defaults::CONTEXT_WINDOW);
    let signature = set
        .items()
        .iter()
        .map(|item| format!("{}|{}", item.id, item.label))
        .collect::<Vec<_>>()
        .join("\u{1}");
    if *cached == signature {
        return;
    }
    *cached = signature;

    for chip in chips.iter() {
        commands.entity(chip).despawn();
    }
    for ratio in ratios.iter() {
        commands.entity(ratio).despawn();
    }

    for item in set.items() {
        let id = item.id.clone();
        commands.entity(row).with_children(|row| {
            row.spawn((
                Button,
                Hovered::default(),
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(space::XS),
                    padding: UiRect::horizontal(px(space::XS + 1.0)),
                    border_radius: BorderRadius::MAX,
                    border: UiRect::all(px(1.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BorderColor::all(theme.border),
                HoverTint::ghost(&theme),
                Tooltip {
                    text: item.id.clone(),
                },
                ContextChipMarker,
            ))
            .with_children(|chip| {
                chip.spawn(icon(&icons, "file", 12.0, theme.st_pending));
                chip.spawn(ui_text(
                    item.label.clone(),
                    type_scale::MICRO,
                    400,
                    theme.text_dim,
                    type_scale::line_height::UI,
                ));
                chip.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        padding: UiRect::horizontal(px(space::XS)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    HoverTint::ghost(&theme),
                    ContextChipRemoveMarker { id: id.clone() },
                ))
                .with_children(|remove| {
                    remove.spawn(ui_text(
                        "×",
                        type_scale::MICRO,
                        400,
                        theme.text_muted,
                        type_scale::line_height::UI,
                    ));
                });
            });
        });
    }

    let pct = if window == 0 {
        0.0
    } else {
        set.total_tokens() as f64 * 100.0 / window as f64
    };
    let ratio = crate::i18n::tr_with(
        &loc,
        "context-token-ratio",
        &[
            ("used", set.total_tokens().to_string()),
            ("window", window.to_string()),
            ("pct", format!("{pct:.1}")),
        ],
    );
    commands.entity(row).with_children(|row| {
        row.spawn((
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            ui_text(
                ratio,
                type_scale::MICRO,
                400,
                theme.text_muted,
                type_scale::line_height::UI,
            ),
            ContextTokenRatioMarker,
        ));
    });
}

/// 「添加上下文」点击 → 打开文件抽屉（M5-T6 抽屉化）。
fn handle_add_click(
    q: Query<(), (With<ContextAddMarker>, Added<Pressed>)>,
    mut drawer_toggle: MessageWriter<crate::overlay::DrawerToggle>,
) {
    for _ in q.iter() {
        // v8 M1：与 rail / 快捷键走同一条消息，互斥由 OverlayRegistry 裁决。
        drawer_toggle.write(crate::overlay::DrawerToggle(
            crate::overlay::OverlayId::FileDrawer,
        ));
    }
}
