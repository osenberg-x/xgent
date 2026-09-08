//! 终端输出历史渲染：PTY 字节流 → vte 解析 → `RenderHistory` → UI。
//!
//! 详见 `doc/design/terminal-design.md` §3.4、§6。
//!
//! 行模型（非屏幕字符网格）：每个 tab 持一个 [`RenderHistory`]（`Vec<RenderLine>`），
//! PTY 字节经 `TerminalParser` 增量解析累积成行。MVP 渲染策略：把每行作为独立
//! Text 节点 spawn 进 `TerminalOutputMarker` 容器（非虚拟滚动——行数大时性能
//! 待优化，对齐 chat_panel 的「虚拟化留后续」策略）。历史上限 10k 行超丢头部。

use bevy::prelude::*;

use crate::fonts::{UiFonts, mono_text};
use crate::terminal::io::{TerminalOutputChunk, TerminalResize};
use crate::terminal::{
    TerminalOutputMarker, TerminalStatusBarMarker, TerminalTab, TerminalTabStatus, TerminalTabs,
};
use crate::theme::{Theme, px, type_scale};

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

/// 已渲染到的行游标（挂于输出容器，增量渲染：只追加新行，不全量重建）。
///
/// 切 tab / 清屏时重置为 0 并清空容器（全量重建仅这两条路径）。
#[derive(Component, Debug, Default)]
pub struct RenderedCursor {
    /// 已渲染的 `hist.lines` 行数。
    pub lines: usize,
    /// 已渲染的 current_line 版本（内容串签名，检测 prompt/输入行变化）。
    pub current_sig: u64,
}

/// 单行输出节点标记（用于 despawn 重建）。
#[derive(Component, Default)]
pub struct OutputLineMarker;

/// 当前未 flush 行（prompt / 正在输入的命令行）节点标记——每次内容变化
/// 整行 despawn 重 spawn，保证 shell 回显实时可见。
#[derive(Component, Default)]
pub struct OutputCurrentMarker;

/// 渲染一行（空行占位 或 span 行）。
///
/// `is_current` 为 true 时额外挂 [`OutputCurrentMarker`]（current 行定位用）。
fn spawn_line(
    c: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    line: &RenderLine,
    font: f32,
    theme: &Theme,
    fonts: &UiFonts,
    is_current: bool,
) {
    let mut e = c.spawn((Node {
        width: Val::Percent(100.0),
        height: if line.spans.is_empty() {
            px(font * 1.4)
        } else {
            Val::Auto
        },
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        ..default()
    },));
    e.insert(OutputLineMarker);
    if is_current {
        e.insert(OutputCurrentMarker);
    }
    if line.spans.is_empty() {
        return;
    }
    e.with_children(|row| {
        for span in &line.spans {
            row.spawn((
                mono_text(
                    fonts,
                    span.text.clone(),
                    type_scale::MONO,
                    map_color(span.style.fg, theme),
                    type_scale::line_height::TERM,
                ),
                OutputSpanMarker,
            ));
        }
    });
}

