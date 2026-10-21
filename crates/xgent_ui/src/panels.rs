//! v8 M5：槽位注册表与派生可见性（修 D1–D6、I3）。
//!
//! 核心思想（布局架构 §1 P-1 / P-2 / P-5）：
//!
//! - **区域是注册表，不是枚举**：任何界面元素经 [`PanelDescriptor`] 进入，
//!   宿主只认描述符。`Slot` 描述它落在哪个槽位。
//! - **状态是权威，呈现是派生**：可见性（[`visibility::derive_panel_visibility`]）、
//!   rail 选中态（[`PanelRegistry::active_rail`]）都由注册表 + [`SlotLayout`] 算出。
//! - **子系统间只经 ECS Events / Messages**：注册 / 激活 / 失活一律是
//!   [`PanelRegister`] / [`PanelActivate`] / [`PanelDeactivate`] 消息，禁直接方法调用。

use std::collections::BTreeMap;

use bevy::math::Vec2;
use bevy::prelude::*;

pub mod split;
pub mod tabs;
pub mod visibility;

/// 面板 id 常量（注册表的主键）。
pub mod ids {
    /// 主区对话面板（`Slot::Center`）。
    pub const CHAT: &str = "chat";
    /// 文件抽屉面板（`Slot::Overlay`）。
    pub const FILE_DRAWER: &str = "file-drawer";
    /// 会话历史面板（`Slot::Overlay`）。
    pub const SESSION_HISTORY: &str = "session-history";
    /// 编辑器 / 预览面板（`Slot::SideDock`）。
    pub const EDITOR: &str = "editor";
    /// 差异面板（`Slot::SideDock`）。
    pub const DIFF: &str = "diff";
    /// 终端面板（`Slot::SideDock`）。
    pub const TERMINAL: &str = "terminal";

    /// rail 入口：对话。
    pub const RAIL_CHAT: &str = "rail-chat";
    /// rail 入口：文件。
    pub const RAIL_FILES: &str = "rail-files";
    /// rail 入口：历史。
    pub const RAIL_HISTORY: &str = "rail-history";
    /// rail 入口：终端。
    pub const RAIL_TERMINAL: &str = "rail-terminal";
}

/// 可注册目标槽位。
///
/// `Center` 与 `SideDock` / `Overlay` 的差别是**互斥**（同槽至多一个可见），
/// `Rail` / `SettingsSection` / `StatusSegment` / `Palette` 等并列。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// 图标轨入口。
    Rail,
    /// 中央列（主区对话）。
    Center,
    /// 右侧分屏（编辑器 / 差异 / 终端）。
    SideDock,
    /// 底部停靠区。
    BottomDock,
    /// 设置面板分区。
    SettingsSection,
    /// 状态栏分段。
    StatusSegment,
    /// 命令面板条目。
    Palette,
    /// 可覆盖层（抽屉类）。
    Overlay,
}

impl Slot {
    /// 该槽位是否**单例**（同槽任意时刻至多一个可见面板）。
    pub fn is_exclusive(&self) -> bool {
        matches!(self, Slot::Center | Slot::SideDock | Slot::Overlay)
    }
}

/// 面板的可关闭性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closeability {
    /// 可关闭（页签上的 ✕）。
    Closable,
    /// 可钉住（关闭后仍保留入口）。
    Sticky,
}

/// rail 入口的点击语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailAction {
    /// 切换目标面板的可见性（开 → 关 / 关 → 开）。
    Toggle,
    /// 关闭所有可覆盖层并回到主区（对话）。
    FocusMain,
}

/// 图标轨入口（`Slot::Rail` 专用）。
#[derive(Debug, Clone, Copy)]
pub struct RailEntry {
    /// 被驱动的目标面板 id。
    pub target: &'static str,
    /// 点击语义。
    pub action: RailAction,
    /// 是否在该项之前画一条分隔线。
    pub divider_before: bool,
}

/// 面板描述符：进入宿主系统的唯一凭据。
#[derive(Debug, Clone)]
pub struct PanelDescriptor {
    /// 面板 id（注册表主键，全局唯一）。
    pub id: &'static str,
    /// 落在哪个槽位。
    pub slot: Slot,
    /// i18n fluent key（**必填**：页签 / 标题一律从这里取，禁止裸字符串）。
    pub title_key: &'static str,
    /// 图标名（`assets/icons/{name}@2x.png`）。
    pub icon: &'static str,
    /// 同槽内排序（升序，允许稀疏）。
    pub order: i16,
    /// 可关闭性。
    pub closeability: Closeability,
    /// 最小尺寸（逻辑像素）。
    pub min_size: Vec2,
    /// 默认尺寸（逻辑像素；`None` = 由槽位分配）。
    pub default_size: Option<f32>,
    /// 注册后是否立即可见。
    pub default_visible: bool,
    /// rail 入口（仅 `Slot::Rail` 面板使用）。
    pub rail: Option<RailEntry>,
}

