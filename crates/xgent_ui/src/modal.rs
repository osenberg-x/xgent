//! 模态栈：Esc 与模态 z 的唯一权威（v8 M1）。
//!
//! 规则：
//! - 入栈即独占键盘：栈非空时全局快捷键（含 `chat.abort`）不派发到底层。
//! - 栈顶决定 z：`MODAL_BASE + depth * MODAL_STRIDE`。
//! - Esc 只作用于栈顶，是否关闭由 [`Dismiss`] 决定；具体关闭动作由持有者
//!   监听 [`ModalDismissRequested`] 后自行执行（例如设置面板把 `open` 置 false）。
//! - 出栈时把 `InputFocus` 还原到该层打开前的实体。

use bevy::input::ButtonInput;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;

use crate::theme::z;

/// 模态的关闭策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dismiss {
    /// 仅 Esc 可关。
    EscOnly,
    /// Esc 与点击遮罩均可关。
    EscAndScrim,
    /// 不可由通用路径关闭（持有者自行处理键盘，例如确认弹窗的 Esc=拒绝）。
    NotDismissable,
}

/// 栈内一层。
#[derive(Debug, Clone, Copy)]
pub struct ModalLayer {
    /// 稳定标识，如 `"settings"` / `"confirm"`。
    pub id: &'static str,
    /// 该层根实体。
    pub entity: Entity,
    /// 关闭策略。
    pub dismiss: Dismiss,
    /// 打开前的焦点实体，出栈时还原。
    pub opener_focus: Option<Entity>,
}

/// 模态栈。
#[derive(Resource, Debug, Default)]
pub struct ModalStack {
    layers: Vec<ModalLayer>,
}

impl ModalStack {
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// 栈深。
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// 栈顶。
    pub fn top(&self) -> Option<&ModalLayer> {
        self.layers.last()
    }

    /// 指定 id 是否在栈内。
    pub fn contains(&self, id: &str) -> bool {
        self.layers.iter().any(|layer| layer.id == id)
    }

    /// 只读视图。
    pub fn layers(&self) -> &[ModalLayer] {
        &self.layers
    }
}

/// 请求入栈。
#[derive(Message, Debug, Clone, Copy)]
pub struct ModalPush {
    /// 稳定标识。
    pub id: &'static str,
    /// 该层根实体。
    pub entity: Entity,
    /// 关闭策略。
    pub dismiss: Dismiss,
    /// 打开前的焦点实体。
    pub opener_focus: Option<Entity>,
}

/// 请求出栈；`id = None` 表示栈顶。
#[derive(Message, Debug, Clone, Copy)]
pub struct ModalPop {
    /// 目标层；`None` 为栈顶。
    pub id: Option<&'static str>,
}

/// 通用 dismiss 请求：持有者收到后自行关闭，并在关闭路径发 [`ModalPop`]。
#[derive(Message, Debug, Clone, Copy)]
pub struct ModalDismissRequested {
    /// 目标层 id。
    pub id: &'static str,
}

/// 栈顶变化消息。
#[derive(Message, Debug, Clone, Copy)]
pub struct ModalTopChanged {
    /// 新栈顶；空栈为 `None`。
    pub top: Option<&'static str>,
}

/// 本帧 Esc 已被模态栈消费，快捷键分发应整体让位。
#[derive(Resource, Debug, Default)]
pub struct ModalEscConsumed(pub bool);

/// 模态栈插件。
pub struct ModalPlugin;

impl Plugin for ModalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModalStack>()
            .init_resource::<ModalEscConsumed>()
            .add_message::<ModalPush>()
            .add_message::<ModalPop>()
            .add_message::<ModalDismissRequested>()
            .add_message::<ModalTopChanged>()
            .add_systems(Last, clear_esc_consumed)
            .add_systems(
                Update,
                (route_modal_input, apply_modal_pop, apply_modal_push)
                    .chain()
                    .before(crate::shortcuts::handle_hotkey_triggers),
            );
    }
}

