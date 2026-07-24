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
        self.spans.push(StyledSpan { text: text.into(), style });
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
}

impl Accumulator {
    fn new() -> Self {
        Self {
            current: RenderLine::new(),
            style: SpanStyle::default(),
            finished: Vec::new(),
            pending_text: String::new(),
        }
    }

    fn flush_pending(&mut self) {
        if !self.pending_text.is_empty() {
            let text = std::mem::take(&mut self.pending_text);
            self.current.push(StyledSpan { text, style: self.style });
        }
    }

    fn take_lines(&mut self) -> Vec<RenderLine> {
        std::mem::take(&mut self.finished)
    }
}

impl Perform for Accumulator {
    fn print(&mut self, c: char) {
        self.pending_text.push(c);
    }

    fn execute(&mut self, byte: u8) {
        // 控制字符：\n=LF（换行）, \r=CR（回车，忽略）, \t=Tab, \x08=BS
        match byte {
            b'\n' => {
                self.flush_pending();
                let line = std::mem::take(&mut self.current);
                self.finished.push(line);
            }
            b'\r' => {
                // CR：MVP 忽略（不实现光标回行首覆盖语义）
                self.flush_pending();
            }
            b'\t' => {
                self.pending_text.push_str("    ");
            }
            _ => {
                // 其他控制字符忽略
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
        // MVP 只处理 SGR（`m`，无 intermediates）；其他 CSI（光标移动/清屏）忽略
        if byte != 'm' || !intermediates.is_empty() {
            // 非清屏类 CSI 简单忽略；清屏（J/K）MVP 不实现（行模型下表现待后续）
            return;
        }
        self.flush_pending();
        apply_sgr(&mut self.style, params);
    }
}

/// 应用 SGR（Select Graphic Rendition）参数到 [`SpanStyle`]。
///
/// MVP 支持：0=重置, 1=bold, 3=italic, 4=underline, 30-37=基本前景, 90-97=亮前景,
/// 40-47=基本背景, 100-107=亮背景, 38;5;n=256色前景, 48;5;n=256色背景,
/// 38;2;r;g;b=truecolor 前景, 48;2;r;g;b=truecolor 背景, 39=默认前景, 49=默认背景。
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
                // 扩展前景色：38;5;n（256）或 38;2;r;g;b（truecolor）
                if let Some(next) = iter.next() {
                    match next[0] as u8 {
                        5 => {
                            if let Some(n) = iter.next() {
                                style.fg = Some(Color::Indexed(n[0] as u8));
                            }
                        }
                        2 => {
                            if let (Some(r), Some(g), Some(b)) =
                                (iter.next(), iter.next(), iter.next())
                            {
                                style.fg = Some(Color::Rgb(r[0] as u8, g[0] as u8, b[0] as u8));
                            }
                        }
                        _ => {}
                    }
                }
            }
            48 => {
                if let Some(next) = iter.next() {
                    match next[0] as u8 {
                        5 => {
                            if let Some(n) = iter.next() {
                                style.bg = Some(Color::Indexed(n[0] as u8));
                            }
                        }
                        2 => {
                            if let (Some(r), Some(g), Some(b)) =
                                (iter.next(), iter.next(), iter.next())
                            {
                                style.bg = Some(Color::Rgb(r[0] as u8, g[0] as u8, b[0] as u8));
                            }
                        }
                        _ => {}
                    }
                }
            }
            39 => style.fg = None,
            49 => style.bg = None,
            _ => {}
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
}
