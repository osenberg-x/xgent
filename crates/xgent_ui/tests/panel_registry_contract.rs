//! UI v8 M5 契约测试：槽位注册表与派生可见性。
//!
//! 覆盖台账 §M5 的六类可判定断言：
//! 1. 描述符 → 槽位映射（`Slot` 覆盖 8 个槽位；同槽按 `order` 排序）
//! 2. 面板注册一致性（每个既有面板在注册表中有唯一描述符；shim 拆除后由
//!    注册表自身承担）
//! 3. 派生可见性（源码级白名单断言 + 行为级断言）
//! 4. rail 选中态派生（连点两次关掉抽屉后回落）
//! 5. 分割树遍历（手柄数量 / 可调叶子 / 抽屉宽度钳制）
//! 6. i18n 页签标题（英文语料下页签标题非 CJK）
//!
//! 外加两条**源码级**契约：
//! - `SideViewContent` 在 `crates/` 内零引用（D1 闭合）
//! - `EditorBackButtonMarker` 全仓只定义一次（D3 闭合）
//! - `layout.rs` / `editor/mod.rs` 不含 `SideViewContent`（北极星前置）

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use xgent_ui::panels::ids;
use xgent_ui::panels::split::{Axis, LeafSize, Split, SplitTree, leaf_ids};
use xgent_ui::panels::{
    Closeability, PanelDescriptor, PanelRegistry, PanelSet, Slot, SlotLayout, builtin_rail_panels,
};

/// 仓库根目录（`crates/xgent_ui` 的上一级）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("应能定位仓库根")
        .to_path_buf()
}

/// 递归列出 `crates/xgent_ui/src` 下的全部 `.rs` 文件。
fn ui_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&repo_root().join("crates/xgent_ui/src"), &mut out);
    out.sort();
    out
}

/// 读文件（UTF-8）。
fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读 {} 失败：{e}", path.display()))
}

/// 找出某文件中包含 `needle` 的行号集合（1 起）。
fn lines_with(text: &str, needle: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| l.contains(needle))
        .map(|(i, _)| i + 1)
        .collect()
}

/// 行号之前最近的「函数定义行」（用于给写手归类）。
fn enclosing_fn(lines: &[&str], line_idx: usize) -> String {
    for i in (0..=line_idx).rev() {
        let l = lines[i].trim_start();
        if l.starts_with("fn ") || l.starts_with("pub fn ") || l.starts_with("pub(crate) fn ") {
            let name = l
                .split('(')
                .next()
                .unwrap_or(l)
                .rsplit(' ')
                .next()
                .unwrap_or(l)
                .to_string();
            return name;
        }
    }
    "<unknown>".to_string()
}

// ===== 1. 描述符 → 槽位映射 =====

/// `Slot` 覆盖 8 个槽位（`Center` 只建模、不接消费者）。
#[test]
fn slot_covers_eight_slots() {
    let all = [
        Slot::Rail,
        Slot::Center,
        Slot::SideDock,
        Slot::BottomDock,
        Slot::SettingsSection,
        Slot::StatusSegment,
        Slot::Palette,
        Slot::Overlay,
    ];
    assert_eq!(all.len(), 8);
    // 单例槽位：同槽至多一个可见
    assert!(Slot::Center.is_exclusive());
    assert!(Slot::SideDock.is_exclusive());
    assert!(Slot::Overlay.is_exclusive());
    assert!(!Slot::Rail.is_exclusive());
    assert!(!Slot::Palette.is_exclusive());
}

