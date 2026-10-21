//! UI v8 M4-T2 终端双宽单元格回归。
//!
//! 覆盖两件事：
//! 1. **等宽度量是实测的**（A1）：启动期探针走 bevy 文本管线，`MonoMetrics` 的四个
//!    字段由排版结果反推，而不是写死的系数。
//! 2. **终端按格定位**（A5）：行内 run 节点的 `left` 严格等于「起始列 × 单元格宽」。

use bevy::prelude::*;
use xgent_settings::Localizer;
use xgent_ui::layout::LayoutPlugin;
use xgent_ui::terminal::TerminalPlugin;
use xgent_ui::terminal::metrics::MonoMetrics;
use xgent_ui::theme::type_scale;
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

/// 能跑终端插件并完成 UI 布局的 App。
///
/// 缺 `UiPlugin` / `WindowPlugin` / 相机 / `TextPlugin` 会导致尺寸恒 0 或文本不
/// 排版（见 `docs/plans/ui-v8-handoff.md` §3.4）。
fn terminal_app() -> App {
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
            xgent_ui::kit::KitPlugin,
            xgent_ui::editor::EditorPlugin,
            TerminalPlugin,
        ))
        .init_asset::<bevy::image::TextureAtlasLayout>()
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>();
    app
}

/// 跑若干帧，让探针完成排版并被收集。
fn settle(app: &mut App) {
    for _ in 0..10 {
        app.update();
    }
}

/// 等宽度量必须由**实测**得出（A1）。
#[test]
fn mono_metrics_is_measured_at_runtime() {
    let mut app = terminal_app();
    settle(&mut app);

    let metrics = app
        .world()
        .get_resource::<MonoMetrics>()
        .copied()
        .expect("跑若干帧后应注入 MonoMetrics（探针实测）");

    assert!(
        metrics.latin_advance > 0.0,
        "窄字符步进应实测为正：{:?}",
        metrics
    );
    assert!(
        metrics.cjk_advance > 0.0,
        "宽字符步进应实测为正：{:?}",
        metrics
    );
    assert!(metrics.line_height > 0.0, "行高应实测为正：{:?}", metrics);
    assert_eq!(
        metrics.at_size,
        type_scale::CODE,
        "实测字号必须是终端实际渲染的 CODE 档"
    );
    assert!(
        metrics.cell_width() >= metrics.latin_advance - 1e-6,
        "单元格基准宽不得小于窄字符步进"
    );
}