impl PanelDescriptor {
    /// 构造描述符（其余字段取默认值）。
    pub fn new(
        id: &'static str,
        slot: Slot,
        title_key: &'static str,
        icon: &'static str,
        order: i16,
    ) -> Self {
        Self {
            id,
            slot,
            title_key,
            icon,
            order,
            closeability: Closeability::Closable,
            min_size: Vec2::ZERO,
            default_size: None,
            default_visible: false,
            rail: None,
        }
    }

    /// 附上 rail 入口。
    pub fn with_rail(
        mut self,
        target: &'static str,
        action: RailAction,
        divider_before: bool,
    ) -> Self {
        self.rail = Some(RailEntry {
            target,
            action,
            divider_before,
        });
        self
    }

    /// 设为注册后立即可见。
    pub fn with_default_visible(mut self) -> Self {
        self.default_visible = true;
        self
    }

    /// 设为可钉住。
    pub fn sticky(mut self) -> Self {
        self.closeability = Closeability::Sticky;
        self
    }
}

/// 单个面板的运行时状态。
#[derive(Debug, Clone, Default)]
pub struct PanelRuntime {
    /// 面板视图实体。
    pub view: Option<Entity>,
    /// 当前是否可见。
    pub visible: bool,
    /// 当前是否聚焦。
    pub focused: bool,
    /// 最近一次激活的序号（单调递增）：rail 选中态据此判定「后激活者优先」。
    pub activated_seq: u64,
}

/// 注册表条目（描述符 + 运行时状态）。
#[derive(Debug, Clone)]
pub struct PanelEntry {
    /// 描述符。
    pub desc: PanelDescriptor,
    /// 运行时状态。
    pub runtime: PanelRuntime,
}

/// 面板注册表（唯一权威）。
#[derive(Resource, Debug, Default)]
pub struct PanelRegistry {
    entries: Vec<PanelEntry>,
    seq: u64,
}

impl PanelRegistry {
    /// 注册一个面板（重复 id 只补齐视图实体，不重置运行时状态）。
    pub fn register(&mut self, desc: PanelDescriptor, view: Option<Entity>) {
        match self.entries.iter_mut().find(|e| e.desc.id == desc.id) {
            Some(existing) => {
                // 已有条目：只补视图实体（模块在 Startup 注册时可能与内置项
                // 先后顺序不定），运行时状态（可见 / 激活序号）保持不变。
                if existing.runtime.view.is_none() {
                    existing.runtime.view = view;
                }
                existing.desc = desc;
            }
            None => {
                let runtime = PanelRuntime {
                    view,
                    visible: desc.default_visible,
                    focused: false,
                    activated_seq: 0,
                };
                self.entries.push(PanelEntry { desc, runtime });
            }
        }
    }

    /// 按 id 取条目。
    pub fn get(&self, id: &str) -> Option<&PanelEntry> {
        self.entries.iter().find(|e| e.desc.id == id)
    }

    /// 按 id 取条目（可变）。
    pub fn get_mut(&mut self, id: &str) -> Option<&mut PanelEntry> {
        self.entries.iter_mut().find(|e| e.desc.id == id)
    }

    /// 是否已注册该 id。
    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// 全部条目（注册顺序）。
    pub fn iter(&self) -> impl Iterator<Item = &PanelEntry> {
        self.entries.iter()
    }

    /// 某槽位的条目（按 `order` 升序）。
    pub fn iter_slot(&self, slot: Slot) -> impl Iterator<Item = &PanelEntry> {
        let mut items: Vec<&PanelEntry> = self
            .entries
            .iter()
            .filter(|e| e.desc.slot == slot)
            .collect();
        items.sort_by_key(|e| (e.desc.order, e.desc.id));
        items.into_iter()
    }

    /// rail 入口条目（按 `order` 升序）。
    pub fn iter_rail(&self) -> impl Iterator<Item = &PanelEntry> {
        self.iter_slot(Slot::Rail)
    }

    /// 面板当前是否可见（仅看运行时状态，不看槽位开合）。
    pub fn is_visible(&self, id: &str) -> bool {
        self.get(id).map(|e| e.runtime.visible).unwrap_or(false)
    }

