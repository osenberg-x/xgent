//! Bevy 0.20 交互模型契约测试（headless 全链路）。
//!
//! 验证 0.20 适配（`doc/notes/bevy-0.20-migration.md` §3.1）所依赖的承重语义，
//! 全部经真实调度（First 输入 → PreUpdate picking → Update 处理）端到端驱动：
//!
//! 1. **后代悬停**：指针悬停在交互根的子节点上，根的 `Hovered` 为 true
//!    （CSS `:hover` 后代语义；旧 `ui_focus_system` 靠 FocusPolicy 默认 Pass 穿透实现同等效果）。
//! 2. **按下冒泡**：按在子节点上，根的 `Pressed` 被插入（ButtonPlugin 全局 observer
//!    随事件冒泡逐级运行），`Added<Pressed>` 查询在按下帧触发一次、按住不重复、释放清除。
//! 3. **嵌套消费**：嵌套按钮（父 Button 内的 × Button）被按下时，事件由 ButtonPlugin
//!    在最内层 `propagate(false)` 消费——子钮 Pressed、父钮不 Pressed（等价旧 FocusPolicy::Block）。
//! 4. **面板压遮罩**：0.20 中非 Button 子节点上的按下会**冒泡到 Button 祖先**
//!    （本轮功能性 review 发现的回归面），面板须挂 [`xgent_ui::kit::block_press_bubbling`]
//!    消费 `PointerPress`——验证：按在面板上遮罩不 Pressed；按在面板外遮罩 Pressed。
//!
//! 另含一个边界观测：同帧 press+release（长帧吞快点的风险面，行为由上游决定）。
//!
//! 运行前提：无渲染器，喂 `WindowEvent` 消息（winit 同款入口）驱动 picking；
//! 可见性传播（VisibilityPlugin）、mesh 资产（MeshPlugin）等在 headless 下需手动补齐。

use bevy::ecs::hierarchy::ChildOf;
use bevy::input::ButtonState;
use bevy::math::Vec2;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button as WidgetButton;
use bevy::window::{CursorMoved, PrimaryWindow, WindowEvent};
use xgent_ui::kit;

/// 记录某实体 `Added<Pressed>` 触发次数的观测资源。
#[derive(Resource, Default)]
struct PressCount(u32);

/// 在指定实体上监听 `Added<Pressed>` 的处理系统（xgent 各点击系统的同款模式）。
#[allow(clippy::type_complexity)]
fn press_observer(
    target: Entity,
) -> impl FnMut(Query<(), (With<Pressed>, Added<Pressed>)>, ResMut<PressCount>) {
    move |q: Query<(), (With<Pressed>, Added<Pressed>)>, mut count: ResMut<PressCount>| {
        if q.contains(target) {
            count.0 += 1;
        }
    }
}

/// headless 测试 App：MinimalPlugins + picking 全链路 + UiPlugin + widgets 插件组。
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::image::ImagePlugin::default())
        .add_plugins(bevy::window::WindowPlugin::default())
        .add_message::<WindowEvent>()
        .add_plugins(bevy::picking::input::PointerInputPlugin)
        .add_plugins(bevy::picking::PickingPlugin)
        .add_plugins(bevy::picking::InteractionPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::input_focus::InputFocusPlugin)
        .add_plugins(bevy::text::TextPlugin)
        // UiPickingPlugin 已由 UiPlugin 内部挂载（bevy_ui lib.rs），无需显式添加
        .add_plugins(bevy::ui::UiPlugin)
        // ButtonPlugin 等（维护 Pressed / 发 Activate）在 UiWidgetsPlugins，正常由
        // DefaultPlugins 挂载；测试为 MinimalPlugins 组装需显式补齐
        .add_plugins(bevy::ui_widgets::UiWidgetsPlugins);

    // headless 补齐（正常由 bevy_sprite / 渲染侧注册）：
    // - Assets<TextureAtlasLayout>：bevy_ui 的 image content-size 系统无条件读取
    // - VisibilityPlugin：无渲染器时可见性传播无人运行，InheritedVisibility 恒 false
    //   （后端跳过不可见节点）；其 mesh 系统需要 Mesh 资产（MeshPlugin 注册）
    app.init_asset::<bevy::image::TextureAtlasLayout>();
    app.add_plugins(bevy::camera::visibility::VisibilityPlugin);
    app.add_plugins(bevy::mesh::MeshPlugin);

    // 指向主窗口的相机（主窗口由 WindowPlugin 自动创建；无渲染器：仅要布局与命中测试）。
    app.world_mut().spawn(Camera::default());
    app.init_resource::<PressCount>();
    app
}

