//! 分割模型（修 D6）：`SplitTree` / `Split` / `LeafSize`。
//!
//! 布局架构 §3.4：区域之间的分割关系**声明式**注册，用户只能改尺寸、不能改
//! 拓扑（不做自由 docking / 拖拽重排）。`resize.rs` 遍历本树生成分割条，
//! 因此**注册一个新的纵向分割不需要改 `resize.rs`**。
//!
//! 本模块是**纯数据 + 纯函数**（不依赖 Bevy 渲染），可脱离渲染快速单测。

use bevy::prelude::Resource;

/// 分割轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// 水平排列（左右分割，拖拽改宽度）。
    Horizontal,
    /// 纵向排列（上下分割，拖拽改高度）。
    Vertical,
}

/// 叶节点的尺寸策略。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LeafSize {
    /// 固定尺寸（逻辑像素）。
    Fixed(f32),
    /// 最小尺寸 + 首选尺寸（逻辑像素）。
    Min(f32, f32),
    /// 按权重伸展（其余空间按权重分配）。
    Grow(f32),
}

impl LeafSize {
    /// 首选尺寸。
    pub fn preferred(&self) -> f32 {
        match self {
            LeafSize::Fixed(v) => *v,
            LeafSize::Min(_, preferred) => *preferred,
            LeafSize::Grow(weight) => *weight,
        }
    }

    /// 最小尺寸。
    pub fn min(&self) -> f32 {
        match self {
            LeafSize::Fixed(v) => *v,
            LeafSize::Min(min, _) => *min,
            LeafSize::Grow(weight) => *weight,
        }
    }

    /// 把给定尺寸钳制到本策略允许的区间（上限由调用方给出）。
    pub fn clamp(&self, value: f32, max: f32) -> f32 {
        let min = self.min();
        let upper = if max < min { min } else { max };
        value.clamp(min, upper)
    }

    /// 写回当前尺寸（拖拽后）：`Min` 保留下限，只改当前值。
    ///
    /// `Min(min, preferred)` 的 `preferred` 兼作**当前尺寸**——拖拽写回它，
    /// 下限 `min` 不变。
    pub fn with_current(&self, value: f32) -> LeafSize {
        match self {
            LeafSize::Fixed(_) => *self,
            LeafSize::Min(min, _) => LeafSize::Min(*min, value),
            LeafSize::Grow(weight) => LeafSize::Grow(*weight),
        }
    }
}

/// 分割树的节点。
#[derive(Debug, Clone)]
pub enum Split {
    /// 叶节点：一个具名区域。
    Leaf {
        /// 区域 id。
        id: &'static str,
        /// 尺寸策略。
        size: LeafSize,
    },
    /// 分割节点：沿 `axis` 排列若干子节点，子节点之间各有一条手柄。
    Splitter {
        /// 分割轴。
        axis: Axis,
        /// 子节点（按主轴顺序）。
        children: Vec<Split>,
        /// 手柄状态（与 `children` 的相邻边界一一对应）。
        handles: Vec<HandleState>,
    },
}

/// 手柄状态：记录该手柄调整哪两个子节点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleState {
    /// 被调整的子节点下标（手柄位于 `children[i]` 与 `children[i + 1]` 之间）。
    pub target: usize,
}

/// 一个分割手柄（供 `resize.rs` 生成拖拽手柄节点）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitHandle {
    /// 手柄在分割节点中的下标（位于 `children[index]` 与 `children[index + 1]` 之间）。
    pub index: usize,
    /// 分割轴。
    pub axis: Axis,
    /// 主轴负向一侧的边界叶子 id。
    pub before: &'static str,
    /// 主轴正向一侧的边界叶子 id。
    pub after: &'static str,
}

impl SplitHandle {
    /// 该手柄实际调整的叶子 id。
    ///
    /// 规则：两侧叶子中**非弹性**的那个（`Grow` 的叶子吸收剩余空间，不被拖动
    /// 调整）——如「抽屉 ↔ 对话区」调抽屉、「对话区 ↔ 右侧分屏」调右侧分屏。
    pub fn adjustable_leaf(&self, tree: &SplitTree) -> Option<&'static str> {
        let before = tree.leaf_size(self.before)?;
        let after = tree.leaf_size(self.after)?;
        match (before, after) {
            (LeafSize::Grow(_), LeafSize::Grow(_)) => Some(self.before),
            (LeafSize::Grow(_), _) => Some(self.after),
            (_, _) => Some(self.before),
        }
    }
}

