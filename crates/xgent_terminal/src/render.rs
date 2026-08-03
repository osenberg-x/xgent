//! 终端输出渲染模型——ANSI 转义序列解析为 [`RenderLine`]。
//!
//! 详见 `doc/design/terminal-design.md` §3.4、§6。
//!
//! MVP 行模型（非屏幕字符网格）：PTY 字节流经 [`TerminalParser`] 增量解析，
//! 累积成 `Vec<RenderLine>`，每行是 `Vec<StyledSpan>`（带颜色的文本段）。
//! SGR 参数（颜色码）映射到 [`Color`]；非 SGR 转义（光标移动/清屏）MVP 简化
//! 处理——不实现全屏 TUI 的 alternate screen（见 §6 能力边界）。

use vte::{Params, Perform};

/// 渲染行：一段带样式的文本 span 序列。
#[derive(Debug, Clone, Default)]
pub struct RenderLine {
    pub spans: Vec<StyledSpan>,
}

impl RenderLine {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加 span。
    pub fn push(&mut self, span: StyledSpan) {
        self.spans.push(span);
    }

    /// 追加默认样式文本。
    pub fn push_text(&mut self, text: &str, style: SpanStyle) {
        if text.is_empty() {
            return;
        }
        self.spans.push(StyledSpan {
            text: text.into(),
            style,
        });
    }

    /// 拼接所有 span 文本（用于测试断言）。
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// 带样式的文本段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub style: SpanStyle,
}

/// 文本段样式（前景/背景色 + 加粗/斜体/下划线）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpanStyle {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// 终端颜色（8/16 色基础 + 256 色 + truecolor）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// 标准前景/背景色（黑/红/绿/黄/蓝/品红/青/白）。
    Basic(u8),
    /// 亮色变体（bright black/red/...）。
    Bright(u8),
    /// 256 色调色板索引。
    Indexed(u8),
    /// 24-bit truecolor。
    Rgb(u8, u8, u8),
}

/// 终端输出增量解析器。
///
/// 内部持 [`vte::Parser`] + 当前行累积状态，把字节流喂入后产出
/// `Vec<RenderLine>`（可多行，PTY 输出 `\n` 触发换行）。
pub struct TerminalParser {
    parser: vte::Parser,
    performer: Accumulator,
}

impl TerminalParser {
    pub fn new() -> Self {
        Self {
            parser: vte::Parser::new(),
            performer: Accumulator::new(),
        }
    }

    /// 喂入字节流，返回本轮新产生的完整行（按 `\n` 切分）。
    ///
    /// 未结束的行（无尾随 `\n`）保留在内部缓冲，下次 feed 续接。
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<RenderLine> {
        self.parser.advance(&mut self.performer, bytes);
        self.performer.take_lines()
    }

    /// 取当前未结束的行（含尚未 flush 到 spans 的 pending 文本）。
    pub fn current_line(&self) -> RenderLine {
        let mut line = self.performer.current.clone();
        if !self.performer.pending_text.is_empty() {
            line.push(StyledSpan {
                text: self.performer.pending_text.clone(),
                style: self.performer.style,
            });
        }
        line
    }
}

impl Default for TerminalParser {
    fn default() -> Self {
        Self::new()
    }
}

/// vte Perform 实现：累积输出成 [`RenderLine`]。
struct Accumulator {
    /// 当前正在累积的行（未遇 `\n`）。
    current: RenderLine,
    /// 当前 span 样式状态。
    style: SpanStyle,
    /// 本次 feed 产出的完整行。
    finished: Vec<RenderLine>,
    /// 当前 span 的文本缓冲（遇样式变更或特殊字符时 flush 到 current）。
    pending_text: String,
    /// 遇 `\r` 后置 true，延迟到下一个 print/execute 时决定行为：
    /// - 若下一个是 `\n`（CRLF 换行），不清除——直接把当前行入 finished；
    /// - 若下一个是普通字符（行重绘场景），先清空 current 再写新内容。
    ///   这样 `hello\r\n` 得 "hello"，`old\rnew\n` 得 "new"。
    cr_pending: bool,
}

impl Accumulator {
    fn new() -> Self {
        Self {
            current: RenderLine::new(),
            style: SpanStyle::default(),
            finished: Vec::new(),
            pending_text: String::new(),
            cr_pending: false,
        }
    }

    fn flush_pending(&mut self) {
        if !self.pending_text.is_empty() {
            let text = std::mem::take(&mut self.pending_text);
            self.current.push(StyledSpan {
                text,
                style: self.style,
            });
        }
    }

    fn take_lines(&mut self) -> Vec<RenderLine> {
        std::mem::take(&mut self.finished)
    }