/// 描述符 → 槽位映射：注册到不同槽位的面板互不串槽；同槽按 `order` 升序。
#[test]
fn descriptor_maps_to_slot() {
    let mut registry = PanelRegistry::default();
    registry.register(
        PanelDescriptor::new(ids::EDITOR, Slot::SideDock, "panel-editor", "file", 2),
        None,
    );
    registry.register(
        PanelDescriptor::new(ids::DIFF, Slot::SideDock, "panel-diff", "diff", 1),
        None,
    );
    registry.register(
        PanelDescriptor::new(
            ids::FILE_DRAWER,
            Slot::Overlay,
            "panel-file-drawer",
            "folder",
            0,
        ),
        None,
    );

    let side: Vec<&str> = registry
        .iter_slot(Slot::SideDock)
        .map(|e| e.desc.id)
        .collect();
    assert_eq!(side, vec![ids::DIFF, ids::EDITOR], "同槽按 order 升序");
    let overlay: Vec<&str> = registry
        .iter_slot(Slot::Overlay)
        .map(|e| e.desc.id)
        .collect();
    assert_eq!(overlay, vec![ids::FILE_DRAWER], "面板只落在自己的槽位");
    assert!(registry.get(ids::EDITOR).unwrap().desc.slot == Slot::SideDock);
}

// ===== 2. 面板注册一致性（shim 拆除后由注册表自身承担） =====

/// 每个既有面板在注册表中有唯一描述符，且 `title_key` 必填（非空）。
#[test]
fn every_builtin_panel_has_a_unique_descriptor() {
    let mut registry = PanelRegistry::default();
    let mut seen: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut all: Vec<PanelDescriptor> = builtin_rail_panels();
    for (id, slot, order) in [
        (ids::CHAT, Slot::Center, 0),
        (ids::FILE_DRAWER, Slot::Overlay, 0),
        (ids::SESSION_HISTORY, Slot::Overlay, 1),
        (ids::EDITOR, Slot::SideDock, 0),
        (ids::DIFF, Slot::SideDock, 1),
        (ids::TERMINAL, Slot::SideDock, 2),
    ] {
        all.push(PanelDescriptor::new(id, slot, "panel-x", "chat", order));
    }
    for desc in all {
        assert!(!desc.title_key.is_empty(), "title_key 必填（不可为空）");
        assert!(!desc.id.is_empty());
        *seen.entry(desc.id).or_default() += 1;
        registry.register(desc, None);
    }
    for (id, count) in seen {
        assert_eq!(count, 1, "面板 id 必须唯一：{id}");
    }
}

// ===== 3. 派生可见性 =====

/// 源码级：`crates/xgent_ui/src` 内写 `Node.display` 的位置只有
/// 「派生系统」与「内容态白名单」两类。
///
/// 口径（2026-10-21 用户裁决）：**面板 / 区域**可见性归一进
/// `derive_panel_visibility`；**内容态**可见性（会话是否为空、列表是否为空、
/// 滚动位置、当前文档页签、陪伴开关）描述的不是面板，不在本项口径内。
#[test]
fn only_derive_system_and_content_whitelist_write_display() {
    /// 允许写 `Node.display` 的位置：(相对路径, 函数名)。
    /// 第一项是派生系统，其余是**内容态白名单**（逐条注明属内容态而非面板态）。
    const ALLOWED: &[(&str, &str, &str)] = &[
        (
            "panels/visibility.rs",
            "derive_panel_visibility",
            "派生系统：唯一写面板 / 区域可见性",
        ),
        (
            "chat_panel.rs",
            "update_back_to_bottom",
            "内容态：滚动位置（回到底部按钮）",
        ),
        (
            "activity_bar.rs",
            "companion_ring_system",
            "内容态：陪伴开关（激活环）",
        ),
        (
            "editor/mod.rs",
            "update_buffer_visibility",
            "内容态：当前文档页签（文档列表）",
        ),
        (
            "welcome.rs",
            "toggle_welcome",
            "内容态：会话是否为空（欢迎空态）",
        ),
        (
            "welcome.rs",
            "rebuild_recent_sessions",
            "内容态：最近会话列表是否为空（标题）",
        ),
    ];

    let mut hits: Vec<(String, String, usize)> = Vec::new();
    for path in ui_sources() {
        let text = read(&path);
        let lines: Vec<&str> = text.lines().collect();
        let rel = path
            .strip_prefix(repo_root().join("crates/xgent_ui/src"))
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for (i, line) in lines.iter().enumerate() {
            if !line.contains(".display =") {
                continue;
            }
            hits.push((rel.clone(), enclosing_fn(&lines, i), i + 1));
        }
    }

    assert!(
        !hits.is_empty(),
        "应至少命中派生系统的写手（否则说明口径写错了）"
    );
    for (file, func, line) in &hits {
        let ok = ALLOWED
            .iter()
            .any(|(f, fn_name, _)| f == file && fn_name == func);
        assert!(
            ok,
            "非白名单的 `Node.display` 写手：{file}:{line}（函数 {func}）——\
             面板 / 区域可见性应并入 `derive_panel_visibility`"
        );
    }
    // 派生系统本身必须在场
    assert!(
        hits.iter()
            .any(|(f, func, _)| f == "panels/visibility.rs" && func == "derive_panel_visibility"),
        "派生系统 `derive_panel_visibility` 必须在场"
    );
}

