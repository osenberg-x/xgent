//! `xui::TextEditor` — 通用代码编辑器裸件。
//!
//! 纯依赖 `bevy` + `xui_i18n` + `tree-sitter` + `tree-sitter-rust`，**不依赖任何 `xgent_*`**。
//!
//! 详见 `doc/design/editor-design.md` 第 5 节。
//!
//! 能力边界（中等）：
//! - 多行文本编辑（基于官方 `EditableText`，复用 IME/光标/选区）
//! - 行号列
//! - undo/redo 栈（组件内持久化，基于文本快照）
//! - 查找替换（API + 状态机，UI overlay 由调用方或本组件简易渲染）
//! - tree-sitter 语法高亮（Rust grammar，MVP 唯一）
//! - 光标 / 选区视觉（光标条独立节点）
//! - 虚拟滚动（复用 `xui::VirtualList` 思路，大文件只渲染可见行）
//!
//! 不提供：多标签页、文件 IO、LSP/诊断/跳转、split view（业务层或后续）。
//!
//! # 渲染策略
//!
//! bevy 的 `EditableText` 自身单色渲染文本。为同时支持输入与语法高亮，
//! `TextEditor` 维护一棵 `TextSpan` 子树作为**只读高亮显示层**：
//! - 编辑态：显示 `EditableText` 输入层（单色），高亮层隐藏
//! - 只读态（`readonly = true`）：隐藏输入层，显示高亮层（多色 span）
//!
//! 编辑态下仍计算 tree-sitter spans 并暴露于 `TextEditor::spans`，供
//! @ 引用查询（光标所在符号）与未来升级到彩色编辑渲染使用。

pub mod buffer;
pub mod find;
pub mod highlight;
pub mod render;
pub mod undo;
pub mod virtual_render;

pub use buffer::{EditorText, Rope, TextSnapshot};
pub use find::{FindMatch, FindState};
pub use highlight::{HighlightSpan, SpanKind, highlight};
pub use undo::UndoStack;

use bevy::prelude::*;

/// 支持的语言（MVP 唯一 Rust）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    Rust,
}

/// 代码编辑器组件。
///
/// 挂载在一个含 `EditableText` 的实体上（由 `spawn_text_editor` 或调用方构造）。
/// 本组件持有：语言、只读标志、tab 宽度、undo 栈、查找状态、tree-sitter spans、
/// 行表（按 `\n` 切分，供虚拟化渲染按行切片，避免一次性渲染全文）。
///
/// 虚拟化策略（参考 zed element.rs:7050）：固定行高 + scroll_position.y(行) →
/// [start_row, end_row) 可见区间，只渲染可见行的 TextSpan。
#[derive(Component, Debug)]
pub struct TextEditor {
    /// 语法语言（MVP 唯一 Rust）
    pub language: Language,
    /// 只读模式：禁止编辑，仅显示高亮
    pub readonly: bool,
    /// tab 宽度（空格数）
    pub tab_size: u32,
    /// undo/redo 栈
    pub undo: UndoStack,
    /// 查找替换状态
    pub find: FindState,
    /// tree-sitter 高亮 span 列表（按字节区间，不重叠）
    pub spans: Vec<HighlightSpan>,
    /// 行高（逻辑像素），固定值（字体度量派生），用于虚拟滚动定位
    pub line_height: f32,
    /// 光标最近已知位置（行，列，1-based），由输入系统更新
    pub cursor: (usize, usize),
    /// 脏标志（有未保存修改）
    pub dirty: bool,
    /// 文本 rope：按行/字符 O(log n) 访问，cheap clone 供 undo 快照。
    /// 由 `update_syntax_highlight` 在文本变化时从 `EditableText` 同步重建。
    /// 替代早期 `Vec<String>` 行表——大文件不再 O(n) 全文拷贝。
    pub rope: Rope,
}

impl Default for TextEditor {
    fn default() -> Self {
        Self {
            language: Language::Rust,
            readonly: false,
            tab_size: 4,
            undo: UndoStack::default(),
            find: FindState::default(),
            spans: Vec::new(),
            line_height: 20.0,
            cursor: (1, 1),
            dirty: false,
            rope: Rope::new(),
        }
    }
}

