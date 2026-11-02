//! 3D 空间视口：离屏纹理 + 实例化星体/边 + 投影 HUD（S1 首期）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::camera::RenderTarget;
use bevy::input::mouse::{
    AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton, MouseScrollUnit,
};
use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui_widgets::Button;
use bevy::window::{Monitor, PrimaryMonitor, PrimaryWindow, WindowOccluded};
use xgent_core::spatial::{SpatialGraph, SpatialNode, SpatialNodeId, SpatialNodeKind};
use xgent_settings::{GlobalConfigRes, Localizer};
use xgent_spatial::{LayoutConfig, layout};
use xui::Ellipsis;

use crate::editor::conflict::FileChangedEvent;
use crate::editor::tabs::OpenFileRequest;
use crate::file_panel::ProjectRoot;
use crate::fonts::{ellipsis_text, ui_text};
use crate::i18n::{tr, tr_with};
use crate::kit::ToastMessage;
use crate::layout::CenterHostMarker;
use crate::layout_mode::LayoutMode;
use crate::panels::{
    PanelDescriptor, PanelRegister, PanelRegistry, PanelVisibility, Slot, SlotLayout, ids,
};
use crate::settings_panel::Save3DHomeMessage;
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
    root: Option<PathBuf>,
}

/// 文件变更后的重扫节奏（防抖，避免每次保存都全量重扫）。
#[derive(Resource, Default)]
struct SpaceRefresh {
    dirty: bool,
    last_event: f64,
}

/// 当前图的包围球半径：相机距离夹取与星体尺寸都按它缩放。
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct SpaceExtent {
    /// 包围球半径（世界单位）。
    pub radius: f32,
}

/// 运行期帧率观测（G7 退化梯的自动触发依据）。
#[derive(Resource, Default)]
struct SpacePerf {
    /// 预热结束时刻（秒）；`None` = 尚未开始观测。
    warmup_until: Option<f64>,
    /// 连续超预算帧数。
    over_frames: u32,
}

/// 窗口是否被完全遮挡：被遮挡时 DWM 会限帧，不能据此降级。
#[derive(Resource, Default)]
struct SpaceOccluded(bool);

/// 连续超预算帧数达到该值时降一档（约 3 秒 @60fps）。
const DEGRADE_AFTER_FRAMES: u32 = 180;
/// 帧间隔超过该值视为“掉出 60fps”（22ms ≈ 45fps）。
const FRAME_BUDGET_MS: f64 = 22.0;
/// 预热时长（秒）：启动扫描/布局/首帧编译不计入。
const DEGRADE_WARMUP_SECS: f64 = 5.0;

/// 退化梯的下一步动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PerfAction {
    /// 不动。
    None,
    /// 降到 800 档（关边、降节点上限）。
    Reduce,
    /// 停用 3D 首页，回退 2D。
    Fallback2d,
}

/// 纯决策：当前档位 + 连续超预算帧数 → 下一步。
fn degrade_after(quality: ViewQuality, over_frames: u32) -> PerfAction {
    if over_frames < DEGRADE_AFTER_FRAMES {
        return PerfAction::None;
    }
    match quality {
        ViewQuality::Full => PerfAction::Reduce,
        ViewQuality::Reduced => PerfAction::Fallback2d,
        // Static 是手动/调试档，不再自动推进。
        ViewQuality::Static => PerfAction::None,
    }
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
    /// 节点类型（只有 `File` 可打开）。
    pub kind: SpatialNodeKind,
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
            .init_resource::<SpaceRefresh>()
            .init_resource::<SpaceExtent>()
            .init_resource::<SpacePerf>()
            .init_resource::<SpaceOccluded>()
            .init_resource::<SpacePickState>()
            .init_resource::<ViewQuality>()
            .add_message::<FileChangedEvent>()
            .add_message::<WindowOccluded>()
            .add_message::<ToastMessage>()
            .add_message::<Save3DHomeMessage>()
            .add_systems(PostStartup, setup_space)
            .add_systems(
                Update,
                (
                    mark_space_dirty,
                    track_space_occlusion,
                    load_space_graph,
                    reset_space_quality_on_mode,
                    sync_space_graph,
                    orbit_space_camera,
                    pick_space_stars,
                    sync_space_theme,
                    sync_space_quality_badge,
                    sync_space_camera_active,
                    watch_space_performance,
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
        tracing::warn!("缺少 CenterHostMarker，3D 空间视图未装配");
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

/// 文件变更 → 标记空间图待刷新（防抖在 `load_space_graph` 里做）。
fn mark_space_dirty(
    mut reader: MessageReader<FileChangedEvent>,
    root: Option<Res<ProjectRoot>>,
    time: Res<Time>,
    mut refresh: ResMut<SpaceRefresh>,
) {
    let root = root.as_deref().map(|root| root.path.as_path());
    for event in reader.read() {
        if is_refresh_ignored(root, &event.path) {
            continue;
        }
        refresh.dirty = true;
        refresh.last_event = time.elapsed_secs_f64();
    }
}

/// 变更路径是否落在空间图忽略目录下。
///
/// 只按**项目根内的目录分量**判断：项目本身放在 `.worktrees/`、`node_modules/`
/// 之类目录下时不能把整个项目当成噪声；文件名恰好叫 `target` 也不忽略。
fn is_refresh_ignored(root: Option<&Path>, path: &Path) -> bool {
    let Some(relative) = root.and_then(|root| path.strip_prefix(root).ok()) else {
        return false;
    };
    relative.ancestors().skip(1).any(|dir| {
        dir.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| xgent_spatial::workspace::SKIP_DIRS.contains(&name))
        })
    })
}