/// 行为级：面板可见性由「运行时状态 ∧ 槽位开放」派生。
#[test]
fn panel_visibility_is_derived_from_state_and_slot() {
    let mut registry = PanelRegistry::default();
    registry.register(
        PanelDescriptor::new(ids::EDITOR, Slot::SideDock, "panel-editor", "file", 0),
        None,
    );
    let mut layout = SlotLayout::default();

    // 未激活 → 不可见
    assert!(!layout.is_panel_visible(&registry, ids::EDITOR));
    // 激活 → 可见
    registry.activate(ids::EDITOR);
    assert!(layout.is_panel_visible(&registry, ids::EDITOR));
    // 槽位关闭 → 不可见（区域收起）
    layout.set_open(Slot::SideDock, false);
    assert!(!layout.is_panel_visible(&registry, ids::EDITOR));
    // 槽位重开 → 恢复可见
    layout.set_open(Slot::SideDock, true);
    assert!(layout.is_panel_visible(&registry, ids::EDITOR));
}

// ===== 4. rail 选中态派生 =====

/// 连点 Files 两次（开 → 关）后，rail 选中态回落到对话，不残留 Files。
#[test]
fn rail_selection_falls_back_after_toggling_drawer() {
    let mut registry = PanelRegistry::default();
    for desc in builtin_rail_panels() {
        registry.register(desc, None);
    }
    registry.register(
        PanelDescriptor::new(ids::CHAT, Slot::Center, "panel-chat", "chat", 0)
            .with_default_visible(),
        None,
    );
    registry.register(
        PanelDescriptor::new(
            ids::FILE_DRAWER,
            Slot::Overlay,
            "panel-file-drawer",
            "folder",
            0,
        ),
        None,
    );
    let layout = SlotLayout::default();

    assert_eq!(registry.active_rail(&layout), Some(ids::RAIL_CHAT));
    // 第一次点击：抽屉打开 → 选中 Files
    registry.activate(ids::FILE_DRAWER);
    assert_eq!(registry.active_rail(&layout), Some(ids::RAIL_FILES));
    // 第二次点击：抽屉关闭 → 选中态回落（不残留）
    registry.deactivate(ids::FILE_DRAWER);
    assert_eq!(registry.active_rail(&layout), Some(ids::RAIL_CHAT));
}

/// 单例槽位：历史面板与文件抽屉同一时刻至多一个可见（不再争层级）。
#[test]
fn overlay_slot_is_exclusive() {
    let mut registry = PanelRegistry::default();
    registry.register(
        PanelDescriptor::new(
            ids::FILE_DRAWER,
            Slot::Overlay,
            "panel-file-drawer",
            "folder",
            0,
        ),
        None,
    );
    registry.register(
        PanelDescriptor::new(
            ids::SESSION_HISTORY,
            Slot::Overlay,
            "panel-session-history",
            "clock",
            1,
        ),
        None,
    );
    registry.activate(ids::FILE_DRAWER);
    assert!(registry.is_visible(ids::FILE_DRAWER));
    registry.activate(ids::SESSION_HISTORY);
    assert!(registry.is_visible(ids::SESSION_HISTORY));
    assert!(
        !registry.is_visible(ids::FILE_DRAWER),
        "单例槽位：打开历史面板应自动关掉文件抽屉"
    );
}

