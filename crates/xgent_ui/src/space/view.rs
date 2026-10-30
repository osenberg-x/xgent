//! 3D 空间视口：离屏纹理 + 实例化星体/边 + 投影 HUD（S1 首期）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::camera::RenderTarget;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui_widgets::Button;
use bevy::window::PrimaryWindow;
use xgent_core::spatial::{SpatialGraph, SpatialNode, SpatialNodeId, SpatialNodeKind};
use xgent_settings::Localizer;
use xgent_spatial::{LayoutConfig, layout};
use xui::Ellipsis;

use crate::editor::tabs::OpenFileRequest;
use crate::file_panel::ProjectRoot;
use crate::fonts::{ellipsis_text, ui_text};
use crate::i18n::tr;
use crate::layout::CenterHostMarker;
use crate::panels::{
    PanelDescriptor, PanelRegister, PanelRegistry, PanelVisibility, Slot, SlotLayout, ids,
};
use crate::space::scan::ScanJob;
use crate::theme::Theme;

const HUD_LABELS: usize = 24;
const HUD_LABEL_W: f32 = 160.0;
const MAX_EDGES: usize = 6000;

/// 当前要显示的空间图；设置/替换后由 `sync_space_graph` 重建实例。
#[derive(Resource, Default)]
pub struct SpaceGraph(pub SpatialGraph);

/// 后台扫描状态。
#[derive(Resource, Default)]
pub struct SpaceLoadState {
    job: Option<ScanJob>,
    loaded: bool,
}

/// 星体与边共享的 mesh/material。
#[derive(Resource)]
pub struct SpaceMaterials {
    /// 星体 mesh。
    pub star_mesh: Handle<Mesh>,
    /// 星体材质（主题 accent）。
    pub star: Handle<StandardMaterial>,
    /// 悬停材质。
    pub hover: Handle<StandardMaterial>,
    /// 选中材质。
    pub selected: Handle<StandardMaterial>,
    /// 边 mesh（细长立方体）。
    pub edge_mesh: Handle<Mesh>,
    /// 边材质（主题 border）。
    pub edge: Handle<StandardMaterial>,
}

/// 离屏纹理与当前尺寸。
#[derive(Resource, Debug, Clone)]
pub struct SpaceTarget {
    /// 目标纹理句柄。
    pub image: Handle<Image>,
    /// 当前纹理尺寸（物理像素）。
    pub size: UVec2,
}

/// UI 里显示离屏纹理的节点。
#[derive(Component)]
pub struct SpaceViewportMarker;

/// 渲染到离屏纹理的 3D 相机。
#[derive(Component)]
pub struct SpaceCamera;

/// 轨道相机状态。
#[derive(Component, Debug, Clone, Copy)]
pub struct OrbitCamera {
    /// 注视点。
    pub target: Vec3,
    /// 水平角。
    pub yaw: f32,
    /// 俯仰角。
    pub pitch: f32,
    /// 距离。
    pub dist: f32,
}

/// 一个星体实例。
#[derive(Component, Debug, Clone)]
pub struct SpatialStar {
    /// 空间节点 id。
    pub id: SpatialNodeId,
    /// 展示名。
    pub label: String,
    /// 文件/目录路径。
    pub path: Option<String>,
    /// 未选中/悬停时的缩放。
    pub base_scale: f32,
}

/// 一条边实例。
#[derive(Component)]
pub struct SpaceEdge;

/// HUD 标签池中的一项。
#[derive(Component, Debug, Clone, Copy)]
pub struct SpaceHudLabel {
    /// 池槽位（0..HUD_LABELS）。
    pub slot: usize,
}

/// HUD 覆盖层根。
#[derive(Component)]
pub struct SpaceHudMarker;

/// 拾取/选择状态。
#[derive(Resource, Default)]
pub struct SpacePickState {
    /// 当前悬停节点。
    pub hovered: Option<SpatialNodeId>,
    /// 当前选中节点。
    pub selected: Option<SpatialNodeId>,
    last_click: Option<(SpatialNodeId, f64)>,
}

