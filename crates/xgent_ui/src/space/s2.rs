//! S2 交互联动：Tab 遍历、搜索飞行、2D↔3D 选中同步、Esc 逐级与选中反馈。

use std::path::PathBuf;

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{AutoFocus, FocusCause, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::TextInput;
use bevy::window::PrimaryWindow;
use xgent_core::spatial::{SpatialNodeId, SpatialNodeKind};
use xgent_settings::Localizer;

use crate::editor::buffer::EditorBuffer;
use crate::editor::tabs::EditorTabs;
use crate::fonts::{ellipsis_text, ui_text, ui_text_style};
use crate::i18n::tr;
use crate::panels::{PanelRegistry, SlotLayout, ids};
use crate::space::view::{
    OrbitCamera, SpaceCamera, SpaceExtent, SpaceGraph, SpacePickState, SpaceViewRootMarker,
    SpaceViewportMarker, SpatialStar, apply_orbit, ndc_to_viewport,
};
use crate::theme::{Theme, px, radius, space, type_scale};

/// 搜索状态（快捷键/测试可直接读写）。
#[derive(Resource, Default)]
pub struct SpaceSearchState {
    /// 搜索覆盖层是否打开。
    pub open: bool,
    /// 当前查询。
    pub query: String,
    /// 过滤后的 File 节点 id（最多 20 个）。
    pub results: Vec<SpatialNodeId>,
    /// 当前结果下标。
    pub selected: usize,
    /// 搜索 Enter 关闭后，吞掉同帧的第二次 Enter。
    pub suppress_enter: bool,
    dirty: bool,
    focus: Option<Entity>,
}

/// Esc 是否被 S2（搜索/选中）消费；快捷键系统据此让位。
#[derive(Resource, Default)]
pub struct SpaceEscConsumed(pub bool);

#[derive(Resource, Default)]
pub(crate) struct SpaceTraversal {
    order: Vec<SpatialNodeId>,
    last_selected: Option<SpatialNodeId>,
    yaw: f32,
    pitch: f32,
    dist: f32,
}

#[derive(Resource, Default)]
pub(crate) struct SpaceHover {
    id: Option<SpatialNodeId>,
    since: f64,
}

#[derive(Component)]
pub struct SpaceSearchRootMarker;
#[derive(Component)]
pub struct SpaceSearchInputMarker;
#[derive(Component)]
pub struct SpaceSearchListMarker;
#[derive(Component)]
pub struct SpaceSearchEmptyMarker;
#[derive(Component, Debug, Clone, Copy)]
pub struct SpaceSearchItemMarker {
    /// 结果节点 id。
    pub id: SpatialNodeId,
}

#[derive(Component)]
pub struct SpaceHoverTooltipMarker;
#[derive(Component)]
pub struct SpaceTooltipText;
#[derive(Component)]
pub struct SpaceDetailCardMarker;
#[derive(Component)]
pub struct SpaceDetailText;

/// 注册 S2 资源（由 `SpacePlugin` 调用）。
pub fn register(app: &mut App) {
    app.init_resource::<SpaceSearchState>()
        .init_resource::<SpaceEscConsumed>()
        .init_resource::<SpaceTraversal>()
        .init_resource::<SpaceHover>();
}

/// 在 HUD 下创建悬停 tooltip 与选中详情卡（初始隐藏）。
pub fn spawn_feedback_nodes(hud: &mut ChildSpawnerCommands, theme: &Theme) {
    hud.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Px(260.0),
            padding: UiRect::all(px(space::SM)),
            flex_direction: FlexDirection::Column,
            row_gap: px(space::XS),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(radius::SMALL)),
            ..default()
        },
        BackgroundColor(theme.elevated),
        BorderColor::all(theme.border),
        SpaceHoverTooltipMarker,
        Visibility::Hidden,
    ))
    .with_children(|tip| {
        tip.spawn((
            ui_text(
                "",
                type_scale::TINY,
                400,
                theme.text,
                type_scale::line_height::UI,
            ),
            SpaceTooltipText,
        ));
    });

    hud.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(12.0),
            bottom: Val::Px(12.0),
            width: Val::Px(280.0),
            padding: UiRect::all(px(space::MD)),
            flex_direction: FlexDirection::Column,
            row_gap: px(space::XS),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(radius::SMALL)),
            ..default()
        },
        BackgroundColor(theme.surface),
        BorderColor::all(theme.border),
        SpaceDetailCardMarker,
        Visibility::Hidden,
    ))
    .with_children(|card| {
        card.spawn((
            ui_text(
                "",
                type_scale::SMALL,
                600,
                theme.text,
                type_scale::line_height::UI,
            ),
            SpaceDetailText,
        ));
    });
}