/// 增量渲染激活 tab 的输出（R-终端 修复：替代原全量重建）。
///
/// 原实现每次 `RenderHistory` 变化就全量 despawn+respawn 所有行，且
/// `current_line`（未 flush 的 prompt/输入行）的内容变化**不触发**
/// `RenderHistory` change detection——首次唤起时 PTY 横幅已在后台到达并
/// 渲染，但 prompt 行永远停留在首帧状态，视觉上「tty 没有显示」；而旧
/// 实现对 `tabs.is_changed()` 也全量重建，与流式输出交叠时产生行重复。
///
/// 新策略：
/// - 常态只追加 `hist.lines[rendered..]` 新行 + 重渲 current 行（签名变化时）；
/// - 全量重建仅发生在 tab 切换 / 清屏（游标重置）；
/// - current 行单独用一个节点（`OutputCurrentMarker`），每次签名变化 despawn
///   重 spawn——prompt/输入行始终实时。
pub fn update_output_visibility(
    tabs: Res<TerminalTabs>,
    q_hist: Query<(&RenderHistory, &TerminalTab)>,
    mut q_output: Query<(Entity, &mut RenderedCursor), With<crate::terminal::TerminalOutputMarker>>,
    q_current_node: Query<Entity, With<OutputCurrentMarker>>,
    theme: Res<Theme>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
    q_line_children: Query<Entity, With<OutputLineMarker>>,
) {
    let Ok((output_container, mut cursor)) = q_output.single_mut() else {
        return;
    };
    let Some(active) = tabs.active_entity() else {
        // 无 tab：清空容器 + 游标归零
        for entity in q_line_children.iter() {
            commands.entity(entity).despawn();
        }
        for entity in q_current_node.iter() {
            commands.entity(entity).despawn();
        }
        cursor.lines = 0;
        cursor.current_sig = 0;
        return;
    };
    let Ok((hist, tab)) = q_hist.get(active) else {
        return;
    };

    // tab 切换 / 清屏（游标 > 当前行数 = 行被清掉）→ 全量重建
    let full_rebuild = cursor.lines > hist.lines.len() || tabs.is_changed();
    if full_rebuild {
        for entity in q_line_children.iter() {
            commands.entity(entity).despawn();
        }
        for entity in q_current_node.iter() {
            commands.entity(entity).despawn();
        }
        cursor.lines = 0;
        cursor.current_sig = 0;
    }

    let font = theme.font_size;
    // 追加新增的完整行
    if cursor.lines < hist.lines.len() {
        let start = cursor.lines;
        commands.entity(output_container).with_children(|c| {
            for line in &hist.lines[start..] {
                spawn_line(c, line, font, &theme, &fonts, false);
            }
        });
        cursor.lines = hist.lines.len();
    }

    // current 行（prompt / 正在输入的命令，未 flush 进 lines）：
    // 内容签名变化 → despawn 旧行重 spawn，保证始终实时显示
    let current = hist.parser.current_line();
    let mut hasher = std::hash::DefaultHasher::new();
    for span in &current.spans {
        std::hash::Hash::hash(&span.text, &mut hasher);
    }
    std::hash::Hash::hash(&current.spans.len(), &mut hasher);
    let sig = std::hash::Hasher::finish(&hasher);
    if sig != cursor.current_sig {
        for entity in q_current_node.iter() {
            commands.entity(entity).despawn();
        }
        if !current.spans.is_empty() {
            commands.entity(output_container).with_children(|c| {
                spawn_line(c, &current, font, &theme, &fonts, true);
            });
        }
        cursor.current_sig = sig;
    }
}

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
        bevy::color::Color::srgb_u8(0, 0, 0),       // 黑
        bevy::color::Color::srgb_u8(194, 54, 33),   // 红
        bevy::color::Color::srgb_u8(37, 188, 36),   // 绿
        bevy::color::Color::srgb_u8(173, 173, 39),  // 黄
        bevy::color::Color::srgb_u8(73, 46, 225),   // 蓝
        bevy::color::Color::srgb_u8(211, 56, 211),  // 品红
        bevy::color::Color::srgb_u8(51, 187, 200),  // 青
        bevy::color::Color::srgb_u8(203, 204, 205), // 白
    ];
    COLORS[(idx as usize) % 8]
}