/// 运行期渲染档位。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewQuality {
    /// 完整：2k 节点 + 边 + HUD。
    #[default]
    Full,
    /// 降级：800 节点、关闭边、HUD 保留。
    Reduced,
    /// 静态：冻结最后一帧，不再渲染 3D。
    Static,
}

impl ViewQuality {
    fn node_limit(self) -> usize {
        match self {
            Self::Full => 2000,
            Self::Reduced | Self::Static => 800,
        }
    }
}

/// HUD 右上角的档位角标。
#[derive(Component)]
pub struct SpaceQualityBadge;

/// 3D 空间视图插件。
pub struct SpacePlugin;

impl Plugin for SpacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpaceGraph>()
            .init_resource::<SpaceLoadState>()
            .init_resource::<SpacePickState>()
            .init_resource::<ViewQuality>()
            .add_systems(PostStartup, setup_space)
            .add_systems(
                Update,
                (
                    load_space_graph,
                    sync_space_graph,
                    orbit_space_camera,
                    pick_space_stars,
                    sync_space_theme,
                    sync_space_quality_badge,
                    sync_space_camera_active,
                    update_space_hud,
                    resize_space_target,
                )
                    .chain(),
            );
    }
}

fn setup_space(
    mut commands: Commands,
    q_main: Query<Entity, With<CenterHostMarker>>,
    theme: Res<Theme>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut register: MessageWriter<PanelRegister>,
) {
    let Ok(center) = q_main.single() else {
        return;
    };

    let image = Image::new_target_texture(
        1280,
        800,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    let image_handle = images.add(image);

    let star_mesh = meshes.add(Sphere::new(1.0).mesh().uv(8, 6));
    let star = materials.add(StandardMaterial {
        base_color: theme.accent,
        unlit: true,
        ..default()
    });
    let hover = materials.add(StandardMaterial {
        base_color: theme.accent_interactive,
        unlit: true,
        ..default()
    });
    let selected = materials.add(StandardMaterial {
        base_color: theme.star,
        unlit: true,
        ..default()
    });
    let edge_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let edge = materials.add(StandardMaterial {
        base_color: theme.border_hover,
        unlit: true,
        ..default()
    });
    commands.insert_resource(SpaceMaterials {
        star_mesh,
        star,
        hover,
        selected,
        edge_mesh,
        edge,
    });

    let view = commands
        .spawn((
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                ..xui::region_fill()
            },
            PanelVisibility::of_panel(ids::SPACE, Slot::Center),
        ))
        .with_children(|parent| {
            parent.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                ImageNode {
                    image: image_handle.clone(),
                    ..default()
                },
                SpaceViewportMarker,
                Button,
                Hovered::default(),
            ));
            parent
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    SpaceHudMarker,
                ))
                .with_children(|hud| {
                    hud.spawn(ui_text("", 11.0, 500, theme.text_muted, 1.4))
                        .insert((
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(8.0),
                                top: Val::Px(8.0),
                                ..default()
                            },
                            SpaceQualityBadge,
                        ));
                    for slot in 0..HUD_LABELS {
                        hud.spawn(ellipsis_text("", 11.0, 400, theme.text_muted, 1.4))
                            .insert((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(0.0),
                                    top: Val::Px(0.0),
                                    width: Val::Px(HUD_LABEL_W),
                                    ..default()
                                },
                                SpaceHudLabel { slot },
                                Visibility::Hidden,
                            ));
                    }
                });
        })
        .id();
    commands.entity(center).add_child(view);

    register.write(PanelRegister {
        desc: PanelDescriptor::new(ids::SPACE, Slot::Center, "panel-space", "panel-right", 10),
        view: Some(view),
    });

    let mut camera_transform = Transform::from_xyz(0.0, 0.0, 24.0);
    let orbit = OrbitCamera {
        target: Vec3::ZERO,
        yaw: 0.6,
        pitch: 0.35,
        dist: 30.0,
    };
    apply_orbit(&mut camera_transform, &orbit);
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: Color::BLACK.into(),
            ..default()
        },
        RenderTarget::Image(image_handle.clone().into()),
        camera_transform,
        orbit,
        SpaceCamera,
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(8.0, 12.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.insert_resource(SpaceTarget {
        image: image_handle,
        size: UVec2::new(1280, 800),
    });
}