    /// 处理 CR 延迟语义：若 cr_pending 为 true 且不是 `\n` 紧随其后，
    /// 清空当前行（行重绘覆盖场景）。调用方在判断完是否为 `\n` 后调用。
    fn resolve_cr(&mut self, is_newline: bool) {
        if self.cr_pending {
            self.cr_pending = false;
            if !is_newline {
                // 非 CRLF：行重绘场景，清空旧行内容让重绘覆盖
                self.pending_text.clear();
                self.current = RenderLine::new();
            }
            // CRLF：保留内容，让 \n 正常入 finished
        }
    }
}

impl Perform for Accumulator {
    fn print(&mut self, c: char) {
        // 非 \n 紧随 \r → 行重绘场景，先清空旧行
        self.resolve_cr(false);
        self.pending_text.push(c);
    }

    fn execute(&mut self, byte: u8) {
        // 控制字符：\n=LF（换行）, \r=CR（回行首覆盖）, \t=Tab, \x08=BS
        match byte {
            b'\n' => {
                // CRLF 场景：\r 已设 cr_pending，此处 resolve_cr(true) 保留内容
                self.resolve_cr(true);
                self.flush_pending();
                let line = std::mem::take(&mut self.current);
                self.finished.push(line);
            }
            b'\r' => {
                // CR：设延迟标志，由后续 print/execute 决定是 CRLF 还是行重绘
                self.cr_pending = true;
            }
            b'\t' => {
                self.resolve_cr(false);
                self.pending_text.push_str("    ");
            }
            b'\x08' => {
                // BS：退格。pending 有字符时删末尾字符；否则退到 current 末 span
                // 的末尾字符（跨样式边界场景：如 "ab" 已 flush 为 span，遇 BS 应
                // 删 'b' 而非弹出整个 "ab" span）。span 文本删空后移除该 span。
                self.resolve_cr(false);
                if self.pending_text.pop().is_none()
                    && let Some(last) = self.current.spans.last_mut()
                {
                    last.text.pop();
                    if last.text.is_empty() {
                        self.current.spans.pop();
                    }
                }
            }
            _ => {
                // 其他控制字符忽略（但需消费 cr_pending）
                self.resolve_cr(false);
            }
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        intermediates: &[u8],
        _ignored_intermediates: bool,
        byte: char,
    ) {
        // CSI 序列非 \n 紧随 \r，视为行重绘场景先清空旧行
        self.resolve_cr(false);
        if !intermediates.is_empty() {
            return;
        }
        match byte {
            // SGR（颜色/样式）
            'm' => {
                self.flush_pending();
                apply_sgr(&mut self.style, params);
            }
            // EL — Erase in Line（\x1b[K / \x1b[0K / \x1b[2K）
            // 行模型下统一清空当前行（pending + current），让重绘覆盖。
            'K' => {
                self.pending_text.clear();
                self.current = RenderLine::new();
            }
            // ED — Erase in Display（\x1b[J / \x1b[0J / \x1b[2J）
            // 0/1：清屏到光标/从光标清——行模型近似清当前行。
            // 2：全屏清——丢弃全部历史 + 当前行（shell `clear` 命令）。
            'J' => {
                let mode = params
                    .iter()
                    .next()
                    .and_then(|p| p.first())
                    .copied()
                    .unwrap_or(0);
                if mode == 2 {
                    self.finished.clear();
                }
                self.pending_text.clear();
                self.current = RenderLine::new();
            }
            // 光标移动（H/A/B/C/D）：行模型下无法精确还原屏幕位置，
            // 不产生行内容——让 \r + 重绘自然覆盖。忽略即可。
            _ => {}
        }
    }
}

/// 应用 SGR（Select Graphic Rendition）参数到 [`SpanStyle`]。
///
/// MVP 支持：0=重置, 1=bold, 3=italic, 4=underline, 30-37=基本前景, 90-97=亮前景,
/// 40-47=基本背景, 100-107=亮背景, 38;5;n=256色前景, 48;5;n=256色背景,
/// 38;2;r;g;b=truecolor 前景, 48;2;r;g;b=truecolor 背景, 39=默认前景, 49=默认背景。
///
/// 支持两种子参数分隔格式：
/// - 分号分隔（常见）：`38;5;n` / `38;2;r;g;b` — vte 拆成多个独立 sub-param
/// - 冒号分隔（ITU-T T.416）：`38:5:n` / `38:2::r:g:b` — vte 归入同一个 sub-param slice
fn apply_sgr(style: &mut SpanStyle, params: &Params) {
    let mut iter = params.iter().peekable();
    while let Some(sub) = iter.next() {
        let code = sub[0] as u8;
        match code {
            0 => {
                *style = SpanStyle::default();
            }
            1 => style.bold = true,
            3 => style.italic = true,
            4 => style.underline = true,
            22 => style.bold = false,
            23 => style.italic = false,
            24 => style.underline = false,
            30..=37 => style.fg = Some(Color::Basic(code - 30)),
            90..=97 => style.fg = Some(Color::Bright(code - 90)),
            40..=47 => style.bg = Some(Color::Basic(code - 40)),
            100..=107 => style.bg = Some(Color::Bright(code - 100)),
            38 => {
                style.fg = parse_extended_color(sub, &mut iter);
            }
            48 => {
                style.bg = parse_extended_color(sub, &mut iter);
            }
            39 => style.fg = None,
            49 => style.bg = None,
            _ => {}
        }
    }
}

/// 解析扩展颜色参数（SGR 38/48），兼容冒号分隔与分号分隔。
///
/// `sub` 是 38/48 所在的 sub-param slice：
/// - 冒号分隔（`38:5:n` / `38:2:r:g:b`）：`sub = [38, 5, n]` / `[38, 2, r, g, b]`
/// - 分号分隔（`38;5;n`）：`sub = [38]`，后续值在 `iter` 的后续 sub-param 中
fn parse_extended_color<'a>(
    sub: &[u16],
    iter: &mut impl Iterator<Item = &'a [u16]>,
) -> Option<Color> {
    if sub.len() > 1 {
        // 冒号分隔：所有值在同一个 sub-param slice 内
        // 格式：38:5:n 或 38:2:r:g:b（ITU-T 还可有 38:2::r:g:b 的色彩空间占位）
        match sub.get(1).map(|&v| v as u8) {
            Some(5) => sub.get(2).map(|&n| Color::Indexed(n as u8)),
            Some(2) => {
                // ITU-T: 38:2::r:g:b 色彩空间 ID 可空（sub[2] 跳过或 = 0）
                // 简化：跳过可能的色彩空间 ID，取后 3 个为 RGB
                let rgb: Vec<u16> = sub.iter().skip(2).copied().collect();
                if rgb.len() >= 3 {
                    Some(Color::Rgb(
                        rgb[rgb.len() - 3] as u8,
                        rgb[rgb.len() - 2] as u8,
                        rgb[rgb.len() - 1] as u8,
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        // 分号分隔：后续值在独立 sub-param 中
        match iter
            .next()
            .and_then(|s| s.first().copied())
            .map(|v| v as u8)
        {
            Some(5) => iter
                .next()
                .and_then(|s| s.first().copied())
                .map(|n| Color::Indexed(n as u8)),
            Some(2) => {
                let r = iter.next().and_then(|s| s.first().copied());
                let g = iter.next().and_then(|s| s.first().copied());
                let b = iter.next().and_then(|s| s.first().copied());
                match (r, g, b) {
                    (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r as u8, g as u8, b as u8)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_accumulates() {
        let mut p = TerminalParser::new();
        let lines = p.feed(b"hello world\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].plain_text(), "hello world");
    }

    #[test]
    fn multiple_lines() {
        let mut p = TerminalParser::new();
        let lines = p.feed(b"line1\nline2\nline3");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].plain_text(), "line1");
        assert_eq!(lines[1].plain_text(), "line2");
        // line3 未结束，留在 current
        assert_eq!(p.current_line().plain_text(), "line3");
    }

    #[test]
    fn red_text_sgr() {
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[31mred\x1b[0mnormal\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 2);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Basic(1)));
        assert_eq!(lines[0].spans[1].style.fg, None);
        assert_eq!(lines[0].plain_text(), "rednormal");
    }

    #[test]
    fn truecolor_sgr() {
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[38;2;255;0;128mpink\x1b[0m\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(255, 0, 128)));
    }

    #[test]
    fn bold_and_reset() {
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[1mbold\x1b[22mnot\n");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans[0].style.bold);
        assert!(!lines[0].spans[1].style.bold);
    }
    #[test]
    fn carriage_return_overwrites_line() {
        // cooked shell 用 \r 回行首重绘：旧内容应被覆盖
        let mut p = TerminalParser::new();
        let lines = p.feed(b"old\rnew\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].plain_text(), "new");
    }

    #[test]
    fn erase_in_line_clears_current() {
        // \x1b[K（EL）清当前行
        let mut p = TerminalParser::new();
        let lines = p.feed(b"keep\n\x1b[Kcleared\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].plain_text(), "keep");
        // \x1b[K 清空了 "cleared" 之前无内容，第二行应为 "cleared"
        assert_eq!(lines[1].plain_text(), "cleared");
    }

    #[test]
    fn erase_display_2_clears_history() {
        // \x1b[2J（ED 2）全屏清：丢弃全部历史
        let mut p = TerminalParser::new();
        let lines = p.feed(b"line1\nline2\n\x1b[2Jafter_clear\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].plain_text(), "after_clear");
    }
    #[test]
    fn empty_sgr_resets() {
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[31mred\x1b[mnormal\n");
        assert_eq!(lines[0].spans.len(), 2, "empty SGR should reset style");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Basic(1)));
        assert_eq!(lines[0].spans[1].style.fg, None);
    }

    #[test]
    fn bs_across_style_boundary() {
        let mut p = TerminalParser::new();
        let lines = p.feed(b"ab\x1b[31m\x08c\n");
        assert_eq!(
            lines[0].plain_text(),
            "ac",
            "BS should remove one char not whole span"
        );
    }
    #[test]
    fn crlf_produces_content_lines() {
        // CRLF 换行：\r\n 应保留行内容，不产生空行
        let mut p = TerminalParser::new();
        let lines = p.feed(b"hello\r\nworld\r\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].plain_text(), "hello");
        assert_eq!(lines[1].plain_text(), "world");
    }

    #[test]
    fn cr_then_text_overwrites() {
        // \r 后非 \n：行重绘覆盖旧内容
        let mut p = TerminalParser::new();
        let lines = p.feed(b"old\rnew\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].plain_text(), "new");
    }

    #[test]
    fn cr_then_sgr_then_text_overwrites() {
        // \r 后 SGR 再文本：CSI 序列触发行重绘覆盖
        let mut p = TerminalParser::new();
        let lines = p.feed(b"old\r\x1b[31mnew\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].plain_text(), "new");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Basic(1)));
    }

    #[test]
    fn cr_then_tab_overwrites() {
        // \r 后 Tab：触发行重绘覆盖
        let mut p = TerminalParser::new();
        let lines = p.feed(b"old\r\tnew\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].plain_text(), "    new");
    }

    #[test]
    fn mixed_lf_and_crlf() {
        // 混合 LF 和 CRLF 换行
        let mut p = TerminalParser::new();
        let lines = p.feed(b"lf\n\rcrlf\r\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].plain_text(), "lf");
        // 第二行：\r 清空空行（无内容）→ \n 入空行？不——\n 后的 \r 设 cr_pending，
        // \n 前无 cr_pending → "crlf" 累积 → \r 设 cr_pending → \n resolve_cr(true) 保留
        assert_eq!(lines[1].plain_text(), "crlf");
    }
    #[test]
    fn colon_separated_256_color_fg() {
        // ITU-T T.416: \x1b[38:5:196m（冒号分隔 256 色）
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[38:5:196mtext\n");
        assert_eq!(
            lines[0].spans[0].style.fg,
            Some(Color::Indexed(196)),
            "colon-separated 38:5:n should set 256-color fg"
        );
    }

    #[test]
    fn colon_separated_truecolor_fg() {
        // ITU-T T.416: \x1b[38:2:255:0:128m（冒号分隔 truecolor）
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[38:2:255:0:128mtext\n");
        assert_eq!(
            lines[0].spans[0].style.fg,
            Some(Color::Rgb(255, 0, 128)),
            "colon-separated 38:2:r:g:b should set truecolor fg"
        );
    }

    #[test]
    fn colon_separated_256_color_bg() {
        // ITU-T T.416: \x1b[48:5:21m（冒号分隔 256 色背景）
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[48:5:21mtext\n");
        assert_eq!(
            lines[0].spans[0].style.bg,
            Some(Color::Indexed(21)),
            "colon-separated 48:5:n should set 256-color bg"
        );
    }

    #[test]
    fn semicolon_256_color_still_works() {
        // 回归：分号分隔 38;5;n 仍应正常工作
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[38;5;196mtext\n");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Indexed(196)));
    }

    #[test]
    fn semicolon_truecolor_still_works() {
        // 回归：分号分隔 38;2;r;g;b 仍应正常工作
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[38;2;255;0;128mtext\n");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(255, 0, 128)));
    }

    #[test]
    fn mixed_colon_and_semicolon_sgr() {
        // 混合格式：38:5:46（冒号 256 色 fg）分号 1（bold）
        let mut p = TerminalParser::new();
        let lines = p.feed(b"\x1b[38:5:46;1mbold_green\n");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Indexed(46)));
        assert!(lines[0].spans[0].style.bold);
    }
}
