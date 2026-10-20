//! v8 M3-T6 虚拟列表接入回归（审计 I4 / R3-6）。
//!
//! 缺陷 I4：`xui::VirtualList` 写了 140 行但**零消费者**——大列表（文件树、
//! 会话历史）一次性 spawn 全部节点，条目上千时帧时间明显劣化。
//!
//! 这里断言两件事：
//! 1. **数据层拍平正确**：文件树被拍成一维列表（层级记在每项的 depth 上）。
//! 2. **虚拟化真的生效**：容器带 `VirtualList`、占位块撑出**全部**条目的高度
//!    （滚动范围正确），而渲染出来的行数远少于总条目数。

use std::fs;

use bevy::prelude::*;
use xgent_settings::Localizer;
use xgent_ui::file_panel::{
    FilePanelPlugin, FileTreeItems, FileTreeMarker, FileTreeSpacerMarker, ProjectRoot,
};
use xgent_ui::layout::LayoutPlugin;
use xui::i18n_bridge::Strings;
use xui_i18n::StringSource;

/// 空 StringSource（测试不关心 i18n 文案）。
struct NoopStrings;
impl StringSource for NoopStrings {
    fn get(&self, key: &str, _args: &[(&str, String)]) -> String {
        key.to_string()
    }
    fn current_lang(&self) -> &str {
        "zh-CN"
    }
}

/// 能跑文件树插件的最小 App。
fn file_tree_app(root: std::path::PathBuf) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<bevy::image::Image>()
        .init_asset::<bevy::text::Font>()
        .insert_resource(xgent_ui::fonts::UiFonts::fallback())
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            bevy::input_focus::InputDispatchPlugin,
            bevy::picking::input::PointerInputPlugin,
            bevy::picking::PickingPlugin,
            bevy::picking::InteractionPlugin,
            bevy::image::ImagePlugin::default(),
            bevy::text::TextPlugin,
            bevy::ui::UiPlugin,
            bevy::camera::visibility::VisibilityPlugin,
            bevy::mesh::MeshPlugin,
            bevy::window::WindowPlugin::default(),
        ))
        .add_plugins((
            xui::XuiPlugin,
            LayoutPlugin,
            xgent_ui::overlay::OverlayPlugin,
            xgent_ui::editor::EditorPlugin,
            FilePanelPlugin,
            xgent_ui::kit::KitPlugin,
        ))
        // agent 消息（真实 app 由 agent 插件注册；headless 需手动补）
        .add_message::<xgent_agent::UserInputMessage>()
        .add_message::<xgent_agent::DeltaMessage>()
        .add_message::<xgent_agent::DoneMessage>()
        .add_message::<xgent_agent::ErrorMessage>()
        .add_message::<xgent_agent::RetryMessage>()
        .add_message::<xgent_agent::SteeringMessage>()
        .add_message::<xgent_agent::CompactedMessage>()
        .add_message::<xgent_agent::SessionClearedMessage>()
        .add_message::<xgent_agent::BoundedMessage>()
        .add_message::<xgent_agent::ToolCallMessage>()
        .add_message::<xgent_agent::ToolProgressMessage>()
        .add_message::<xgent_agent::ToolResultMessage>()
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>()
        .insert_resource(ProjectRoot { path: root });
    app
}

/// 文件树数据层：目录被拍平成一维列表，层级记在 depth 上。
#[test]
fn file_tree_flattens_into_one_dimension() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();
    fs::create_dir(root.join("sub")).expect("创建子目录");
    fs::write(root.join("sub").join("inner.txt"), "inner").expect("写文件");
    fs::write(root.join("top.txt"), "top").expect("写文件");

    let mut app = file_tree_app(root);
    for _ in 0..5 {
        app.update();
    }

    let items = app.world().resource::<FileTreeItems>();
    assert!(!items.items.is_empty(), "文件树数据应非空");
    // 顶层：sub（目录）+ top.txt（文件）
    let top_level: Vec<_> = items.items.iter().filter(|i| i.depth == 0).collect();
    assert_eq!(top_level.len(), 2, "顶层应有 2 项");
    // 折叠的目录不产生子项
    assert!(
        items.items.iter().all(|i| i.depth == 0),
        "未展开目录不应有子项"
    );
}

/// 虚拟化：容器带 VirtualList、占位块撑出全部高度、渲染行数远少于总数。
#[test]
fn file_tree_renders_only_visible_rows() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();
    for i in 0..300 {
        fs::write(root.join(format!("file-{i:03}.txt")), "x").expect("写文件");
    }

    let mut app = file_tree_app(root);
    for _ in 0..5 {
        app.update();
    }

    // 容器带虚拟列表
    let (tree, item_count) = {
        let mut q = app
            .world_mut()
            .query_filtered::<(Entity, &xui::VirtualList), With<FileTreeMarker>>();
        let (e, list) = q.iter(app.world()).next().expect("应有文件树容器");
        (e, list.item_count)
    };
    assert_eq!(item_count, 300, "虚拟列表应记录全部 300 条");

    // 占位块撑出总高度（滚动范围正确）
    let spacer_h = {
        let mut q = app
            .world_mut()
            .query_filtered::<&Node, With<FileTreeSpacerMarker>>();
        let node = q.iter(app.world()).next().expect("应有占位块");
        match node.height {
            Val::Px(h) => h,
            other => panic!("占位块高度应为像素值，实际 {other:?}"),
        }
    };
    assert!(
        spacer_h >= 300.0 * 20.0,
        "占位块应撑出全部 300 行的高度（实测 {spacer_h}）"
    );

    // 渲染出来的行远少于总数（虚拟化生效）
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, With<xgent_ui::file_panel::FileTreeRow>>();
    let rendered = q.iter(app.world()).count();
    assert!(
        rendered < item_count,
        "只应渲染可见行（渲染 {rendered} / 总数 {item_count}）"
    );
    let _ = tree;
}

/// M3-T6 验收：VirtualList 消费者 ≥ 2（此前零消费者）。
#[test]
fn virtual_list_has_at_least_two_consumers() {
    let consumers = [
        ("file_panel.rs", include_str!("../src/file_panel.rs")),
        (
            "session_history.rs",
            include_str!("../src/session_history.rs"),
        ),
    ];
    let n = consumers
        .iter()
        .filter(|(_, src)| src.contains("VirtualList::new("))
        .count();
    assert!(n >= 2, "VirtualList 消费者应 ≥ 2（实测 {n}）");
}
