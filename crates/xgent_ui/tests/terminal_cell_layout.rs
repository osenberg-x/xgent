//! UI v8 M4-T2 终端双宽单元格回归。
//!
//! 覆盖两件事：
//! 1. **等宽度量是实测的**（A1）：启动期探针走 bevy 文本管线，`MonoMetrics` 的四个
//!    字段由排版结果反推，而不是写死的系数。
//! 2. **终端按格定位**（A5）：行内 run 节点的 `left` 严格等于「起始列 × 单元格宽」。

use bevy::prelude::*;
use xgent_settings::Localizer;
use xgent_ui::editor::SideViewContent;
use xgent_ui::layout::LayoutPlugin;
use xgent_ui::terminal::TerminalPlugin;
use xgent_ui::terminal::io::TerminalOutputChunk;
use xgent_ui::terminal::metrics::{CellGrid, MonoMetrics};
#[cfg(not(feature = "legacy-terminal-layout"))]
use xgent_ui::terminal::output::OutputCellRun;
use xgent_ui::terminal::output::RenderHistory;
use xgent_ui::terminal::{TerminalTab, TerminalTabStatus, TerminalTabs};
use xgent_ui::theme::type_scale;
use xui::i18n_bridge::Strings;
use xui::scrollbar::Scrollbar;
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
    // Startup 会 spawn 相机（layout::spawn_layout）。跑一帧后给它一个非零的
    // 渲染目标——headless 环境没有渲染管线，相机视口恒 0 → UI 视口恒 0 →
    // 所有节点尺寸为 0（台账 §10 勘误 15 记的同一个坑）。
    app.update();
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Camera>();
        for mut camera in q.iter_mut(world) {
            camera.computed.target_info = Some(bevy::camera::RenderTargetInfo {
                physical_size: UVec2::new(1280, 720),
                scale_factor: 1.0,
            });
        }
    }
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

/// 造一个 tab 并喂入一行输出，返回 tab 实体。
fn feed_output(app: &mut App, bytes: &[u8]) -> bevy::ecs::entity::Entity {
    let tab = app
        .world_mut()
        .spawn((
            RenderHistory::default(),
            TerminalTab {
                pty_id: None,
                title: "test".into(),
                status: TerminalTabStatus::Running,
                shell: xgent_terminal::ShellSpec::FromEnv,
                cwd: std::env::temp_dir(),
                exit_code: None,
            },
        ))
        .id();
    app.world_mut().resource_mut::<TerminalTabs>().open(tab);
    app.world_mut().write_message(TerminalOutputChunk {
        tab,
        bytes: bytes.to_vec(),
    });
    tab
}

/// 终端行内 run 节点必须按「起始列 × 单元格宽」定位（A5）。
///
/// 旧路径（cargo feature `legacy-terminal-layout`）不 spawn `OutputCellRun`，
/// 本用例只对按格定位路径有意义。
#[cfg(not(feature = "legacy-terminal-layout"))]
#[test]
fn terminal_runs_are_positioned_by_cell() {
    let mut app = terminal_app();
    settle(&mut app);
    let cell_w = app.world().resource::<MonoMetrics>().cell_width();

    // "ab中cd"：a(0) b(1) 中(2) c(4) d(5)
    feed_output(&mut app, "ab\u{4e2d}cd\n".as_bytes());
    app.update();

    // 收集输出容器内所有按格定位的 run（按起始列排序）
    let mut runs: Vec<(usize, usize, Val)> = {
        let world = app.world_mut();
        let mut q = world.query::<(&OutputCellRun, &Node)>();
        q.iter(world)
            .map(|(run, node)| (run.start_col, run.cells, node.left))
            .collect()
    };
    runs.sort_by_key(|(col, _, _)| *col);
    assert!(
        runs.len() >= 3,
        "「ab中cd」应切出至少 3 个 run（窄字符合并 + 宽字符独立），实得 {}",
        runs.len()
    );

    // 起始列按累计格数推进：0 / 2 / 4
    assert_eq!(runs[0].0, 0, "首个 run 应从第 0 格开始");
    assert_eq!(runs[1].0, 2, "宽字符之后应跳到第 2 格");
    assert_eq!(runs[2].0, 4, "宽字符占 2 格，后续应到第 4 格");

    // 每个 run 的 left == 起始列 × cell_w
    for (col, _cells, left) in &runs {
        let Val::Px(px) = left else {
            panic!("run 的 left 应为绝对像素，实得 {left:?}");
        };
        let expected = *col as f32 * cell_w;
        assert!(
            (px - expected).abs() < 0.01,
            "run（起始列 {col}）的 left 应为 {expected}px，实得 {px}px"
        );
    }
}

