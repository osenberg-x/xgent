//! 差异页（v7，M5-T4）：上下文面板「差异」页签——当前 buffer vs 磁盘的行级差异。
//!
//! 数据源：激活 buffer 的 `EditorBuffer.disk_content`（磁盘快照）与
//! `xui::TextEditor.rope`（当前内容，pub 字段，Display 转 string）；diff 算法
//! 用共享的 [`crate::diff::line_diff`]。渲染：每行一个节点（行底 tint + 文字着色），
//! 无差异/无 buffer 时空态文案（i18n）。

use bevy::prelude::*;
use bevy::text::FontSize;
use xgent_settings::Localizer;

use crate::diff::{DiffKind, line_diff};
use crate::editor::buffer::EditorBuffer;
use crate::editor::tabs::EditorTabs;
use crate::i18n::tr;
use crate::theme::{Theme, space, type_scale};

/// 差异页容器标记（挂 `crate::layout::SideViewMarker`，显隐由
/// `apply_editor_view_visibility` 据 `SideViewContent::Diff` 控制）。
#[derive(Component, Default)]
pub struct DiffViewMarker;

/// 差异行节点标记（重建时批量清除）。
#[derive(Component, Default)]
pub struct DiffLineRowMarker;

/// 差异页空态文本标记。
#[derive(Component, Default)]
pub struct DiffEmptyMarker;

/// 差异页插件。
pub struct DiffViewPlugin;

impl Plugin for DiffViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_diff_view.after(super::spawn_editor_view))
            .add_systems(Update, rebuild_diff_view);
    }
}

/// 启动时在右侧分屏内 spawn 差异页容器（默认隐藏）。
pub fn spawn_diff_view(
    mut commands: Commands,
    q_side: Query<Entity, With<crate::layout::SideViewMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
) {
    let Ok(side) = q_side.single() else {
        return;
    };
    let view = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme.code_bg),
            DiffViewMarker,
        ))
        .with_children(|view| {
            view.spawn((
                Node {
                    padding: UiRect::all(px(space::MD)),
                    ..default()
                },
                Text::new(tr(&loc, "diff-empty-nobuffer").to_string()),
                TextFont {
                    font_size: FontSize::Px(type_scale::SMALL),
                    ..default()
                },
                TextColor(theme.text_muted),
                DiffEmptyMarker,
            ));
        })
        .id();
    commands.entity(side).add_child(view);
}

/// 内容切到 Diff、buffer 内容变化或激活 tab 变化时重建差异行。
pub(crate) fn rebuild_diff_view(
    mut commands: Commands,
    content: Res<crate::editor::SideViewContent>,
    tabs: Res<EditorTabs>,
    buffers: Query<&EditorBuffer>,
    editors: Query<&xui::TextEditor>,
    editors_changed: Query<(), Changed<xui::TextEditor>>,
    tabs_changed: Query<(), Changed<EditorTabs>>,
    q_view: Query<(Entity, &Children), With<DiffViewMarker>>,
    rows: Query<(), With<DiffLineRowMarker>>,
    empties: Query<(), With<DiffEmptyMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
) {
    if *content != crate::editor::SideViewContent::Diff {
        return;
    }
    // 触发：切入 Diff / buffer 内容变化 / 激活 tab 变化
    let triggered = content.is_changed()
        || tabs_changed.iter().count() > 0
        || editors_changed.iter().count() > 0;
    if !triggered {
        return;
    }

    let Ok((view, children)) = q_view.single() else {
        return;
    };

    // 激活 buffer
    let active = tabs
        .active
        .and_then(|idx| tabs.tabs.get(idx).copied())
        .or_else(|| tabs.tabs.last().copied());
    let Some(buf_entity) = active else {
        show_empty(
            &mut commands,
            &view,
            children,
            &empties,
            &rows,
            &tr(&loc, "diff-empty-nobuffer"),
        );
        return;
    };
    let (Ok(buffer), Ok(editor)) = (buffers.get(buf_entity), editors.get(buf_entity)) else {
        show_empty(
            &mut commands,
            &view,
            children,
            &empties,
            &rows,
            &tr(&loc, "diff-empty-nobuffer"),
        );
        return;
    };

    // 清旧行与空态
    for child in children.iter() {
        if rows.contains(child) || empties.contains(child) {
            commands.entity(child).despawn();
        }
    }

    let new_text = editor.rope.to_string();
    let diff = line_diff(&buffer.disk_content, &new_text);
    if !diff.iter().any(|l| l.kind != DiffKind::Context) {
        show_empty(
            &mut commands,
            &view,
            children,
            &empties,
            &rows,
            &tr(&loc, "diff-empty-clean"),
        );
        return;
    }

    for line in &diff {
        let (bg, fg) = match line.kind {
            DiffKind::Add => (theme.st_ok_bg, theme.str_),
            DiffKind::Del => (theme.st_fail_bg, theme.st_fail),
            DiffKind::Context => (Color::NONE, theme.text_faint),
        };
        commands.entity(view).with_children(|v| {
            v.spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::horizontal(px(space::SM)),
                    ..default()
                },
                BackgroundColor(bg),
                DiffLineRowMarker,
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(line.text.clone()),
                    TextFont {
                        font_size: FontSize::Px(type_scale::MONO),
                        ..default()
                    },
                    TextColor(fg),
                ));
            });
        });
    }
}

/// 空态：清行 → 显示文案（已翻译）。
fn show_empty(
    commands: &mut Commands,
    view: &Entity,
    children: &[Entity],
    empties: &Query<(), With<DiffEmptyMarker>>,
    rows: &Query<(), With<DiffLineRowMarker>>,
    text: &str,
) {
    if children.iter().any(|c| empties.contains(*c)) {
        return;
    }
    for child in children.iter() {
        if rows.contains(*child) {
            commands.entity(*child).despawn();
        }
    }
    commands.entity(*view).with_children(|v| {
        v.spawn((
            Node {
                padding: UiRect::all(px(space::MD)),
                ..default()
            },
            Text::new(text.to_string()),
            TextFont {
                font_size: FontSize::Px(type_scale::SMALL),
                ..default()
            },
            TextColor(Theme::dark().text_muted),
            DiffEmptyMarker,
        ));
    });
}
