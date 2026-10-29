//! @ 内联补全：输入 @ 触发补全浮层，方向键导航，Enter 接受。
//!
//! M6-T4：浮层挂 DROPDOWN 层（避免被 ChatPanel 的 clip 裁掉）。

use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use crate::fonts::ui_text;
use crate::theme::{Theme, type_scale};

/// 补全候选项。
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionCandidate {
    /// 显示文本。
    pub label: String,
    /// 插入文本（接受后插入的内容）。
    pub insert_text: String,
    /// 类型（文件/符号/selection）。
    pub kind: CompletionKind,
}

/// 补全类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// 文件。
    File,
    /// 符号（函数、类等）。
    Symbol,
    /// 选区。
    Selection,
}

/// 补全状态资源。
#[derive(Resource, Default)]
pub struct CompletionState {
    /// 是否激活。
    pub active: bool,
    /// 候选列表。
    pub candidates: Vec<CompletionCandidate>,
    /// 当前选中索引。
    pub selected_index: usize,
    /// 触发位置（输入框中的字符索引）。
    pub trigger_position: usize,
}

impl CompletionState {
    /// 激活补全。
    pub fn activate(&mut self, candidates: Vec<CompletionCandidate>, trigger_pos: usize) {
        self.active = true;
        self.candidates = candidates;
        self.selected_index = 0;
        self.trigger_position = trigger_pos;
    }

    /// 停用补全。
    pub fn deactivate(&mut self) {
        self.active = false;
        self.candidates.clear();
        self.selected_index = 0;
    }

    /// 向下选择。
    pub fn select_next(&mut self) {
        if !self.candidates.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.candidates.len();
        }
    }

    /// 向上选择。
    pub fn select_prev(&mut self) {
        if !self.candidates.is_empty() {
            self.selected_index = if self.selected_index == 0 {
                self.candidates.len() - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    /// 获取当前选中的候选项。
    pub fn selected(&self) -> Option<&CompletionCandidate> {
        self.candidates.get(self.selected_index)
    }
}

/// 补全浮层容器标记。
#[derive(Component)]
pub struct CompletionPopupMarker;

/// 补全候选项标记。
#[derive(Component)]
pub struct CompletionItemMarker {
    pub index: usize,
}

/// 渲染补全浮层。
pub fn spawn_completion_popup(
    commands: &mut Commands,
    state: &CompletionState,
    theme: &Theme,
    dropdown_layer: Entity,
    anchor: Vec2,
    input_size: Vec2,
    viewport: Vec2,
) -> Option<Entity> {
    if !state.active || state.candidates.is_empty() {
        return None;
    }

    let popup = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(anchor.x.min(viewport.x - 16.0)),
                top: Val::Px((anchor.y - 208.0).max(0.0)),
                width: Val::Px(
                    (input_size.x.max(240.0)).min(viewport.x - anchor.x.max(0.0) - 16.0),
                ),
                flex_direction: FlexDirection::Column,
                border_radius: BorderRadius::all(Val::Px(6.0)),
                padding: UiRect::all(Val::Px(4.0)),
                max_height: Val::Px(200.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(theme.elevated),
            BorderColor::all(theme.border),
            CompletionPopupMarker,
        ))
        .id();

    for (i, candidate) in state.candidates.iter().enumerate() {
        let is_selected = i == state.selected_index;
        let item = commands
            .spawn((
                Button,
                bevy::picking::hover::Hovered::default(),
                Node {
                    padding: UiRect::all(Val::Px(8.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(if is_selected {
                    theme.accent_bg
                } else {
                    Color::NONE
                }),
                CompletionItemMarker { index: i },
            ))
            .with_children(|parent| {
                parent.spawn(ui_text(
                    &candidate.label,
                    type_scale::BODY_SM,
                    400,
                    if is_selected {
                        theme.text
                    } else {
                        theme.text_dim
                    },
                    type_scale::line_height::UI,
                ));
            })
            .id();

        commands.entity(popup).add_child(item);
    }

    commands.entity(dropdown_layer).add_child(popup);
    Some(popup)
}

/// 处理候选项点击。
pub fn handle_completion_item_click(
    mut q: Query<(&CompletionItemMarker, &Pressed), Changed<Pressed>>,
    mut state: ResMut<CompletionState>,
    mut q_input: Query<&mut bevy::text::EditableText, With<crate::chat_panel::ChatInputMarker>>,
) {
    for (marker, _) in q.iter_mut() {
        if let Some(candidate) = state.candidates.get(marker.index).cloned() {
            if let Ok(mut editable) = q_input.single_mut() {
                accept_candidate(&mut editable, &state, &candidate);
            }
            state.deactivate();
        }
    }
}

/// 把选中候选替换掉输入框里的 `@query`。
///
/// 采用「整段重写 + queue_edit」而不是直接改 parley 选区：这是一条已被
/// 文本输入系统消费的路径，能保证 `EditableText` 的变更通知与重排都触发。
pub fn accept_candidate(
    editable: &mut bevy::text::EditableText,
    state: &CompletionState,
    candidate: &CompletionCandidate,
) {
    let text = editable.editor().text().to_string();
    let cursor = editable.editor().raw_selection().focus().index();
    let start = state.trigger_position.min(text.len());
    let end = cursor.min(text.len()).max(start);
    let mut new_text = String::with_capacity(text.len() + candidate.insert_text.len());
    new_text.push_str(&text[..start]);
    new_text.push_str(&candidate.insert_text);
    new_text.push_str(&text[end..]);
    editable.clear();
    editable.queue_edit(bevy::text::TextEdit::Insert(new_text.into()));
}

/// 补全接受事件（保留用于未来扩展）。
#[derive(Debug, Clone)]
pub struct CompletionAccepted {
    pub candidate: CompletionCandidate,
}

/// 检测输入中的 @ 触发。
pub fn detect_at_trigger(input_text: &str, cursor_pos: usize) -> Option<usize> {
    if cursor_pos == 0 {
        return None;
    }

    let bytes = input_text.as_bytes();
    let mut i = cursor_pos - 1;

    // 向前查找 @
    loop {
        if bytes[i] == b'@' {
            // 检查 @ 前面是否是空白或开头
            if i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\n' {
                return Some(i);
            }
            return None;
        }
        if bytes[i] == b' ' || bytes[i] == b'\n' {
            return None;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_state_navigation() {
        let mut state = CompletionState::default();
        state.activate(
            vec![
                CompletionCandidate {
                    label: "file1.rs".into(),
                    insert_text: "file1.rs".into(),
                    kind: CompletionKind::File,
                },
                CompletionCandidate {
                    label: "file2.rs".into(),
                    insert_text: "file2.rs".into(),
                    kind: CompletionKind::File,
                },
            ],
            0,
        );

        assert_eq!(state.selected_index, 0);
        state.select_next();
        assert_eq!(state.selected_index, 1);
        state.select_next();
        assert_eq!(state.selected_index, 0);
        state.select_prev();
        assert_eq!(state.selected_index, 1);
    }

    #[test]
    fn detect_at_trigger_basic() {
        assert_eq!(detect_at_trigger("@", 1), Some(0));
        assert_eq!(detect_at_trigger(" @", 2), Some(1));
        assert_eq!(detect_at_trigger("hello @", 7), Some(6));
        assert_eq!(detect_at_trigger("hello@", 6), None);
        assert_eq!(detect_at_trigger("test@", 5), None);
    }
}
