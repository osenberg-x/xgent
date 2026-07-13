//! 文件面板：项目文件树预览 + 当前文件内容（MVP 只读）。
//!
//! MVP 仅展示一个占位提示；文件树遍历与内容读取留待后续接入 xgent_context 的目录树逻辑。

use bevy::prelude::*;
use xgent_settings::Localizer;

use crate::i18n::tr;
use crate::layout::FilePanelMarker;
use crate::theme::Theme;

/// 文件面板插件。
pub struct FilePanelPlugin;

impl Plugin for FilePanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_file_panel);
    }
}

/// 启动时在文件面板内 spawn 占位提示。
fn spawn_file_panel(
    mut commands: Commands,
    q: Query<Entity, With<FilePanelMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
) {
    let Ok(entity) = q.single() else {
        return;
    };
    let font = theme.font_size;
    commands.entity(entity).with_children(|p| {
        p.spawn((
            Node { ..default() },
            Text::new(tr(&loc, "file-panel-placeholder")),
            TextFont {
                font_size: FontSize::Px(font),
                ..default()
            },
            TextColor(theme.text_dim),
        ));
    });
}
