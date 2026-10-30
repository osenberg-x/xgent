//! G7 3D 性能 spike：离屏纹理 + 2k 实例化节点 + 轨道相机。
//!
//! 一次性测量工具，不进入产品代码。用法：
//! `G7_NODES=2000 cargo run -p xgent_app --example g7_spike --release`
//!
//! 输出：预热 120 帧、采样 300 帧后的 avg/p50/p95 帧时间与 fps，然后退出。

use bevy::{
    camera::RenderTarget, prelude::*, render::render_resource::TextureFormat, window::PresentMode,
};
use std::time::Instant;

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 800;
const WARMUP_FRAMES: usize = 120;
const MEASURE_FRAMES: usize = 300;

#[derive(Resource, Debug, Clone, Copy)]
struct SpikeConfig {
    nodes: usize,
    width: u32,
    height: u32,
    vsync: bool,
}

#[derive(Resource, Default)]
struct FrameStats {
    last: Option<Instant>,
    warmup: usize,
    samples: Vec<f64>,
    reported: bool,
}

#[derive(Component)]
struct OrbitCamera {
    target: Vec3,
    yaw: f32,
    pitch: f32,
    dist: f32,
}

fn main() {
    let nodes = std::env::var("G7_NODES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let width = std::env::var("G7_WIDTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_WIDTH);
    let height = std::env::var("G7_HEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_HEIGHT);
    let vsync = std::env::var("G7_VSYNC").ok().as_deref() != Some("0");

    App::new()
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("G7 spike · {nodes} nodes · {width}×{height}"),
                resolution: (width, height).into(),
                present_mode: if vsync {
                    PresentMode::AutoVsync
                } else {
                    PresentMode::AutoNoVsync
                },
                ..default()
            }),
            ..default()
        }))
        .insert_resource(SpikeConfig {
            nodes,
            width,
            height,
            vsync,
        })
        .init_resource::<FrameStats>()
        .add_systems(Startup, setup)
        .add_systems(Update, (orbit_camera, measure_frames))
        .run();
}

fn setup(
    mut commands: Commands,
    cfg: Res<SpikeConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let image = Image::new_target_texture(
        cfg.width,
        cfg.height,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    let image_handle = images.add(image);

    // 3D 相机：只渲染到离屏纹理。
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: Color::BLACK.into(),
            ..default()
        },
        RenderTarget::Image(image_handle.clone().into()),
        Transform::from_xyz(0.0, 0.0, 24.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.35,
            dist: 24.0,
        },
    ));

    // 2D 相机：把离屏结果铺到窗口，便于截图与肉眼确认。
    commands.spawn((
        Camera2d,
        Camera {
            order: 0,
            clear_color: Color::BLACK.into(),
            ..default()
        },
    ));
    commands.spawn((
        Sprite {
            image: image_handle,
            custom_size: Some(Vec2::new(cfg.width as f32, cfg.height as f32)),
            ..default()
        },
        Transform::default(),
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(8.0, 12.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let mesh = meshes.add(Sphere::new(0.18).mesh().uv(8, 6));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.65, 1.0),
        perceptual_roughness: 0.7,
        ..default()
    });

    let n = cfg.nodes.max(1);
    for i in 0..n {
        // 黄金角球面分布，确定性、无随机。
        let t = i as f32 / n as f32;
        let y = 1.0 - 2.0 * t;
        let r = (1.0 - y * y).sqrt();
        let theta = i as f32 * 2.399_963_1;
        let pos = Vec3::new(r * theta.cos(), y, r * theta.sin()) * 8.0;
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(pos),
        ));
    }
}

fn orbit_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    let Ok((mut transform, mut orbit)) = query.single_mut() else {
        return;
    };
    let speed = 1.2 * time.delta_secs();
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
        orbit.dist = (orbit.dist * 0.98).max(4.0);
    }
    if keys.pressed(KeyCode::Minus) {
        orbit.dist = (orbit.dist * 1.02).min(80.0);
    }

    let (sy, cy) = orbit.yaw.sin_cos();
    let (sp, cp) = orbit.pitch.sin_cos();
    transform.translation =
        orbit.target + Vec3::new(orbit.dist * cp * sy, orbit.dist * sp, orbit.dist * cp * cy);
    transform.look_at(orbit.target, Vec3::Y);
}

fn measure_frames(
    mut stats: ResMut<FrameStats>,
    cfg: Res<SpikeConfig>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = Instant::now();
    if let Some(last) = stats.last {
        let dt = now.duration_since(last).as_secs_f64();
        if stats.warmup < WARMUP_FRAMES {
            stats.warmup += 1;
        } else if stats.samples.len() < MEASURE_FRAMES {
            stats.samples.push(dt);
        } else if !stats.reported {
            stats.reported = true;
            let mut sorted = stats.samples.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let p50 = sorted[sorted.len() / 2];
            let p95 = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
            let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
            println!(
                "G7 nodes={} size={}x{} vsync={} samples={} avg_fps={:.1} p50_ms={:.2} p95_ms={:.2}",
                cfg.nodes,
                cfg.width,
                cfg.height,
                cfg.vsync,
                sorted.len(),
                1.0 / avg,
                p50 * 1000.0,
                p95 * 1000.0
            );
            exit.write(AppExit::Success);
        }
    }
    stats.last = Some(now);
}
