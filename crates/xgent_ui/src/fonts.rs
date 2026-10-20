//! 字体资源与文本构造器（v7 排版体系 / v8 M4 字体装配层）。
//!
//! 全局默认字体为 Inter Variable（由 `xgent_app` 启动时覆盖 `Assets<Font>`
//! 的默认 `AssetId`），未显式指定 `font` 的 [`TextFont`] 自动继承；
//! 等宽文本经 [`UiFonts::mono`] 显式引用。OpenType 特性（cv01/ss03）在
//! 构造器内统一附加，业务代码不感知。
//!
//! 字号/行高取值见 [`crate::theme::type_scale`]，禁止就地发明字号。
//!
//! # v8 M4-T1：槽位与候选栈
//!
//! `mono` 曾经是「macOS 独占特权路径」：非 macOS 走 `Handle::default()`，而默认
//! 槽位已被 Inter 占据——**Windows/Linux 上的「等宽」其实是比例字体**。
//!
//! 现在字体按**槽位**装配：每个槽位有一份**有序候选栈**，启动期按栈序取第一个
//! 系统里真实存在的家族；全缺则用该槽位的**内嵌兜底资产**；再全缺才落到该槽位
//! 声明的通用族，并**记警告**（绝不静默回退到别的槽位——`mono` 永不落到 `ui`）。
//!
//! 平台差异只存在于候选栈的**顺序数据**里（[`font_specs`]），代码不分叉
//! （禁止 `cfg!(target_os = …)`）。

use bevy::prelude::*;
use bevy::text::{
    FontFeatureTag, FontFeatures, FontSize, FontWeight, GenericFontFamily, LineHeight,
};

use bevy::text::FontSource;

/// UI 字体句柄。
#[derive(Resource, Debug, Clone)]
pub struct UiFonts {
    /// 全局默认字体（Inter Variable；weak 句柄指向 `Assets<Font>` 默认 `AssetId`）。
    ///
    /// 界面拉丁文本走它；未显式指定 `font` 的 [`TextFont`] 自动继承。
    pub ui: Handle<Font>,
    /// 等宽字体（代码 / 终端 / diff / 行号）——候选栈解析结果。
    pub mono: FontSource,
    /// 界面中文字体（中文与中英混排）——候选栈解析结果。
    pub ui_cjk: FontSource,
    /// 等宽中文字体（终端 / 代码内中文）——候选栈解析结果。
    pub mono_cjk: FontSource,
}

impl UiFonts {
    /// 最小装配（headless / 无字体资产的环境用）。
    ///
    /// 界面拉丁走默认字体资产，等宽与 CJK 走系统**通用族**（不是具体家族）。
    /// 真实 app 由 `xgent_app::startup::load_fonts` 解析候选栈后注入。
    pub fn fallback() -> Self {
        Self {
            ui: Handle::default(),
            mono: FontSource::Generic(GenericFontFamily::Monospace),
            ui_cjk: FontSource::Generic(GenericFontFamily::SansSerif),
            mono_cjk: FontSource::Generic(GenericFontFamily::Monospace),
        }
    }
}

/// 字体槽位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontSlot {
    /// 界面拉丁文本（内嵌 Inter Variable，作为默认字体资产）。
    Ui,
    /// 界面中文与中英混排。
    UiCjk,
    /// 等宽：代码 / 终端 / diff / 行号。
    Mono,
    /// 等宽中文：终端与代码内的中文。
    MonoCjk,
}

impl FontSlot {
    /// 槽位名（日志与文档用）。
    pub fn as_str(self) -> &'static str {
        match self {
            FontSlot::Ui => "ui",
            FontSlot::UiCjk => "ui_cjk",
            FontSlot::Mono => "mono",
            FontSlot::MonoCjk => "mono_cjk",
        }
    }
}