/// 跟踪窗口遮挡状态：被遮挡时 DWM 可能把渲染限到 30fps。
fn track_space_occlusion(
    mut reader: MessageReader<WindowOccluded>,
    mut occluded: ResMut<SpaceOccluded>,
) {
    for event in reader.read() {
        occluded.0 = event.occluded;
        tracing::info!(occluded = event.occluded, "空间视口遮挡状态变化");
    }
}

fn load_space_graph(
    mut state: ResMut<SpaceLoadState>,
    root: Option<Res<ProjectRoot>>,
    mode: Option<Res<LayoutMode>>,
    time: Res<Time>,
    mut refresh: ResMut<SpaceRefresh>,
    mut graph: ResMut<SpaceGraph>,
) {
    let Some(root) = root else {
        return;
    };
    if state.root.as_deref() != Some(root.path.as_path()) {
        if let Some(job) = state.job.take() {
            job.cancel();
        }
        state.root = Some(root.path.clone());
        state.loaded = false;
        refresh.dirty = false;
    }
    // 经典 2D 回退不扫描：用户看不到 3D，不必为整棵文件树付扫描成本。
    if mode
        .as_deref()
        .is_some_and(|mode| *mode == LayoutMode::Classic)
    {
        return;
    }
    if state.loaded {
        if refresh.dirty && time.elapsed_secs_f64() - refresh.last_event >= 0.8 {
            state.loaded = false;
            refresh.dirty = false;
        } else {
            return;
        }
    }
    if state.job.is_none() {
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

/// 回到宇宙时恢复完整档位：自动退化留下的 `Static` 不能跨模式粘住。
fn reset_space_quality_on_mode(
    mode: Option<Res<LayoutMode>>,
    mut last: Local<Option<LayoutMode>>,
    mut quality: ResMut<ViewQuality>,
    mut perf: ResMut<SpacePerf>,
) {
    let mode_now = mode.as_deref().copied().unwrap_or_default();
    if *last == Some(mode_now) {
        return;
    }
    *last = Some(mode_now);
    if mode_now == LayoutMode::Universe && *quality != ViewQuality::Full {
        *quality = ViewQuality::Full;
        perf.warmup_until = None;
        perf.over_frames = 0;
    }
}

fn sync_space_graph(
    mut commands: Commands,
    graph: Res<SpaceGraph>,
    quality: Res<ViewQuality>,
    materials: Option<Res<SpaceMaterials>>,
    stars: Query<Entity, With<SpatialStar>>,
    edges: Query<Entity, With<SpaceEdge>>,
    mut extent: ResMut<SpaceExtent>,
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
    extent.radius = radius;

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
                kind: node.kind,
                label: node.label.clone(),
                path: node.path.as_ref().map(|path| path.display().to_string()),
                base_scale: scale,
            },
        ));
    }

    let mut edge_count = 0;
    if *quality == ViewQuality::Full {
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

/// 空间面板当前是否可见（缺注册表的极简装配按不可见处理）。
fn space_visible(registry: Option<&PanelRegistry>, layout: Option<&SlotLayout>) -> bool {
    match (registry, layout) {
        (Some(registry), Some(layout)) => layout.is_panel_visible(registry, ids::SPACE),
        _ => false,
    }
}

fn orbit_space_camera(
    time: Option<Res<Time>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    extent: Option<Res<SpaceExtent>>,
    input_focus: Option<Res<InputFocus>>,
    viewport: Query<&Hovered, With<SpaceViewportMarker>>,
    mut cameras: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    if !space_visible(registry.as_deref(), layout.as_deref()) {
        return;
    }
    let Ok((mut transform, mut orbit)) = cameras.single_mut() else {
        return;
    };
    let hovered = viewport
        .single()
        .map(|hovered| hovered.get())
        .unwrap_or(false);
    let typing = input_focus
        .as_deref()
        .is_some_and(|focus| focus.get().is_some());
    let radius = extent.as_deref().map_or(0.0, |extent| extent.radius);
    let (min_dist, max_dist) = if radius > 1.0 {
        (radius * 0.15, radius * 6.0)
    } else {
        (6.0, 120.0)
    };

    if hovered {
        if let (Some(mouse), Some(motion)) = (mouse.as_ref(), motion.as_ref())
            && mouse.pressed(MouseButton::Left)
        {
            orbit.yaw -= motion.delta.x * 0.006;
            orbit.pitch = (orbit.pitch + motion.delta.y * 0.006).clamp(-1.2, 1.2);
        }
        if let Some(scroll) = scroll.as_ref() {
            // 触控板/高精度滚轮给的是像素，机械滚轮给的是行：先归一到「行」。
            let steps = match scroll.unit {
                MouseScrollUnit::Line => scroll.delta.y,
                MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
            };
            if steps != 0.0 {
                orbit.dist = (orbit.dist * 0.9_f32.powf(steps)).clamp(min_dist, max_dist);
            }
        }
    }

    // 文本输入框有焦点时，方向键/Enter/`+`/`-` 属于输入法，不得转相机。
    let speed = time.as_ref().map_or(0.0, |time| 1.2 * time.delta_secs());
    if !typing && let Some(keys) = keys.as_ref() {
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
            orbit.dist = (orbit.dist * 0.98).max(min_dist);
        }
        if keys.pressed(KeyCode::Minus) {
            orbit.dist = (orbit.dist * 1.02).min(max_dist);
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
    mut input_focus: Option<ResMut<InputFocus>>,
    mut stars: Query<(
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Transform,
        &SpatialStar,
    )>,
    mut open: MessageWriter<OpenFileRequest>,
) {
    if !space_visible(registry.as_deref(), layout.as_deref()) {
        state.hovered = None;
        return;
    }
    let typing = input_focus
        .as_deref()
        .is_some_and(|focus| focus.get().is_some());
    let viewport_hovered = viewport
        .single()
        .map(|(_, _, hovered)| hovered.get())
        .unwrap_or(false);

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
                let candidates: Vec<(SpatialNodeId, Vec2)> = stars
                    .iter()
                    .filter_map(|(_, transform, star)| {
                        let ndc = camera.world_to_ndc(camera_transform, transform.translation)?;
                        ndc_to_viewport(ndc, size).map(|screen| (star.id, screen))
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

    // 键盘选中：[ / ] 在当前节点间循环（Tab 留给 UI 焦点导航）。
    // 有文本输入焦点时不抢键，避免在对话/设置输入时改变宇宙选中项。
    if !typing && let Some(keys) = keys.as_ref() {
        let step = if keys.just_pressed(KeyCode::BracketRight) {
            1
        } else if keys.just_pressed(KeyCode::BracketLeft) {
            -1
        } else {
            0
        };
        if step != 0 {
            let mut ids: Vec<SpatialNodeId> = stars.iter().map(|(_, _, star)| star.id).collect();
            ids.sort_by_key(|id| id.0);
            if !ids.is_empty() {
                let index = state
                    .selected
                    .and_then(|selected| ids.iter().position(|id| *id == selected));
                let next = match index {
                    Some(index) => (index as isize + step).rem_euclid(ids.len() as isize) as usize,
                    None if step > 0 => 0,
                    None => ids.len() - 1,
                };
                state.selected = Some(ids[next]);
                state.last_click = None;
            }
        }
    }

    if let (Some(mouse), Some(time)) = (mouse.as_ref(), time.as_ref())
        && mouse.just_pressed(MouseButton::Left)
    {
        // 只有点视口才把键盘从输入框交还给宇宙；点对话/设置输入框不能误清焦点。
        if viewport_hovered && let Some(focus) = input_focus.as_mut() {
            focus.clear();
        }
        if let Some(id) = hovered {
            let now = time.elapsed_secs_f64();
            let double_click = state
                .last_click
                .is_some_and(|(last, at)| last == id && now - at < 0.35);
            state.selected = Some(id);
            if double_click {
                state.last_click = None;
                open_selected(&state, &mut stars, &mut open);
            } else {
                state.last_click = Some((id, now));
            }
        }
    }

    if !typing
        && keys
            .as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Enter))
    {
        open_selected(&state, &mut stars, &mut open);
    }

    if let Some(selected) = state.selected
        && !stars.iter().any(|(_, _, star)| star.id == selected)
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
    stars: &mut Query<(
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Transform,
        &SpatialStar,
    )>,
    open: &mut MessageWriter<OpenFileRequest>,
) {
    let Some(selected) = state.selected else {
        return;
    };
    for (_, _, star) in stars.iter() {
        if star.id != selected {
            continue;
        }
        // 目录/星团不是文件：双击/Enter 不应把文件夹送进编辑器。
        if star.kind == SpatialNodeKind::File
            && let Some(path) = star.path.as_ref()
        {
            open.write(OpenFileRequest {
                path: PathBuf::from(path),
                line: None,
            });
        }
        return;
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
    graph: Res<SpaceGraph>,
    loc: Res<Localizer>,
    mut badge: Query<&mut Text, With<SpaceQualityBadge>>,
) {
    if !quality.is_changed() && !loc.is_changed() && !graph.is_changed() {
        return;
    }
    let key = match *quality {
        ViewQuality::Full => "space-quality-full",
        ViewQuality::Reduced => "space-quality-reduced",
        ViewQuality::Static => "space-quality-static",
    };
    let mut label = tr(&loc, key);
    if graph.0.omitted_nodes > 0 {
        label = format!(
            "{label} · {}",
            tr_with(
                &loc,
                "space-omitted",
                &[("count", graph.0.omitted_nodes.to_string())]
            )
        );
    }
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
    occluded: Option<Res<SpaceOccluded>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<SpaceCamera>>,
) {
    let visible = space_visible(registry.as_deref(), layout.as_deref());
    let focused = window.single().map_or(true, |window| window.focused);
    let occluded = occluded.as_deref().is_some_and(|occluded| occluded.0);
    let active = *quality != ViewQuality::Static && visible && focused && !occluded;
    for mut camera in &mut cameras {
        if camera.is_active != active {
            camera.is_active = active;
        }
    }
}

/// G7 退化梯的自动触发：连续 ~3 秒掉出 60fps 就降档；800 档仍不足则回退 2D。
///
/// 预热 5 秒，且只在相机真正渲染（可见 + 聚焦 + 非 Static）时计数；
/// 偶发卡顿会被“连续”条件过滤，不会因为一次编译/切换窗口就降级。
fn watch_space_performance(
    time: Res<Time>,
    mut quality: ResMut<ViewQuality>,
    mut perf: ResMut<SpacePerf>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    occluded: Option<Res<SpaceOccluded>>,
    cameras: Query<&Camera, With<SpaceCamera>>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    loc: Res<Localizer>,
    mut toast: MessageWriter<ToastMessage>,
    mut save_3d_home: MessageWriter<Save3DHomeMessage>,
    mut global: Option<ResMut<GlobalConfigRes>>,
) {
    // G7 的门槛是 release 帧率；debug 构建慢是预期，别让开发中的 `cargo run`
    // 在 10 秒后自己掉进 2D。纯决策 `degrade_after` 仍有单测覆盖。
    if cfg!(debug_assertions) {
        return;
    }
    let occluded = occluded.as_deref().is_some_and(|occluded| occluded.0);
    let active = !occluded
        && space_visible(registry.as_deref(), layout.as_deref())
        && cameras.single().is_ok_and(|camera| camera.is_active);
    if !active {
        perf.over_frames = 0;
        perf.warmup_until = None;
        return;
    }
    let now = time.elapsed_secs_f64();
    let warmup_until = *perf.warmup_until.get_or_insert(now + DEGRADE_WARMUP_SECS);
    if now < warmup_until {
        perf.over_frames = 0;
        return;
    }

    let delta_ms = time.delta_secs_f64() * 1000.0;
    // 30Hz 屏/远程会话的上限本来就是 33ms，不能按 60Hz 的预算判它不合格。
    let refresh_hz = monitors
        .single()
        .ok()
        .and_then(|monitor| monitor.refresh_rate_millihertz)
        .map_or(60_000.0, |millihertz| f64::from(millihertz) / 1000.0);
    let budget_ms = if refresh_hz > 1.0 {
        (1000.0 / refresh_hz * 1.35).max(FRAME_BUDGET_MS)
    } else {
        FRAME_BUDGET_MS
    };
    if delta_ms > budget_ms {
        perf.over_frames = perf.over_frames.saturating_add(1);
    } else {
        perf.over_frames = 0;
    }

    match degrade_after(*quality, perf.over_frames) {
        PerfAction::None => {}
        PerfAction::Reduce => {
            *quality = ViewQuality::Reduced;
            perf.over_frames = 0;
            toast.write(ToastMessage {
                text: tr(&loc, "space-degraded-800"),
            });
            tracing::warn!(delta_ms, "3D 视口帧率不足，降到 800 档");
        }
        PerfAction::Fallback2d => {
            *quality = ViewQuality::Static;
            perf.over_frames = 0;
            if let Some(global) = global.as_mut() {
                global.0.preferences.three_d_home = false;
                save_3d_home.write(Save3DHomeMessage { enabled: false });
            }
            toast.write(ToastMessage {
                text: tr(&loc, "space-degraded-2d"),
            });
            tracing::warn!(delta_ms, "3D 视口 800 档仍不足，回退 2D");
        }
    }
}

fn update_space_hud(
    theme: Res<Theme>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
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
    if !space_visible(registry.as_deref(), layout.as_deref()) {
        for (_, _, _, _, _, mut visibility) in &mut labels {
            *visibility = Visibility::Hidden;
        }
        return;
    }
    // 只保留前 24 个节点，且用 `&str` 借用——2k 节点时不要每帧克隆标签。
    let mut items: Vec<(SpatialNodeId, Vec3, &str)> = stars
        .iter()
        .map(|(transform, star)| (star.id, transform.translation, star.label.as_str()))
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
        if text.0.as_str() != *name {
            text.0 = name.to_string();
            ellipsis.source = name.to_string();
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
    fn degrade_ladder_thresholds() {
        assert_eq!(degrade_after(ViewQuality::Full, 179), PerfAction::None);
        assert_eq!(degrade_after(ViewQuality::Full, 180), PerfAction::Reduce);
        assert_eq!(
            degrade_after(ViewQuality::Reduced, 180),
            PerfAction::Fallback2d
        );
        assert_eq!(
            degrade_after(ViewQuality::Static, 180),
            PerfAction::None,
            "Static 是手动档，不再自动推进"
        );
    }

    #[test]
    fn refresh_ignore_is_relative_to_project_root() {
        let root = Path::new("F:/proj");
        assert!(!is_refresh_ignored(
            Some(root),
            Path::new("F:/proj/src/a.rs")
        ));
        assert!(is_refresh_ignored(
            Some(root),
            Path::new("F:/proj/target/a.rs")
        ));
        assert!(
            !is_refresh_ignored(Some(root), Path::new("F:/proj/src/target")),
            "文件名恰好叫 target 不是目录噪声"
        );
        assert!(
            !is_refresh_ignored(
                Some(Path::new("F:/ws/.worktrees/feat")),
                Path::new("F:/ws/.worktrees/feat/src/a.rs")
            ),
            "项目根位于跳过目录下时不能整体静默"
        );
        assert!(
            !is_refresh_ignored(Some(root), Path::new("F:/other/target/a.rs")),
            "项目根之外的路径不参与忽略判断"
        );
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