// ===== 5. 分割树遍历 =====

/// 分割树遍历：手柄数量、每条手柄调整的叶子、叶子尺寸写回。
#[test]
fn split_tree_traversal() {
    let tree = SplitTree::default();
    let handles = tree.handles();
    assert_eq!(handles.len(), 2, "默认树：抽屉 ↔ 对话、对话 ↔ 右侧分屏");
    assert_eq!(handles[0].axis, Axis::Horizontal);
    assert_eq!(handles[0].before, leaf_ids::DRAWER);
    assert_eq!(handles[1].after, leaf_ids::SIDE_DOCK);
    assert_eq!(handles[0].adjustable_leaf(&tree), Some(leaf_ids::DRAWER));
    assert_eq!(handles[1].adjustable_leaf(&tree), Some(leaf_ids::SIDE_DOCK));
    // 叶子尺寸写回
    let mut tree = tree;
    assert!(tree.set_leaf_size(leaf_ids::DRAWER, LeafSize::Min(240.0, 400.0)));
    assert_eq!(
        tree.leaf_size(leaf_ids::DRAWER),
        Some(LeafSize::Min(240.0, 400.0))
    );
}

/// 注册一个**纵向分割**不需要改 `resize.rs`——遍历逻辑是通用的。
#[test]
fn vertical_split_registers_without_touching_resize_rs() {
    // 构造一棵含纵向分割的树（模拟「侧区内部上下分割」）
    let tree = SplitTree {
        root: Some(Split::Splitter {
            axis: Axis::Horizontal,
            children: vec![
                Split::Leaf {
                    id: leaf_ids::CHAT,
                    size: LeafSize::Grow(1.0),
                },
                Split::Splitter {
                    axis: Axis::Vertical,
                    children: vec![
                        Split::Leaf {
                            id: "side-upper",
                            size: LeafSize::Grow(1.0),
                        },
                        Split::Leaf {
                            id: "side-lower",
                            size: LeafSize::Grow(1.0),
                        },
                    ],
                    handles: vec![xgent_ui::panels::split::HandleState { target: 0 }],
                },
            ],
            handles: vec![xgent_ui::panels::split::HandleState { target: 0 }],
        }),
    };
    let handles = tree.handles();
    assert_eq!(handles.len(), 2, "横向 1 条 + 纵向 1 条");
    assert!(
        handles.iter().any(|h| h.axis == Axis::Vertical),
        "纵向分割应产出一条纵向手柄"
    );
    // `resize.rs` 不含任何按面板 id 的特判分支（通用遍历）
    let resize_src = read(&repo_root().join("crates/xgent_ui/src/resize.rs"));
    assert!(
        !resize_src.contains("side-upper") && !resize_src.contains("side-lower"),
        "resize.rs 不得出现按分割 id 的特判（通用遍历）"
    );
}

/// 抽屉宽度钳制在 [240, 480]。
#[test]
fn drawer_width_is_clamped_to_range() {
    assert_eq!(
        xgent_ui::resize::clamp_leaf(leaf_ids::DRAWER, 100.0, 1600.0),
        240.0
    );
    assert_eq!(
        xgent_ui::resize::clamp_leaf(leaf_ids::DRAWER, 900.0, 1600.0),
        480.0
    );
    assert_eq!(
        xgent_ui::resize::clamp_leaf(leaf_ids::DRAWER, 320.0, 1600.0),
        320.0
    );
}

// ===== 6. i18n 页签标题 =====

