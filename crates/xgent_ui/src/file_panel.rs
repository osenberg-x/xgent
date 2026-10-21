//! 文件抽屉（v7 抽屉化）：项目文件树浏览，左侧 overlay（方案 §8.7）。
//!
//! 由 rail 文件按钮 / `filepanel.toggle` 快捷键切换 `FileDrawerOpen`。
//! 文件树从项目根遍历，按字母排序、目录优先。点击目录展开/折叠，
//! 点击文件发 `OpenFileRequest` 走上下文面板预览页加载（原内嵌预览区已取消）。
//! 忽略路径：MVP 硬编码匹配构建产物（`target/`、`node_modules/` 等）+ dotfile 白名单。
//! 文件系统变更时（daemon `FileChangedEvent`）自动重建文件树，保留已展开目录。

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::layout::{FileDrawerOpen, FilePanelMarker, UiRoot};
use crate::theme::{Theme, space};

/// 文件树容器标记。
#[derive(Component, Default)]
pub struct FileTreeMarker;

/// 抽屉遮罩标记（点击关闭抽屉）。
#[derive(Component, Default)]
pub struct FileDrawerOverlayMarker;

/// 文件树行标记（虚拟列表的行，携带数据下标与代次）。
#[derive(Component, Default)]
pub struct FileTreeRow {
    /// 对应 `FileTreeItems` 的下标
    pub index: usize,
    /// 数据代次：数据重建后旧行作废（否则区间内的旧行会留着过期内容）。
    pub generation: u32,
}

/// 文件树占位块标记：撑出列表总高度（虚拟列表只渲染可见行）。
#[derive(Component, Default)]
pub struct FileTreeSpacerMarker;

/// 文件树扁平条目（数据层，与渲染解耦）。
#[derive(Debug, Clone)]
pub struct FileTreeItem {
    /// 缩进层级
    pub depth: u16,
    /// 是否目录
    pub is_dir: bool,
    /// 目录是否已展开
    pub expanded: bool,
    /// 显示名
    pub name: String,
    /// 绝对路径
    pub path: PathBuf,
}

/// 文件树数据（扁平列表；虚拟列表按可见区间渲染其中一段）。
#[derive(Resource, Default)]
pub struct FileTreeItems {
    /// 扁平条目
    pub items: Vec<FileTreeItem>,
    /// 数据代次（每次重建 +1）：行按代次对齐，数据换了旧行作废。
    pub generation: u32,
}

/// 文件树行高（逻辑像素）。固定行高是虚拟化定位的前提。
const FILE_TREE_ROW_H: f32 = 26.0;

/// 文件树最小高度（逻辑像素）。
///
/// 窗口极矮时（以及 headless 装配没有渲染目标、视口高 0 时）树仍保持可滚，
/// 而不是塌成 0 高——虚拟列表没有视口就不渲染任何行。
const FILE_TREE_MIN_H: f32 = 240.0;

/// 当前选中的文件条目标记（高亮显示）。
#[derive(Component, Default)]
pub struct FileSelectedMarker;

/// 目录条目标记（记录路径与展开状态）。
#[derive(Component, Default)]
pub struct DirEntry {
    pub path: PathBuf,
    pub expanded: bool,
}

/// 文件条目标记（记录路径）。
#[derive(Component, Default)]
pub struct FileEntry {
    pub path: PathBuf,
}

/// 项目根路径（由 xgent_app 注入）。
#[derive(Resource, Default)]
pub struct ProjectRoot {
    pub path: PathBuf,
}

/// 文件树脏标记：收到文件系统变更事件时置 true 并重置 debounce 计数器，
/// 计数器归零后 `rebuild_file_tree` 才实际重建（避免高频事件连续触发）。
#[derive(Resource, Default)]
pub struct FileTreeDirty(pub bool, pub u32);

/// 已展开的目录路径集合（重建文件树时恢复展开状态）。
#[derive(Resource, Default)]
pub struct ExpandedDirs(pub HashSet<PathBuf>);

/// 当前选中的文件路径（重建/折叠恢复选中态，与 `FileSelectedMarker` 同步）。
#[derive(Resource, Default)]
pub struct SelectedFilePath(pub Option<PathBuf>);
/// 文件面板插件。
///
/// **组合约束**（R1 记录）：文件树条目为矢量图标，依赖 `KitPlugin` 注入的
/// `IconAssets`——单独注册本插件而不加 KitPlugin 时，`rebuild_file_tree`
/// 在 ProjectRoot 变化帧会因缺资源 panic。真实 app 走 `XgentUiPlugin` 组合
/// （已含 KitPlugin）不受影响。
pub struct FilePanelPlugin;

