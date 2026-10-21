//! 终端输出渲染模型——ANSI 转义序列解析为 [`RenderLine`]。
//!
//! 详见 `docs/design/terminal-design.md` §3.4、§6。
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

// ============================================================================
// v8 M4-T2：双宽单元格模型
// ============================================================================
//
// 终端输出此前按「shaper 连续排版」渲染：每行是一个 flex 行，span 里的字符由
// 文本管线连续排布。比例字体 + 拉丁/CJK 两条回退链拼接时，**没有共享的连续
// 排版度量**可言——中文输出必然与列对齐错开。
//
// 这里引入显式的单元格模型：
// - [`display_width`]：一个字符占几格（wcwidth 语义）。
// - [`layout_cells`]：把带样式的 span 序列切成「起始列 + 占格数」的 run 序列，
//   渲染层据此按格手工定位（`left = 起始列 × cell_w`），不再依赖连续排版。

/// 零宽码位（组合记号、变体选择符、零宽连接/分隔符等）。
///
/// 这些字符在终端里与**前一个**字符共用一格，故占 0 格。
const ZERO_WIDTH_RANGES: &[(u32, u32)] = &[
    (0x0300, 0x036F), // 组合附加符号
    (0x0483, 0x0489), // 西里尔组合记号
    (0x0591, 0x05BD), // 希伯来文标记
    (0x05BF, 0x05BF),
    (0x05C1, 0x05C2),
    (0x05C4, 0x05C5),
    (0x05C7, 0x05C7),
    (0x0610, 0x061A), // 阿拉伯文标记
    (0x064B, 0x065F),
    (0x0670, 0x0670),
    (0x06D6, 0x06DC),
    (0x06DF, 0x06E4),
    (0x06E7, 0x06E8),
    (0x06EA, 0x06ED),
    (0x0711, 0x0711),
    (0x0730, 0x074A),
    (0x07A6, 0x07B0),
    (0x07EB, 0x07F3),
    (0x0816, 0x0819),
    (0x081B, 0x0823),
    (0x0825, 0x0827),
    (0x0829, 0x082D),
    (0x0859, 0x085B),
    (0x08D3, 0x08E1),
    (0x08E3, 0x0903),
    (0x093A, 0x094F), // 天城文标记
    (0x0951, 0x0957),
    (0x0962, 0x0963),
    (0x0981, 0x0983),
    (0x09BC, 0x09CD),
    (0x0A01, 0x0A03),
    (0x0A3C, 0x0A4D),
    (0x0E31, 0x0E31),
    (0x0E34, 0x0E3A),
    (0x0E47, 0x0E4E),
    (0x135D, 0x135F),
    (0x1AB0, 0x1AFF),   // 组合记号扩展
    (0x1DC0, 0x1DFF),   // 组合记号补充
    (0x200B, 0x200F),   // 零宽空格 / ZWNJ / ZWJ / LRM / RLM
    (0x202A, 0x202E),   // 双向控制符
    (0x2060, 0x2064),   // 单词连接符等
    (0x20D0, 0x20F0),   // 符号组合记号
    (0xFE00, 0xFE0F),   // 变体选择符
    (0xFE20, 0xFE2F),   // 组合半符号
    (0xFEFF, 0xFEFF),   // 零宽不换行空格
    (0xE0100, 0xE01EF), // 变体选择符补充
];