fn space_visible(registry: Option<&PanelRegistry>, layout: Option<&SlotLayout>) -> bool {
    match (registry, layout) {
        (Some(registry), Some(layout)) => layout.is_panel_visible(registry, ids::SPACE),
        _ => false,
    }
}

fn text_focused(input_focus: Option<&InputFocus>, text: &Query<(), With<EditableText>>) -> bool {
    input_focus
        .and_then(InputFocus::get)
        .is_some_and(|entity| text.get(entity).is_ok())
}

fn refill_results(search: &mut SpaceSearchState, stars: &Query<(&SpatialStar, &Visibility)>) {
    let query = search.query.to_lowercase();
    let mut items: Vec<&SpatialStar> = stars
        .iter()
        .filter(|(_, visibility)| **visibility != Visibility::Hidden)
        .map(|(star, _)| star)
        .filter(|star| star.kind == SpatialNodeKind::File)
        .filter(|star| {
            if query.is_empty() {
                return true;
            }
            star.label.to_lowercase().contains(&query)
                || star
                    .path
                    .as_deref()
                    .is_some_and(|path| path.to_lowercase().contains(&query))
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.path.cmp(&b.path)));
    search.results = items.into_iter().take(20).map(|star| star.id).collect();
    search.selected = 0;
    search.dirty = true;
}

/// `/` 打开视口内搜索（仅宇宙可见、无文本焦点时）。
pub(crate) fn handle_space_search_open(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    modal: Option<Res<crate::modal::ModalStack>>,
    input_focus: Option<Res<InputFocus>>,
    text: Query<(), With<EditableText>>,
    stars: Query<(&SpatialStar, &Visibility)>,
    mut search: ResMut<SpaceSearchState>,
) {
    if search.open {
        return;
    }
    if !space_visible(registry.as_deref(), layout.as_deref()) {
        return;
    }
    if modal.as_deref().is_some_and(|stack| !stack.is_empty()) {
        return;
    }
    if text_focused(input_focus.as_deref(), &text) {
        return;
    }
    if keys
        .as_ref()
        .is_some_and(|keys| keys.just_pressed(KeyCode::Slash))
    {
        search.open = true;
        search.query.clear();
        refill_results(&mut search, &stars);
    }
}

/// 搜索输入变化 → 重新过滤 File 节点。
pub(crate) fn sync_space_search_results(
    input: Query<&EditableText, With<SpaceSearchInputMarker>>,
    stars: Query<(&SpatialStar, &Visibility)>,
    mut search: ResMut<SpaceSearchState>,
) {
    if !search.open {
        return;
    }
    let Ok(text) = input.single() else {
        return;
    };
    let value = text.value().to_string();
    if value == search.query {
        return;
    }
    search.query = value;
    refill_results(&mut search, &stars);
}