    /// 激活面板：置可见并记录激活序号。
    ///
    /// - **单例槽位**（[`Slot::is_exclusive`]）内的其它面板自动失活——切换由
    ///   `PanelActivate` 表达，旧面板自动失活，业务侧不写 `display`。
    /// - 激活 `Slot::Center`（回到主区）面板时，**所有可覆盖层面板失活**
    ///   （回到主区即关掉抽屉，与既有 rail 行为一致）。
    ///
    /// 返回是否发生了变化。
    pub fn activate(&mut self, id: &str) -> bool {
        let Some(entry) = self.get(id) else {
            return false;
        };
        let slot = entry.desc.slot;
        let exclusive = slot.is_exclusive();
        self.seq += 1;
        let seq = self.seq;
        let mut changed = false;
        if exclusive {
            for e in self.entries.iter_mut() {
                if e.desc.slot == slot && e.desc.id != id && e.runtime.visible {
                    e.runtime.visible = false;
                    changed = true;
                }
            }
        }
        if slot == Slot::Center {
            for e in self.entries.iter_mut() {
                if e.desc.slot == Slot::Overlay && e.runtime.visible {
                    e.runtime.visible = false;
                    changed = true;
                }
            }
        }
        if let Some(entry) = self.get_mut(id) {
            if !entry.runtime.visible {
                entry.runtime.visible = true;
                changed = true;
            }
            entry.runtime.activated_seq = seq;
        }
        changed
    }

    /// 失活面板。返回是否发生了变化。
    pub fn deactivate(&mut self, id: &str) -> bool {
        match self.get_mut(id) {
            Some(entry) if entry.runtime.visible => {
                entry.runtime.visible = false;
                true
            }
            _ => false,
        }
    }

    /// 槽位内当前可见的面板 id（单例槽位至多一个）。
    pub fn visible_in_slot(&self, slot: Slot) -> Option<&'static str> {
        self.iter_slot(slot)
            .find(|e| e.runtime.visible)
            .map(|e| e.desc.id)
    }

    /// **rail 选中态**：从「注册表 ∩ 槽位可见性」派生。
    ///
    /// 规则：在**可见**的 rail 入口中，取其目标面板「最近激活」者；都没激活过
    /// 时取第一个（对话）。连点同一项两次把面板关掉后，该入口不再可见，
    /// 选中态自然回落到派生结果，不会停留（修 D4）。
    pub fn active_rail(&self, layout: &SlotLayout) -> Option<&'static str> {
        let mut best: Option<(&'static str, u64)> = None;
        for entry in self.iter_rail() {
            let Some(rail) = entry.desc.rail else {
                continue;
            };
            let Some(target) = self.get(rail.target) else {
                continue;
            };
            if !layout.is_panel_visible(self, rail.target) {
                continue;
            }
            let seq = target.runtime.activated_seq;
            if best.is_none_or(|(_, best_seq)| seq > best_seq) {
                best = Some((entry.desc.id, seq));
            }
        }
        best.map(|(id, _)| id)
    }
}

/// 槽位布局状态（槽位开合 + 槽位页签条）。
#[derive(Resource, Debug, Default)]
pub struct SlotLayout {
    open: BTreeMap<Slot, bool>,
    tab_bar: Option<Entity>,
}

impl SlotLayout {
    /// 槽位是否开放（默认开放）。
    pub fn is_open(&self, slot: Slot) -> bool {
        self.open.get(&slot).copied().unwrap_or(true)
    }

    /// 设置槽位开合；返回是否发生变化。
    pub fn set_open(&mut self, slot: Slot, open: bool) -> bool {
        let cur = self.is_open(slot);
        if cur == open {
            return false;
        }
        self.open.insert(slot, open);
        true
    }

    /// 翻转槽位开合；返回新的状态。
    pub fn toggle(&mut self, slot: Slot) -> bool {
        let next = !self.is_open(slot);
        self.set_open(slot, next);
        next
    }

    /// 面板当前是否可见（运行时状态 **且** 所属槽位开放）。
    pub fn is_panel_visible(&self, registry: &PanelRegistry, id: &str) -> bool {
        registry
            .get(id)
            .map(|e| e.runtime.visible && self.is_open(e.desc.slot))
            .unwrap_or(false)
    }

    /// 槽位页签条实体（由 [`tabs`] 拥有并渲染）。
    pub fn tab_bar(&self) -> Option<Entity> {
        self.tab_bar
    }

    /// 记录槽位页签条实体。
    pub fn set_tab_bar(&mut self, entity: Entity) {
        self.tab_bar = Some(entity);
    }
}

