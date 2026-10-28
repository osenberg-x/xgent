//! v8 M2b 密度档位：真实界面下的间距重排（brief A2 / A3）。
//!
//! 这些断言看的是**真实插件产出的节点**（rail、对话面板的滚动区、设置面板卡片），
//! 不是测试里自造的替身——否则「切档后全界面收紧」就只是把单元测试的结论复述一遍。

mod common;

use bevy::prelude::*;
use xgent_settings::Localizer;
use xgent_ui::density::DensityPlugin;
use xgent_ui::layout::{ActivityBarMarker, LayoutPlugin, TopBarMarker};
use xgent_ui::theme::{Density, Theme, ThemeHandle};

/// 跑若干帧（布局与懒捕获都在 Update 里落地）。
fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

/// 切档（等价于设置面板点了另一枚按钮：改偏好 → `ThemeHandle` 跟上 → generation 前进）。
fn set_density(app: &mut App, density: Density) {
    let mut handle = app.world_mut().resource_mut::<ThemeHandle>();
    assert_ne!(handle.density, density, "切档前档位应不同");
    handle.density = density;
    handle.generation = handle.generation.wrapping_add(1);
}

/// 按标记取唯一节点的 `Node` 快照。
fn node_of<M: Component>(app: &mut App) -> Node {
    let mut q = app.world_mut().query_filtered::<&Node, With<M>>();
    q.iter(app.world())
        .next()
        .cloned()
        .unwrap_or_else(|| panic!("应存在带 {} 的节点", std::any::type_name::<M>()))
}

/// 只有布局的最小 App（rail / 顶栏都在这一层）。
fn layout_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Localizer>()
        .init_resource::<Theme>()
        .add_plugins(LayoutPlugin)
        .add_plugins(DensityPlugin);
    run(&mut app, 3);
    app
}

/// A2（rail 与顶栏）：切到紧凑后内边距与间隙按 0.75 收紧，切回舒适逐字还原。
#[test]
fn rail_and_top_bar_tighten_on_compact() {
    let mut app = layout_app();

    let rail = node_of::<ActivityBarMarker>(&mut app);
    assert_eq!(rail.padding.top, Val::Px(8.0), "rail 竖向内边距基线 8");
    assert_eq!(rail.row_gap, Val::Px(4.0), "rail 行间隙基线 4");

    let bar = node_of::<TopBarMarker>(&mut app);
    assert_eq!(bar.padding.left, Val::Px(16.0), "顶栏横向内边距基线 16");
    assert_eq!(bar.column_gap, Val::Px(12.0), "顶栏列间隙基线 12");

    set_density(&mut app, Density::Compact);
    run(&mut app, 2);

    let rail = node_of::<ActivityBarMarker>(&mut app);
    assert_eq!(rail.padding.top, Val::Px(6.0), "8 → 6");
    assert_eq!(rail.padding.bottom, Val::Px(6.0));
    assert_eq!(rail.row_gap, Val::Px(3.0), "4 → 3");

    let bar = node_of::<TopBarMarker>(&mut app);
    assert_eq!(bar.padding.left, Val::Px(12.0), "16 → 12");
    assert_eq!(bar.column_gap, Val::Px(9.0), "12 → 9");

    // 固定尺寸不参与（决策 D-2）：rail 仍是 52 宽
    assert_eq!(rail.width, Val::Px(52.0), "rail 宽度不随密度变化");

    // A3：多跑几帧不再继续缩小
    run(&mut app, 5);
    assert_eq!(
        node_of::<ActivityBarMarker>(&mut app).padding.top,
        Val::Px(6.0)
    );

    set_density(&mut app, Density::Comfortable);
    run(&mut app, 2);
    assert_eq!(
        node_of::<ActivityBarMarker>(&mut app).padding.top,
        Val::Px(8.0)
    );
    assert_eq!(node_of::<TopBarMarker>(&mut app).column_gap, Val::Px(12.0));
}

/// A2（对话区）：对话面板消息列表的滚动区内边距与行间距同样按档位收紧。
#[test]
fn chat_panel_spacing_tightens_on_compact() {
    let mut app = chat_app();

    let list = chat_scroll_node(&mut app);
    assert_eq!(list.padding.left, Val::Px(24.0), "对话区内边距基线 24");
    assert_eq!(list.row_gap, Val::Px(20.0), "对话区行间距基线 20");

    set_density(&mut app, Density::Compact);
    run(&mut app, 2);

    let list = chat_scroll_node(&mut app);
    assert_eq!(list.padding.left, Val::Px(18.0), "24 → 18");
    assert_eq!(list.row_gap, Val::Px(15.0), "20 → 15");
}

/// 对话面板的滚动区（消息列表）：内边距 24 / 行间距 20 的那一个。
fn chat_scroll_node(app: &mut App) -> Node {
    let mut q = app
        .world_mut()
        .query_filtered::<&Node, With<xgent_ui::chat_panel::MessageListMarker>>();
    q.iter(app.world())
        .next()
        .cloned()
        .expect("对话面板应有消息列表节点")
}