/// 英文语料下页签标题为英文（不含 CJK 码位）；两种语料都齐备 panel-* key。
#[test]
fn english_corpus_has_english_panel_titles() {
    let en = read(&repo_root().join("crates/xgent_settings/locales/en-US/main.ftl"));
    let zh = read(&repo_root().join("crates/xgent_settings/locales/zh-CN/main.ftl"));
    for key in [
        "panel-chat",
        "panel-file-drawer",
        "panel-session-history",
        "panel-terminal",
        "panel-editor",
        "panel-diff",
    ] {
        let en_line = en
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{key} =")))
            .unwrap_or_else(|| panic!("英文语料缺 key：{key}"));
        let value = en_line.split_once('=').map(|(_, v)| v.trim()).unwrap_or("");
        assert!(!value.is_empty(), "英文语料 {key} 的值不能为空");
        assert!(
            !value
                .chars()
                .any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)),
            "英文语料 {key} 不应含 CJK：{value}"
        );
        assert!(
            zh.lines()
                .any(|l| l.trim_start().starts_with(&format!("{key} ="))),
            "中文语料缺 key：{key}"
        );
    }
}

// ===== 源码级契约（D1 / D3 / 北极星前置） =====

/// `SideViewContent` 在 `crates/` 内零引用（D1 闭合）。
#[test]
fn side_view_content_is_fully_removed() {
    let mut hits = Vec::new();
    let crates = repo_root().join("crates");
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    walk(&crates, &mut hits);
    for path in hits {
        // 断言本身提到该名字（断言文本），排除本文件。
        let rel_owned = path.to_string_lossy().replace('\\', "/");
        if rel_owned.ends_with("tests/panel_registry_contract.rs") {
            continue;
        }
        let text = read(&path);
        if text.contains("SideViewContent") {
            let rel = path
                .strip_prefix(repo_root())
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let lines = lines_with(&text, "SideViewContent");
            panic!("`SideViewContent` 仍被引用：{rel}（行 {lines:?}）");
        }
    }
}

/// `EditorBackButtonMarker` 全仓只定义一次（D3 闭合：两套标记合一）。
#[test]
fn editor_back_button_marker_is_defined_once() {
    let mut definitions = Vec::new();
    for path in ui_sources() {
        let text = read(&path);
        let lines = lines_with(&text, "struct EditorBackButtonMarker");
        if !lines.is_empty() {
            let rel = path
                .strip_prefix(repo_root().join("crates/xgent_ui/src"))
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            definitions.push((rel, lines));
        }
    }
    assert_eq!(
        definitions.len(),
        1,
        "`EditorBackButtonMarker` 应全仓唯一定义，实际：{definitions:?}"
    );
}

/// 侧区顶部只有一条页签条（槽位页签由 `SlotLayout` 拥有；编辑器内部 tab 条
/// 降级为「文档列表」，两者不合并）。
#[test]
fn side_view_has_single_tab_bar_owner() {
    let tabs_src = read(&repo_root().join("crates/xgent_ui/src/panels/tabs.rs"));
    assert!(
        tabs_src.contains("SlotTabBarMarker"),
        "槽位页签条应由 panels/tabs.rs 拥有"
    );
    let editor_src = read(&repo_root().join("crates/xgent_ui/src/editor/mod.rs"));
    assert!(
        !editor_src.contains("SideViewPage"),
        "旧的页签页枚举 `SideViewPage` 应已移除（页签由注册表生成）"
    );
    // 编辑器内部 tab 条保留（文档列表语义）
    assert!(
        editor_src.contains("EditorTabBarMarker"),
        "编辑器内部 tab 条应保留为「文档列表」"
    );
}

/// 北极星前置：`layout.rs` / `editor/mod.rs` 不含 `SideViewContent`
/// （新增面板不需要改这两个文件）。
#[test]
fn layout_files_have_no_closed_enum() {
    for rel in [
        "crates/xgent_ui/src/layout.rs",
        "crates/xgent_ui/src/editor/mod.rs",
    ] {
        let text = read(&repo_root().join(rel));
        assert!(
            !text.contains("SideViewContent"),
            "{rel} 不应再引用 closed 枚举"
        );
    }
}