fn load_space_graph(
    mut state: ResMut<SpaceLoadState>,
    root: Option<Res<ProjectRoot>>,
    mut graph: ResMut<SpaceGraph>,
) {
    if state.loaded {
        return;
    }
    if state.job.is_none() {
        let Some(root) = root else {
            return;
        };
        state.job = Some(ScanJob::spawn(
            root.path.clone(),
            xgent_spatial::ScanOptions {
                l1: true,
                node_limit: 2000,
                ignore: true,
            },
        ));
    }
    let Some(job) = state.job.as_mut() else {
        return;
    };
    match job.poll() {
        Some(Ok(scanned)) => {
            tracing::info!(
                nodes = scanned.nodes.len(),
                edges = scanned.edges.len(),
                omitted = scanned.omitted_nodes,
                generation = scanned.generation,
                "空间图加载完成"
            );
            graph.0 = scanned;
            state.loaded = true;
            state.job = None;
        }
        Some(Err(error)) => {
            tracing::warn!(%error, "空间图扫描失败");
            state.loaded = true;
            state.job = None;
        }
        None => {}
    }
}

fn sync_space_graph(
    mut commands: Commands,
    graph: Res<SpaceGraph>,
    quality: Res<ViewQuality>,
    materials: Option<Res<SpaceMaterials>>,
    stars: Query<Entity, With<SpatialStar>>,
    edges: Query<Entity, With<SpaceEdge>>,
    mut cameras: Query<(&mut Transform, &mut OrbitCamera), With<SpaceCamera>>,
    mut framed: Local<bool>,
) {
    if !graph.is_changed() && !quality.is_changed() {
        return;
    }
    if *quality == ViewQuality::Static {
        return;
    }
    let Some(materials) = materials else {
        return;
    };

    for entity in &stars {
        commands.entity(entity).despawn();
    }
    for entity in &edges {
        commands.entity(entity).despawn();
    }
    if graph.0.nodes.is_empty() {
        return;
    }

    let mut data = graph.0.clone();
    if !data.clusters.is_empty() && data.nodes.iter().any(|node| node.anchor.is_none()) {
        layout(&mut data, &LayoutConfig::default());
    }

    let limit = quality.node_limit();
    let mut render_nodes: Vec<&SpatialNode> = data
        .nodes
        .iter()
        .filter(|node| node.anchor.is_some())
        .collect();
    if render_nodes.len() > limit {
        render_nodes.sort_by(|a, b| {
            render_kind_rank(a.kind)
                .cmp(&render_kind_rank(b.kind))
                .then_with(|| b.weight.total_cmp(&a.weight))
                .then_with(|| a.path.cmp(&b.path))
        });
        render_nodes.truncate(limit);
    }

    let positions: BTreeMap<u32, Vec3> = render_nodes
        .iter()
        .filter_map(|node| {
            node.anchor
                .map(|anchor| (node.id.0, Vec3::from_array(anchor)))
        })
        .collect();

    let Some((center, radius)) = graph_bounds(&positions) else {
        return;
    };

    // 首帧取景：按整张图的包围球定相机距离与注视点。布局簇半径默认 420，
    // 而相机初始距离只有 30——不取景的话真实仓库会是一片黑。
    // 只做一次：之后的数据刷新/重扫不移动用户已经调整好的相机。
    if !*framed && let Ok((mut transform, mut orbit)) = cameras.single_mut() {
        orbit.target = center;
        orbit.dist = (radius * 2.8).max(24.0);
        apply_orbit(&mut transform, &orbit);
        *framed = true;
    }

    let star_base = (radius * 0.012).max(0.05);
    let edge_thickness = (radius * 0.0015).max(0.01);

    for node in &render_nodes {
        let Some(position) = positions.get(&node.id.0) else {
            continue;
        };
        let scale = (star_base * (0.8 + 0.25 * (node.weight.max(0.0) + 1.0).ln()))
            .clamp(star_base * 0.6, star_base * 2.5);
        commands.spawn((
            Mesh3d(materials.star_mesh.clone()),
            MeshMaterial3d(materials.star.clone()),
            Transform::from_translation(*position).with_scale(Vec3::splat(scale)),
            SpatialStar {
                id: node.id,
                label: node.label.clone(),
                path: node.path.as_ref().map(|path| path.display().to_string()),
                base_scale: scale,
            },
        ));
    }

    let mut edge_count = 0;
    for edge in &data.edges {
        if edge_count >= MAX_EDGES {
            break;
        }
        let (Some(from), Some(to)) = (positions.get(&edge.from.0), positions.get(&edge.to.0))
        else {
            continue;
        };
        let delta = *to - *from;
        let length = delta.length();
        if length < 0.001 {
            continue;
        }
        let midpoint = (*from + *to) * 0.5;
        let rotation = Quat::from_rotation_arc(Vec3::Y, delta.normalize());
        commands.spawn((
            Mesh3d(materials.edge_mesh.clone()),
            MeshMaterial3d(materials.edge.clone()),
            Transform::from_translation(midpoint)
                .with_rotation(rotation)
                .with_scale(Vec3::new(edge_thickness, length, edge_thickness)),
            SpaceEdge,
        ));
        edge_count += 1;
    }
}