impl TextEditor {
    /// 构造只读编辑器。
    pub fn readonly() -> Self {
        Self {
            readonly: true,
            ..Self::default()
        }
    }

    /// 构造可编辑编辑器，指定语言。
    pub fn new(language: Language) -> Self {
        Self {
            language,
            ..Self::default()
        }
    }

    /// 显示行数：行号列与虚拟化内容渲染的行数。
    ///
    /// ropey 的 [`Rope::len_lines`] = 换行数 + 1——文件以 `\n` 结尾时会把末尾
    /// 换行之后的空行也计为一行（幽灵行）。主流编辑器（VSCode/zed）不为该
    /// 幽灵行显示行号，故此处约定：末行为空（由尾部换行产生）时不计数，
    /// `"a\n"` 显示 1 行、行号只到 1。空文本 `""` 仍显示 1 行。
    ///
    /// 注意：只影响显示，不修改 rope——光标仍可定位到幽灵行（len_lines 行）。
    pub fn display_line_count(&self) -> usize {
        let len = self.rope.len_lines();
        if len > 1 && self.rope.line(len - 1).len_chars() == 0 {
            len - 1
        } else {
            len
        }
    }
}
#[derive(Message)]
pub struct EditorSaveRequested {
    /// 触发保存的编辑器实体
    pub entity: Entity,
}

/// 激活 buffer 标记：宿主（多 tab 场景）把它挂在当前激活 tab 的编辑器
/// 实体上并随切换增删。编辑器快捷键（Cmd+S/Z/F/H）只作用于带此标记的
/// buffer——否则 Cmd+S 会保存所有打开的 tab。
#[derive(Component, Default)]
pub struct EditorActive;

/// 编辑器脏标志变化（dirty true→false 或 false→true）。
#[derive(Message)]
pub struct EditorDirtyChanged {
    /// 编辑器实体
    pub entity: Entity,
    /// 新的脏标志
    pub dirty: bool,
}

/// `TextEditor` 的系统集（供跨 plugin 排序，避免 `&mut` 冲突）。
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub struct TextEditorUpdateSet;

/// `TextEditor` 插件：注册输入、查找、高亮、虚拟化渲染系统。
pub struct TextEditorPlugin;

impl Plugin for TextEditorPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(Update, TextEditorUpdateSet)
            .init_resource::<render::EditorTheme>()
            .add_message::<EditorSaveRequested>()
            .add_message::<EditorDirtyChanged>()
            .add_systems(
                Update,
                (
                    handle_editor_keys,
                    update_syntax_highlight,
                    render::sync_highlight_layer,
                    render::update_line_numbers,
                    render::update_cursor_bar,
                    virtual_render::update_virtual_lines,
                )
                    .chain()
                    .in_set(TextEditorUpdateSet),
            );
    }
}
/// 处理编辑器快捷键：Cmd+S / Cmd+Z / Cmd+Shift+Z / Cmd+F / Cmd+H。
///
/// 不干扰 bevy 的 `EditableText` 默认输入处理（光标/删除/IME）——
/// 那些由 bevy 的 `text_input` 系统自动处理。
///
/// MVP 简化：仅响应 `Cmd/Ctrl+` 组合键（字母键单按由 EditableText 处理）。
/// undo/redo 从 [`UndoStack`] 取快照恢复文本。
///
/// 只处理带 [`EditorActive`] 标记的 buffer（宿主负责标注当前激活 tab）：
/// 否则 Cmd+S 会保存所有打开的 buffer，Cmd+Z 作用于非激活 buffer。
///
/// Cmd+S 只发保存请求，不乐观清脏——写成功由宿主的 save-result 路径
/// 回写 dirty=false，写失败保持脏标记（界面与磁盘一致）。
pub fn handle_editor_keys(
    mut q: Query<(Entity, &mut TextEditor, &mut HighlightCache), With<EditorActive>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut save_writer: MessageWriter<EditorSaveRequested>,
    mut dirty_writer: MessageWriter<EditorDirtyChanged>,
) {
    use KeyCode as K;
    let primary = keys.any_pressed([K::SuperLeft, K::SuperRight, K::ControlLeft, K::ControlRight]);
    if !primary {
        return;
    }
    for (entity, mut editor, mut cache) in q.iter_mut() {
        if editor.readonly {
            continue;
        }
        // Cmd+S：保存（非乐观清脏，见函数文档）
        if keys.just_pressed(K::KeyS) {
            save_writer.write(EditorSaveRequested { entity });
            continue;
        }
        // Cmd+Z / Cmd+Shift+Z：undo / redo（恢复 rope + 清 cache 触发重解析）
        let shift = keys.any_pressed([K::ShiftLeft, K::ShiftRight]);
        if keys.just_pressed(K::KeyZ) {
            let snap = if shift {
                editor.undo.redo()
            } else {
                editor.undo.undo()
            };
            if let Some(snap) = snap {
                editor.rope = Rope::from_str(&snap.text);
                cache.0 = 0; // 触发下帧重解析
                if !editor.dirty {
                    editor.dirty = true;
                    dirty_writer.write(EditorDirtyChanged {
                        entity,
                        dirty: true,
                    });
                }
            }
            continue;
        }
        // Cmd+F：查找；Cmd+H：查找替换
        if keys.just_pressed(K::KeyF) {
            editor.find.open_find();
            continue;
        }
        if keys.just_pressed(K::KeyH) {
            editor.find.open_replace();
            continue;
        }
    }
}