/// 给 PTY 的列数必须来自实测 cell_w，而不是「字号 × 0.6」估算（A6）。
#[test]
fn pty_cols_come_from_measured_cell_width() {
    let mut app = terminal_app();
    settle(&mut app);
    let metrics = *app.world().resource::<MonoMetrics>();

    // 切到终端视图（resize 只在终端激活时跟踪；分屏展开与布局需要几帧）
    *app.world_mut().resource_mut::<SideViewContent>() = SideViewContent::Terminal;
    feed_output(&mut app, b"ready\n");
    for _ in 0..6 {
        app.update();
    }

    // 视口可用宽 = 输出容器内容盒宽 − 滚动条占位
    let (content_width, scrollbar_width) = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<
            (&ComputedNode, Option<&Scrollbar>),
            With<xgent_ui::terminal::TerminalOutputMarker>,
        >();
        let (node, bar) = q.single(world).expect("终端输出容器应存在");
        (
            node.content_box().size().x * node.inverse_scale_factor(),
            bar.map(|b| b.width).unwrap_or(0.0),
        )
    };
    assert!(
        content_width > 0.0,
        "headless 下需给相机一个非零渲染目标，否则 UI 视口恒 0"
    );

    let grid = *app.world().resource::<CellGrid>();
    assert!(grid.cell_w > 0.0, "单元格宽应来自实测度量");
    assert!(
        (grid.cell_w - metrics.cell_width()).abs() < 1e-4,
        "网格的单元格宽应等于实测度量"
    );
    let expected_cols = ((content_width - scrollbar_width) / grid.cell_w).floor() as usize;
    assert_eq!(
        grid.cols, expected_cols,
        "列数应为 floor(视口逻辑宽 / cell_w) = {expected_cols}，实得 {}",
        grid.cols
    );
    assert!(grid.rows >= 1, "行数应为正：{grid:?}");
}

/// 输出行的行高用实测行高（不再是「字号 × 1.4」估算）。
#[test]
fn output_line_height_uses_measured_line_height() {
    let mut app = terminal_app();
    settle(&mut app);
    let metrics = *app.world().resource::<MonoMetrics>();

    feed_output(&mut app, b"hello\n");
    app.update();

    let world = app.world_mut();
    let mut q = world.query::<&Node>();
    let heights: Vec<Val> = q
        .iter(world)
        .filter(|node| matches!(node.height, Val::Px(_)))
        .map(|node| node.height)
        .collect();
    assert!(
        heights.contains(&Val::Px(metrics.line_height)),
        "输出行高应等于实测行高 {}px，实得 {heights:?}",
        metrics.line_height
    );
}

/// 旧的列宽估算（字号 × 0.6 / × 1.4）必须已删除（A6）。
#[test]
fn legacy_column_width_estimate_is_removed() {
    let src = include_str!("../src/terminal/output.rs");
    assert!(
        !src.starts_with('\u{feff}'),
        "output.rs 不应带 UTF-8 BOM（会污染首行的注释过滤）"
    );
    // 去掉注释行：文档注释里会提到「被删除的系数」，不能误伤。
    let code: String = src
        .lines()
        .map(|line| line.trim_start().trim_start_matches('\u{feff}'))
        .filter(|line| !line.starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("* 0.6"),
        "output.rs 仍残留「字号 × 0.6」估算"
    );
    assert!(
        !code.contains("* 1.4"),
        "output.rs 仍残留「字号 × 1.4」估算"
    );
    assert!(
        !code.contains("theme.font_size"),
        "output.rs 不应再用界面正文字号（theme.font_size）估算终端尺寸"
    );
}

/// run 不得在 **run 内**折行（D-6：UI 侧不二次换行，超出视口裁掉）。
///
/// 绝对定位节点若被约束宽度，靠右的 run 会把文本折成两行、行高超出固定的
/// `line_h` 压到下一行。本用例把这条不变量钉住（当前实现靠 taffy 对绝对定位
/// 子节点的 max-content 定宽 + run 上的 `NoWrap` 双重保证）。
///
/// 旧路径（cargo feature `legacy-terminal-layout`）不 spawn `OutputCellRun`，
/// 本用例只对按格定位路径有意义。
#[cfg(not(feature = "legacy-terminal-layout"))]
#[test]
fn long_run_does_not_wrap_inside_the_run() {
    let mut app = terminal_app();
    settle(&mut app);
    let metrics = *app.world().resource::<MonoMetrics>();

    // 切到终端视图（输出容器需有真实尺寸才有意义）
    *app.world_mut().resource_mut::<SideViewContent>() = SideViewContent::Terminal;
    // 「中」占 2 格，其后 200 个 ASCII 字符（≈1320px）远超输出容器内容盒宽
    // （≈700px）——第二个 run 的 left 非零，正是容易在 run 内折行的位置。
    let long = "x".repeat(200);
    feed_output(&mut app, format!("中{long}\n").as_bytes());
    for _ in 0..4 {
        app.update();
    }

    let world = app.world_mut();
    let mut q = world.query::<(&OutputCellRun, &ComputedNode)>();
    let heights: Vec<f32> = q.iter(world).map(|(_, node)| node.size().y).collect();
    assert!(heights.len() >= 2, "应渲染出「中」与长串两个 run");
    assert!(
        heights.iter().all(|h| *h <= metrics.line_height + 1.0),
        "run 高度应保持单行（≤ {:.1}px），实得 {heights:?} —— 文本在 run 内折行了",
        metrics.line_height
    );
}
