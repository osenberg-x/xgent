//! 编辑器主题同步测试：验证 xgent_ui `Theme` 的字号/颜色同步到 xui `EditorTheme`。
//!
//! 设计意图（对齐 zed 编辑器字号模型）：编辑器正文字号应跟随单一可配字号源
//! （`Theme::font_size`），未来 `Theme` 接入 settings 后即可「跟随系统/用户偏好」
//! 调整编辑器字号，无需改 xui。

use bevy::prelude::*;
use xgent_ui::editor::EditorPlugin;
use xgent_ui::theme::Theme;
use xui::text_editor::render::EditorTheme;

fn make_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::input_focus::InputFocusPlugin)
        .add_plugins(xui::TextEditorPlugin)
        .add_plugins(xgent_ui::layout::LayoutPlugin)
        .add_plugins(EditorPlugin)
        .init_resource::<Theme>()
        .init_resource::<xgent_settings::Localizer>();
    // 缺 ProjectRoot 等 app 级资源的系统在 minimal 集下会报错，
    // 降级为 warn 不影响本测试关注的 EditorTheme 同步逻辑。
    app.set_error_handler(bevy::ecs::error::warn);
    app
}

/// 改 `Theme::font_size` 后，`EditorTheme::font_size` 应同步更新。
#[test]
fn editor_theme_follows_theme_font_size() {
    let mut app = make_app();
    app.update();

    // 首帧已同步：EditorTheme.font_size == Theme.font_size (14.0)
    {
        let editor_theme = app.world().resource::<EditorTheme>();
        assert_eq!(editor_theme.font_size, 14.0, "初始应同步 Theme 默认 14.0");
        assert_eq!(editor_theme.line_height_ratio, 1.5, "行高比默认 1.5");
    }

    // 改 Theme 字号 → EditorTheme 跟随
    app.world_mut().resource_mut::<Theme>().font_size = 12.0;
    app.update();
    {
        let editor_theme = app.world().resource::<EditorTheme>();
        assert_eq!(
            editor_theme.font_size, 12.0,
            "EditorTheme 应跟随 Theme.font_size 变化"
        );
    }
}

/// 改 `Theme` 的文本色后，`EditorTheme` 的 text/text_dim 应同步。
#[test]
fn editor_theme_follows_theme_colors() {
    let mut app = make_app();
    app.update();

    let new_text = Color::srgb(0.1, 0.2, 0.3);
    let new_dim = Color::srgb(0.4, 0.5, 0.6);
    {
        let mut theme = app.world_mut().resource_mut::<Theme>();
        theme.text = new_text;
        theme.text_dim = new_dim;
    }
    app.update();
    let editor_theme = app.world().resource::<EditorTheme>();
    assert_eq!(editor_theme.text, new_text, "text 应同步");
    assert_eq!(editor_theme.text_dim, new_dim, "text_dim 应同步");
}