/// 文本变化时重新解析 tree-sitter 并更新 `spans`。
///
/// 若实体含 `EditableText`（用户可编辑态）：从 EditableText 同步文本到 `rope`。
/// 若无 `EditableText`（只读虚拟化态，业务层直接写 `rope`）：跳过同步，仅当
/// `HighlightCache` 未命中时重算 spans（基于 `rope` 当前内容）。
/// MVP 全量解析，大文件增量解析留待后续（设计文档 5.3 节）。
pub fn update_syntax_highlight(
    mut q: Query<(
        &mut TextEditor,
        &mut HighlightCache,
        Option<&bevy::text::EditableText>,
    )>,
) {
    for (mut editor, mut cache, editable) in q.iter_mut() {
        // 文本源：有 EditableText 从它同步；无则用 rope 当前内容。
        //
        // rope 分支逐 chunk 哈希（等价于对拼接全文哈希）：此前每帧
        // `rope.to_string()` 做全量分配+拷贝只为算一个"内容没变"的哈希，
        // 大文件 × 每帧 × 每个打开的 tab 开销显著。两分支各自内部一致
        // （同一 buffer 不切换文本源），不要求跨分支哈希值一致。
        let cur_hash = match &editable {
            Some(e) => hash_text(&e.value().to_string()),
            None => hash_rope(&editor.rope),
        };
        // cache.0 == 0 是 HighlightCache::default 的"从未解析"哨兵（正常内容
        // 哈希非 0）。此前条件带 `!spans.is_empty()`，空文件（spans 恒空）
        // 会每帧重跑 tree-sitter。
        if cur_hash == cache.0 && cache.0 != 0 {
            continue;
        }
        cache.0 = cur_hash;
        if let Some(e) = editable {
            // EditableText 文本变化：重建 rope
            let t = e.value().to_string();
            editor.rope = Rope::from(t.as_str());
            if !editor.readonly {
                editor.undo.push(TextSnapshot { text: t });
                if !editor.dirty {
                    editor.dirty = true;
                }
            }
        }
        // 重解析（基于 rope，无论源）
        let text = editor.rope.to_string();
        editor.spans = highlight(&text, editor.language);
    }
}

/// 高亮缓存：上一帧文本哈希，避免重复解析。
#[derive(Component, Default)]
pub struct HighlightCache(pub u64);

/// rope 内容哈希：逐 chunk 写入同一 hasher（等价于对拼接全文哈希，
/// 避免 O(文件大小) 的 String 分配）。
fn hash_rope(rope: &Rope) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for chunk in rope.chunks() {
        chunk.hash(&mut h);
    }
    h.finish()
}