impl Plugin for FilePanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProjectRoot>()
            .init_resource::<SelectedFilePath>()
            .init_resource::<FileTreeDirty>()
            .init_resource::<ExpandedDirs>()
            .init_resource::<FileTreeItems>();
        // 面板注册表（幂等：宿主可能已装配）
        if !app.is_plugin_added::<crate::panels::PanelsPlugin>() {
            app.add_plugins(crate::panels::PanelsPlugin);
        }
        if !app.is_plugin_added::<crate::panels::visibility::VisibilityPlugin>() {
            app.add_plugins(crate::panels::visibility::VisibilityPlugin);
        }
        app
            // FileChangedEvent 由 EditorPlugin 注册，此处幂等再注册确保独立可用
            .add_message::<crate::editor::conflict::FileChangedEvent>()
            .add_systems(Startup, spawn_file_panel.after(crate::layout::spawn_layout))
            .add_systems(
                Update,
                (
                    handle_drawer_overlay_click,
                    handle_file_click,
                    handle_dir_click,
                    rebuild_file_tree,
                    sync_file_tree_rows,
                    mark_file_tree_dirty_on_fs_change,
                )
                    .chain()
                    .before(update_file_entry_style),
            )
            .add_systems(
                Update,
                sync_file_drawer_open.in_set(crate::panels::PanelSet::Visibility),
            );
    }
}

/// 启动时 spawn 抽屉 overlay：遮罩 + 左侧 320px 抽屉面板（标题头 + 文件树）。
///
/// 抽屉与遮罩初始 `Display::None`，由 [`handle_drawer_visibility`] 据
/// `FileDrawerOpen` 切换显隐；挂在 `UiRoot` 下（Absolute 定位覆盖全窗）。
fn spawn_file_panel(
    mut commands: Commands,
    q_root: Query<Entity, With<UiRoot>>,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
    mut register: MessageWriter<crate::panels::PanelRegister>,
) {
    let Ok(root) = q_root.single() else {
        return;
    };
    // 遮罩（点击关闭抽屉；初始隐藏）
    let scrim = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme.overlay),
            crate::kit::drawer_z(),
            Button,
            Hovered::default(),
            FileDrawerOverlayMarker,
            // v8 M5-T3：抽屉遮罩的显隐由派生系统统一写。
            crate::panels::PanelVisibility::of_panel(
                crate::panels::ids::FILE_DRAWER,
                crate::panels::Slot::Overlay,
            ),
        ))
        .id();
    commands.entity(root).add_child(scrim);
    // 抽屉面板（左侧贴齐，surface 底 + 右边框；初始隐藏）
    let panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                bottom: px(0.0),
                width: px(crate::theme::size::DRAWER_W),
                flex_direction: FlexDirection::Column,
                border: UiRect::right(px(1.0)),
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme.surface),
            BorderColor::all(theme.line),
            crate::kit::drawer_z_offset(1),
            // 0.20 指针事件会冒泡到 Button 祖先：面板消费 PointerPress，
            // 点面板不会误触遮罩关闭（等价旧 FocusPolicy::Block）
            crate::kit::block_press_bubbling(),
            // require_markers 下没有 Pickable 就收不到指针事件：面板必须自带，
            // 否则点面板空白处会穿透到遮罩、误触「点外关闭」。
            bevy::picking::Pickable::default(),
            FilePanelMarker,
            // v8 M5-T3：抽屉面板的显隐由派生系统统一写。
            crate::panels::PanelVisibility::of_panel(
                crate::panels::ids::FILE_DRAWER,
                crate::panels::Slot::Overlay,
            ),
        ))
        .id();
    commands.entity(root).add_child(panel);
    // v8 M5：注册为 `Slot::Overlay` 面板（与会话历史同属单例槽位）。
    register.write(crate::panels::PanelRegister {
        desc: crate::panels::PanelDescriptor::new(
            crate::panels::ids::FILE_DRAWER,
            crate::panels::Slot::Overlay,
            "panel-file-drawer",
            "folder",
            0,
        ),
        view: Some(panel),
    });
    commands.entity(panel).with_children(|p| {
        // 标题头：资源管理器
        p.spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(px(space::MD)),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                border: UiRect::bottom(px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme.surface),
            BorderColor::all(theme.border),
        ))
        .with_children(|head| {
            // 标题（资源管理器，大写小字体）
            head.spawn(crate::fonts::ui_text(
                crate::i18n::tr(&loc, "file-panel-title").to_uppercase(),
                crate::theme::type_scale::MICRO,
                510,
                theme.text_dim,
                crate::theme::type_scale::line_height::UI,
            ));
        });
        // 文件树区（可滚动 + 虚拟化，独占抽屉）
        // v8 M3-T6：只渲染可见行（行数多时不再一次性 spawn 几千个节点）。
        // 可见区间由 xui::VirtualList 计算，占位块撑出列表总高度。
        // 树的最小高度：窗口极矮时（headless 装配无渲染目标，视口高 0）
        // 树仍是可滚的，而不是 0 高（虚拟列表没有视口就不渲染任何行）。
        let mut tree_frame = xui::ScrollFrame::vertical();
        tree_frame.style().min_height = px(FILE_TREE_MIN_H);
        p.spawn((
            tree_frame,
            xui::VirtualList::new(0, FILE_TREE_ROW_H),
            FileTreeMarker,
        ))
        .with_children(|c| {
            // 占位块：高度 = 总行数 × 行高（撑出滚动范围）
            c.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(0.0),
                    ..default()
                },
                FileTreeSpacerMarker,
            ));
        });
    });
}

