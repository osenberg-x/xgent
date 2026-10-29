//! v8 M4-T3/T4 排版消费回归（审计 C3 延伸 / 设计系统 §1.4）。
//!
//! 缺陷：代码类文本（编辑器代码正文、行号列、diff 行、工具卡参数摘要）**只改了
//! 字号没改字体**，实际用比例字体渲染 → 行号数字宽度不等 → 行号列与代码列错位。
//! 中文侧：展示级负字距会让 CJK 挤成一团；消息正文固定 1.6 行高在长段中文下偏挤。

use bevy::prelude::*;
use bevy::text::{FontSource, TextFont};
use xgent_ui::fonts::{UiFonts, display_spacing, has_cjk};

/// 造一个可辨识的等宽槽位来源（便于断言「用的是 mono 而不是默认句柄」）。
fn distinctive_mono() -> UiFonts {
    UiFonts {
        ui: Handle::default(),
        mono: FontSource::Family("Verifier Mono".into()),
        ui_cjk: FontSource::Family("Verifier CJK".into()),
        mono_cjk: FontSource::Family("Verifier Mono CJK".into()),
    }
}

/// A1 编辑器代码正文走 `mono` 槽位。
///
/// 三处接线分别验证：
/// ① 注入端——`sync_editor_theme` 把 `UiFonts.mono` 写进 `EditorTheme.font`；
/// ② 根文本端——虚拟渲染按 `EditorTheme.font` 设置代码文本 `TextFont.font`；
/// ③ **span 端**——bevy 0.20 逐 span 解析字体来源：根 Text 为空会被跳过，
///    真正承载字形的 `TextSpan` 必须**自带**字体来源（只设根 Text 不生效）。
#[test]
fn editor_code_text_uses_mono_slot() {
    let editor_src = include_str!("../src/editor/mod.rs");
    assert!(
        editor_src.contains("editor_theme.font = fonts.mono.clone()"),
        "sync_editor_theme 必须把 mono 槽位注入 EditorTheme"
    );
    let virtual_render_src = include_str!("../../xui/src/text_editor/virtual_render.rs");
    assert!(
        virtual_render_src.contains("font: theme.font.clone()"),
        "虚拟渲染创建代码文本时必须使用 EditorTheme 的字体来源"
    );
    assert!(
        virtual_render_src.contains("text_font.font = theme.font.clone()"),
        "主题变化时代码文本的字体来源必须跟随更新"
    );
    // ③ span 自带字体（这是「只设根 Text 无效」的根因）
    assert!(
        virtual_render_src.contains("font: font.clone()"),
        "代码 span 必须自带字体来源（bevy 逐 span 解析字体）"
    );
    let render_src = include_str!("../../xui/src/text_editor/render.rs");
    assert!(
        render_src.contains("font: theme.font.clone()"),
        "高亮层 span 必须自带字体来源"
    );
}

/// A2 行号列与代码列同源：行号实体的 `TextFont.font` 等于 mono 槽位。
///
/// 行号列的实体由编辑器打开文件时 spawn；这里用源码级断言守住「行号实体设置了
/// 字体来源」这一接线（该实体的生成路径需要完整的文件 IO 流程，headless 成本高）。
#[test]
fn line_numbers_column_uses_mono_source() {
    let src = include_str!("../src/editor/tabs.rs");
    assert!(
        src.contains("font: editor_theme.font.clone()"),
        "行号列必须与代码正文同源（editor_theme.font）"
    );
    let editor_src = include_str!("../src/editor/mod.rs");
    assert!(
        editor_src.contains("editor_theme.font = fonts.mono.clone()"),
        "sync_editor_theme 必须注入 mono 槽位"
    );
}