/// 构建契约 4 场景：全屏遮罩 Button + 其上 400x300 普通面板（带按压防护）。
///
/// 遮罩用窗口实际分辨率的 px 尺寸：headless 无相机更新系统，`Val::Percent`
/// 根节点会解析为 0 尺寸（真实 app 有渲染管线，不受影响）。
fn spawn_mask_with_panel(app: &mut App) -> Entity {
    let (w, h) = {
        let mut q = app
            .world_mut()
            .query_filtered::<&Window, With<PrimaryWindow>>();
        let res = q
            .single(app.world())
            .expect("primary window")
            .resolution
            .physical_size();
        (res.x as f32, res.y as f32)
    };
    app.world_mut()
        .spawn((
            WidgetButton,
            bevy::picking::hover::Hovered::default(),
            Node {
                width: Val::Px(w),
                height: Val::Px(h),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    position_type: bevy::ui::PositionType::Absolute,
                    left: Val::Px(50.0),
                    top: Val::Px(50.0),
                    width: Val::Px(400.0),
                    height: Val::Px(300.0),
                    ..default()
                },
                kit::block_press_bubbling(),
            ));
        })
        .id()
}

/// 把光标移到 `pos`（逻辑像素 = 物理像素，headless scale factor = 1）。
fn move_cursor(app: &mut App, pos: Vec2) {
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .expect("primary window");
    app.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::CursorMoved(CursorMoved {
            window,
            position: pos,
            delta: None,
        }));
}

/// 发送鼠标左键按下/释放事件。
fn click_left(app: &mut App, state: ButtonState) {
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .expect("primary window");
    app.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::MouseButtonInput(
            bevy::input::mouse::MouseButtonInput {
                window,
                button: bevy::input::mouse::MouseButton::Left,
                state,
            },
        ));
}

/// 契约 1：悬停在子节点上 → 根 Hovered = true；移开 → false。
#[test]
fn descendant_hover_reaches_root() {
    let mut app = test_app();
    // 根按钮 200x200 @ (100,100)，子节点铺满右半（指针落点 (250,200) 在子节点上）。
    let root = app
        .world_mut()
        .spawn((
            WidgetButton,
            bevy::picking::hover::Hovered::default(),
            Node {
                position_type: bevy::ui::PositionType::Absolute,
                left: Val::Px(100.0),
                top: Val::Px(100.0),
                width: Val::Px(200.0),
                height: Val::Px(200.0),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn(Node {
                position_type: bevy::ui::PositionType::Absolute,
                left: Val::Percent(50.0),
                width: Val::Percent(50.0),
                height: Val::Percent(100.0),
                ..default()
            });
        })
        .id();
    // 先跑两帧：布局 + 相机/窗口就绪
    app.update();
    app.update();

    move_cursor(&mut app, Vec2::new(250.0, 200.0));
    app.update();
    let hovered = app
        .world()
        .get::<bevy::picking::hover::Hovered>(root)
        .expect("root 应有 Hovered 组件（spawn 时插入）")
        .get();
    assert!(hovered, "悬停子节点应使根 Hovered=true（后代语义）");

    move_cursor(&mut app, Vec2::new(50.0, 50.0));
    app.update();
    let hovered = app
        .world()
        .get::<bevy::picking::hover::Hovered>(root)
        .unwrap()
        .get();
    assert!(!hovered, "移开指针后根 Hovered 应复位 false");
}

/// 契约 2：按在子节点上 → 根 Pressed 插入、Added<Pressed> 恰好触发一次（按住不重复）。
#[test]
fn press_on_child_presses_root_once() {
    let mut app = test_app();
    let root = app
        .world_mut()
        .spawn((
            WidgetButton,
            bevy::picking::hover::Hovered::default(),
            Node {
                position_type: bevy::ui::PositionType::Absolute,
                left: Val::Px(100.0),
                top: Val::Px(100.0),
                width: Val::Px(200.0),
                height: Val::Px(200.0),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn(Node {
                position_type: bevy::ui::PositionType::Absolute,
                left: Val::Percent(50.0),
                width: Val::Percent(50.0),
                height: Val::Percent(100.0),
                ..default()
            });
        })
        .id();
    app.add_systems(Update, press_observer(root));
    app.update();
    app.update();

    move_cursor(&mut app, Vec2::new(250.0, 200.0));
    app.update();
    click_left(&mut app, ButtonState::Pressed);
    app.update();
    assert_eq!(app.world().resource::<PressCount>().0, 1, "按下帧触发一次");

    // 持续按住多帧：Added 过滤不重复触发
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<PressCount>().0,
        1,
        "按住期间不得重复触发"
    );
    assert!(
        app.world().get::<Pressed>(root).is_some(),
        "按住期间根应保持 Pressed"
    );

    click_left(&mut app, ButtonState::Released);
    app.update();
    assert!(
        app.world().get::<Pressed>(root).is_none(),
        "释放后 ButtonPlugin 应移除 Pressed"
    );
}

/// 契约 3：嵌套按钮——按内层 × 钮，子钮 Pressed、父钮不 Pressed。
#[test]
fn nested_button_consumes_press_at_innermost() {
    let mut app = test_app();
    let parent = app
        .world_mut()
        .spawn((
            WidgetButton,
            bevy::picking::hover::Hovered::default(),
            Node {
                position_type: bevy::ui::PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(300.0),
                height: Val::Px(60.0),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                WidgetButton,
                bevy::picking::hover::Hovered::default(),
                Node {
                    position_type: bevy::ui::PositionType::Absolute,
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(40.0),
                    height: Val::Px(60.0),
                    ..default()
                },
            ));
        })
        .id();
    let child = app
        .world_mut()
        .query_filtered::<(Entity, &ChildOf), With<WidgetButton>>()
        .single(app.world())
        .expect("内层按钮")
        .0;
    app.add_systems(Update, (press_observer(parent), press_observer(child)));
    app.update();
    app.update();

    // 指针落在内层钮中心（父 300x60，内层占右侧 40px）
    move_cursor(&mut app, Vec2::new(280.0, 30.0));
    app.update();
    click_left(&mut app, ButtonState::Pressed);
    app.update();

    assert!(
        app.world().get::<Pressed>(child).is_some(),
        "内层钮应被 Pressed"
    );
    assert!(
        app.world().get::<Pressed>(parent).is_none(),
        "事件应在最内层被 ButtonPlugin 消费（propagate(false)），父钮不得 Pressed"
    );

    // 反向：按父钮本体（避开内层区域）
    click_left(&mut app, ButtonState::Released);
    app.update();
    move_cursor(&mut app, Vec2::new(150.0, 30.0));
    app.update();
    click_left(&mut app, ButtonState::Pressed);
    app.update();
    assert!(
        app.world().get::<Pressed>(parent).is_some(),
        "按父钮本体应使父钮 Pressed"
    );
}

