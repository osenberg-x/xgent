//! 终端的**等宽实测度量**与单元格网格（v8 M4-T2）。
//!
//! 终端此前用「字号 × 0.6」估算字符宽度（`terminal/output.rs`），且字号取的是
//! **界面正文**档（14）而非终端实际渲染的 **CODE** 档（12）——列数因此被低估
//! 约 17%，shell 的自动换行与 UI 显示宽度对不上。
//!
//! 本模块给出两条正路：
//!
//! 1. [`MonoMetrics`]：等宽槽位（`mono`）的**实测**度量——`latin_advance`
//!    （窄字符步进）、`cjk_advance`（宽字符步进）、`line_height`（行高）、
//!    `at_size`（实测字号）。实测走 bevy 文本管线（探针节点 + `TextLayoutInfo`），
//!    与终端渲染**同一条 measure 路径**、同源同字号同行高，自动处理 CJK 回退链。
//! 2. [`CellGrid`]：视口逻辑宽高 → 行列格数，`cols = floor(宽 / cell_w)`。
//!
//! # 为什么 `MonoMetrics` 不塞进 `UiFonts`
//!
//! 实测需要 UI 布局管线先跑起来（探针节点要有 `ComputedNode`），而 `UiFonts`
//! 在 Startup 就必须可用——塞进去会形成「等 `UiFonts` → 等布局 → 等 `UiFonts`」
//! 的循环。故 `MonoMetrics` 是独立 `Resource`，由测量系统在首帧之后注入。

use bevy::prelude::*;
use bevy::text::{ComputedTextBlock, FontSize, LineHeight, TextFont};

use crate::fonts::UiFonts;
use crate::theme::{px, type_scale};

/// 双宽比例的容差（相对 `2 × latin_advance`）。
///
/// 设计系统 C-FONT-4：`cjk_advance` 应约等于 `2 × latin_advance`。CJK 回退字体
/// 多为比例字体（汉字步进 1em，而等宽拉丁步进 0.6em），实测值往往**不**满足该
/// 关系——此时记警告并调整单元格基准宽（见 [`MonoMetrics::check`]）。
pub const CJK_ADVANCE_TOLERANCE: f32 = 0.1;

/// 等宽槽位的实测度量（逻辑像素）。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct MonoMetrics {
    /// 窄字符（拉丁）的前进宽度。
    pub latin_advance: f32,
    /// 宽字符（CJK 全角 / emoji）的前进宽度。
    pub cjk_advance: f32,
    /// 行高。
    pub line_height: f32,
    /// 实测时的字号。
    pub at_size: f32,
}

/// 双宽校验的结论。
#[derive(Debug, Clone, PartialEq)]
pub struct MonoMetricsCheck {
    /// 单元格基准宽（逻辑像素）——一个窄格占多宽。
    pub cell_w: f32,
    /// 是否满足 `cjk_advance ≈ 2 × latin_advance`。
    pub double_width_ok: bool,
    /// 不满足时的告警文案。
    pub warning: Option<String>,
}

impl MonoMetrics {
    /// 校验双宽比例并给出单元格基准宽（纯函数）。
    ///
    /// - 满足 `cjk_advance ≈ 2 × latin_advance` → 基准宽 = `latin_advance`。
    /// - 不满足 → **告警**，基准宽 = `max(latin_advance, cjk_advance / 2)`：
    ///   基准宽不得小于窄字符步进（否则窄字符互相压字），又应容得下宽字符
    ///   （否则宽字符溢出到下一格）。
    pub fn check(&self) -> MonoMetricsCheck {
        if self.latin_advance <= 0.0 {
            return MonoMetricsCheck {
                cell_w: 0.0,
                double_width_ok: false,
                warning: Some("等宽度量尚未实测完成（latin_advance ≤ 0）".to_string()),
            };
        }
        let expected = self.latin_advance * 2.0;
        let deviation = (self.cjk_advance - expected).abs();
        let double_width_ok = deviation <= expected * CJK_ADVANCE_TOLERANCE;
        let cell_w = if double_width_ok {
            self.latin_advance
        } else {
            self.latin_advance.max(self.cjk_advance / 2.0)
        };
        let warning = if double_width_ok {
            None
        } else {
            Some(format!(
                "等宽 CJK 步进不是拉丁步进的 2 倍：latin={:.3}px cjk={:.3}px \
                 （期望 {:.3}px，偏差 {:.1}%）→ 单元格基准宽调整为 {:.3}px；\
                 宽字符将按 2 格定位，与相邻窄字符之间留有间隙",
                self.latin_advance,
                self.cjk_advance,
                expected,
                if expected > 0.0 {
                    deviation / expected * 100.0
                } else {
                    0.0
                },
                cell_w
            ))
        };
        MonoMetricsCheck {
            cell_w,
            double_width_ok,
            warning,
        }
    }

