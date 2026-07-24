//! 终端输出历史渲染：PTY 字节流 → vte 解析 → `RenderHistory` → UI。
//!
//! 详见 `doc/design/terminal-design.md` §3.4、§6。
//!
//! 行模型（非屏幕字符网格）：每个 tab 持一个 [`RenderHistory`]（`Vec<RenderLine>`），
//! PTY 字节经 `TerminalParser` 增量解析累积成行。MVP 渲染策略：把每行作为独立
//! Text 节点 spawn 进 `TerminalOutputMarker` 容器（非虚拟滚动——行数大时性能
//! 待优化，对齐 chat_panel 的「虚拟化留后续」策略）。历史上限 10k 行超丢头部。

use bevy::prelude::*;

use crate::terminal::io::{TerminalOutputChunk, TerminalResize};
use crate::terminal::{
    TerminalOutputMarker, TerminalStatusBarMarker, TerminalTab,
    TerminalTabStatus, TerminalTabs,
};
use crate::theme::{Theme, px};

use xgent_terminal::{RenderLine, TerminalParser};
/// 历史上限（行）。
const MAX_LINES: usize = 10_000;

/// 单个 tab 的输出历史（PTY 字节经 vte 解析后的行序列）。
#[derive(Component, Default)]
pub struct RenderHistory {
    pub lines: Vec<RenderLine>,
    /// 增量解析器（持有跨 feed 的未结束行状态）。
    pub parser: TerminalParser,
}

impl RenderHistory {
    /// 清空历史 + 重置解析器。
    pub fn clear(&mut self) {
        self.lines.clear();
        self.parser = TerminalParser::new();
    }

    /// 喂入 PTY 字节流，累积进 lines。
    pub fn feed(&mut self, bytes: &[u8]) {
        let new_lines = self.parser.feed(bytes);
        self.lines.extend(new_lines);
        // 上限裁剪：超丢头部
        if self.lines.len() > MAX_LINES {
            let drop = self.lines.len() - MAX_LINES;
            self.lines.drain(0..drop);
        }
    }

    /// 拼接所有行的纯文本（测试/调试用）。
    #[allow(dead_code)]
    pub fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.plain_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 消费 [`TerminalOutputChunk`]：喂入对应 tab 的 [`RenderHistory`]。
pub fn append_output_chunks(
    mut reader: MessageReader<TerminalOutputChunk>,
    mut q: Query<&mut RenderHistory>,
) {
    for chunk in reader.read() {
        if let Ok(mut hist) = q.get_mut(chunk.tab) {
            hist.feed(&chunk.bytes);
        }
    }
}

/// 重建激活 tab 的输出节点：despawn 旧行 + spawn 新行。
///
/// MVP 非虚拟滚动——直接渲染所有行。行数大时（>1k）可能卡顿，虚拟化留后续
/// （对齐 chat_panel 的 VirtualList 接入策略）。
pub fn update_output_visibility(
    tabs: Res<TerminalTabs>,
    q_hist: Query<&RenderHistory>,
    q_hist_changed: Query<&RenderHistory, Changed<RenderHistory>>,
    q_output: Query<Entity, With<crate::terminal::TerminalOutputMarker>>,
    theme: Res<Theme>,
    mut commands: Commands,
    q_line_children: Query<(Entity, &ChildOf), With<OutputLineMarker>>,
) {
    let Ok(output_container) = q_output.single() else {
        return;
    };
    let Some(active) = tabs.active_entity() else {
        // 无 tab：清空容器
        for (entity, _) in q_line_children.iter() {
            commands.entity(entity).despawn();
        }
        return;
    };
    let Ok(hist) = q_hist.get(active) else {
        return;
    };
    // 历史变化或激活 tab 切换时重建
    let hist_changed = q_hist_changed.get(active).is_ok();
    if !hist_changed && !tabs.is_changed() {
        return;
    }
    // despawn 旧行
    for (entity, _) in q_line_children.iter() {
        commands.entity(entity).despawn();
    }
    let font = theme.font_size;
    commands.entity(output_container).with_children(|c| {
        for line in &hist.lines {
            if line.spans.is_empty() {
                c.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: px(font * 1.4),
                        ..default()
                    },
                    OutputLineMarker,
                ));
                continue;
            }
            c.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                },
                OutputLineMarker,
            ))
            .with_children(|row| {
                for span in &line.spans {
                    row.spawn((
                        Text::new(span.text.clone()),
                        TextFont {
                            font_size: FontSize::Px(font),
                            ..default()
                        },
                        TextColor(map_color(span.style.fg, &theme)),
                        OutputSpanMarker,
                    ));
                }
            });
        }
    });
}

/// 单行输出节点标记（用于 despawn 重建）。
#[derive(Component, Default)]
pub struct OutputLineMarker;

/// 单 span 标记。
#[derive(Component, Default)]
pub struct OutputSpanMarker;

/// 终端颜色 → Bevy `Color`（前景；None 用默认文本色）。
fn map_color(color: Option<xgent_terminal::Color>, theme: &Theme) -> bevy::color::Color {
    match color {
        None => theme.text,
        Some(xgent_terminal::Color::Basic(idx)) => basic_color(idx),
        Some(xgent_terminal::Color::Bright(idx)) => bright_color(idx),
        Some(xgent_terminal::Color::Indexed(idx)) => indexed_color(idx),
        Some(xgent_terminal::Color::Rgb(r, g, b)) => bevy::color::Color::srgb_u8(r, g, b),
    }
}