/// 节点位置的包围球（中心 + 半径）；空集合返回 `None`。
fn graph_bounds(positions: &BTreeMap<u32, Vec3>) -> Option<(Vec3, f32)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for position in positions.values() {
        min = min.min(*position);
        max = max.max(*position);
    }
    if !min.is_finite() || !max.is_finite() {
        return None;
    }
    let center = (min + max) * 0.5;
    let radius = (max - min).length() * 0.5;
    Some((center, radius))
}

fn orbit_space_camera(
    time: Option<Res<Time>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    viewport: Query<&Hovered, With<SpaceViewportMarker>>,
    mut cameras: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    let visible = match (registry.as_ref(), layout.as_ref()) {
        (Some(registry), Some(layout)) => layout.is_panel_visible(registry, ids::SPACE),
        _ => false,
    };
    if !visible {
        return;
    }
    let Ok((mut transform, mut orbit)) = cameras.single_mut() else {
        return;
    };
    let hovered = viewport
        .single()
        .map(|hovered| hovered.get())
        .unwrap_or(false);

    if hovered {
        if let (Some(mouse), Some(motion)) = (mouse.as_ref(), motion.as_ref())
            && mouse.pressed(MouseButton::Left)
        {
            orbit.yaw -= motion.delta.x * 0.006;
            orbit.pitch = (orbit.pitch + motion.delta.y * 0.006).clamp(-1.2, 1.2);
        }
        if let Some(scroll) = scroll.as_ref()
            && scroll.delta.y != 0.0
        {
            orbit.dist = (orbit.dist * 0.9_f32.powf(scroll.delta.y)).clamp(6.0, 120.0);
        }
    }

    let speed = time.as_ref().map_or(0.0, |time| 1.2 * time.delta_secs());
    if let Some(keys) = keys.as_ref() {
        if keys.pressed(KeyCode::ArrowLeft) {
            orbit.yaw -= speed;
        }
        if keys.pressed(KeyCode::ArrowRight) {
            orbit.yaw += speed;
        }
        if keys.pressed(KeyCode::ArrowUp) {
            orbit.pitch = (orbit.pitch + speed).min(1.2);
        }
        if keys.pressed(KeyCode::ArrowDown) {
            orbit.pitch = (orbit.pitch - speed).max(-1.2);
        }
        if keys.pressed(KeyCode::Equal) {
            orbit.dist = (orbit.dist * 0.98).max(6.0);
        }
        if keys.pressed(KeyCode::Minus) {
            orbit.dist = (orbit.dist * 1.02).min(120.0);
        }
    }

    apply_orbit(&mut transform, &orbit);
}

fn apply_orbit(transform: &mut Transform, orbit: &OrbitCamera) {
    let (sy, cy) = orbit.yaw.sin_cos();
    let (sp, cp) = orbit.pitch.sin_cos();
    transform.translation =
        orbit.target + Vec3::new(orbit.dist * cp * sy, orbit.dist * sp, orbit.dist * cp * cy);
    transform.look_at(orbit.target, Vec3::Y);
}