/// 叶子 id 常量（默认分割树）。
pub mod leaf_ids {
    /// 文件抽屉（覆盖层，宽度独立可拖）。
    pub const DRAWER: &str = "drawer";
    /// 对话主区（弹性，填满剩余空间）。
    pub const CHAT: &str = "chat";
    /// 右侧分屏（编辑器 / 差异 / 终端）。
    pub const SIDE_DOCK: &str = "side-dock";
}

/// 分割树（区域布局的声明式描述）。
#[derive(Resource, Debug, Clone)]
pub struct SplitTree {
    /// 根节点（`None` 表示无分割）。
    pub root: Option<Split>,
}

impl Default for SplitTree {
    /// 默认分割树：抽屉（覆盖层，可拖宽）+ 对话主区（弹性）+ 右侧分屏。
    ///
    /// 抽屉宽度从定值 320 改为 `Min(240, Preferred(320))`（上限 480，见
    /// `crate::theme::size::DRAWER_W_MAX`）；右侧分屏默认按经典编辑器坞的 720
    /// 起步，宇宙模式再由 `resize::apply_chat_side_preference` 落到对话侧栏偏好
    /// （默认 400、下限 320）。
    fn default() -> Self {
        Self {
            root: Some(Split::Splitter {
                axis: Axis::Horizontal,
                children: vec![
                    Split::Leaf {
                        id: leaf_ids::DRAWER,
                        size: LeafSize::Min(240.0, 320.0),
                    },
                    Split::Leaf {
                        id: leaf_ids::CHAT,
                        size: LeafSize::Grow(1.0),
                    },
                    Split::Leaf {
                        id: leaf_ids::SIDE_DOCK,
                        // 默认按经典编辑器坞的宽度起步；宇宙模式下
                        // `resize::apply_chat_side_preference` 会把它落到对话侧栏
                        // 偏好（默认 400）。
                        size: LeafSize::Min(
                            crate::theme::size::CHAT_SIDE_W_MIN,
                            crate::theme::size::CONTEXT_W_DEFAULT,
                        ),
                    },
                ],
                handles: vec![HandleState { target: 0 }, HandleState { target: 1 }],
            }),
        }
    }
}

impl SplitTree {
    /// 构造一棵只有单个叶子的树。
    pub fn leaf(id: &'static str, size: LeafSize) -> Self {
        Self {
            root: Some(Split::Leaf { id, size }),
        }
    }

    /// 全部叶子 id（深度优先，按主轴顺序）。
    pub fn leaf_ids(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            collect_leaf_ids(root, &mut out);
        }
        out
    }

    /// 全部手柄（深度优先；`resize.rs` 据此生成分割条）。
    pub fn handles(&self) -> Vec<SplitHandle> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            collect_handles(root, &mut out);
        }
        out
    }

    /// 叶子所在分割节点的轴（沿该轴排布）。
    ///
    /// 轴向**取自分割树**而非按 id 猜——新增叶子（无论横纵）都自动得到正确的轴。
    pub fn leaf_axis(&self, id: &str) -> Option<Axis> {
        fn find(split: &Split, id: &str, parent: Option<Axis>) -> Option<Axis> {
            match split {
                Split::Leaf { id: leaf_id, .. } => {
                    (*leaf_id == id).then_some(parent.unwrap_or(Axis::Horizontal))
                }
                Split::Splitter { axis, children, .. } => {
                    children.iter().find_map(|c| find(c, id, Some(*axis)))
                }
            }
        }
        self.root.as_ref().and_then(|r| find(r, id, None))
    }

    /// 取叶子的尺寸策略。
    pub fn leaf_size(&self, id: &str) -> Option<LeafSize> {
        fn find(split: &Split, id: &str) -> Option<LeafSize> {
            match split {
                Split::Leaf { id: leaf_id, size } => (*leaf_id == id).then_some(*size),
                Split::Splitter { children, .. } => children.iter().find_map(|c| find(c, id)),
            }
        }
        self.root.as_ref().and_then(|r| find(r, id))
    }

    /// 写回叶子的尺寸策略（拖拽后调用）。
    pub fn set_leaf_size(&mut self, id: &str, size: LeafSize) -> bool {
        fn set(split: &mut Split, id: &str, size: LeafSize) -> bool {
            match split {
                Split::Leaf {
                    id: leaf_id,
                    size: current,
                } => {
                    if *leaf_id == id {
                        *current = size;
                        true
                    } else {
                        false
                    }
                }
                Split::Splitter { children, .. } => children.iter_mut().any(|c| set(c, id, size)),
            }
        }
        match &mut self.root {
            Some(root) => set(root, id, size),
            None => false,
        }
    }
}