/// 一个候选：家族名 + 可选内嵌兜底资产。
#[derive(Debug, Clone, Copy)]
pub struct FontCandidate {
    /// 家族名（与系统里注册的家族名一致；内嵌资产的家族名也写在这里）。
    pub family: &'static str,
    /// 内嵌兜底资产的相对路径（相对宿主 `assets/` 目录）；`None` 表示纯系统候选。
    pub embedded: Option<&'static str>,
}

impl FontCandidate {
    /// 纯系统候选。
    pub const fn system(family: &'static str) -> Self {
        Self {
            family,
            embedded: None,
        }
    }

    /// 内嵌兜底候选（同时也是合法命中项：系统里装了同名字族时优先命中系统）。
    pub const fn embedded(family: &'static str, path: &'static str) -> Self {
        Self {
            family,
            embedded: Some(path),
        }
    }
}

/// 一个槽位的候选栈。
#[derive(Debug, Clone, Copy)]
pub struct FontSpec {
    /// 槽位。
    pub slot: FontSlot,
    /// 有序候选：第一个真实存在的胜出。
    pub candidates: &'static [FontCandidate],
    /// 候选全缺时的兜底通用族。
    ///
    /// `mono` 用 [`GenericFontFamily::Monospace`]（仍是等宽）——**绝不**落到 `ui`
    /// 槽位的比例字体（设计系统 C-FONT-1）。
    pub fallback_generic: GenericFontFamily,
}

/// 候选命中来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontOrigin {
    /// 命中系统里真实存在的家族。
    System,
    /// 命中内嵌兜底资产。
    Embedded,
    /// 候选全缺（调用方应记警告；`source` 是该槽位声明的兜底通用族）。
    Failed,
}

impl FontOrigin {
    /// 来源名（日志用）。
    pub fn as_str(self) -> &'static str {
        match self {
            FontOrigin::System => "system",
            FontOrigin::Embedded => "embedded",
            FontOrigin::Failed => "failed",
        }
    }
}

/// 一个槽位的解析结果。
#[derive(Debug, Clone)]
pub struct ResolvedFont {
    /// 命中来源。
    pub origin: FontOrigin,
    /// 命中的家族名（失败时为兜底通用族名）。
    pub family: String,
    /// 解析出的字体来源（直接进 [`TextFont::font`]）。
    pub source: FontSource,
}

/// 解析一个槽位的候选栈。
///
/// - `available`：判定某个家族名在系统字体集合里是否**真实存在**。
/// - `embedded`：按候选取内嵌资产的句柄（`None` 表示该候选的内嵌资产不可用）。
///
/// 规则（设计系统 C-FONT-1 / C-FONT-2）：
/// 1. 按栈序，第一个 `available` 的**系统**候选胜出；
/// 2. 栈里的**内嵌**候选次之（系统没装同名字族时兜底）；
/// 3. 全缺 → [`FontOrigin::Failed`] + 该槽位声明的通用族兜底（调用方记警告）。
///
/// 注意：`mono` 的兜底永远是等宽（通用 Monospace 或内嵌 JetBrains Mono），
/// **不会**回退到 `ui` 槽位的比例字体。
pub fn resolve_slot(
    spec: &FontSpec,
    available: &dyn Fn(&str) -> bool,
    embedded: &dyn Fn(&FontCandidate) -> Option<Handle<Font>>,
) -> ResolvedFont {
    // 第一轮：系统家族（按栈序）
    for candidate in spec.candidates {
        if candidate.embedded.is_some() {
            continue;
        }
        if available(candidate.family) {
            return ResolvedFont {
                origin: FontOrigin::System,
                family: candidate.family.to_string(),
                source: FontSource::Family(candidate.family.into()),
            };
        }
    }
    // 第二轮：内嵌兜底资产
    for candidate in spec.candidates {
        if candidate.embedded.is_none() {
            continue;
        }
        if let Some(handle) = embedded(candidate) {
            return ResolvedFont {
                origin: FontOrigin::Embedded,
                family: candidate.family.to_string(),
                source: FontSource::Handle(handle),
            };
        }
    }
    // 第三轮：全缺 → 该槽位声明的通用族（调用方记警告，绝不静默）
    ResolvedFont {
        origin: FontOrigin::Failed,
        family: format!("{:?}", spec.fallback_generic),
        source: FontSource::Generic(spec.fallback_generic),
    }
}

