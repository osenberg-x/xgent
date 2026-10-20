//! 拾取白名单（v8 M1）。
//!
//! `UiPickingSettings { require_markers: true }` 之后，没有 `Pickable` 的节点对
//! 拾取完全惰性——层根与布局容器因此不再吞点击。代价是可交互叶子必须显式带
//! `Pickable`，否则点不动。本模块用一条 `Added` 系统统一补齐，避免 50+ 处
//! spawn 点逐个手写。

use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Button;

/// 拾取白名单插件。
pub struct PickablePlugin;

impl Plugin for PickablePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach_pickable_to_interactive);
    }
}

/// 为所有可交互叶子补 `Pickable`（默认 block + hoverable）。
///
/// 覆盖 `Button`、文本输入与已有 `Interaction` 的节点；层根与装饰节点不在
/// 查询范围内，因此保持惰性。
fn attach_pickable_to_interactive(
    mut commands: Commands,
    q: Query<
        Entity,
        (
            Or<(With<Button>, With<EditableText>)>,
            Without<Pickable>,
        ),
    >,
) {
    for entity in &q {
        commands.entity(entity).insert(Pickable::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_get_pickable_but_plain_nodes_do_not() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(PickablePlugin);

        let button = app.world_mut().spawn(Button).id();
        let plain = app.world_mut().spawn(Node::default()).id();

        app.update();

        assert!(app.world().get::<Pickable>(button).is_some());
        assert!(app.world().get::<Pickable>(plain).is_none());
    }
}
