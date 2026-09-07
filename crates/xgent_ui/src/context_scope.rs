//! 上下文条（v7）：会话区顶部的「工作文件」只读 chips（方案 §8.10）。
//!
//! 数据源 = 编辑器已打开 tabs（`EditorTabs` + `EditorBuffer.path`）；**只读展示**、
//! 无删除钮——agent 侧暂无会话上下文文件集概念（`UserInputMessage` 仅携带
//! editor_queries），删 chip 关 tab 会误伤用户。「+ 添加」打开文件面板。
//! chips 挂 `ContextChipMarker`，tab 列表变化时按签名比对整批重建（标签/添加钮保留）。

use crate::fonts::ui_text;
use bevy::prelude::*;
use xgent_settings::Localizer;

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

/// 上下文条插件。
pub struct ContextScopePlugin;

impl Plugin for ContextScopePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            spawn_context_scope
                .after(crate::layout::spawn_layout)
                .after(crate::chat_panel::spawn_chat_panel),
        )
        .add_systems(Update, (rebuild_context_chips, handle_add_click));
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

/// tabs 列表变化时重建 chips（签名比对避免每帧重建；标签/添加钮保留）。
fn rebuild_context_chips(
    mut commands: Commands,
    tabs: Res<EditorTabs>,
    buffers: Query<&EditorBuffer>,
    theme: Res<Theme>,
    icons: Res<IconAssets>,
    mut cached: Local<String>,
    q_row: Query<Entity, With<ContextScopeMarker>>,
    q_children: Query<&Children, With<ContextScopeMarker>>,
    chips: Query<Entity, With<ContextChipMarker>>,
) {
    let Ok(row) = q_row.single() else {
        return;
    };
    let Ok(_children) = q_children.single() else {
        return;
    };

    // 签名比对
    let mut paths: Vec<String> = Vec::new();
    for &e in &tabs.tabs {
        if let Ok(buf) = buffers.get(e) {
            paths.push(buf.path.display().to_string());
        }
    }
    let signature = paths.join("\u{1}");
    if *cached == signature {
        return;
    }
    *cached = signature;

    // 清旧 chips
    for chip in chips.iter() {
        commands.entity(chip).despawn();
    }

    // 重建
    for path in &paths {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        commands.entity(row).with_children(|row| {
            row.spawn((
                Button,
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
                Tooltip { text: path.clone() },
                ContextChipMarker,
            ))
            .with_children(|chip| {
                chip.spawn(icon(&icons, "file", 12.0, theme.st_pending));
                chip.spawn(ui_text(
                    name,
                    type_scale::MICRO,
                    400,
                    theme.text_dim,
                    type_scale::line_height::UI,
                ));
            });
        });
    }
}

/// 「添加上下文」点击 → 打开文件抽屉（M5-T6 抽屉化）。
fn handle_add_click(
    q: Query<&Interaction, (With<ContextAddMarker>, Changed<Interaction>)>,
    mut drawer: ResMut<crate::layout::FileDrawerOpen>,
) {
    for interaction in q.iter() {
        if *interaction == Interaction::Pressed {
            drawer.0 = true;
        }
    }
}
