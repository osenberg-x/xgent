//! 差异页（v7，M5-T4）：上下文面板「差异」页签——当前 buffer vs 磁盘的行级差异。
//!
//! 数据源：激活 buffer 的 `EditorBuffer.disk_content`（磁盘快照）与
//! `xui::TextEditor.rope`（当前内容，pub 字段，Display 转 string）；diff 算法
//! 用共享的 [`crate::diff::line_diff`]。渲染：每行一个节点（行底 tint + 文字着色），
//! 无差异/无 buffer 时空态文案（i18n）。

use bevy::prelude::*;
use xgent_settings::Localizer;

use crate::diff::{DiffKind, line_diff};
use crate::editor::buffer::EditorBuffer;
use crate::editor::tabs::EditorTabs;
use crate::fonts::{mono_text, ui_text};
use crate::i18n::tr;
use crate::theme::{Theme, space, type_scale};
/// 差异页容器标记（挂 `crate::layout::CenterHostMarker`，显隐由
/// `derive_panel_visibility` 依据注册表中的 `diff` 面板派生）。
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

/// 启动时在主区 CenterHost 内 spawn 差异页容器（默认隐藏）。
pub fn spawn_diff_view(
    mut commands: Commands,
    q_side: Query<Entity, With<crate::layout::CenterHostMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    mut register: MessageWriter<crate::panels::PanelRegister>,
) {
    let Ok(side) = q_side.single() else {
        return;
    };
    let view = commands
        .spawn((
            // v8 M3-T3：同列有 38px 页签条，页面用 `height:100%` 会把底部顶出
            // 视口（审计 D2）。改用 `region_fill`（flex 填满剩余空间）。
            // v8 M5-T3：初始隐藏改为构造期字面量（不再是运行时写手）。
            Node {
                display: Display::None,
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..xui::region_fill()
            },
            BackgroundColor(theme.code_bg),
            DiffViewMarker,
            crate::panels::PanelVisibility::of_panel(
                crate::panels::ids::DIFF,
                crate::panels::Slot::Center,
            ),
        ))
        .with_children(|view| {
            view.spawn((
                Node {
                    padding: UiRect::all(px(space::MD)),
                    ..default()
                },
                ui_text(
                    tr(&loc, "diff-empty-nobuffer").to_string(),
                    type_scale::SMALL,
                    400,
                    theme.text_muted,
                    type_scale::line_height::UI,
                ),
                // 种子空态也要带标记：否则 `show_empty` 的幂等守卫认不出它
                // （会再叠一份），真实差异行出现后清理循环也认不出它（永远留着）。
                DiffEmptyMarker,
            ));
        })
        .id();
    commands.entity(side).add_child(view);
    // v8 M5：注册为 `Slot::Center` 面板。
    register.write(crate::panels::PanelRegister {
        desc: crate::panels::PanelDescriptor::new(
            crate::panels::ids::DIFF,
            crate::panels::Slot::Center,
            "panel-diff",
            "diff",
            1,
        ),
        view: Some(view),
    });
}

/// 内容切到 Diff、buffer 内容变化或激活 tab 变化时重建差异行。
pub(crate) fn rebuild_diff_view(
    mut commands: Commands,
    registry: Res<crate::panels::PanelRegistry>,
    tabs: Res<EditorTabs>,
    buffers: Query<&EditorBuffer>,
    editors: Query<&xui::TextEditor>,
    editors_changed: Query<(), Changed<xui::TextEditor>>,
    tabs_changed: Query<(), Changed<EditorTabs>>,
    q_view: Query<(Entity, &Children), With<DiffViewMarker>>,
    rows: Query<(), With<DiffLineRowMarker>>,
    empties: Query<(), With<DiffEmptyMarker>>,
    theme: Res<Theme>,
    fonts: Res<crate::fonts::UiFonts>,
    loc: Res<Localizer>,
) {
    let diff_active = registry.is_visible(crate::panels::ids::DIFF);
    if !diff_active {
        return;
    }
    // 触发：切入 Diff / buffer 内容变化 / 激活 tab 变化
    let triggered = registry.is_changed()
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
            &theme,
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
            &theme,
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
            &theme,
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
                // v8 M4-T3：diff 行走等宽（此前是 ui_text + 等宽字号 = 比例字体冒充等宽）
                row.spawn(mono_text(
                    &fonts,
                    line.text.clone(),
                    type_scale::CODE,
                    fg,
                    type_scale::line_height::CTRL,
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
    theme: &Theme,
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
            ui_text(
                text.to_string(),
                type_scale::SMALL,
                400,
                theme.text_muted,
                type_scale::line_height::UI,
            ),
            DiffEmptyMarker,
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::{PanelDescriptor, PanelRegistry, Slot, ids};

    /// 最小环境：注册并激活 `diff` 面板，只跑差异页的两条系统。
    fn diff_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<crate::panels::PanelRegister>()
            .init_resource::<Theme>()
            .init_resource::<Localizer>()
            .insert_resource(crate::fonts::UiFonts::fallback())
            .init_resource::<EditorTabs>()
            .init_resource::<PanelRegistry>();
        app.world_mut().spawn(crate::layout::CenterHostMarker);
        {
            let mut registry = app.world_mut().resource_mut::<PanelRegistry>();
            registry.register(
                PanelDescriptor::new(ids::DIFF, Slot::Center, "panel-diff", "diff", 1),
                None,
            );
            registry.activate(ids::DIFF);
        }
        app.add_systems(Startup, spawn_diff_view);
        app.add_systems(Update, rebuild_diff_view);
        app
    }

    /// 连续触发 `times` 次重建（tab 指向不存在的 buffer → 每帧都走空态分支）。
    fn trigger(app: &mut App, times: usize) {
        for _ in 0..times {
            app.world_mut()
                .resource_mut::<EditorTabs>()
                .tabs
                .push(Entity::PLACEHOLDER);
            app.update();
        }
    }

    fn entities_with<M: Component>(app: &mut App) -> Vec<Entity> {
        let mut q = app.world_mut().query_filtered::<Entity, With<M>>();
        q.iter(app.world()).collect()
    }

    /// 空态节点必须带 `DiffEmptyMarker`，且反复触发只有一份。
    ///
    /// 缺标记时 `show_empty` 的幂等守卫（`empties.contains`）恒假 → 每次内容变化
    /// 都再 spawn 一份空态文案；同时 `rebuild_diff_view` 的清理循环
    /// （`rows.contains(child) || empties.contains(child)`）认不出它们 → 真实差异
    /// 行出现后，旧的「无差异」文案永远叠在上方。
    #[test]
    fn empty_state_row_carries_its_marker_and_is_not_duplicated() {
        let mut app = diff_app();
        trigger(&mut app, 3);

        let views = entities_with::<DiffViewMarker>(&mut app);
        assert_eq!(views.len(), 1, "差异页容器应恰好一个");
        let empties = entities_with::<DiffEmptyMarker>(&mut app);
        assert_eq!(
            empties.len(),
            1,
            "空态文案必须带 DiffEmptyMarker（种子节点与 show_empty 都要），\
             否则幂等守卫与清理循环双双失效"
        );
        let kids: Vec<Entity> = app
            .world()
            .get::<Children>(views[0])
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        assert_eq!(
            kids.len(),
            1,
            "连续三次触发后容器里只应剩一份空态，实际 {} 份",
            kids.len()
        );
        assert!(
            empties.iter().all(|e| kids.contains(e)),
            "空态应是容器的直接子节点"
        );
    }
}