    /// 单元格基准宽（逻辑像素）——含双宽校验后的调整。
    pub fn cell_width(&self) -> f32 {
        self.check().cell_w
    }
}

/// 终端单元格网格（视口 → 行列格数）。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct CellGrid {
    /// 列数（格）。
    pub cols: usize,
    /// 行数（格）。
    pub rows: usize,
    /// 单元格基准宽（逻辑像素）。
    pub cell_w: f32,
    /// 行高（逻辑像素）。
    pub line_h: f32,
}

impl Default for CellGrid {
    fn default() -> Self {
        // 等宽度量未实测完成前的占位（cell_w = 0 → 列数退化为 1）。
        Self {
            cols: 1,
            rows: 1,
            cell_w: 0.0,
            line_h: 0.0,
        }
    }
}

impl CellGrid {
    /// 由视口逻辑宽高与实测度量派生。
    ///
    /// `cols = floor(视口逻辑宽 / cell_w)`；`rows = floor(视口逻辑高 / line_h)`。
    pub fn from_viewport(width: f32, height: f32, metrics: &MonoMetrics) -> Self {
        let cell_w = metrics.cell_width();
        let line_h = metrics.line_height;
        let cols = if cell_w > 0.0 {
            (width / cell_w).floor().max(1.0) as usize
        } else {
            1
        };
        let rows = if line_h > 0.0 {
            (height / line_h).floor().max(1.0) as usize
        } else {
            1
        };
        Self {
            cols,
            rows,
            cell_w,
            line_h,
        }
    }
}

/// 探针类别。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonoProbeKind {
    /// 窄字符样本。
    Latin,
    /// 宽字符样本。
    Cjk,
}

/// 等宽实测探针节点标记。
#[derive(Component, Debug, Clone, Copy)]
pub struct MonoProbe {
    /// 探针类别。
    pub kind: MonoProbeKind,
    /// 样本字符数（用于把总宽除回单字符步进）。
    pub samples: usize,
}

/// 窄字符样本的字符数。
const LATIN_SAMPLES: usize = 32;
/// 宽字符样本的字符数。
const CJK_SAMPLES: usize = 16;

/// 探针节点：挂在 UI 根下、绝对定位到视口外，`Visibility::Hidden` 只隐藏渲染、
/// **保留布局**（bevy 0.20 语义：隐藏但保留布局用 `Visibility`，用 `Display::None`
/// 才会被布局系统跳过）。
fn spawn_probe(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    kind: MonoProbeKind,
    sample: char,
    samples: usize,
    fonts: &UiFonts,
) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(-100_000.0),
            top: px(-100_000.0),
            width: Val::Auto,
            height: Val::Auto,
            ..default()
        },
        Visibility::Hidden,
        Text::new(sample.to_string().repeat(samples)),
        TextFont {
            font: fonts.mono.clone(),
            font_size: FontSize::Px(type_scale::CODE),
            ..default()
        },
        TextColor(Color::WHITE),
        LineHeight::RelativeToFont(type_scale::line_height::TERM),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        },
        MonoProbe { kind, samples },
    ));
}

/// 生成等宽实测探针（每个 App 只生成一次）。
///
/// 探针在 `Update` 生成而非 `Startup`：`UiFonts` 由宿主在 `Startup` 注入，
/// Startup 内的 `commands.insert_resource` 要到调度结束才生效。
pub fn spawn_mono_probes(
    mut commands: Commands,
    q_root: Query<Entity, With<crate::layout::UiRoot>>,
    q_probe: Query<(), With<MonoProbe>>,
    fonts: Option<Res<UiFonts>>,
) {
    if !q_probe.is_empty() {
        return;
    }
    let (Some(fonts), Ok(root)) = (fonts, q_root.single()) else {
        return;
    };
    commands.entity(root).with_children(|p| {
        spawn_probe(p, MonoProbeKind::Latin, '0', LATIN_SAMPLES, &fonts);
        spawn_probe(p, MonoProbeKind::Cjk, '中', CJK_SAMPLES, &fonts);
    });
}