/// 可见性受管节点：挂上它，该节点的 `Node.display` 由
/// [`visibility::derive_panel_visibility`] 统一写。
#[derive(Component, Debug, Clone)]
pub struct PanelVisibility {
    /// 关联面板 id（`None` = 只跟随所属槽位的开合，如区域容器 / 分割手柄）。
    pub panel: Option<&'static str>,
    /// 所属槽位。
    pub slot: Slot,
    /// **反向**跟随槽位：槽位**关闭**时可见（如 rail 的「展开面板」钮）。
    pub invert_slot: bool,
}

impl PanelVisibility {
    /// 跟随某个面板的可见性（面板可见 **且** 槽位开放）。
    pub fn of_panel(panel: &'static str, slot: Slot) -> Self {
        Self {
            panel: Some(panel),
            slot,
            invert_slot: false,
        }
    }

    /// 只跟随槽位开合（槽位开放时可见）。
    pub fn of_slot(slot: Slot) -> Self {
        Self {
            panel: None,
            slot,
            invert_slot: false,
        }
    }

    /// 反向跟随槽位（槽位**关闭**时可见）。
    ///
    /// 用于「展开面板」这类**恢复入口**：分屏折叠时按钮才出现。
    pub fn of_closed_slot(slot: Slot) -> Self {
        Self {
            panel: None,
            slot,
            invert_slot: true,
        }
    }
}

/// 注册一个面板（消息）。
#[derive(Message, Debug, Clone)]
pub struct PanelRegister {
    /// 面板描述符。
    pub desc: PanelDescriptor,
    /// 面板视图实体（`None` = 该面板没有独立视图实体，如 rail 入口）。
    pub view: Option<Entity>,
}

/// 激活一个面板（消息）。
#[derive(Message, Debug, Clone, Copy)]
pub struct PanelActivate {
    /// 面板 id。
    pub id: &'static str,
}

/// 失活一个面板（消息）。
#[derive(Message, Debug, Clone, Copy)]
pub struct PanelDeactivate {
    /// 面板 id。
    pub id: &'static str,
}

/// 面板相关的系统集合（跨插件排序用）。
///
/// 链式顺序：`Requests`（业务系统发面板消息）→ `Registry`（消息写注册表）
/// → `Visibility`（从注册表派生可见性 / 选中态 / 页签条）。
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PanelSet {
    /// 业务系统产生面板消息。
    Requests,
    /// 注册表更新。
    Registry,
    /// 派生呈现。
    Visibility,
}

/// 槽位注册表插件。
pub struct PanelsPlugin;

impl Plugin for PanelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanelRegistry>()
            .init_resource::<SlotLayout>()
            // 分割树（区域尺寸的声明式描述；`resize` 消费它生成分割条）。
            .init_resource::<split::SplitTree>()
            // 页签标题要 i18n 语料；宿主（xgent_app）会用真实 Localizer 覆盖。
            .init_resource::<xgent_settings::Localizer>()
            // 页签样式要主题令牌（与 ThemePlugin 同一默认值，init 不覆盖）。
            .init_resource::<crate::theme::Theme>()
            .add_message::<PanelRegister>()
            .add_message::<PanelActivate>()
            .add_message::<PanelDeactivate>()
            .configure_sets(
                Update,
                (PanelSet::Requests, PanelSet::Registry, PanelSet::Visibility).chain(),
            )
            // 内置 rail 入口直接入表（它们没有独立视图实体，按钮由
            // `activity_bar::sync_rail_items` 从注册表生成）；内容面板由各自
            // 模块经 `PanelRegister` 消息注册。
            ;
        {
            // 内置 rail 入口预置：保证首帧即可用（rail 选中态派生需要它们）。
            let mut registry = app.world_mut().resource_mut::<PanelRegistry>();
            for desc in builtin_rail_panels() {
                registry.register(desc, None);
            }
        }
        app.add_systems(
            Update,
            (
                handle_panel_register,
                handle_panel_activate,
                handle_panel_deactivate,
            )
                .chain()
                .in_set(PanelSet::Registry),
        );
    }
}

/// 内置 rail 入口描述符。
///
/// rail 入口的 `title_key` 与其目标面板一致（tooltip 与面板标题同源）。
pub fn builtin_rail_panels() -> Vec<PanelDescriptor> {
    vec![
        PanelDescriptor::new(ids::RAIL_CHAT, Slot::Rail, "panel-chat", "chat", 0).with_rail(
            ids::CHAT,
            RailAction::FocusMain,
            false,
        ),
        PanelDescriptor::new(
            ids::RAIL_FILES,
            Slot::Rail,
            "panel-file-drawer",
            "folder",
            1,
        )
        .with_rail(ids::FILE_DRAWER, RailAction::Toggle, false),
        PanelDescriptor::new(
            ids::RAIL_HISTORY,
            Slot::Rail,
            "panel-session-history",
            "clock",
            2,
        )
        .with_rail(ids::SESSION_HISTORY, RailAction::Toggle, false),
        PanelDescriptor::new(
            ids::RAIL_TERMINAL,
            Slot::Rail,
            "panel-terminal",
            "terminal",
            3,
        )
        .with_rail(ids::TERMINAL, RailAction::Toggle, true),
    ]
}