/// 派生视图：文件抽屉开关（由注册表派生，非独立存储的权威状态）。
///
/// v8 M5：抽屉是 `Slot::Overlay` 的注册面板，开关状态由注册表算出。
pub fn sync_file_drawer_open(
    registry: Res<crate::panels::PanelRegistry>,
    layout: Res<crate::panels::SlotLayout>,
    mut file: ResMut<FileDrawerOpen>,
) {
    let want = layout.is_panel_visible(&registry, crate::panels::ids::FILE_DRAWER);
    if file.0 != want {
        file.0 = want;
    }
}

/// 遮罩点击关闭抽屉。
fn handle_drawer_overlay_click(
    q: Query<(), (With<FileDrawerOverlayMarker>, Added<Pressed>)>,
    mut drawer_toggle: MessageWriter<crate::overlay::DrawerToggle>,
) {
    for _ in q.iter() {
        // v8 M1：关闭也走统一消息，OverlayRegistry 才能同步。
        drawer_toggle.write(crate::overlay::DrawerToggle(
            crate::overlay::OverlayId::FileDrawer,
        ));
    }
}

/// 判断路径是否被忽略（MVP 简单匹配）。
///
/// 过滤构建产物、VCS 元数据、IDE 配置及系统临时文件（如 `.DS_Store`）。
/// 隐藏文件（`.` 开头）除显式允许的外均过滤，避免树被噪声淹没。
fn is_ignored(name: &str) -> bool {
    if name.starts_with('.') {
        return !matches!(name, ".github" | ".gitignore" | ".cargo");
    }
    matches!(
        name,
        "target"
            | "node_modules"
            | "__pycache__"
            | "dist"
            | "build"
            | "bin"
            | "obj"
            | "venv"
            | "Thumbs.db"
    )
}

/// 目录或文件内容（一次遍历的一层条目）。
struct DirContent {
    /// 显示名（文件/目录名）
    name: String,
    /// 绝对路径
    path: PathBuf,
    /// 是否目录
    is_dir: bool,
}

/// 列出目录下的一层条目（目录优先，字母排序）。
///
/// 跳过符号链接以防止循环链接导致的无限递归。
fn list_dir(dir: &std::path::Path) -> Vec<DirContent> {
    let mut entries = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return entries;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if is_ignored(&name) {
            continue;
        }
        // 跳过符号链接，防止循环链接导致 spawn_entry 递归栈溢出
        // 用 file_type()（不跟随符号链接）比 symlink_metadata 少一次系统调用
        let is_dir = match entry.file_type() {
            Ok(ft) => ft.is_dir() && !ft.is_symlink(),
            Err(_) => continue,
        };
        entries.push(DirContent { name, path, is_dir });
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    entries
}

