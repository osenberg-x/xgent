//! 命令面板（K-03）：Cmd+P / Cmd+Shift+P 风格的命令选择面板。
//!
//! xui 只提供面板状态与触发事件，**不执行命令逻辑**（handler 由调用方订阅
//! [`PaletteTriggered`] 实现），保持与业务解耦。
//!
//! 模糊匹配采用简单子串 + 大小写无关匹配（MVP）；后续可换 fzf 算法。

use bevy::prelude::*;

/// 命令种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// 文件类（如快速打开文件）
    File,
    /// 动作类（如执行某个命令）
    Action,
}

/// 命令定义。`label` 为已本地化字符串（由调用方提供，xui 不依赖 i18n）。
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    pub id: String,
    pub label: String,
    pub kind: CommandKind,
}

/// 命令注册表 Resource。
#[derive(Resource, Default, Debug)]
pub struct CommandRegistry {
    pub commands: Vec<PaletteCommand>,
}

impl CommandRegistry {
    /// 注册命令。
    pub fn register(&mut self, cmd: PaletteCommand) {
        self.commands.push(cmd);
    }

    /// 按 id 移除命令。
    pub fn remove(&mut self, id: &str) {
        self.commands.retain(|c| c.id != id);
    }
}

/// 命令面板状态 Resource。
#[derive(Resource, Debug, Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    /// 匹配的命令下标列表（在 `CommandRegistry.commands` 中的下标）。
    pub filtered: Vec<usize>,
}

impl CommandPaletteState {
    /// 打开面板并清空查询。
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    /// 关闭面板。
    pub fn close(&mut self) {
        self.open = false;
    }

    /// 向上选择（回绕）。
    pub fn select_prev(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.filtered.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// 向下选择（回绕）。
    pub fn select_next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
    }
}

/// 命令被选中触发事件。调用方订阅此事件执行业务逻辑。
#[derive(Message, Debug, Clone)]
pub struct PaletteTriggered {
    pub command_id: String,
}

/// 命令面板插件。
pub struct CommandPalettePlugin;

impl Plugin for CommandPalettePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PaletteTriggered>()
            .init_resource::<CommandRegistry>()
            .init_resource::<CommandPaletteState>()
            .add_systems(Update, filter_commands);
    }
}

/// query 变化时重新做模糊匹配，更新 `filtered`。
pub fn filter_commands(mut state: ResMut<CommandPaletteState>, registry: Res<CommandRegistry>) {
    // 若未打开，无需过滤
    if !state.open {
        return;
    }
    // 简化为每帧重算（MVP，命令数有限）
    let q = state.query.to_lowercase();
    let filtered: Vec<usize> = registry
        .commands
        .iter()
        .enumerate()
        .filter(|(_, c)| q.is_empty() || c.label.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect();
    state.filtered = filtered;
    if state.selected >= state.filtered.len() && !state.filtered.is_empty() {
        state.selected = 0;
    }
}

/// 用当前选中项触发命令（供调用方在 UI 按键处理中调用）。
pub fn trigger_selected(
    state: &CommandPaletteState,
    registry: &CommandRegistry,
    writer: &mut MessageWriter<PaletteTriggered>,
) {
    let Some(&idx) = state.filtered.get(state.selected) else {
        return;
    };
    let Some(cmd) = registry.commands.get(idx) else {
        return;
    };
    writer.write(PaletteTriggered {
        command_id: cmd.id.clone(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(id: &str, label: &str) -> PaletteCommand {
        PaletteCommand {
            id: id.into(),
            label: label.into(),
            kind: CommandKind::Action,
        }
    }

    #[test]
    fn filter_matches_substring_case_insensitive() {
        let mut reg = CommandRegistry::default();
        reg.register(cmd("open", "Open File"));
        reg.register(cmd("save", "Save All"));
        reg.register(cmd("term", "Toggle Terminal"));
        let mut state = CommandPaletteState {
            open: true,
            query: "file".into(),
            ..Default::default()
        };
        let filtered = compute_filtered(&state, &reg);
        assert_eq!(filtered, vec![0]);

        state.query = "T".into();
        let filtered = compute_filtered(&state, &reg);
        // "Toggle Terminal" 与 "Open File" 都不含小写 't'？contains 是小写比较
        // "toggle terminal".contains("t") = true, "open file".contains("t")=false, "save all"=false
        assert_eq!(filtered, vec![2]);

        state.query.clear();
        let filtered = compute_filtered(&state, &reg);
        assert_eq!(filtered, vec![0, 1, 2]);
    }

    #[test]
    fn select_prev_next_wraps() {
        let mut state = CommandPaletteState {
            filtered: vec![0, 1, 2],
            ..Default::default()
        };
        state.select_next();
        assert_eq!(state.selected, 1);
        state.select_next();
        state.select_next();
        assert_eq!(state.selected, 0); // 回绕
        state.select_prev();
        assert_eq!(state.selected, 2); // 回绕
    }

    fn compute_filtered(state: &CommandPaletteState, reg: &CommandRegistry) -> Vec<usize> {
        let q = state.query.to_lowercase();
        reg.commands
            .iter()
            .enumerate()
            .filter(|(_, c)| q.is_empty() || c.label.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }
}