/// 搜索结果键盘操作：↑/↓ 选择，Enter 选中并飞行（不打开编辑器）。
pub(crate) fn handle_space_search_keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    stars: Query<(&Transform, &SpatialStar, &Visibility), Without<SpaceCamera>>,
    mut cameras: Query<(&mut Transform, &mut OrbitCamera), With<SpaceCamera>>,
    extent: Option<Res<SpaceExtent>>,
    mut pick: ResMut<SpacePickState>,
    mut search: ResMut<SpaceSearchState>,
) {
    if !search.open {
        return;
    }
    let Some(keys) = keys.as_ref() else {
        return;
    };
    if keys.just_pressed(KeyCode::ArrowDown) && !search.results.is_empty() {
        search.selected = (search.selected + 1) % search.results.len();
        search.dirty = true;
    }
    if keys.just_pressed(KeyCode::ArrowUp) && !search.results.is_empty() {
        search.selected = (search.selected + search.results.len() - 1) % search.results.len();
        search.dirty = true;
    }
    if keys.just_pressed(KeyCode::Enter) {
        let Some(id) = search.results.get(search.selected).copied() else {
            return;
        };
        pick.selected = Some(id);
        fly_to_node(id, &stars, &mut cameras, extent.as_deref());
        search.open = false;
        search.suppress_enter = true;
        search.dirty = true;
    }
}

fn fly_to_node(
    id: SpatialNodeId,
    stars: &Query<(&Transform, &SpatialStar, &Visibility), Without<SpaceCamera>>,
    cameras: &mut Query<(&mut Transform, &mut OrbitCamera), With<SpaceCamera>>,
    extent: Option<&SpaceExtent>,
) {
    let Some((star_transform, _, _)) = stars
        .iter()
        .find(|(_, star, visibility)| star.id == id && **visibility != Visibility::Hidden)
    else {
        return;
    };
    let Ok((mut camera_transform, mut orbit)) = cameras.single_mut() else {
        return;
    };
    orbit.target = star_transform.translation;
    if let Some(extent) = extent
        && extent.radius > 1.0
    {
        orbit.dist = (extent.radius * 0.35).max(8.0);
    }
    apply_orbit(&mut camera_transform, &orbit);
}

/// 搜索覆盖层的创建/销毁与结果列表重建。
pub(crate) fn sync_space_search_ui(
    mut commands: Commands,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    roots: Query<Entity, With<SpaceSearchRootMarker>>,
    view_roots: Query<Entity, With<SpaceViewRootMarker>>,
    lists: Query<Entity, With<SpaceSearchListMarker>>,
    items: Query<Entity, With<SpaceSearchItemMarker>>,
    mut empty: Query<&mut Visibility, With<SpaceSearchEmptyMarker>>,
    stars: Query<(&SpatialStar, &Visibility)>,
    mut input_focus: Option<ResMut<InputFocus>>,
    mut search: ResMut<SpaceSearchState>,
) {
    if !search.open || !space_visible(registry.as_deref(), layout.as_deref()) {
        search.open = false;
        for root in &roots {
            commands.entity(root).despawn();
        }
        if let Some(focus) = input_focus.as_mut()
            && let Some(previous) = search.focus
            && focus.get() == Some(previous)
        {
            focus.clear();
        }
        search.focus = None;
        search.results.clear();
        search.query.clear();
        search.selected = 0;
        search.dirty = false;
        return;
    }

    if roots.is_empty() {
        let Ok(view_root) = view_roots.single() else {
            return;
        };
        let input_entity = spawn_search_overlay(&mut commands, view_root, &theme, &loc);
        if let Some(focus) = input_focus.as_mut() {
            focus.set(input_entity, FocusCause::Navigated);
        }
        search.focus = Some(input_entity);
        search.dirty = true;
        return;
    }

    let Ok(list) = lists.single() else {
        return;
    };
    if !search.dirty {
        return;
    }
    for item in &items {
        commands.entity(item).despawn();
    }
    for (index, id) in search.results.iter().copied().enumerate() {
        let Some(star) = stars
            .iter()
            .find(|(star, visibility)| star.id == id && **visibility != Visibility::Hidden)
            .map(|(star, _)| star)
        else {
            continue;
        };
        let label = match star.path.as_deref() {
            Some(path) => format!("{}  {path}", star.label),
            None => star.label.clone(),
        };
        let selected = index == search.selected;
        commands.entity(list).with_children(|list| {
            list.spawn((
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(28.0),
                    padding: UiRect::horizontal(px(space::SM)),
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(if selected {
                    theme.accent_glow
                } else {
                    Color::NONE
                }),
                SpaceSearchItemMarker { id },
            ))
            .with_children(|row| {
                row.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_shrink: 1.0,
                        min_width: Val::ZERO,
                        ..default()
                    },
                    ellipsis_text(
                        label,
                        type_scale::SMALL,
                        400,
                        if selected { theme.text } else { theme.text_dim },
                        type_scale::line_height::UI,
                    ),
                ));
            });
        });
    }
    if let Ok(mut visibility) = empty.single_mut() {
        *visibility = if search.results.is_empty() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    search.dirty = false;
}

