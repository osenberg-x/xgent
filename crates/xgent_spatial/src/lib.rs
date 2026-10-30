//! `xgent_spatial` — 3D 空间视图的 S0 内核。
//!
//! 职责边界（与 `docs/design/ui-v8-3d-viewport.md` §3 一致）：
//! - 把工作区的 crate 依赖投影成 [`xgent_core::spatial::SpatialGraph`]；
//! - 为图计算**确定性**的三维坐标，支持增量更新时钉住既有节点；
//! - 不依赖 Bevy，不读写 UI 状态，也不成为任何代码事实的真相源。
//!
//! S0 只产出 crate 级（L0）图。文件、符号、`use` 边与时间轴留给 S1/S2。

pub mod error;
pub mod layout;
pub mod manifest;
pub mod workspace;

pub use error::SpatialError;
pub use layout::{LayoutConfig, LayoutReport, layout, layout_incremental};
pub use manifest::{DependencyKind, ManifestDependency, ManifestInfo, parse_manifest};
pub use workspace::{ScanOptions, scan, scan_with_options};