/// 读取探针的排版结果，反推等宽度量并注入 [`MonoMetrics`]。
///
/// 必须在 bevy 文本管线排版**之后**跑（`UiSystems::PostLayout`）。度量已存在
/// 时不再重复测量。
///
/// 度量取自 [`ComputedTextBlock`] 的 parley layout（**未取整**的原始值）——比
/// `TextLayoutInfo.size` 好在后者是 `ceil` 过的（12px × 1.6 行高会被抬成
/// 20.0 而不是 19.2），误差会直接进到单元格基准宽上。
pub fn collect_mono_metrics(
    q_probes: Query<(&MonoProbe, &ComputedTextBlock)>,
    existing: Option<Res<MonoMetrics>>,
    mut commands: Commands,
) {
    if existing.is_some() {
        return;
    }
    let mut latin: Option<f32> = None;
    let mut cjk: Option<f32> = None;
    let mut line_height: Option<f32> = None;
    for (probe, block) in &q_probes {
        let layout = block.buffer();
        let scale = layout.scale();
        if scale <= 0.0 {
            continue;
        }
        let width = layout.full_width();
        if width <= 0.0 {
            continue;
        }
        // layout 的单位是物理像素；除以缩放因子还原逻辑像素。
        let advance = (width / scale) / probe.samples as f32;
        match probe.kind {
            MonoProbeKind::Latin => latin = Some(advance),
            MonoProbeKind::Cjk => cjk = Some(advance),
        }
        if line_height.is_none() {
            let height = layout.height();
            if height > 0.0 {
                line_height = Some(height / scale);
            }
        }
    }
    let (Some(latin_advance), Some(cjk_advance)) = (latin, cjk) else {
        return;
    };
    let metrics = MonoMetrics {
        latin_advance,
        cjk_advance,
        line_height: line_height.unwrap_or(type_scale::CODE * type_scale::line_height::TERM),
        at_size: type_scale::CODE,
    };
    let check = metrics.check();
    match &check.warning {
        Some(warning) => {
            tracing::warn!("终端等宽度量：{warning}");
            tracing::info!(
                "终端等宽度量：latin={:.3}px cjk={:.3}px 行高={:.3}px 字号={:.1}px 单元格宽={:.3}px",
                metrics.latin_advance,
                metrics.cjk_advance,
                metrics.line_height,
                metrics.at_size,
                check.cell_w
            );
        }
        None => {
            tracing::info!(
                "终端等宽度量：latin={:.3}px cjk={:.3}px 行高={:.3}px 字号={:.1}px 单元格宽={:.3}px（双宽比例正常）",
                metrics.latin_advance,
                metrics.cjk_advance,
                metrics.line_height,
                metrics.at_size,
                check.cell_w
            );
        }
    }
    commands.insert_resource(metrics);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一组度量。
    fn metrics(latin: f32, cjk: f32) -> MonoMetrics {
        MonoMetrics {
            latin_advance: latin,
            cjk_advance: cjk,
            line_height: 19.2,
            at_size: 12.0,
        }
    }

    #[test]
    fn double_width_ratio_ok_uses_latin_advance() {
        // 等宽 CJK：cjk 正好是 latin 的 2 倍
        let check = metrics(7.2, 14.4).check();
        assert!(check.double_width_ok);
        assert!(check.warning.is_none());
        assert!((check.cell_w - 7.2).abs() < 1e-6);
    }

    #[test]
    fn double_width_ratio_violation_warns_and_widens_cell() {
        // 比例 CJK 回退字体：汉字步进 1em = 12px < 2 × 7.2 = 14.4px
        let check = metrics(7.2, 12.0).check();
        assert!(!check.double_width_ok);
        let warning = check.warning.expect("应给出告警");
        assert!(warning.contains("2 倍"), "告警应说明双宽关系：{warning}");
        // 基准宽不得小于窄字符步进
        assert!((check.cell_w - 7.2).abs() < 1e-6);
    }

    #[test]
    fn double_width_ratio_violation_widens_cell_when_cjk_is_wider() {
        // 极端：CJK 步进比 2 倍拉丁还宽 → 基准宽取 cjk / 2
        let check = metrics(7.0, 20.0).check();
        assert!(!check.double_width_ok);
        assert!((check.cell_w - 10.0).abs() < 1e-6, "基准宽应为 cjk/2 = 10");
    }

    #[test]
    fn unmeasured_metrics_do_not_panic() {
        let check = metrics(0.0, 0.0).check();
        assert!(!check.double_width_ok);
        assert!(check.warning.is_some());
        assert_eq!(check.cell_w, 0.0);
    }

    #[test]
    fn cell_grid_cols_is_floor_of_width_over_cell_width() {
        let m = metrics(7.2, 14.4);
        let grid = CellGrid::from_viewport(500.0, 300.0, &m);
        assert_eq!(grid.cols, 69, "floor(500 / 7.2) = 69");
        assert_eq!(grid.rows, 15, "floor(300 / 19.2) = 15");
        assert!((grid.cell_w - 7.2).abs() < 1e-6);
        assert!((grid.line_h - 19.2).abs() < 1e-6);
    }

    #[test]
    fn cell_grid_never_reports_zero() {
        let m = metrics(0.0, 0.0);
        let grid = CellGrid::from_viewport(0.0, 0.0, &m);
        assert_eq!(grid.cols, 1);
        assert_eq!(grid.rows, 1);
    }
}