fn hash_text(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::text::EditableText;

    /// 显示行数排除尾部换行产生的幽灵空行——行号必须与文件实际行数一致
    /// （回归：`"a\n"` 曾显示行号 1、2，比内容多一行）。
    #[test]
    fn display_line_count_excludes_ghost_trailing_line() {
        let cases: Vec<(&str, usize)> = vec![
            ("", 1),       // 空文件 → 1 行
            ("a", 1),      // 无换行 → 1 行
            ("a\n", 1),    // 尾部换行 → 幽灵行不计
            ("\n", 1),     // 仅一个换行 → 1 行（空行）
            ("a\r\n", 1),  // CRLF 尾部 → 1 行
            ("a\nb", 2),   // 无尾部换行 → 2 行
            ("a\nb\n", 2), // 尾部换行 → 2 行
            ("a\n\n", 2),  // 中间空行计数、尾部幽灵行不计
            ("a\n\nb", 3), // 中间空行计数
        ];
        for (content, expected) in cases {
            let mut editor = TextEditor::readonly();
            editor.rope = Rope::from_str(content);
            assert_eq!(
                editor.display_line_count(),
                expected,
                "content={content:?} 显示行数应为 {expected}"
            );
        }
    }

    /// smoke：update_syntax_highlight 从 EditableText 同步文本到 rope + 生成 spans。
    ///
    /// 这是 ropey 改造的核心数据流契约：EditableText(String) → rope → tree-sitter → spans。
    /// 若 rope 同步断裂或 tree-sitter 解析失败，spans 会空或 rope 与原文不符。
    #[test]
    fn update_syntax_highlight_syncs_rope_and_spans() {
        let mut app = App::new();
        app.add_systems(Update, update_syntax_highlight);

        let code = "fn main() { let x = 1; }";
        let entity = app
            .world_mut()
            .spawn((
                TextEditor::new(Language::Rust),
                HighlightCache::default(),
                EditableText::new(code),
            ))
            .id();

        app.update();

        let editor = app.world().entity(entity).get::<TextEditor>().unwrap();
        // rope 与原文一致
        assert_eq!(editor.rope.to_string(), code);
        // rope 行数正确（单行）
        assert_eq!(editor.rope.len_lines(), 1);
        // spans 非空（tree-sitter 解析成功）
        assert!(
            !editor.spans.is_empty(),
            "spans 应非空，tree-sitter 应解析成功"
        );
        // spans 覆盖全文
        let covered: usize = editor.spans.iter().map(|s| s.end - s.start).sum();
        assert_eq!(covered, code.len(), "spans 应覆盖全文");
    }

    /// smoke：两个不同文本的编辑器实体，系统对各自独立同步 rope + spans。
    /// 验证 update_syntax_highlight 不串实体、各自 rope 与原文一致。
    #[test]
    fn update_syntax_highlight_independent_per_entity() {
        let mut app = App::new();
        app.add_systems(Update, update_syntax_highlight);

        let code_a = "let a = 1;";
        let code_b = "fn add(x: i32, y: i32) -> i32 { x + y }";
        let ea = app
            .world_mut()
            .spawn((
                TextEditor::new(Language::Rust),
                HighlightCache::default(),
                EditableText::new(code_a),
            ))
            .id();
        let eb = app
            .world_mut()
            .spawn((
                TextEditor::new(Language::Rust),
                HighlightCache::default(),
                EditableText::new(code_b),
            ))
            .id();

        app.update();

        let a = app.world().entity(ea).get::<TextEditor>().unwrap();
        let b = app.world().entity(eb).get::<TextEditor>().unwrap();
        assert_eq!(a.rope.to_string(), code_a);
        assert_eq!(b.rope.to_string(), code_b);
        // 各自 spans 非空
        assert!(!a.spans.is_empty(), "a spans 应非空");
        assert!(!b.spans.is_empty(), "b spans 应非空");
        // cache 各自不同（hash 不同）
        let ca = app.world().entity(ea).get::<HighlightCache>().unwrap().0;
        let cb = app.world().entity(eb).get::<HighlightCache>().unwrap().0;
        assert_ne!(ca, cb, "两实体文本不同，hash 应不同");
    }
}