/// 亮色（0-7）。
fn bright_color(idx: u8) -> bevy::color::Color {
    const COLORS: [bevy::color::Color; 8] = [
        bevy::color::Color::srgb_u8(129, 131, 131), // 亮黑（灰）
        bevy::color::Color::srgb_u8(252, 57, 31),   // 亮红
        bevy::color::Color::srgb_u8(49, 231, 34),   // 亮绿
        bevy::color::Color::srgb_u8(231, 197, 71),  // 亮黄
        bevy::color::Color::srgb_u8(88, 86, 214),   // 亮蓝
        bevy::color::Color::srgb_u8(249, 53, 248),  // 亮品红
        bevy::color::Color::srgb_u8(63, 230, 224),  // 亮青
        bevy::color::Color::srgb_u8(233, 235, 235), // 亮白
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
///
/// 缓存按 tab 实体区分——切换 tab 时即使视口尺寸不变，新 tab 的 PTY
/// 也需要 resize 到匹配视口（新 spawn 的 tab 初始 80×24，可能与视口不符）。
#[derive(Resource, Default)]
pub struct TerminalResizeTracker {
    /// (tab, cols, rows) 上次发送的尺寸 + 目标 tab。
    pub last: Option<(Entity, u16, u16)>,
}

/// 监测 `TerminalOutputMarker` 的视口尺寸变化 → 发 [`TerminalResize`]。
///
/// SideView 展开/窗口 resize 时触发；按字体大小估算 cols/rows（等宽字体
/// 宽 ≈ font × 0.6，行高 ≈ font × 1.4）。物理像素经 inverse_scale_factor
/// 转逻辑像素后再算（HiDPI 一致）。精确字符度量留后续。
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
    // size() 返回物理像素，font 是逻辑像素——乘 inverse_scale_factor
    // 转逻辑像素后再算字符宽/行高（HiDPI 下不转会导致 cols/rows 翻倍，
    // shell 自动换行与 UI 显示错位）。
    let scale = node.inverse_scale_factor();
    let width = node.size().x * scale;
    let height = node.size().y * scale;
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let cols = (width / (font * 0.6)).max(1.0) as u16;
    let rows = (height / (font * 1.4)).max(1.0) as u16;
    if tracker.last == Some((active_tab, cols, rows)) {
        return;
    }
    tracker.last = Some((active_tab, cols, rows));
    writer.write(TerminalResize {
        tab: active_tab,
        cols,
        rows,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-终端 回归：current 行内容变化必须反映到渲染签名。
    ///
    /// 旧实现仅据 `RenderHistory`（lines）change detection 重建，prompt 行
    /// （存在于 parser.current_line，未 flush 进 lines）永远停留在首帧——
    /// 首次唤起时 tty 「没有显示」的根因。新实现以内容签名驱动 current 行重渲。
    #[test]
    fn current_line_signature_tracks_content() {
        let mut hist = RenderHistory::default();
        // 首帧：PTY 输出 "user@host ~ "（prompt，无尾随 \n → 留在 current）
        hist.feed(b"user@host ~ ");
        let sig1 = {
            let current = hist.parser.current_line();
            let mut hasher = std::hash::DefaultHasher::new();
            for span in &current.spans {
                std::hash::Hash::hash(&span.text, &mut hasher);
            }
            std::hash::Hash::hash(&current.spans.len(), &mut hasher);
            std::hash::Hasher::finish(&hasher)
        };
        // 用户输入 "ls"（回显进 current，仍无 \n）
        hist.feed(b"ls");
        let sig2 = {
            let current = hist.parser.current_line();
            let mut hasher = std::hash::DefaultHasher::new();
            for span in &current.spans {
                std::hash::Hash::hash(&span.text, &mut hasher);
            }
            std::hash::Hash::hash(&current.spans.len(), &mut hasher);
            std::hash::Hasher::finish(&hasher)
        };
        assert_ne!(sig1, sig2, "current 行内容变化应改变签名（触发重渲）");

        // 回车后 prompt 行 flush 进 lines：lines 增长 → 增量追加路径触发
        hist.feed(b"\n");
        assert_eq!(hist.lines.len(), 1, "回车后 current 行应 flush 为完整行");
        assert!(hist.lines[0].plain_text().contains("ls"));
    }

    /// 增量游标契约：RenderedCursor 记录已渲染行数，新行只追加不重建。
    #[test]
    fn rendered_cursor_appends_without_rebuild() {
        let mut cursor = RenderedCursor::default();
        let mut hist = RenderHistory::default();
        hist.feed(b"line1\nline2\n");
        assert_eq!(hist.lines.len(), 2);

        // 首次渲染：游标 0 → 2
        cursor.lines = hist.lines.len();
        // 又来一行：只追加 1 行（游标差 = 新增行数，非全量）
        hist.feed(b"line3\n");
        let appended = hist.lines.len() - cursor.lines;
        assert_eq!(appended, 1, "增量渲染应只追加新行");
        cursor.lines = hist.lines.len();

        // 清屏：lines 清空 → 游标 > 行数 → 全量重建分支（游标归零）
        hist.clear();
        assert!(cursor.lines > hist.lines.len(), "清屏应触发全量重建分支");
    }
}