/// 处理注册消息：写入注册表。
pub fn handle_panel_register(
    mut reader: MessageReader<PanelRegister>,
    mut registry: ResMut<PanelRegistry>,
) {
    for msg in reader.read() {
        registry.register(msg.desc.clone(), msg.view);
    }
}

/// 处理激活消息。
pub fn handle_panel_activate(
    mut reader: MessageReader<PanelActivate>,
    mut registry: ResMut<PanelRegistry>,
) {
    for msg in reader.read() {
        registry.activate(msg.id);
    }
}

/// 处理失活消息。
pub fn handle_panel_deactivate(
    mut reader: MessageReader<PanelDeactivate>,
    mut registry: ResMut<PanelRegistry>,
) {
    for msg in reader.read() {
        registry.deactivate(msg.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(id: &'static str, slot: Slot, order: i16) -> PanelDescriptor {
        PanelDescriptor::new(id, slot, "t", "chat", order)
    }

    /// 描述符 → 槽位映射：同槽内按 `order` 升序。
    #[test]
    fn iter_slot_is_ordered_by_order() {
        let mut reg = PanelRegistry::default();
        reg.register(desc("b", Slot::SideDock, 5), None);
        reg.register(desc("a", Slot::SideDock, 1), None);
        reg.register(desc("c", Slot::SideDock, 3), None);
        reg.register(desc("d", Slot::Overlay, 0), None);
        let ids: Vec<&str> = reg.iter_slot(Slot::SideDock).map(|e| e.desc.id).collect();
        assert_eq!(ids, vec!["a", "c", "b"]);
    }

    /// 单例槽位：激活一个面板自动让同槽其它面板失活。
    #[test]
    fn exclusive_slot_deactivates_siblings() {
        let mut reg = PanelRegistry::default();
        reg.register(desc("a", Slot::SideDock, 0), None);
        reg.register(desc("b", Slot::SideDock, 1), None);
        reg.activate("a");
        assert!(reg.is_visible("a"));
        reg.activate("b");
        assert!(reg.is_visible("b"));
        assert!(!reg.is_visible("a"), "单例槽位内的其它面板应自动失活");
    }

    /// 非单例槽位：互不影响。
    #[test]
    fn parallel_slot_keeps_both_visible() {
        let mut reg = PanelRegistry::default();
        reg.register(desc("a", Slot::Palette, 0), None);
        reg.register(desc("b", Slot::Palette, 1), None);
        reg.activate("a");
        reg.activate("b");
        assert!(reg.is_visible("a") && reg.is_visible("b"));
    }

    /// rail 选中态：后激活者优先；目标不可见时回落到对话。
    #[test]
    fn active_rail_falls_back_when_target_hidden() {
        let mut reg = PanelRegistry::default();
        let mut chat = desc("chat", Slot::Center, 0);
        chat = chat.with_default_visible();
        reg.register(chat, None);
        reg.register(
            desc("rail-chat", Slot::Rail, 0).with_rail("chat", RailAction::FocusMain, false),
            None,
        );
        reg.register(desc("files", Slot::Overlay, 0).with_default_visible(), None);
        reg.register(
            desc("rail-files", Slot::Rail, 1).with_rail("files", RailAction::Toggle, false),
            None,
        );
        let layout = SlotLayout::default();

        assert_eq!(reg.active_rail(&layout), Some("rail-chat"));
        reg.activate("files");
        assert_eq!(reg.active_rail(&layout), Some("rail-files"));
        // 关掉抽屉 → 选中态回落到对话（不残留）
        reg.deactivate("files");
        assert_eq!(reg.active_rail(&layout), Some("rail-chat"));
    }

    /// 槽位开合影响面板可见性。
    #[test]
    fn slot_closed_hides_panels() {
        let mut reg = PanelRegistry::default();
        reg.register(desc("a", Slot::SideDock, 0), None);
        reg.activate("a");
        let mut layout = SlotLayout::default();
        assert!(layout.is_panel_visible(&reg, "a"));
        layout.set_open(Slot::SideDock, false);
        assert!(!layout.is_panel_visible(&reg, "a"));
    }
}