/// 把目录树拍平成「可见条目」列表（只下钻已展开的目录）。
///
/// 虚拟列表需要**扁平**的数据：一维数组 + 每项的缩进层级。展开状态由
/// [`ExpandedDirs`] 给出，折叠的目录不产生子项。
fn flatten_tree(root: &std::path::Path, expanded_dirs: &HashSet<PathBuf>) -> Vec<FileTreeItem> {
    fn walk(
        dir: &std::path::Path,
        depth: u16,
        expanded_dirs: &HashSet<PathBuf>,
        out: &mut Vec<FileTreeItem>,
    ) {
        for entry in list_dir(dir) {
            let is_expanded = entry.is_dir && expanded_dirs.contains(&entry.path);
            out.push(FileTreeItem {
                depth,
                is_dir: entry.is_dir,
                expanded: is_expanded,
                name: entry.name.clone(),
                path: entry.path.clone(),
            });
            if is_expanded {
                walk(&entry.path, depth + 1, expanded_dirs, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 0, expanded_dirs, &mut out);
    out
}

/// spawn 一个文件树行（目录或文件，扁平结构）。
///
/// 目录行（折叠箭头 + 矢量目录图标 + 名称）/ 文件行（矢量文件图标 + 名称），
/// 条目选中态由 [`update_file_entry_style`] 绘制背景。
///
/// v8 M3-T6：行是**绝对定位**的（`top = index × 行高`），缩进改由每行的
/// 左内边距表达（原递归结构靠每层容器的 padding 缩进）。
fn spawn_tree_row(
    parent: &mut ChildSpawnerCommands,
    item: &FileTreeItem,
    index: usize,
    generation: u32,
    theme: &Theme,
    icons: &crate::kit::IconAssets,
    selected_path: Option<&std::path::Path>,
) {
    let row_node = Node {
        position_type: PositionType::Absolute,
        top: px(index as f32 * FILE_TREE_ROW_H),
        left: px(0.0),
        width: Val::Percent(100.0),
        height: px(FILE_TREE_ROW_H),
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: px(space::XS),
        padding: UiRect {
            left: px(item.depth as f32 * space::LG + space::XS),
            right: px(space::XS),
            top: px(0.0),
            bottom: px(0.0),
        },
        border_radius: BorderRadius::all(px(crate::theme::radius::SMALL)),
        ..default()
    };
    if item.is_dir {
        parent
            .spawn((
                Button,
                Hovered::default(),
                row_node,
                DirEntry {
                    path: item.path.clone(),
                    expanded: item.expanded,
                },
                BackgroundColor(Color::NONE),
                FileTreeRow { index, generation },
            ))
            .with_children(|row| {
                // 折叠箭头（chevron-right 折叠 / chevron-down 展开）
                row.spawn(crate::kit::icon(
                    icons,
                    if item.expanded {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    12.0,
                    theme.text_muted,
                ));
                // 目录图标（folder，v7 accent 染色）
                row.spawn(crate::kit::icon(
                    icons,
                    "folder",
                    14.0,
                    theme.accent_interactive,
                ));
                // 名称
                // v8 M3-T5：长名不再溢出抽屉宽度（审计 C3），中段省略保留末两段。
                row.spawn((
                    Node {
                        flex_shrink: 1.0,
                        min_width: Val::ZERO,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    crate::fonts::ellipsis_path_text(
                        item.name.clone(),
                        crate::theme::type_scale::CAPTION,
                        400,
                        theme.text_dim,
                        crate::theme::type_scale::line_height::UI,
                    ),
                ));
            });
    } else {
        let mut cmd = parent.spawn((
            Button,
            Hovered::default(),
            row_node,
            FileEntry {
                path: item.path.clone(),
            },
            BackgroundColor(Color::NONE),
            FileTreeRow { index, generation },
        ));
        // 重建后恢复选中态
        if selected_path == Some(item.path.as_path()) {
            cmd.insert(FileSelectedMarker);
        }
        cmd.with_children(|row| {
            // 图标占位（对齐目录行的箭头宽度，保持图标列竖向对齐）
            row.spawn((Node {
                width: px(12.0),
                ..default()
            },));
            // 文件图标（file，v7 中性灰染色）
            row.spawn(crate::kit::icon(icons, "file", 14.0, theme.text_muted));
            // 名称
            // v8 M3-T5：长名不再溢出抽屉宽度（审计 C3）。
            row.spawn((
                Node {
                    flex_shrink: 1.0,
                    min_width: Val::ZERO,
                    overflow: Overflow::clip(),
                    ..default()
                },
                crate::fonts::ellipsis_path_text(
                    item.name.clone(),
                    crate::theme::type_scale::CAPTION,
                    400,
                    theme.text_dim,
                    crate::theme::type_scale::line_height::UI,
                ),
            ));
        });
    }
}

/// 根据项目根路径构建文件树。
///
/// 触发条件：项目根路径变化（`is_changed`/`is_added`）或收到文件系统变更事件
///（`FileTreeDirty`）。重建时从 `ExpandedDirs` 恢复已展开目录状态。
fn rebuild_file_tree(
    root: Res<ProjectRoot>,
    mut dirty: ResMut<FileTreeDirty>,
    expanded: Res<ExpandedDirs>,
    mut items: ResMut<FileTreeItems>,
) {
    // 仅在项目根路径变化或文件系统变更时重建
    // dirty.0=true 时进行 debounce 倒计时，归零后才实际重建
    if !root.is_changed() && !root.is_added() && !dirty.0 {
        return;
    }
    if dirty.0 && dirty.1 > 0 {
        dirty.1 -= 1;
        return;
    }
    if root.path.as_os_str().is_empty() {
        dirty.0 = false;
        dirty.1 = 0;
        return;
    }
    // v8 M3-T6：只重建**数据**（扁平列表），渲染由 `sync_file_tree_rows`
    // 按可见区间增量做（可见行才 spawn）。代次 +1 → 旧行全部作废。
    items.items = flatten_tree(&root.path, &expanded.0);
    items.generation = items.generation.wrapping_add(1);
    dirty.0 = false;
    dirty.1 = 0;
}

/// 虚拟列表同步：按可见区间增删文件树行。
///
/// 可见区间由 `xui::VirtualList` 每帧算出（`first_visible` / `visible_count`）；
/// 本系统把区间外的行 despawn、把缺失的行 spawn。占位块撑出列表总高度，
/// 滚动条因此反映真实内容长度。
fn sync_file_tree_rows(
    items: Res<FileTreeItems>,
    theme: Res<Theme>,
    icons: Res<crate::kit::IconAssets>,
    selected_file: Res<SelectedFilePath>,
    mut q_list: Query<(Entity, &mut xui::VirtualList), With<FileTreeMarker>>,
    q_rows: Query<(Entity, &FileTreeRow), With<FileTreeRow>>,
    mut q_spacer: Query<&mut Node, With<FileTreeSpacerMarker>>,
    mut commands: Commands,
) {
    let Ok((list_entity, mut list)) = q_list.single_mut() else {
        return;
    };
    let total = items.items.len();
    let generation = items.generation;
    if list.item_count != total {
        list.item_count = total;
    }
    let first = list.first_visible;
    let last = (first + list.visible_count).min(total);

    // 区间外（或数据已换代）的行删掉
    for (entity, row) in q_rows.iter() {
        if row.generation != generation || row.index < first || row.index >= last {
            commands.entity(entity).despawn();
        }
    }
    // 区间内缺的行补上
    let present: HashSet<usize> = q_rows
        .iter()
        .map(|(_, row)| row.index)
        .filter(|idx| *idx >= first && *idx < last)
        .collect();
    let selected_path = selected_file.0.clone();
    for index in first..last {
        if present.contains(&index) {
            continue;
        }
        let Some(item) = items.items.get(index) else {
            continue;
        };
        commands.entity(list_entity).with_children(|p| {
            spawn_tree_row(
                p,
                item,
                index,
                generation,
                &theme,
                &icons,
                selected_path.as_deref(),
            );
        });
    }

    // 占位块：撑出列表总高度（否则滚动条只反映已渲染的行）
    let spacer_height = total as f32 * FILE_TREE_ROW_H;
    if let Ok(mut node) = q_spacer.single_mut() {
        let want = px(spacer_height);
        if node.height != want {
            node.height = want;
        }
    }
}

/// debounce 帧数：收到文件系统变更事件后等待 N 帧无新事件才重建文件树。
/// 避免 `cargo build` 等高频文件变更连续触发重建卡帧。
const DEBOUNCE_FRAMES: u32 = 5;

/// 收到文件系统变更事件时标记文件树为脏并重置 debounce 计数器。
///
/// 仅当变更路径在项目根目录下时才标记脏，避免项目外文件变更触发无谓重建。
fn mark_file_tree_dirty_on_fs_change(
    mut reader: MessageReader<crate::editor::conflict::FileChangedEvent>,
    project_root: Res<ProjectRoot>,
    mut dirty: ResMut<FileTreeDirty>,
) {
    let root = &project_root.path;
    if root.as_os_str().is_empty() {
        return;
    }
    for ev in reader.read() {
        if ev.path.starts_with(root) {
            dirty.0 = true;
            dirty.1 = DEBOUNCE_FRAMES;
        }
    }
}

/// 处理文件条目点击：发 `OpenFileRequest`，由上下文面板预览页加载（v7 行为变更）。
///
/// 原内嵌预览区已取消——代码/非代码文件统一走编辑器 buffer 加载：
/// `handle_open_file_requests` 会激活 `editor` 面板并展开分屏。
/// 点击后同时关闭抽屉（方案 §8.7：浏览→点文件→context 预览打开）。
///
/// 选中态（`FileSelectedMarker`）由本系统维护；条目背景色由
/// [`update_file_entry_style`] 统一绘制。
fn handle_file_click(
    q_files: Query<(Entity, &FileEntry), Added<Pressed>>,
    q_selected: Query<Entity, With<FileSelectedMarker>>,
    mut selected_file: ResMut<SelectedFilePath>,
    mut drawer_toggle: MessageWriter<crate::overlay::DrawerToggle>,
    mut open_writer: MessageWriter<crate::editor::tabs::OpenFileRequest>,
    mut commands: Commands,
) {
    for (entity, file) in q_files.iter() {
        // 选中态：清除旧选中，标记当前
        for old in q_selected.iter() {
            commands.entity(old).remove::<FileSelectedMarker>();
        }
        commands.entity(entity).insert(FileSelectedMarker);
        *selected_file = SelectedFilePath(Some(file.path.clone()));
        // 统一发打开请求（编辑器层负责视图切换与 IO）
        open_writer.write(crate::editor::tabs::OpenFileRequest {
            path: file.path.clone(),
            line: None,
        });
        // 关闭抽屉，露出上下文面板预览页
        drawer_toggle.write(crate::overlay::DrawerToggle(
            crate::overlay::OverlayId::FileDrawer,
        ));
    }
}
/// 处理目录条目点击：展开/折叠切换。
///
/// v8 M3-T6：文件树改成**扁平虚拟列表**后不再有「子项容器」，展开/折叠只改
/// [`ExpandedDirs`] 并把树标脏（重建时重新拍平 + 重新渲染可见行）。折叠箭头
/// 在行重建时按展开状态生成，无需再单独换图标。
fn handle_dir_click(
    q_dirs: Query<&DirEntry, Added<Pressed>>,
    mut dirty: ResMut<FileTreeDirty>,
    mut expanded: ResMut<ExpandedDirs>,
) {
    for dir in q_dirs.iter() {
        if dir.expanded {
            // 折叠：清理该目录及所有子孙目录的展开记录，避免悬空路径残留
            expanded.0.retain(|p| !p.starts_with(&dir.path));
        } else {
            expanded.0.insert(dir.path.clone());
        }
        // 标脏 → 下一帧重建（拍平 + 重渲染）
        dirty.0 = true;
        dirty.1 = 0;
    }
}
/// 更新文件/目录条目背景色：选中态半透明 accent、悬停态更淡 accent、默认透明。
///
/// 条目 Button 在 spawn 时挂 `BackgroundColor(Color::NONE)`，本系统据
/// `FileSelectedMarker`（选中）与 `Hovered`（悬停）改写背景色。
///
/// 优化：仅处理 Hovered 变化或 SelectedMarker 变化的条目，避免每帧全量遍历。
fn update_file_entry_style(
    q: Query<
        (Entity, Option<&FileSelectedMarker>, &Hovered),
        (
            Or<(With<FileEntry>, With<DirEntry>)>,
            Or<(Changed<Hovered>, Changed<FileSelectedMarker>)>,
        ),
    >,
    mut q_bg: Query<&mut BackgroundColor>,
    theme: Res<Theme>,
) {
    let sel_color = BackgroundColor(theme.accent_bg);
    let hover_color = BackgroundColor(theme.hover);
    let none_color = BackgroundColor(Color::NONE);
    for (entity, selected, interaction) in q.iter() {
        let want = if selected.is_some() {
            sel_color
        } else if interaction.get() {
            hover_color
        } else {
            none_color
        };
        if let Ok(mut bg) = q_bg.get_mut(entity)
            && *bg != want
        {
            *bg = want;
        }
    }
}