/// 契约 4a：点在面板外 → 遮罩 Button 收到按下（遮罩仍在工作）。
#[test]
fn mask_receives_press_outside_panel() {
    let mut app = test_app();
    let mask = spawn_mask_with_panel(&mut app);
    app.add_systems(Update, press_observer(mask));
    app.update();
    app.update();

    move_cursor(&mut app, Vec2::new(780.0, 580.0)); // 面板外
    app.update();
    app.update();
    click_left(&mut app, ButtonState::Pressed);
    app.update();
    assert!(
        app.world().resource::<PressCount>().0 >= 1,
        "点面板外应命中遮罩"
    );
}

/// 契约 4b：点在面板上 → 面板的 `block_press_bubbling` 消费 PointerPress，
/// 遮罩 Button 不得收到按下（等价旧 FocusPolicy::Block 的阻断语义）。
#[test]
fn panel_consumes_press_above_mask() {
    let mut app = test_app();
    let mask = spawn_mask_with_panel(&mut app);
    app.add_systems(Update, press_observer(mask));
    app.update();
    app.update();

    move_cursor(&mut app, Vec2::new(200.0, 150.0)); // 面板内
    app.update();
    app.update();
    click_left(&mut app, ButtonState::Pressed);
    app.update();
    assert_eq!(
        app.world().resource::<PressCount>().0,
        0,
        "面板消费 PointerPress 后，遮罩不得收到按下"
    );
}

/// 边界观测：同帧 press+release（长帧吞快点的风险面）。
///
/// 本测试**记录行为**而非断言旧语义：实测同帧插拔后 `Added<Pressed>` 为 0 次——
/// 即长帧期间快速点击会被吞。与官方 widgets 的差异点：它们以 `PointerClick`/`Activate`
/// 为准（不受插拔影响）。若实际使用中可感，再把点击系统迁到 Activate observer
/// （迁移文档 §4.2 已记录）。
#[test]
fn same_frame_press_release_behavior() {
    let mut app = test_app();
    let root = app
        .world_mut()
        .spawn((
            WidgetButton,
            bevy::picking::hover::Hovered::default(),
            Node {
                position_type: bevy::ui::PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(100.0),
                height: Val::Px(100.0),
                ..default()
            },
        ))
        .id();
    app.add_systems(Update, press_observer(root));
    app.update();
    app.update();

    move_cursor(&mut app, Vec2::new(50.0, 50.0));
    app.update();
    click_left(&mut app, ButtonState::Pressed);
    click_left(&mut app, ButtonState::Released);
    app.update();
    let count = app.world().resource::<PressCount>().0;
    println!("同帧 press+release 的 Added<Pressed> 触发次数：{count}");
}