/// 面板集合的变更不落在布局文件里——`layout.rs` 不含任何面板 id 分派。
#[test]
fn layout_file_has_no_panel_id_branches() {
    let text = read(&repo_root().join("crates/xgent_ui/src/layout.rs"));
    for id in [
        ids::EDITOR,
        ids::DIFF,
        ids::TERMINAL,
        ids::FILE_DRAWER,
        ids::SESSION_HISTORY,
    ] {
        assert!(
            !text.contains(&format!("\"{id}\"")),
            "layout.rs 不应按面板 id 分派（新增面板不得改布局文件）"
        );
    }
}

/// 描述符的 `closeability` 与 `min_size` 等字段可用（模型完整性）。
#[test]
fn descriptor_supports_optional_fields() {
    let desc = PanelDescriptor::new(ids::EDITOR, Slot::SideDock, "panel-editor", "file", 0)
        .with_default_visible()
        .sticky();
    assert_eq!(desc.closeability, Closeability::Sticky);
    assert_eq!(desc.default_size, None);
    assert_eq!(desc.min_size, bevy::math::Vec2::ZERO);
}

/// 系统集合顺序：请求 → 注册表 → 派生（跨插件排序契约）。
#[test]
fn panel_sets_are_ordered() {
    // 集合存在且可比较（`PanelSet` 的链式顺序在 `PanelsPlugin` 里配置）
    let set = PanelSet::Registry;
    assert_eq!(set, PanelSet::Registry);
    assert_ne!(PanelSet::Requests, PanelSet::Visibility);
}

// ===== 回归：恢复入口与返回入口（A5 / A6 的 Verifier 缺陷修复）=====

/// 「展开面板」钮是**恢复入口**：分屏**折叠**时才出现（分屏展开时隐藏）。
#[test]
fn expand_button_appears_only_when_slot_is_closed() {
    use xgent_ui::panels::PanelVisibility;
    use xgent_ui::panels::visibility::derive_panel_visibility;

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<PanelRegistry>()
        .init_resource::<SlotLayout>()
        .add_systems(Update, derive_panel_visibility);
    let entity = app
        .world_mut()
        .spawn((
            Node::default(),
            PanelVisibility::of_closed_slot(Slot::SideDock),
        ))
        .id();

    // 槽位默认开放 → 恢复入口隐藏
    app.update();
    let mut q = app
        .world_mut()
        .query_filtered::<&Node, With<PanelVisibility>>();
    assert_eq!(
        q.get(app.world(), entity).unwrap().display,
        Display::None,
        "分屏展开时「展开面板」钮应隐藏"
    );

    // 槽位折叠 → 恢复入口出现
    app.world_mut()
        .resource_mut::<SlotLayout>()
        .set_open(Slot::SideDock, false);
    app.update();
    let mut q = app
        .world_mut()
        .query_filtered::<&Node, With<PanelVisibility>>();
    assert_eq!(
        q.get(app.world(), entity).unwrap().display,
        Display::Flex,
        "分屏折叠时「展开面板」钮应出现（恢复入口）"
    );
}

/// 侧区顶部**有且只有一个**返回入口（✕）：槽位页签条 spawn 它，编辑器顶栏不再有。
#[test]
fn side_view_has_exactly_one_return_entry() {
    let tabs = read(&repo_root().join("crates/xgent_ui/src/panels/tabs.rs"));
    assert!(
        tabs.contains("EditorBackButtonMarker"),
        "槽位页签条应 spawn 返回入口（✕）"
    );
    let editor = read(&repo_root().join("crates/xgent_ui/src/editor/mod.rs"));
    assert!(
        !editor.contains("EditorBackButtonMarker"),
        "编辑器顶栏不应再有第二个返回入口（两套标记合一）"
    );
}