/// 内嵌 JetBrains Mono 的资产路径（相对宿主 `assets/` 目录）。
pub const EMBEDDED_MONO_PATH: &str = "fonts/JetBrainsMono-Regular.ttf";

/// 内嵌等宽字体的家族名（JetBrains Mono）。
pub const EMBEDDED_MONO_FAMILY: &str = "JetBrains Mono";

/// Windows 候选栈。
const WINDOWS_SPECS: &[FontSpec] = &[
    FontSpec {
        slot: FontSlot::UiCjk,
        candidates: &[
            FontCandidate::system("Microsoft YaHei UI"),
            FontCandidate::system("Microsoft YaHei"),
            FontCandidate::system("SimHei"),
        ],
        fallback_generic: GenericFontFamily::SansSerif,
    },
    FontSpec {
        slot: FontSlot::Mono,
        candidates: &[
            FontCandidate::system("Cascadia Mono"),
            FontCandidate::system("Consolas"),
            FontCandidate::system("Cascadia Code"),
            FontCandidate::embedded(EMBEDDED_MONO_FAMILY, EMBEDDED_MONO_PATH),
        ],
        fallback_generic: GenericFontFamily::Monospace,
    },
    FontSpec {
        slot: FontSlot::MonoCjk,
        candidates: &[
            FontCandidate::system("Sarasa Mono SC"),
            FontCandidate::system("Noto Sans Mono CJK SC"),
        ],
        fallback_generic: GenericFontFamily::Monospace,
    },
];

/// macOS 候选栈。
const MACOS_SPECS: &[FontSpec] = &[
    FontSpec {
        slot: FontSlot::UiCjk,
        candidates: &[
            FontCandidate::system("PingFang SC"),
            FontCandidate::system("Hiragino Sans GB"),
            FontCandidate::system("Heiti SC"),
        ],
        fallback_generic: GenericFontFamily::SansSerif,
    },
    FontSpec {
        slot: FontSlot::Mono,
        candidates: &[
            FontCandidate::system("SF Mono"),
            FontCandidate::system("Menlo"),
            FontCandidate::system("Monaco"),
            FontCandidate::embedded(EMBEDDED_MONO_FAMILY, EMBEDDED_MONO_PATH),
        ],
        fallback_generic: GenericFontFamily::Monospace,
    },
    FontSpec {
        slot: FontSlot::MonoCjk,
        candidates: &[
            FontCandidate::system("Sarasa Mono SC"),
            FontCandidate::system("Noto Sans Mono CJK SC"),
        ],
        fallback_generic: GenericFontFamily::Monospace,
    },
];

/// Linux / 其它平台候选栈。
const LINUX_SPECS: &[FontSpec] = &[
    FontSpec {
        slot: FontSlot::UiCjk,
        candidates: &[
            FontCandidate::system("Noto Sans CJK SC"),
            FontCandidate::system("Source Han Sans SC"),
            FontCandidate::system("WenQuanYi Micro Hei"),
        ],
        fallback_generic: GenericFontFamily::SansSerif,
    },
    FontSpec {
        slot: FontSlot::Mono,
        candidates: &[
            FontCandidate::system("JetBrains Mono"),
            FontCandidate::system("DejaVu Sans Mono"),
            FontCandidate::system("Liberation Mono"),
            FontCandidate::embedded(EMBEDDED_MONO_FAMILY, EMBEDDED_MONO_PATH),
        ],
        fallback_generic: GenericFontFamily::Monospace,
    },
    FontSpec {
        slot: FontSlot::MonoCjk,
        candidates: &[
            FontCandidate::system("Sarasa Mono SC"),
            FontCandidate::system("Noto Sans Mono CJK SC"),
        ],
        fallback_generic: GenericFontFamily::Monospace,
    },
];