fn spawn_search_overlay(
    commands: &mut Commands,
    view_root: Entity,
    theme: &Theme,
    loc: &Localizer,
) -> Entity {
    let mut input_entity = Entity::PLACEHOLDER;
    commands.entity(view_root).with_children(|view| {
        view.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                padding: UiRect::top(px(64.0)),
                ..default()
            },
            BackgroundColor(theme.overlay),
            bevy::picking::Pickable::default(),
            SpaceSearchRootMarker,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(520.0),
                        max_height: Val::Percent(60.0),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(radius::PANEL)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(theme.elevated),
                    BorderColor::all(theme.border),
                    bevy::picking::Pickable::default(),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            padding: UiRect::all(px(space::SM)),
                            border: UiRect::bottom(px(1.0)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BorderColor::all(theme.border),
                        ui_text(
                            tr(loc, "space-search-hint"),
                            type_scale::TINY,
                            400,
                            theme.text_muted,
                            type_scale::line_height::UI,
                        ),
                    ));
                    input_entity = panel
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                padding: UiRect::all(px(space::SM)),
                                min_height: Val::Px(38.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            ui_text_style(
                                type_scale::BODY,
                                400,
                                theme.text,
                                type_scale::line_height::BODY,
                            ),
                            EditableText {
                                allow_newlines: false,
                                ..default()
                            },
                            TextInput,
                            AutoFocus,
                            TabIndex(10),
                            SpaceSearchInputMarker,
                        ))
                        .id();
                    panel.spawn((xui::ScrollFrame::vertical(), SpaceSearchListMarker));
                    panel.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(96.0),
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        ui_text(
                            tr(loc, "space-search-empty"),
                            type_scale::SMALL,
                            400,
                            theme.text_muted,
                            type_scale::line_height::UI,
                        ),
                        SpaceSearchEmptyMarker,
                        Visibility::Hidden,
                    ));
                });
        });
    });
    input_entity
}