/// Esc → 只作用于栈顶；允许关闭时发 [`ModalDismissRequested`]。
fn route_modal_input(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    stack: Res<ModalStack>,
    mut dismiss: MessageWriter<ModalDismissRequested>,
    mut consumed: ResMut<ModalEscConsumed>,
) {
    let Some(keys) = keys else {
        return;
    };
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let Some(top) = stack.top() else {
        return;
    };
    consumed.0 = true;
    if matches!(top.dismiss, Dismiss::EscOnly | Dismiss::EscAndScrim) {
        dismiss.write(ModalDismissRequested { id: top.id });
    }
}

fn apply_modal_push(
    mut commands: Commands,
    mut reader: MessageReader<ModalPush>,
    mut stack: ResMut<ModalStack>,
    mut top_changed: MessageWriter<ModalTopChanged>,
) {
    for push in reader.read() {
        if stack.contains(push.id) {
            continue;
        }
        if stack.len() >= z::MODAL_MAX_DEPTH {
            tracing::warn!(
                "模态栈已达上限 {}，拒绝入栈 {}",
                z::MODAL_MAX_DEPTH,
                push.id
            );
            continue;
        }
        let depth = stack.len() as i32;
        commands
            .entity(push.entity)
            .insert((
                GlobalZIndex(z::MODAL_BASE + depth * z::MODAL_STRIDE),
                // v8 M7-T2：模态层根挂 TabGroup::modal()，Tab 键不跳出模态
                TabGroup::modal(),
            ));
        stack.layers.push(ModalLayer {
            id: push.id,
            entity: push.entity,
            dismiss: push.dismiss,
            opener_focus: push.opener_focus,
        });
        top_changed.write(ModalTopChanged { top: Some(push.id) });
    }
}

fn apply_modal_pop(
    mut reader: MessageReader<ModalPop>,
    mut stack: ResMut<ModalStack>,
    mut focus: ResMut<InputFocus>,
    mut top_changed: MessageWriter<ModalTopChanged>,
) {
    for pop in reader.read() {
        let index = match pop.id {
            Some(id) => stack.layers.iter().rposition(|layer| layer.id == id),
            None => stack.layers.len().checked_sub(1),
        };
        let Some(index) = index else {
            continue;
        };
        let layer = stack.layers.remove(index);
        if let Some(opener) = layer.opener_focus {
            focus.set(opener, FocusCause::Navigated);
        } else {
            focus.clear();
        }
        top_changed.write(ModalTopChanged {
            top: stack.top().map(|layer| layer.id),
        });
    }
}

fn clear_esc_consumed(mut consumed: ResMut<ModalEscConsumed>) {
    consumed.0 = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_push_pop_and_depth_z() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(ModalPlugin)
            .init_resource::<InputFocus>();

        let first = app.world_mut().spawn_empty().id();
        let second = app.world_mut().spawn_empty().id();

        app.world_mut().write_message(ModalPush {
            id: "settings",
            entity: first,
            dismiss: Dismiss::EscAndScrim,
            opener_focus: None,
        });
        app.update();
        assert_eq!(
            app.world().get::<GlobalZIndex>(first).map(|index| index.0),
            Some(z::MODAL_BASE)
        );

        app.world_mut().write_message(ModalPush {
            id: "confirm",
            entity: second,
            dismiss: Dismiss::NotDismissable,
            opener_focus: None,
        });
        app.update();
        assert_eq!(
            app.world().get::<GlobalZIndex>(second).map(|index| index.0),
            Some(z::MODAL_BASE + z::MODAL_STRIDE)
        );
        assert_eq!(app.world().resource::<ModalStack>().len(), 2);

        app.world_mut().write_message(ModalPop {
            id: Some("settings"),
        });
        app.update();
        assert_eq!(app.world().resource::<ModalStack>().len(), 1);
        assert!(app.world().resource::<ModalStack>().contains("confirm"));
    }
}