fn pick_space_stars(
    mut state: ResMut<SpacePickState>,
    materials: Option<Res<SpaceMaterials>>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<SpaceCamera>>,
    viewport: Query<(&ComputedNode, &UiGlobalTransform, &Hovered), With<SpaceViewportMarker>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    time: Option<Res<Time>>,
    mut stars: Query<(
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Transform,
        &SpatialStar,
    )>,
    mut open: MessageWriter<OpenFileRequest>,
) {
    let visible = match (registry.as_ref(), layout.as_ref()) {
        (Some(registry), Some(layout)) => layout.is_panel_visible(registry, ids::SPACE),
        _ => false,
    };
    if !visible {
        state.hovered = None;
        return;
    }

    let nodes: Vec<(SpatialNodeId, Vec3, Option<PathBuf>)> = stars
        .iter()
        .map(|(_, transform, star)| {
            (
                star.id,
                transform.translation,
                star.path.as_ref().map(PathBuf::from),
            )
        })
        .collect();

    let hovered = if let (
        Ok(window),
        Ok((computed, viewport_transform, hovered)),
        Ok((camera, camera_transform)),
    ) = (window.single(), viewport.single(), camera.single())
    {
        if hovered.get() {
            window.cursor_position().and_then(|cursor| {
                let scale = window.scale_factor().max(0.0001);
                let size = computed.size();
                let center = viewport_transform.affine().translation;
                let top_left = center - size * 0.5;
                let local = cursor * scale - top_left;
                if local.x < 0.0 || local.y < 0.0 || local.x > size.x || local.y > size.y {
                    return None;
                }
                let candidates: Vec<(SpatialNodeId, Vec2)> = nodes
                    .iter()
                    .filter_map(|(id, position, _)| {
                        let ndc = camera.world_to_ndc(camera_transform, *position)?;
                        ndc_to_viewport(ndc, size).map(|screen| (*id, screen))
                    })
                    .collect();
                nearest_hit(local, &candidates, 14.0 * scale)
            })
        } else {
            None
        }
    } else {
        None
    };
    state.hovered = hovered;

    if let (Some(mouse), Some(time)) = (mouse.as_ref(), time.as_ref())
        && mouse.just_pressed(MouseButton::Left)
        && let Some(id) = hovered
    {
        let now = time.elapsed_secs_f64();
        let double_click = state
            .last_click
            .is_some_and(|(last, at)| last == id && now - at < 0.35);
        state.selected = Some(id);
        if double_click {
            state.last_click = None;
            open_selected(&state, &nodes, &mut open);
        } else {
            state.last_click = Some((id, now));
        }
    }

    if keys
        .as_ref()
        .is_some_and(|keys| keys.just_pressed(KeyCode::Enter))
    {
        open_selected(&state, &nodes, &mut open);
    }

    if let Some(selected) = state.selected
        && !nodes.iter().any(|(id, _, _)| *id == selected)
    {
        state.selected = None;
    }

    if let Some(materials) = materials.as_ref() {
        let selected = state.selected;
        let hovered = state.hovered;
        for (mut material, mut transform, star) in &mut stars {
            let target = if Some(star.id) == selected {
                materials.selected.clone()
            } else if Some(star.id) == hovered {
                materials.hover.clone()
            } else {
                materials.star.clone()
            };
            if material.0 != target {
                material.0 = target;
            }
            let factor = if Some(star.id) == selected {
                1.18
            } else if Some(star.id) == hovered {
                1.28
            } else {
                1.0
            };
            let scale = star.base_scale * factor;
            if (transform.scale.x - scale).abs() > 1e-4 {
                transform.scale = Vec3::splat(scale);
            }
        }
    }
}

fn open_selected(
    state: &SpacePickState,
    nodes: &[(SpatialNodeId, Vec3, Option<PathBuf>)],
    open: &mut MessageWriter<OpenFileRequest>,
) {
    let Some(selected) = state.selected else {
        return;
    };
    if let Some((_, _, Some(path))) = nodes.iter().find(|(id, _, _)| *id == selected) {
        open.write(OpenFileRequest {
            path: path.clone(),
            line: None,
        });
    }
}