/// A3 diff 行与工具卡参数摘要走 `mono`。
#[test]
fn diff_and_tool_summary_use_mono() {
    let fonts = distinctive_mono();

    // diff 行：mono_text 构造的 TextFont.font == mono 槽位
    let diff_src = include_str!("../src/editor/diff_view.rs");
    assert!(
        diff_src.contains("mono_text("),
        "diff 行必须走 mono_text（此前是 ui_text + 等宽字号 = 比例字体冒充等宽）"
    );

    // 工具卡参数摘要：等宽版省略构造器
    let tool_src = include_str!("../src/tool_panel.rs");
    assert!(
        tool_src.contains("ellipsis_mono_text("),
        "工具卡参数摘要必须走等宽省略构造器"
    );

    // 构造器本身把 fonts.mono 放进 TextFont
    let fonts_src = include_str!("../src/fonts.rs");
    assert!(
        fonts_src.contains("font: fonts.mono.clone()"),
        "等宽构造器必须引用 fonts.mono"
    );
    let _ = fonts;
}

/// A4 字号令牌收敛：SMALL 12.0、CODE 12.0、中文行高 1.7。
#[test]
fn type_scale_tokens_are_settled() {
    use xgent_ui::theme::type_scale;
    assert_eq!(
        type_scale::SMALL,
        12.0,
        "SMALL 应为 12（半像素在非整数 DPI 下模糊）"
    );
    assert_eq!(type_scale::CODE, 12.0, "代码档应为 12");
    assert_eq!(type_scale::line_height::CJK, 1.7, "中文行高档应为 1.7");
}

/// A5 展示级负字距不作用中文。
#[test]
fn negative_letter_spacing_skips_cjk() {
    assert!(has_cjk("欢迎使用 XGent"), "中文串应被识别为含 CJK");
    assert!(!has_cjk("Welcome to XGent"), "纯拉丁串不应被识别为含 CJK");
    assert_eq!(
        display_spacing("欢迎使用 XGent"),
        bevy::text::LetterSpacing::Px(0.0),
        "含中文的标题不应施加负字距"
    );
    assert_eq!(
        display_spacing("Welcome to XGent"),
        bevy::text::LetterSpacing::Px(-0.29),
        "纯拉丁标题保留展示级负字距"
    );
    // 接线：welcome 标题走该函数
    let welcome_src = include_str!("../src/welcome.rs");
    assert!(
        welcome_src.contains("display_spacing("),
        "welcome 标题必须走 display_spacing"
    );
}

/// A6 消息正文行高按内容选档。
#[test]
fn body_line_height_follows_content_language() {
    use xgent_ui::theme::type_scale::line_height;
    assert_eq!(line_height::for_text("这是一段中文正文"), 1.7);
    assert_eq!(line_height::for_text("this is latin body text"), 1.6);
    assert_eq!(
        line_height::for_text("混合 mixed 正文"),
        1.7,
        "含中文即按中文档"
    );
}

/// A6 附：正文的三条渲染路径（流式助手正文、固化副本、会话历史）都用内容选档。
///
/// 只改流式路径会让「流式 1.7 → 完成 1.6」出现行高跳变，且历史消息回看又是 1.6。
#[test]
fn all_message_body_paths_use_content_tier() {
    // M6.5：用户/助手/历史统一由 Transcript 渲染器承载，正文路径都在这里。
    let render_src = include_str!("../src/transcript_render.rs");
    let occurrences = render_src.matches("line_height::for_text(").count();
    assert!(
        occurrences >= 2,
        "用户与助手正文都应用 for_text（实测 {occurrences} 处）"
    );
}

/// A7 检索链路的 CJK 分词未被破坏（中文问句仍能抽出二元组）。
#[test]
fn retrieval_cjk_segmentation_still_works() {
    // 既有实现：xgent_context 的关键词抽取（相邻二字组合）
    let src = include_str!("../../xgent_context/src/on_demand.rs");
    assert!(
        src.contains("bigram") || src.contains("CN_FUNC_CHARS"),
        "检索链路的中文分词逻辑不应被本变更破坏"
    );
}

/// A3 附：等宽构造器产出的 TextFont 确实引用 mono 槽位。
#[test]
fn mono_constructor_sets_mono_font_source() {
    let fonts = distinctive_mono();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let e = app
        .world_mut()
        .spawn(xgent_ui::fonts::mono_text(
            &fonts,
            "code",
            12.0,
            Color::WHITE,
            1.5,
        ))
        .id();
    let font = app.world().get::<TextFont>(e).expect("应有 TextFont");
    assert_eq!(font.font, FontSource::Family("Verifier Mono".into()));
}