/// Tab / Shift+Tab 在当前可见节点间循环选中；相机 target 跟随，不旋转。
pub(crate) fn traverse_space_selection(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    modal: Option<Res<crate::modal::ModalStack>>,
    input_focus: Option<Res<InputFocus>>,
    text: Query<(), With<EditableText>>,
    search: Res<SpaceSearchState>,
    stars: Query<(&Transform, &SpatialStar, &Visibility), Without<SpaceCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    viewport: Query<(&ComputedNode, &UiGlobalTransform, &Hovered), With<SpaceViewportMarker>>,
    read_camera: Query<(&Camera, &GlobalTransform), With<SpaceCamera>>,
    mut cameras: Query<(&mut Transform, &mut OrbitCamera), With<SpaceCamera>>,
    mut pick: ResMut<SpacePickState>,
    mut traversal: ResMut<SpaceTraversal>,
) {
    if search.open || !space_visible(registry.as_deref(), layout.as_deref()) {
        return;
    }
    if modal.as_deref().is_some_and(|stack| !stack.is_empty()) {
        return;
    }
    if text_focused(input_focus.as_deref(), &text) {
        return;
    }
    let Some(keys) = keys.as_ref() else {
        return;
    };
    if !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    let reverse = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let Ok((_, orbit)) = cameras.single() else {
        return;
    };
    let signature = (orbit.yaw, orbit.pitch, orbit.dist);
    let external = pick.selected != traversal.last_selected;
    let camera_changed = (traversal.yaw - signature.0).abs() > 1e-4
        || (traversal.pitch - signature.1).abs() > 1e-4
        || (traversal.dist - signature.2).abs() > 1e-4;
    if traversal.order.is_empty() || external || camera_changed {
        traversal.order = project_order(&stars, &window, &viewport, &read_camera);
        traversal.last_selected = pick.selected;
        traversal.yaw = signature.0;
        traversal.pitch = signature.1;
        traversal.dist = signature.2;
    }
    if traversal.order.is_empty() {
        return;
    }
    let current = pick.selected.and_then(|id| {
        traversal
            .order
            .iter()
            .position(|candidate| *candidate == id)
    });
    let len = traversal.order.len() as isize;
    let next = match current {
        Some(index) => (index as isize + if reverse { -1 } else { 1 }).rem_euclid(len) as usize,
        None if reverse => traversal.order.len() - 1,
        None => 0,
    };
    let id = traversal.order[next];
    pick.selected = Some(id);
    traversal.last_selected = Some(id);
    if let Ok((mut camera_transform, mut orbit)) = cameras.single_mut() {
        if let Some((transform, _, _)) = stars
            .iter()
            .find(|(_, star, visibility)| star.id == id && **visibility != Visibility::Hidden)
        {
            orbit.target = transform.translation;
            apply_orbit(&mut camera_transform, &orbit);
        }
    }
}

fn project_order(
    stars: &Query<(&Transform, &SpatialStar, &Visibility), Without<SpaceCamera>>,
    window: &Query<&Window, With<PrimaryWindow>>,
    viewport: &Query<(&ComputedNode, &UiGlobalTransform, &Hovered), With<SpaceViewportMarker>>,
    camera: &Query<(&Camera, &GlobalTransform), With<SpaceCamera>>,
) -> Vec<SpatialNodeId> {
    let mut collected: Vec<(f32, SpatialNodeId)> = Vec::new();
    if let (Ok(_window), Ok((computed, _, _)), Ok((camera, camera_transform))) =
        (window.single(), viewport.single(), camera.single())
    {
        let size = computed.size();
        let center = Vec2::new(size.x * 0.5, size.y * 0.5);
        for (transform, star, visibility) in stars.iter() {
            if *visibility == Visibility::Hidden {
                continue;
            }
            let Some(ndc) = camera.world_to_ndc(camera_transform, transform.translation) else {
                continue;
            };
            let Some(screen) = ndc_to_viewport(ndc, size) else {
                continue;
            };
            let angle = (screen.y - center.y).atan2(screen.x - center.x);
            collected.push((angle, star.id));
        }
    }
    if collected.is_empty() {
        let mut ids: Vec<SpatialNodeId> = stars
            .iter()
            .filter(|(_, _, visibility)| **visibility != Visibility::Hidden)
            .map(|(_, star, _)| star.id)
            .collect();
        ids.sort_by_key(|id| id.0);
        return ids;
    }
    collected.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.0.cmp(&b.1.0)));
    collected.into_iter().map(|(_, id)| id).collect()
}