/// NDC → 视口物理坐标；视锥外的点返回 `None`。
pub fn ndc_to_viewport(ndc: Vec3, size: Vec2) -> Option<Vec2> {
    if !(0.0..=1.0).contains(&ndc.z) {
        return None;
    }
    Some(Vec2::new(
        (ndc.x + 1.0) * 0.5 * size.x,
        (1.0 - ndc.y) * 0.5 * size.y,
    ))
}

/// 在候选点中选距离指针最近且在半径内的一个。
pub fn nearest_hit(
    pointer: Vec2,
    candidates: &[(SpatialNodeId, Vec2)],
    radius: f32,
) -> Option<SpatialNodeId> {
    let mut best = None;
    let mut best_distance = radius;
    for (id, screen) in candidates {
        let distance = pointer.distance(*screen);
        if distance <= best_distance {
            best = Some(*id);
            best_distance = distance;
        }
    }
    best
}

fn sync_space_theme(
    theme: Res<Theme>,
    materials: Option<Res<SpaceMaterials>>,
    mut assets: ResMut<Assets<StandardMaterial>>,
    mut labels: Query<&mut TextColor, With<SpaceHudLabel>>,
) {
    if !theme.is_changed() {
        return;
    }
    let Some(materials) = materials else {
        return;
    };
    if let Some(mut material) = assets.get_mut(&materials.star) {
        material.base_color = theme.accent;
    }
    if let Some(mut material) = assets.get_mut(&materials.hover) {
        material.base_color = theme.accent_interactive;
    }
    if let Some(mut material) = assets.get_mut(&materials.selected) {
        material.base_color = theme.star;
    }
    if let Some(mut material) = assets.get_mut(&materials.edge) {
        material.base_color = theme.border_hover;
    }
    for mut color in &mut labels {
        color.0 = theme.text_muted;
    }
}