/// 基本色（0-7）。
fn basic_color(idx: u8) -> bevy::color::Color {
    const COLORS: [bevy::color::Color; 8] = [
        bevy::color::Color::srgb_u8(0, 0, 0),         // 黑
        bevy::color::Color::srgb_u8(194, 54, 33),     // 红
        bevy::color::Color::srgb_u8(37, 188, 36),     // 绿
        bevy::color::Color::srgb_u8(173, 173, 39),    // 黄
        bevy::color::Color::srgb_u8(73, 46, 225),     // 蓝
        bevy::color::Color::srgb_u8(211, 56, 211),    // 品红
        bevy::color::Color::srgb_u8(51, 187, 200),    // 青
        bevy::color::Color::srgb_u8(203, 204, 205),   // 白
    ];
    COLORS[(idx as usize) % 8]
}

/// 亮色（0-7）。
fn bright_color(idx: u8) -> bevy::color::Color {
    const COLORS: [bevy::color::Color; 8] = [
        bevy::color::Color::srgb_u8(129, 131, 131),   // 亮黑（灰）
        bevy::color::Color::srgb_u8(252, 57, 31),     // 亮红
        bevy::color::Color::srgb_u8(49, 231, 34),     // 亮绿
        bevy::color::Color::srgb_u8(231, 197, 71),    // 亮黄
        bevy::color::Color::srgb_u8(88, 86, 214),     // 亮蓝
        bevy::color::Color::srgb_u8(249, 53, 248),    // 亮品红
        bevy::color::Color::srgb_u8(63, 230, 224),    // 亮青
        bevy::color::Color::srgb_u8(233, 235, 235),   // 亮白
    ];
    COLORS[(idx as usize) % 8]
}

/// 256 色调色板索引 → 颜色。
fn indexed_color(idx: u8) -> bevy::color::Color {
    // 0-15: 基础 + 亮色（简化用 basic/bright）
    if idx < 8 {
        basic_color(idx)
    } else if idx < 16 {
        bright_color(idx - 8)
    } else if idx < 232 {
        // 16-231: 6×6×6 RGB 立方体
        let idx = idx - 16;
        let r = idx / 36;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_u8 = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
        bevy::color::Color::srgb_u8(to_u8(r), to_u8(g), to_u8(b))
    } else {
        // 232-255: 灰阶
        let v = 8 + (idx - 232) * 10;
        bevy::color::Color::srgb_u8(v, v, v)
    }
}

/// 更新 tv-statusbar：显示激活 tab 的状态/shell/cwd/exit code。
pub fn update_status_bar(
    tabs: Res<TerminalTabs>,
    q_tabs: Query<&TerminalTab>,
    mut q_status: Query<&mut Text, With<TerminalStatusBarMarker>>,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
) {
    use crate::i18n::tr_with;
    let Ok(mut status_text) = q_status.single_mut() else {
        return;
    };
    let Some(active) = tabs.active_entity() else {
        let s = tr_with(&loc, "terminal-no-tabs", &[]);
        if status_text.0 != s {
            status_text.0 = s;
        }
        return;
    };
    let Ok(tab) = q_tabs.get(active) else {
        return;
    };
    let shell_name = match tab.shell {
        xgent_terminal::ShellSpec::Powershell => tr_with(&loc, "terminal-shell-powershell", &[]),
        xgent_terminal::ShellSpec::FromEnv => tr_with(&loc, "terminal-shell-shell", &[]),
    };
    let status_str = match tab.status {
        TerminalTabStatus::Created => tr_with(
            &loc,
            "terminal-status-created",
            &[
                ("shell", shell_name.clone()),
                ("cwd", tab.cwd.display().to_string()),
            ],
        ),
        TerminalTabStatus::Running => tr_with(
            &loc,
            "terminal-status-running",
            &[
                ("shell", shell_name.clone()),
                ("cwd", tab.cwd.display().to_string()),
            ],
        ),
        TerminalTabStatus::Exited => tr_with(
            &loc,
            "terminal-status-exited",
            &[
                ("code", format!("{:?}", tab.exit_code)),
                ("shell", shell_name.clone()),
            ],
        ),
    };
    if status_text.0 != status_str {
        status_text.0 = status_str;
    }
    let _ = theme; // 主题用于颜色，Text 颜色由 TextColor 组件控制（spawn 时设）
}
/// 上次 PTY resize 的尺寸缓存（避免每帧重复发 resize）。
#[derive(Resource, Default)]
pub struct TerminalResizeTracker {
    /// (cols, rows) 上次发送的尺寸。
    pub last: Option<(u16, u16)>,
}

/// 监测 `TerminalOutputMarker` 的视口尺寸变化 → 发 [`TerminalResize`]。
///
/// SideView 展开/窗口 resize 时触发；按字体大小估算 cols/rows（等宽字体
/// 宽 ≈ font × 0.6，行高 ≈ font × 1.4）。MVP 估算，精确字符度量留后续。
pub fn handle_terminal_resize(
    content: Res<crate::editor::SideViewContent>,
    tabs: Res<TerminalTabs>,
    q_output: Query<&ComputedNode, With<TerminalOutputMarker>>,
    theme: Res<Theme>,
    mut tracker: ResMut<TerminalResizeTracker>,
    mut writer: MessageWriter<TerminalResize>,
) {
    // 仅终端视图激活时跟踪
    if *content != crate::editor::SideViewContent::Terminal {
        return;
    }
    let Some(active_tab) = tabs.active_entity() else {
        return;
    };
    let Ok(node) = q_output.single() else {
        return;
    };
    let font = theme.font_size.max(1.0);
    let width = node.size().x;
    let height = node.size().y;
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let cols = (width / (font * 0.6)).max(1.0) as u16;
    let rows = (height / (font * 1.4)).max(1.0) as u16;
    if tracker.last == Some((cols, rows)) {
        return;
    }
    tracker.last = Some((cols, rows));
    writer.write(TerminalResize {
        tab: active_tab,
        cols,
        rows,
    });
}