/// 取当前平台的候选栈表。
///
/// 平台差异**只**在这里（顺序数据），代码不分叉。
pub fn font_specs() -> &'static [FontSpec] {
    match std::env::consts::OS {
        "windows" => WINDOWS_SPECS,
        "macos" => MACOS_SPECS,
        _ => LINUX_SPECS,
    }
}

/// 按槽位取候选栈规格。
pub fn font_spec(slot: FontSlot) -> Option<&'static FontSpec> {
    font_specs().iter().find(|spec| spec.slot == slot)
}

/// Inter 的字面特征（cv01/ss03——Linear 视觉标识的一部分）。
fn inter_font_features() -> FontFeatures {
    FontFeatures::builder()
        .enable(FontFeatureTag::new(b"cv01"))
        .enable(FontFeatureTag::new(b"ss03"))
        .build()
}

/// UI 文本构造器（Inter，随全局默认）——全部业务文本经此，禁止裸拼 `TextFont`。
///
/// `size`/`line_height` 取 [`crate::theme::type_scale`] 档位；`weight` 用
/// 400（正文）/510（强调）/590（宣告）三值。
pub fn ui_text(
    text: impl Into<String>,
    size: f32,
    weight: u16,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            font_features: inter_font_features(),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
    )
}

/// UI 文本**样式**构造器（不含 `Text` 组件）——给可编辑文本（`EditableText`）用。
///
/// 可编辑文本的内容由编辑器自己驱动（`EditableText` + `visible_lines`）。若同时
/// 挂 `Text`，bevy 的两个系统会争同一个 `ContentSize`（`measure_text_system` 按
/// `Text` 量、`update_editable_text_content_size` 按 `visible_lines` 量），
/// 高度会时对时错。可编辑文本节点因此只给样式，不给 `Text`。
pub fn ui_text_style(size: f32, weight: u16, color: Color, line_height: f32) -> impl Bundle {
    (
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            font_features: inter_font_features(),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
    )
}

/// 单行省略文本构造器（尾部省略）。
///
/// 与 [`ui_text`] 的差别：额外挂 `TextLayout{NoWrap}` 与 `xui::Ellipsis`——
/// 节点宽度不够时渲染成 `前缀…`（而不是溢出或换行）。
///
/// 节点仍需**确定宽度**才会截断（固定宽 / 百分比 / flex 收缩后的宽度）；
/// 文本节点本身要能收缩：`flex_shrink: 1` + `min_width: Val::ZERO`。
pub fn ellipsis_text(
    source: impl Into<String>,
    size: f32,
    weight: u16,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    let source = source.into();
    (
        Text::new(source.clone()),
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            font_features: inter_font_features(),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        },
        xui::Ellipsis::new(source),
    )
}

/// 单行省略文本构造器（中段省略，路径类：保留首段与末两段）。
pub fn ellipsis_path_text(
    source: impl Into<String>,
    size: f32,
    weight: u16,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    let source = source.into();
    (
        Text::new(source.clone()),
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            font_features: inter_font_features(),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        },
        xui::Ellipsis::mid(source),
    )
}

/// 等宽文本构造器（带字重，等宽标签/强调用）。
pub fn mono_text_weighted(
    fonts: &UiFonts,
    text: impl Into<String>,
    size: f32,
    weight: u16,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font: fonts.mono.clone(),
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
    )
}

/// 等宽文本构造器（显式引用 [`UiFonts::mono`]）。
pub fn mono_text(
    fonts: &UiFonts,
    text: impl Into<String>,
    size: f32,
    color: Color,
    line_height: f32,
) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font: fonts.mono.clone(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        LineHeight::RelativeToFont(line_height),
    )
}
