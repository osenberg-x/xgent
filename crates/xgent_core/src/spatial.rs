//! 3D 空间视图的数据契约（F-16 S0）。
//!
//! 这里的类型是「只读投影」的载体：由数据源（工作区扫描、文件索引、git、agent 活动）
//! 填充，由布局内核回填 `anchor`，3D 视口只消费、不写回任何代码事实。
//!
//! 约束：
//! - 不依赖 Bevy，可在 daemon / TUI / 测试中直接使用。
//! - 所有字段可 serde 序列化，便于后续跨进程或插件传递。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 空间节点标识。由数据源按稳定顺序分配。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpatialNodeId(pub u32);

/// 星团（cluster）标识。L0 下通常一个 crate 对应一个星团。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClusterId(pub u32);

/// 节点种类。S0 只产出 `Crate`，其余留给 S1/S2 的文件索引与符号图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpatialNodeKind {
    /// crate（S0 的 L0 节点）。
    Crate,
    /// 模块/目录（L1）。
    Module,
    /// 文件（L1）。
    File,
    /// 符号：函数、结构体等（L2）。
    Symbol,
    /// 工作区外部的依赖。
    External,
}

/// 节点状态。语义与工具状态一致（`st_running` / `st_fail`），不自造状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpatialStatus {
    /// 空闲。
    #[default]
    Idle,
    /// 有未提交改动。
    Changed,
    /// Agent 正在改动。
    Running,
    /// 失败或冲突。
    Failed,
}

/// 边的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeKind {
    /// crate 依赖（Cargo.toml）。
    DependsOn,
    /// 包含关系（目录 → 文件）。
    Contains,
    /// `use` / `mod` 引用（L1）。
    Imports,
    /// 经常一起改动（时间轴聚合）。
    ChangedTogether,
    /// Agent 在同一回合触碰过。
    AgentTouched,
}

/// 空间节点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialNode {
    /// 稳定标识。
    pub id: SpatialNodeId,
    /// 节点种类。
    pub kind: SpatialNodeKind,
    /// 展示名（不含路径）。
    pub label: String,
    /// 文件/目录路径；crate 节点指向 crate 根目录。
    pub path: Option<PathBuf>,
    /// 所属星团。
    pub cluster: ClusterId,
    /// 权重：S0 为 1.0，S1 起改为 LOC 的对数值。
    pub weight: f32,
    /// 当前状态。
    pub status: SpatialStatus,
    /// 布局内核回填的三维坐标；`None` 表示尚未布局。
    pub anchor: Option<[f32; 3]>,
}

/// 星团。L0 下每个 crate 一个。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialCluster {
    /// 稳定标识。
    pub id: ClusterId,
    /// 展示名（crate 名）。
    pub label: String,
    /// crate 根目录。
    pub path: Option<PathBuf>,
    /// 权重：S0 为 1.0。
    pub weight: f32,
    /// 布局内核回填的三维坐标。
    pub anchor: Option<[f32; 3]>,
}

/// 空间边。方向语义由 `kind` 决定：`DependsOn` 的 `from` 是依赖方。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialEdge {
    /// 起点。
    pub from: SpatialNodeId,
    /// 终点。
    pub to: SpatialNodeId,
    /// 边种类。
    pub kind: EdgeKind,
    /// 权重：S0 为 1.0。
    pub weight: f32,
}

/// 一次投影的完整快照。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpatialGraph {
    /// 图内容代数；同一输入必须得到同一值，用于判断是否需要重算布局。
    pub generation: u64,
    /// 节点，按稳定顺序排列。
    pub nodes: Vec<SpatialNode>,
    /// 边，按 `(from, to, kind)` 稳定排序。
    pub edges: Vec<SpatialEdge>,
    /// 星团，按稳定顺序排列。
    pub clusters: Vec<SpatialCluster>,
}

impl SpatialGraph {
    /// 按 id 查节点。节点数量小（S0 为 crate 级），线性查找足够。
    pub fn node(&self, id: SpatialNodeId) -> Option<&SpatialNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// 按 id 查星团。
    pub fn cluster(&self, id: ClusterId) -> Option<&SpatialCluster> {
        self.clusters.iter().find(|cluster| cluster.id == id)
    }

    /// 是否为空图。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