/// 双宽码位（East Asian Wide / Fullwidth + emoji 呈现）。
const WIDE_RANGES: &[(u32, u32)] = &[
    (0x1100, 0x115F), // 谚文字母
    (0x2E80, 0x2E99), // CJK 部首补充
    (0x2E9B, 0x2EF3),
    (0x2F00, 0x2FD5), // 康熙部首
    (0x2FF0, 0x2FFB), // 表意文字描述字符
    (0x3000, 0x303E), // CJK 符号与标点（U+303F 除外）
    (0x3041, 0x3096), // 平假名
    (0x3099, 0x30FF), // 片假名
    (0x3105, 0x312F), // 注音符号
    (0x3131, 0x318E), // 谚文兼容字母
    (0x3190, 0x31E3), // 汉字注音 / 部首扩展 / CJK 笔画
    (0x31F0, 0x321E), // 片假名拼音扩展 / 带圈汉字
    (0x3220, 0x3247),
    (0x3250, 0x4DBF), // 带圈 CJK + CJK 扩展 A
    (0x4E00, 0xA48C), // CJK 统一表意文字 + 彝文
    (0xA490, 0xA4C6), // 彝文部首
    (0xA960, 0xA97C), // 谚文字母扩展 A
    (0xAC00, 0xD7A3), // 谚文音节
    (0xF900, 0xFAFF), // CJK 兼容表意文字
    (0xFE10, 0xFE19), // 竖排标点
    (0xFE30, 0xFE52), // CJK 兼容形式
    (0xFE54, 0xFE66),
    (0xFE68, 0xFE6B),
    (0xFF00, 0xFF60), // 全角形式（含全角空格）
    (0xFFE0, 0xFFE6), // 全角符号
    (0x16FE0, 0x16FE4),
    (0x17000, 0x18AFF), // 西夏文
    (0x1B000, 0x1B2FF), // 假名补充
    (0x1F200, 0x1F251), // 带圈表意文字补充
    (0x1F300, 0x1F64F), // 杂项符号与图形 / 表情
    (0x1F680, 0x1F6FF), // 交通与地图符号
    (0x1F900, 0x1F9FF), // 补充符号与图形
    (0x1FA70, 0x1FAFF), // 符号与图形扩展 A
    (0x1FC00, 0x1FFFD), // 符号与图形扩展 B
    (0x20000, 0x3FFFD), // CJK 扩展 B 及以后
];

/// 「默认窄、但 emoji 呈现时为宽」的码位（经典 wcwidth emoji 扩展表）。
///
/// 这些码位在多数终端里按 **2 格**处理（带 VS16 的 emoji 序列、以及本身就有
/// emoji 呈现的字形）。
const EMOJI_NARROW_RANGES: &[(u32, u32)] = &[
    (0x231A, 0x231B), // ⌚⌛
    (0x23E9, 0x23EC),
    (0x23F0, 0x23F0),
    (0x23F3, 0x23F3),
    (0x25FD, 0x25FE),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267F, 0x267F),
    (0x2693, 0x2693),
    (0x26A1, 0x26A1),
    (0x26AA, 0x26AB),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x26CE, 0x26CE),
    (0x26D4, 0x26D4),
    (0x26EA, 0x26EA),
    (0x26F2, 0x26F3),
    (0x26F5, 0x26F5),
    (0x26FA, 0x26FA),
    (0x26FD, 0x26FD),
    (0x2705, 0x2705),
    (0x270A, 0x270B),
    (0x2728, 0x2728),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
];

