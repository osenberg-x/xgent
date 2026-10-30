//! 3D 空间视图（MVP-2）。

pub mod scan;
pub mod view;

pub use scan::ScanJob;
pub use view::{
    SpaceCamera, SpaceEdge, SpaceGraph, SpaceHudLabel, SpaceMaterials, SpacePickState, SpacePlugin,
    SpaceQualityBadge, SpaceTarget, SpaceViewportMarker, SpatialStar, ViewQuality, ndc_to_viewport,
    nearest_hit, target_size,
};