/// 编辑器活动文件变化 → 同步 3D 选中（不移动相机）。
pub(crate) fn sync_selection_from_editor(
    tabs: Option<Res<EditorTabs>>,
    buffers: Query<&EditorBuffer>,
    graph: Res<SpaceGraph>,
    stars: Query<(&SpatialStar, &Visibility)>,
    mut pick: ResMut<SpacePickState>,
    mut last_path: Local<Option<PathBuf>>,
) {
    let Some(tabs) = tabs else {
        return;
    };
    let tabs_changed = tabs.is_changed();
    let graph_changed = graph.is_changed();
    if !tabs_changed && !graph_changed {
        return;
    }
    // 图刷新不覆盖用户已有的 3D 选中；只有编辑器换文件时才重新同步。
    if graph_changed && !tabs_changed && pick.selected.is_some() {
        return;
    }
    let Some(active) = tabs
        .active
        .and_then(|index| tabs.tabs.get(index))
        .and_then(|entity| buffers.get(*entity).ok())
        .map(|buffer| buffer.path.clone())
    else {
        *last_path = None;
        return;
    };
    if last_path.as_ref() == Some(&active) && !(graph_changed && pick.selected.is_none()) {
        return;
    }
    *last_path = Some(active.clone());
    let selected = stars
        .iter()
        .find(|(star, visibility)| {
            **visibility != Visibility::Hidden
                && star.kind == SpatialNodeKind::File
                && star
                    .path
                    .as_deref()
                    .is_some_and(|path| std::path::Path::new(path) == active)
        })
        .map(|(star, _)| star.id);
    pick.selected = selected;
}

/// 悬停 tooltip + 选中详情卡。
pub(crate) fn update_space_feedback(
    time: Option<Res<Time>>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    loc: Res<Localizer>,
    search: Res<SpaceSearchState>,
    pick: Res<SpacePickState>,
    stars: Query<(&Transform, &SpatialStar)>,
    window: Query<&Window, With<PrimaryWindow>>,
    viewport: Query<(&ComputedNode, &UiGlobalTransform, &Hovered), With<SpaceViewportMarker>>,
    camera: Query<(&Camera, &GlobalTransform), With<SpaceCamera>>,
    mut hover: ResMut<SpaceHover>,
    mut roots: Query<(
        &mut Node,
        &mut Visibility,
        Has<SpaceHoverTooltipMarker>,
        Has<SpaceDetailCardMarker>,
    )>,
    mut texts: Query<(&mut Text, Has<SpaceTooltipText>, Has<SpaceDetailText>)>,
) {
    let now = time.as_ref().map_or(0.0, |time| time.elapsed_secs_f64());
    if hover.id != pick.hovered {
        hover.id = pick.hovered;
        hover.since = now;
    }
    let visible = space_visible(registry.as_deref(), layout.as_deref()) && !search.open;
    let hovered_star = pick.hovered.and_then(|id| {
        stars
            .iter()
            .find(|(_, star)| star.id == id)
            .map(|(transform, star)| (transform.translation, star))
    });
    let tooltip_ready = hovered_star.is_some() && tooltip_delay_ready(now - hover.since);

    for (mut node, mut visibility, is_tooltip, is_detail) in &mut roots {
        if is_tooltip {
            if !visible || !tooltip_ready {
                *visibility = Visibility::Hidden;
                continue;
            }
            let Some((position, star)) = hovered_star else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let Some(mut text) = texts
                .iter_mut()
                .find(|(_, tooltip, _)| *tooltip)
                .map(|(text, _, _)| text)
            else {
                continue;
            };
            update_tooltip_text(&mut text, star);
            *visibility = Visibility::Visible;
            if let (Ok(window), Ok((computed, _, _)), Ok((camera, camera_transform))) =
                (window.single(), viewport.single(), camera.single())
            {
                let size = computed.size();
                let scale = window.scale_factor().max(0.0001);
                if let Ok(screen) = camera.world_to_viewport(camera_transform, position) {
                    let width = size.x / scale;
                    let left = (screen.x / scale + 12.0).min((width - 276.0).max(0.0));
                    node.left = Val::Px(left);
                    node.top = Val::Px((screen.y / scale + 12.0).max(0.0));
                }
            }
        } else if is_detail {
            let selected_star = pick.selected.and_then(|id| {
                stars
                    .iter()
                    .find(|(_, star)| star.id == id)
                    .map(|(_, star)| star)
            });
            if !visible || selected_star.is_none() {
                *visibility = Visibility::Hidden;
                continue;
            }
            let star = selected_star.unwrap();
            if let Some(mut text) = texts
                .iter_mut()
                .find(|(_, _, detail)| *detail)
                .map(|(text, _, _)| text)
            {
                let kind = match star.kind {
                    SpatialNodeKind::File => "space-kind-file",
                    SpatialNodeKind::Module => "space-kind-module",
                    SpatialNodeKind::Crate => "space-kind-crate",
                    SpatialNodeKind::Symbol => "space-kind-symbol",
                    SpatialNodeKind::External => "space-kind-external",
                };
                let kind_label = tr(&loc, kind);
                let hint = if star.kind == SpatialNodeKind::File {
                    format!(" · {}", tr(&loc, "space-detail-hint"))
                } else {
                    String::new()
                };
                let path = star.path.as_deref().unwrap_or("");
                let value = format!("{}\n{}\n{}{hint}", star.label, path, kind_label);
                if text.0 != value {
                    text.0 = value;
                }
            }
            *visibility = Visibility::Visible;
        }
    }
}

