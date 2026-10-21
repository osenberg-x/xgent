//! 迁移 shim（M5-T1 引入，M5-T6 拆除）。
//!
//! 把旧的 `SideViewContent` closed 枚举**一次性**映射到注册表面板 id：
//! 枚举 → 描述符 id 是纯函数，与注册表的一致性由单测守住。
//!
//! shim 的存在只是为了让 M5 的改动可分小批合入、每批都能编译；产品代码不再
//! 依赖该枚举（读侧改读注册表，写侧改发面板消息）。M5-T6 连同枚举一并删除。

use crate::editor::SideViewContent;
use crate::panels::ids;

/// 枚举变体 → 面板 id（纯函数）。
///
/// `None` 表示分屏没有内容（无面板可见）→ `None`。
/// `Preview` 与 `Editor` 同体（预览页由编辑器视图承载），故映射到同一个面板 id。
pub fn panel_id(content: SideViewContent) -> Option<&'static str> {
    match content {
        SideViewContent::None => None,
        SideViewContent::Editor | SideViewContent::Preview => Some(ids::EDITOR),
        SideViewContent::Diff => Some(ids::DIFF),
        SideViewContent::Terminal => Some(ids::TERMINAL),
    }
}

/// 面板 id → 枚举变体（纯函数；`SideDock` 槽位之外的面板返回 `None`）。
pub fn content_of(panel_id: &str) -> Option<SideViewContent> {
    match panel_id {
        ids::EDITOR => Some(SideViewContent::Editor),
        ids::DIFF => Some(SideViewContent::Diff),
        ids::TERMINAL => Some(SideViewContent::Terminal),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::{PanelDescriptor, PanelRegistry, Slot};

    /// shim 是纯函数：同一输入恒返回同一 id。
    #[test]
    fn mapping_is_pure() {
        for content in [
            SideViewContent::None,
            SideViewContent::Editor,
            SideViewContent::Preview,
            SideViewContent::Diff,
            SideViewContent::Terminal,
        ] {
            assert_eq!(panel_id(content), panel_id(content));
        }
    }

    /// shim 的每个变体都能在注册表里查到唯一描述符。
    #[test]
    fn every_variant_maps_to_a_registered_panel() {
        let mut reg = PanelRegistry::default();
        for id in [ids::EDITOR, ids::DIFF, ids::TERMINAL] {
            reg.register(
                PanelDescriptor::new(id, Slot::SideDock, "t", "chat", 0),
                None,
            );
        }
        for content in [
            SideViewContent::Editor,
            SideViewContent::Preview,
            SideViewContent::Diff,
            SideViewContent::Terminal,
        ] {
            let id = panel_id(content).expect("非 None 变体应有映射");
            assert!(reg.contains(id), "shim 变体 {content:?} 映射到未注册的 {id}");
        }
    }
}