/// 能 spawn 对话面板的最小 App。
///
/// 面板的 Update 系统订阅 agent 的消息，所以这些消息类型必须注册；
/// `Conversation` / `Clipboard` / `IconAssets` 是它的读取资源。
fn chat_app() -> App {
    use xgent_agent::{
        CompactedMessage, Conversation, DeltaMessage, DoneMessage, ErrorMessage, RetryMessage,
        SessionClearedMessage, SteeringMessage, UserInputMessage,
    };

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::image::ImagePlugin::default())
        .add_plugins(bevy::window::WindowPlugin::default())
        .add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy::picking::input::PointerInputPlugin)
        .add_plugins(bevy::picking::PickingPlugin)
        .add_plugins(bevy::picking::InteractionPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::input_focus::InputFocusPlugin)
        .add_plugins(bevy::input_focus::InputDispatchPlugin)
        .add_plugins(bevy::text::TextPlugin)
        .add_plugins(bevy::ui::UiPlugin)
        .add_plugins(bevy::ui_widgets::UiWidgetsPlugins)
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .add_plugins(bevy::camera::visibility::VisibilityPlugin)
        .add_plugins(bevy::mesh::MeshPlugin)
        .init_resource::<Theme>()
        .init_resource::<Localizer>()
        .init_resource::<Conversation>()
        .init_resource::<bevy::clipboard::Clipboard>()
        .insert_resource(xgent_ui::fonts::UiFonts::fallback())
        .insert_resource(xgent_ui::kit::IconAssets::default())
        .add_message::<UserInputMessage>()
        .add_message::<DeltaMessage>()
        .add_message::<DoneMessage>()
        .add_message::<ErrorMessage>()
        .add_message::<RetryMessage>()
        .add_message::<SteeringMessage>()
        .add_message::<CompactedMessage>()
        .add_message::<SessionClearedMessage>()
        .add_message::<xgent_agent::BoundedMessage>()
        .add_message::<xgent_agent::AbortMessage>()
        .add_message::<xgent_ui::kit::ToastMessage>()
        .add_message::<xui::input::ChatInputSubmitted>()
        .add_plugins(LayoutPlugin)
        .add_plugins(xgent_ui::chat_panel::ChatPanelPlugin)
        .add_plugins(DensityPlugin);
    app.world_mut().spawn(Camera::default());
    run(&mut app, 3);
    app
}

/// A2（设置面板卡片）：面板卡片的内边距与行间距同样按档位收紧。
#[test]
fn settings_card_tightens_on_compact() {
    let mut app = common::panel_app();
    app.add_plugins(DensityPlugin);
    common::open_panel(&mut app);

    let card = settings_card_node(&mut app);
    assert_eq!(card.padding.left, Val::Px(16.0), "设置卡片内边距基线 16");
    assert_eq!(card.row_gap, Val::Px(8.0), "设置卡片行间距基线 8");

    set_density(&mut app, Density::Compact);
    run(&mut app, 2);

    let card = settings_card_node(&mut app);
    assert_eq!(card.padding.left, Val::Px(12.0), "16 → 12");
    assert_eq!(card.row_gap, Val::Px(6.0), "8 → 6");
}

/// 设置面板卡片：宽度等于 `size::OVERLAY_SETTINGS_W` 的那个滚动容器。
fn settings_card_node(app: &mut App) -> Node {
    let width = Val::Px(xgent_ui::theme::size::OVERLAY_SETTINGS_W);
    let mut q = app.world_mut().query::<&Node>();
    q.iter(app.world())
        .find(|n| n.width == width)
        .cloned()
        .expect("设置面板应有卡片节点")
}

/// 设置面板的密度按钮：点「紧凑」即改偏好、即生效、并只发一条写盘消息。
#[test]
fn density_button_switches_immediately_and_writes_once() {
    use xgent_settings::GlobalConfigRes;
    use xgent_ui::settings_panel::{DensityButton, SaveDensityMessage};

    let mut app = common::panel_app();
    // 真实应用里 ThemePlugin 由 XgentUiPlugin 装配；这里显式加上，
    // 为的是走通「偏好 → sync_theme_from_config → ThemeHandle → 重排」整条链路
    app.add_plugins(xgent_ui::theme::ThemePlugin);
    app.add_plugins(DensityPlugin);
    common::open_panel(&mut app);

    assert_eq!(
        app.world().resource::<ThemeHandle>().density,
        Density::Comfortable,
        "缺省应为舒适档"
    );

    // 点当前档位：不该发写盘消息
    common::press::<DensityButton>(&mut app, |b| b.density == Density::Comfortable);
    assert_eq!(
        app.world().resource::<Messages<SaveDensityMessage>>().len(),
        0,
        "点当前档位不该发写请求"
    );

    // 点「紧凑」：偏好立即改、消息只发一条
    common::press::<DensityButton>(&mut app, |b| b.density == Density::Compact);
    assert_eq!(
        app.world()
            .resource::<GlobalConfigRes>()
            .preferences
            .density,
        Density::Compact,
        "点击应就地改偏好（不等 IPC 往返）"
    );
    assert_eq!(
        app.world().resource::<Messages<SaveDensityMessage>>().len(),
        1,
        "应发一条持久化消息"
    );

    // 下一帧 `ThemeHandle` 跟上（真实链路：偏好 → sync → 重排）
    run(&mut app, 1);
    assert_eq!(
        app.world().resource::<ThemeHandle>().density,
        Density::Compact,
        "偏好变化应在一帧内带到 ThemeHandle"
    );
}