/// 码位是否落在给定区间表内。
fn in_ranges(cp: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .binary_search_by(|&(lo, hi)| {
            if cp < lo {
                std::cmp::Ordering::Greater
            } else if cp > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// 字符占用的终端格数（wcwidth 语义）。
///
/// - **2**：CJK 全角、emoji、全角形式（U+FF00–FF60）。
/// - **0**：组合记号 / 变体选择符 / 零宽连接符（与前一字符共用一格）。
/// - **1**：其余（ASCII、希腊文、西里尔文等窄字符）。
///
/// 说明：U+FE0F（VS16）本身零宽；其**前**字符的宽度按自身判定，不做「emoji
/// 呈现序列升级」——终端里 `❤` 与 `❤️` 的宽度因此可能不同。
pub fn display_width(ch: char) -> usize {
    let cp = ch as u32;
    if in_ranges(cp, ZERO_WIDTH_RANGES) {
        return 0;
    }
    if in_ranges(cp, WIDE_RANGES) || in_ranges(cp, EMOJI_NARROW_RANGES) {
        return 2;
    }
    1
}

/// 整串占用的格数。
pub fn display_width_str(s: &str) -> usize {
    s.chars().map(display_width).sum()
}

/// 按格定位的一个文本 run。
///
/// 渲染层据此把该 run 放到 `left = start_col × cell_w` 的位置——**定位不再
/// 依赖文本 shaper 的连续排版度量**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellRun {
    /// 文本内容。
    pub text: String,
    /// 起始列（0 起的格号）。
    pub start_col: usize,
    /// 本 run 占用的格数。
    pub cells: usize,
    /// 所属 span 的下标（样式索引；渲染层据此取前景色）。
    pub style_index: usize,
}

/// 把带样式的 span 序列切成按格定位的 run 序列。
///
/// 规则：
/// - **窄字符**（1 格）连续且同 span 的相邻字符合并为一个 run——等宽字体下
///   连续排版与按格定位等价，合并可显著减少节点数。
/// - **宽字符**（2 格）**逐字**成 run：CJK 回退字体多为比例字体（汉字步进
///   1em，而等宽拉丁步进 0.6em），连续排版与「2 格」并不等价；逐字定位才能
///   保证宽字符的起点严格落在格边界上。
/// - **零宽字符**（组合记号等）并入当前 run，不推进列号。
pub fn layout_cells(spans: &[StyledSpan]) -> Vec<CellRun> {
    let mut runs: Vec<CellRun> = Vec::new();
    let mut col: usize = 0;
    // 当前待合并的 run（窄字符连续段）。
    let mut cur: Option<CellRun> = None;

    for (style_index, span) in spans.iter().enumerate() {
        for ch in span.text.chars() {
            let w = display_width(ch);
            if w == 0 {
                // 零宽：并入当前 run（没有则丢弃——行首的孤立组合记号无依附对象）
                if let Some(run) = cur.as_mut() {
                    run.text.push(ch);
                }
                continue;
            }
            if w == 2 {
                // 宽字符：逐字成 run，先结掉当前窄字符段
                if let Some(run) = cur.take() {
                    runs.push(run);
                }
                runs.push(CellRun {
                    text: ch.to_string(),
                    start_col: col,
                    cells: 2,
                    style_index,
                });
                col += 2;
                continue;
            }
            // 窄字符：与当前段合并或另起一段
            match cur.as_mut() {
                Some(run) if run.style_index == style_index => {
                    run.text.push(ch);
                    run.cells += 1;
                }
                _ => {
                    if let Some(run) = cur.take() {
                        runs.push(run);
                    }
                    cur = Some(CellRun {
                        text: ch.to_string(),
                        start_col: col,
                        cells: 1,
                        style_index,
                    });
                }
            }
            col += 1;
        }
        // span 结束：样式边界 → 结段
        if let Some(run) = cur.take() {
            runs.push(run);
        }
    }
    if let Some(run) = cur.take() {
        runs.push(run);
    }
    runs
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

    // ===== v8 M4-T2：双宽单元格模型 =====

    /// 构造一个 span（默认样式）。
    fn span(text: &str) -> StyledSpan {
        StyledSpan {
            text: text.into(),
            style: SpanStyle::default(),
        }
    }

    #[test]
    fn ascii_is_one_cell() {
        assert_eq!(display_width('a'), 1);
        assert_eq!(display_width('Z'), 1);
        assert_eq!(display_width(' '), 1);
        assert_eq!(display_width('0'), 1);
        assert_eq!(display_width('~'), 1);
    }

    #[test]
    fn cjk_is_two_cells() {
        // CJK 统一表意文字
        assert_eq!(display_width('中'), 2);
        assert_eq!(display_width('文'), 2);
        // 扩展 A
        assert_eq!(display_width('\u{3400}'), 2);
        // 扩展 B（U+20000）
        assert_eq!(display_width('\u{20000}'), 2);
        // 平假名 / 片假名 / 谚文
        assert_eq!(display_width('あ'), 2);
        assert_eq!(display_width('ア'), 2);
        assert_eq!(display_width('한'), 2);
        // CJK 标点（U+3001 顿号）
        assert_eq!(display_width('\u{3001}'), 2);
    }

    #[test]
    fn fullwidth_forms_are_two_cells() {
        // U+FF00–FF60 → 2（台账/设计系统口径）
        assert_eq!(display_width('\u{FF00}'), 2, "全角空格");
        assert_eq!(display_width('\u{FF01}'), 2, "全角 !");
        assert_eq!(display_width('\u{FF21}'), 2, "全角 A");
        assert_eq!(display_width('\u{FF5E}'), 2, "全角 ~");
        assert_eq!(display_width('\u{FF60}'), 2, "全角 「右括号」区上界");
        // U+FF61 是半角片假名 → 1
        assert_eq!(display_width('\u{FF61}'), 1, "半角片假名应为 1");
    }

    #[test]
    fn emoji_is_two_cells() {
        assert_eq!(display_width('😀'), 2, "U+1F600 表情");
        assert_eq!(display_width('🚀'), 2, "U+1F680 交通符号");
        assert_eq!(display_width('⚡'), 2, "U+26A1 emoji 窄表");
        assert_eq!(display_width('⭐'), 2, "U+2B50");
    }

    #[test]
    fn combining_marks_are_zero_cells() {
        // U+0301 组合锐音符 → 与前一字符共用一格
        assert_eq!(display_width('\u{0301}'), 0);
        // 变体选择符
        assert_eq!(display_width('\u{FE0F}'), 0);
        // 零宽连接符
        assert_eq!(display_width('\u{200D}'), 0);
        // 零宽空格
        assert_eq!(display_width('\u{200B}'), 0);
    }

    #[test]
    fn display_width_str_sums_cells() {
        assert_eq!(display_width_str("abc"), 3);
        assert_eq!(display_width_str("中文"), 4);
        assert_eq!(display_width_str("a中b"), 4);
        assert_eq!(display_width_str("e\u{0301}"), 1, "组合记号不占格");
        assert_eq!(display_width_str(""), 0);
    }

    #[test]
    fn layout_cells_merges_narrow_runs() {
        let runs = layout_cells(&[span("ls -l")]);
        assert_eq!(runs.len(), 1, "连续窄字符应合并为一个 run");
        assert_eq!(runs[0].text, "ls -l");
        assert_eq!(runs[0].start_col, 0);
        assert_eq!(runs[0].cells, 5);
    }

    #[test]
    fn layout_cells_splits_wide_chars_one_by_one() {
        let runs = layout_cells(&[span("中文")]);
        assert_eq!(runs.len(), 2, "宽字符逐字成 run");
        assert_eq!(runs[0].text, "中");
        assert_eq!(runs[0].start_col, 0);
        assert_eq!(runs[0].cells, 2);
        assert_eq!(runs[1].text, "文");
        assert_eq!(runs[1].start_col, 2, "第二个宽字符应落在第 2 格");
        assert_eq!(runs[1].cells, 2);
    }

    #[test]
    fn layout_cells_advances_columns_by_cell_count() {
        // "ab中cd" → a(0) b(1) 中(2..3) c(4) d(5)
        let runs = layout_cells(&[span("ab中cd")]);
        let cols: Vec<(usize, usize, &str)> = runs
            .iter()
            .map(|r| (r.start_col, r.cells, r.text.as_str()))
            .collect();
        assert_eq!(
            cols,
            vec![(0, 2, "ab"), (2, 2, "中"), (4, 2, "cd")],
            "窄字符合并、宽字符独立，列号按累计格数推进"
        );
    }

    #[test]
    fn layout_cells_respects_span_boundaries() {
        let runs = layout_cells(&[span("ab"), span("中")]);
        assert_eq!(runs.len(), 2, "样式边界 → 分 run");
        assert_eq!(runs[0].text, "ab");
        assert_eq!(runs[0].style_index, 0);
        assert_eq!(runs[1].text, "中");
        assert_eq!(runs[1].style_index, 1);
        assert_eq!(runs[1].start_col, 2, "跨 span 的列号继续累计");
    }

    #[test]
    fn layout_cells_keeps_combining_marks_in_run() {
        let runs = layout_cells(&[span("e\u{0301}f")]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "e\u{0301}f");
        assert_eq!(runs[0].cells, 2, "组合记号不占格");
    }

    #[test]
    fn layout_cells_of_empty_line_is_empty() {
        assert!(layout_cells(&[]).is_empty());
        assert!(layout_cells(&[span("")]).is_empty());
    }
}