fn tooltip_delay_ready(elapsed: f64) -> bool {
    elapsed >= 0.12
}

fn update_tooltip_text(text: &mut Text, star: &SpatialStar) {
    let path = star.path.as_deref().unwrap_or("");
    let value = if path.is_empty() {
        star.label.clone()
    } else {
        format!("{}\n{path}", star.label)
    };
    if text.0 != value {
        text.0 = value;
    }
}

/// Esc 的 S2 优先级：搜索 → 3D 选中；编辑器/终端由快捷键系统处理。
pub(crate) fn consume_space_escape(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    modal: Option<Res<crate::modal::ModalStack>>,
    modal_esc: Option<Res<crate::modal::ModalEscConsumed>>,
    completion_esc: Option<Res<crate::completion_ui::CompletionConsumed>>,
    mut input_focus: Option<ResMut<InputFocus>>,
    mut search: ResMut<SpaceSearchState>,
    mut timeline: Option<ResMut<crate::space::s3::TimelineState>>,
    mut pick: ResMut<SpacePickState>,
    mut consumed: ResMut<SpaceEscConsumed>,
) {
    consumed.0 = false;
    let Some(keys) = keys.as_ref() else {
        return;
    };
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let blocked = modal.as_deref().is_some_and(|stack| !stack.is_empty())
        || modal_esc.as_deref().is_some_and(|consumed| consumed.0)
        || completion_esc
            .as_deref()
            .is_some_and(|consumed| consumed.esc);
    if blocked {
        return;
    }
    if search.open {
        search.open = false;
        if let Some(focus) = input_focus.as_mut()
            && let Some(previous) = search.focus
            && focus.get() == Some(previous)
        {
            focus.clear();
        }
        search.focus = None;
        consumed.0 = true;
        return;
    }
    // 历史回放优先于清除选中：Esc 先把视图退回“现在”。
    if space_visible(registry.as_deref(), layout.as_deref())
        && let Some(timeline) = timeline.as_mut()
        && timeline.position.is_some()
    {
        timeline.return_now(&mut pick);
        consumed.0 = true;
        return;
    }
    if space_visible(registry.as_deref(), layout.as_deref()) && pick.selected.is_some() {
        pick.selected = None;
        consumed.0 = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_waits_120ms() {
        assert!(!tooltip_delay_ready(0.119));
        assert!(tooltip_delay_ready(0.12));
        assert!(tooltip_delay_ready(0.5));
    }
}