/// 深度优先收集叶子 id。
fn collect_leaf_ids(split: &Split, out: &mut Vec<&'static str>) {
    match split {
        Split::Leaf { id, .. } => out.push(id),
        Split::Splitter { children, .. } => {
            for child in children {
                collect_leaf_ids(child, out);
            }
        }
    }
}

/// 深度优先收集手柄。
///
/// 每个分割节点的 `children[i]` 与 `children[i + 1]` 之间有一条手柄；手柄记录
/// 两侧**边界叶���**的 id（前一个子树的最后一个叶子 / 后一个子树的第一个叶子）。
fn collect_handles(split: &Split, out: &mut Vec<SplitHandle>) {
    if let Split::Splitter {
        axis,
        children,
        handles,
    } = split
    {
        for (i, handle) in handles.iter().enumerate() {
            let Some(before_node) = children.get(i) else {
                continue;
            };
            let Some(after_node) = children.get(i + 1) else {
                continue;
            };
            let Some(before) = last_leaf(before_node) else {
                continue;
            };
            let Some(after) = first_leaf(after_node) else {
                continue;
            };
            out.push(SplitHandle {
                index: handle.target,
                axis: *axis,
                before,
                after,
            });
        }
        for child in children {
            collect_handles(child, out);
        }
    }
}

/// 取节点的第一个叶子 id。
fn first_leaf(split: &Split) -> Option<&'static str> {
    match split {
        Split::Leaf { id, .. } => Some(id),
        Split::Splitter { children, .. } => children.first().and_then(first_leaf),
    }
}

/// 取节点的最后一个叶子 id。
fn last_leaf(split: &Split) -> Option<&'static str> {
    match split {
        Split::Leaf { id, .. } => Some(id),
        Split::Splitter { children, .. } => children.last().and_then(last_leaf),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> SplitTree {
        SplitTree {
            root: Some(Split::Splitter {
                axis: Axis::Horizontal,
                children: vec![
                    Split::Leaf {
                        id: "drawer",
                        size: LeafSize::Min(240.0, 320.0),
                    },
                    Split::Splitter {
                        axis: Axis::Vertical,
                        children: vec![
                            Split::Leaf {
                                id: "main",
                                size: LeafSize::Grow(1.0),
                            },
                            Split::Leaf {
                                id: "side",
                                size: LeafSize::Min(380.0, 720.0),
                            },
                        ],
                        handles: vec![HandleState { target: 0 }],
                    },
                ],
                handles: vec![HandleState { target: 0 }],
            }),
        }
    }

    /// 叶子收集（深度优先、主轴顺序）。
    #[test]
    fn leaf_ids_are_collected_in_order() {
        assert_eq!(tree().leaf_ids(), vec!["drawer", "main", "side"]);
    }

    /// 手柄遍历：横向 1 条 + 纵向 1 条 = 2 条。
    #[test]
    fn handles_are_traversed_recursively() {
        let handles = tree().handles();
        assert_eq!(handles.len(), 2);
        assert_eq!(handles[0].axis, Axis::Horizontal);
        assert_eq!(handles[0].before, "drawer");
        assert_eq!(handles[0].after, "main");
        assert_eq!(handles[1].axis, Axis::Vertical);
        assert_eq!(handles[1].before, "main");
        assert_eq!(handles[1].after, "side");
    }

    /// 尺寸写回（拖拽后）。
    #[test]
    fn leaf_size_round_trips() {
        let mut t = tree();
        assert_eq!(t.leaf_size("drawer"), Some(LeafSize::Min(240.0, 320.0)));
        assert!(t.set_leaf_size("drawer", LeafSize::Min(240.0, 480.0)));
        assert_eq!(t.leaf_size("drawer"), Some(LeafSize::Min(240.0, 480.0)));
        assert!(!t.set_leaf_size("nope", LeafSize::Fixed(1.0)));
    }

    /// 尺寸钳制：抽屉宽度落在 [240, 480]。
    #[test]
    fn drawer_width_is_clamped() {
        let size = LeafSize::Min(240.0, 320.0);
        assert_eq!(size.clamp(100.0, 480.0), 240.0);
        assert_eq!(size.clamp(600.0, 480.0), 480.0);
        assert_eq!(size.clamp(300.0, 480.0), 300.0);
    }
}