fn sync_space_quality_badge(
    quality: Res<ViewQuality>,
    loc: Res<Localizer>,
    mut badge: Query<&mut Text, With<SpaceQualityBadge>>,
) {
    if !quality.is_changed() && !loc.is_changed() {
        return;
    }
    let key = match *quality {
        ViewQuality::Full => "space-quality-full",
        ViewQuality::Reduced => "space-quality-reduced",
        ViewQuality::Static => "space-quality-static",
    };
    let label = tr(&loc, key);
    for mut text in &mut badge {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}

fn sync_space_camera_active(
    quality: Res<ViewQuality>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<SpaceCamera>>,
) {
    let visible = match (registry.as_ref(), layout.as_ref()) {
        (Some(registry), Some(layout)) => layout.is_panel_visible(registry, ids::SPACE),
        _ => false,
    };
    let focused = window.single().map_or(true, |window| window.focused);
    let active = *quality != ViewQuality::Static && visible && focused;
    for mut camera in &mut cameras {
        if camera.is_active != active {
            camera.is_active = active;
        }
    }
}

fn update_space_hud(
    theme: Res<Theme>,
    camera: Query<(&Camera, &GlobalTransform), With<SpaceCamera>>,
    stars: Query<(&Transform, &SpatialStar)>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut labels: Query<(
        &SpaceHudLabel,
        &mut Node,
        &mut Text,
        &mut Ellipsis,
        &mut TextColor,
        &mut Visibility,
    )>,
) {
    let mut items: Vec<(SpatialNodeId, Vec3, String)> = stars
        .iter()
        .map(|(transform, star)| (star.id, transform.translation, star.label.clone()))
        .collect();
    items.sort_by_key(|(id, _, _)| *id);
    items.truncate(HUD_LABELS);

    let Ok((camera, camera_transform)) = camera.single() else {
        for (_, _, _, _, _, mut visibility) in &mut labels {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let Ok(window) = window.single() else {
        for (_, _, _, _, _, mut visibility) in &mut labels {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let scale = window.scale_factor();

    for (label, mut node, mut text, mut ellipsis, mut color, mut visibility) in &mut labels {
        let Some((_, position, name)) = items.get(label.slot) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let Ok(screen) = camera.world_to_viewport(camera_transform, *position) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Visible;
        node.left = Val::Px(screen.x / scale);
        node.top = Val::Px(screen.y / scale);
        if text.0.as_str() != name.as_str() {
            text.0 = name.clone();
            ellipsis.source = name.clone();
        }
        color.0 = theme.text_muted;
    }
}

fn resize_space_target(
    mut images: ResMut<Assets<Image>>,
    target: Option<ResMut<SpaceTarget>>,
    mut viewport: Query<(&ComputedNode, &mut ImageNode), With<SpaceViewportMarker>>,
    mut cameras: Query<&mut RenderTarget, With<SpaceCamera>>,
) {
    let Some(mut target) = target else {
        return;
    };
    let Ok((computed, mut image_node)) = viewport.single_mut() else {
        return;
    };
    let logical = computed.size();
    if logical.x < 1.0 || logical.y < 1.0 {
        return;
    }
    let size = target_size(logical);
    if size == target.size {
        return;
    }

    let image = Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    let handle = images.add(image);
    image_node.image = handle.clone();
    if let Ok(mut camera_target) = cameras.single_mut() {
        *camera_target = RenderTarget::Image(handle.clone().into());
    }
    target.image = handle;
    target.size = size;
}

fn render_kind_rank(kind: SpatialNodeKind) -> u8 {
    match kind {
        SpatialNodeKind::Crate => 0,
        SpatialNodeKind::Module => 1,
        SpatialNodeKind::File => 2,
        SpatialNodeKind::Symbol => 3,
        SpatialNodeKind::External => 4,
    }
}

/// 视口物理尺寸（至少 1×1）。
pub fn target_size(size: Vec2) -> UVec2 {
    UVec2::new(
        size.x.max(1.0).round().max(1.0) as u32,
        size.y.max(1.0).round().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_size_rounds_and_clamps() {
        assert_eq!(target_size(Vec2::ZERO), UVec2::ONE);
        assert_eq!(target_size(Vec2::new(1280.4, 800.6)), UVec2::new(1280, 801));
        assert_eq!(target_size(Vec2::new(0.2, 10.0)), UVec2::new(1, 10));
    }

    #[test]
    fn orbit_transform_is_stable_and_responds_to_yaw() {
        let orbit = OrbitCamera {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            dist: 10.0,
        };
        let mut transform = Transform::default();
        apply_orbit(&mut transform, &orbit);
        let first = transform.translation;
        apply_orbit(&mut transform, &orbit);
        assert_eq!(transform.translation, first, "无输入时相机不应漂移");

        let turned = OrbitCamera {
            yaw: std::f32::consts::FRAC_PI_2,
            ..orbit
        };
        apply_orbit(&mut transform, &turned);
        assert!((transform.translation.x - 10.0).abs() < 1e-4);
        assert!(transform.translation.z.abs() < 1e-4);
    }

    #[test]
    fn graph_bounds_covers_all_positions() {
        assert_eq!(graph_bounds(&BTreeMap::new()), None, "空图没有包围球");
        let mut positions = BTreeMap::new();
        positions.insert(0, Vec3::ZERO);
        positions.insert(1, Vec3::new(2.0, 0.0, 0.0));
        let (center, radius) = graph_bounds(&positions).expect("应有包围球");
        assert_eq!(center, Vec3::new(1.0, 0.0, 0.0));
        assert!((radius - 1.0).abs() < 1e-5);
    }

    #[test]
    fn ndc_projection_and_nearest_hit() {
        let size = Vec2::new(100.0, 50.0);
        assert_eq!(
            ndc_to_viewport(Vec3::new(0.0, 0.0, 0.5), size),
            Some(Vec2::new(50.0, 25.0))
        );
        assert_eq!(
            ndc_to_viewport(Vec3::new(1.0, 1.0, 1.5), size),
            None,
            "视锥外的点不应命中"
        );

        let a = SpatialNodeId(1);
        let b = SpatialNodeId(2);
        let candidates = [(a, Vec2::new(12.0, 10.0)), (b, Vec2::new(30.0, 30.0))];
        assert_eq!(
            nearest_hit(Vec2::new(10.0, 10.0), &candidates, 5.0),
            Some(a)
        );
        assert_eq!(nearest_hit(Vec2::new(60.0, 10.0), &candidates, 5.0), None);
    }
}
