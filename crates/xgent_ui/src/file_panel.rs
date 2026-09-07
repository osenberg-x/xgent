//! 文件抽屉（v7 抽屉化）：项目文件树浏览，左侧 overlay（方案 §8.7）。
//!
//! 由 rail 文件按钮 / `filepanel.toggle` 快捷键切换 `FileDrawerOpen`。
//! 文件树从项目根遍历，按字母排序、目录优先。点击目录展开/折叠，
//! 点击文件发 `OpenFileRequest` 走上下文面板预览页加载（原内嵌预览区已取消）。
//! 忽略路径：MVP 硬编码匹配构建产物（`target/`、`node_modules/` 等）+ dotfile 白名单。
//! 文件系统变更时（daemon `FileChangedEvent`）自动重建文件树，保留已展开目录。

use bevy::ecs::hierarchy::ChildOf;
use bevy::prelude::*;
use bevy::ui::ScrollPosition;
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

/// 目录子项容器标记（展开时在此 spawn 子条目）。
#[derive(Component, Default)]
pub struct DirChildrenMarker;

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
pub struct FilePanelPlugin;

impl Plugin for FilePanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProjectRoot>()
            .init_resource::<SelectedFilePath>()
            .init_resource::<FileTreeDirty>()
            .init_resource::<ExpandedDirs>()
            // FileChangedEvent 由 EditorPlugin 注册，此处幂等再注册确保独立可用
            .add_message::<crate::editor::conflict::FileChangedEvent>()
            .add_systems(Startup, spawn_file_panel.after(crate::layout::spawn_layout))
            .add_systems(
                Update,
                (
                    handle_drawer_visibility,
                    handle_drawer_overlay_click,
                    handle_file_click,
                    handle_dir_click,
                    rebuild_file_tree,
                    mark_file_tree_dirty_on_fs_change,
                )
                    .chain()
                    .before(update_file_entry_style),
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
) {
    let Ok(root) = q_root.single() else {
        return;
    };
    commands.entity(root).with_children(|root| {
        // 遮罩（点击关闭抽屉；初始隐藏）
        root.spawn((
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
            GlobalZIndex(crate::session_history::DRAWER_Z),
            Button,
            FileDrawerOverlayMarker,
        ));
        // 抽屉面板（左侧贴齐，surface 底 + 右边框；初始隐藏）
        root.spawn((
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
            GlobalZIndex(crate::session_history::DRAWER_Z + 1),
            FilePanelMarker,
        ))
        .with_children(|p| {
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
                    11.0,
                    510,
                    theme.text_dim,
                    crate::theme::type_scale::line_height::UI,
                ));
            });
            // 文件树区（可滚动，独占抽屉）
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip_y(),
                    ..default()
                },
                ScrollPosition::default(),
                FileTreeMarker,
            ));
        });
    });
}

/// 抽屉显隐随 `FileDrawerOpen` 切换（遮罩与面板一起）。
///
/// 只在状态变化时写 `Node.display`，避免每帧 mutation 触发 change detection。
fn handle_drawer_visibility(
    open: Res<FileDrawerOpen>,
    mut q_overlay: Query<&mut Node, With<FileDrawerOverlayMarker>>,
    mut q_panel: Query<&mut Node, (With<FilePanelMarker>, Without<FileDrawerOverlayMarker>)>,
) {
    if !open.is_changed() {
        return;
    }
    let display = if open.0 { Display::Flex } else { Display::None };
    for mut node in q_overlay.iter_mut() {
        node.display = display;
    }
    for mut node in q_panel.iter_mut() {
        node.display = display;
    }
}

/// 遮罩点击关闭抽屉。
fn handle_drawer_overlay_click(
    q: Query<&Interaction, (With<FileDrawerOverlayMarker>, Changed<Interaction>)>,
    mut open: ResMut<FileDrawerOpen>,
) {
    for interaction in q.iter() {
        if *interaction == Interaction::Pressed {
            open.0 = false;
        }
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

/// spawn 一个文件树条目（目录或文件）。
///
/// 目录行（折叠箭头 + 矢量目录图标 + 名称）/ 文件行（矢量文件图标 + 名称），
/// 条目选中态由 [`update_file_entry_style`] 绘制背景。
fn spawn_entry(
    parent: &mut ChildSpawnerCommands,
    entry: &DirContent,
    theme: &Theme,
    icons: &crate::kit::IconAssets,
    expanded_dirs: &HashSet<PathBuf>,
    selected_path: Option<&std::path::Path>,
) {
    if entry.is_dir {
        let is_expanded = expanded_dirs.contains(&entry.path);
        // 外层 Column：目录行 + 子项容器
        parent
            .spawn((Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },))
            .with_children(|col| {
                // 目录行（Button + row: 箭头 + 图标 + 名称）
                col.spawn((
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(space::XS),
                        padding: UiRect::all(px(space::XS)),
                        border_radius: BorderRadius::all(px(crate::theme::radius::MICRO)),
                        ..default()
                    },
                    DirEntry {
                        path: entry.path.clone(),
                        expanded: is_expanded,
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|row| {
                    // 折叠箭头（chevron-right 折叠 / chevron-down 展开）
                    row.spawn(crate::kit::icon(
                        icons,
                        if is_expanded {
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
                    row.spawn(crate::fonts::ui_text(
                        entry.name.clone(),
                        crate::theme::type_scale::CAPTION,
                        400,
                        theme.text_dim,
                        crate::theme::type_scale::line_height::UI,
                    ));
                });
                // 子项容器（折叠态空，预展开时递归 spawn 子条目）
                let mut child_container = col.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::left(px(space::LG)),
                        ..default()
                    },
                    DirChildrenMarker,
                ));
                if is_expanded {
                    let children = list_dir(&entry.path);
                    child_container.with_children(|cc| {
                        for child in &children {
                            spawn_entry(cc, child, theme, icons, expanded_dirs, selected_path);
                        }
                    });
                }
            });
    } else {
        let mut cmd = parent.spawn((
            Button,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(space::XS),
                padding: UiRect::all(px(space::XS)),
                border_radius: BorderRadius::all(px(crate::theme::radius::MICRO)),
                ..default()
            },
            FileEntry {
                path: entry.path.clone(),
            },
            BackgroundColor(Color::NONE),
        ));
        // 重建后恢复选中态
        if selected_path == Some(entry.path.as_path()) {
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
            row.spawn(crate::fonts::ui_text(
                entry.name.clone(),
                crate::theme::type_scale::CAPTION,
                400,
                theme.text_dim,
                crate::theme::type_scale::line_height::UI,
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
    q_tree: Query<Entity, With<FileTreeMarker>>,
    selected_file: Res<SelectedFilePath>,
    theme: Res<Theme>,
    icons: Res<crate::kit::IconAssets>,
    mut commands: Commands,
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
    let Ok(tree) = q_tree.single() else {
        return;
    };
    // 从 resource 取选中路径（重建后 spawn_entry 恢复 FileSelectedMarker）
    let selected_path = selected_file.0.clone();
    // 清除旧条目
    commands.entity(tree).despawn_children();
    let entries = list_dir(&root.path);
    let expanded = &expanded.0;
    commands.entity(tree).with_children(|p| {
        for entry in &entries {
            spawn_entry(p, entry, &theme, &icons, expanded, selected_path.as_deref());
        }
    });
    dirty.0 = false;
    dirty.1 = 0;
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
/// `handle_open_file_requests` 会切 `SideViewContent::Editor` 并展开分屏。
/// 点击后同时关闭抽屉（方案 §8.7：浏览→点文件→context 预览打开）。
///
/// 选中态（`FileSelectedMarker`）由本系统维护；条目背景色由
/// [`update_file_entry_style`] 统一绘制。
fn handle_file_click(
    q_files: Query<(Entity, &FileEntry, &Interaction), Changed<Interaction>>,
    q_selected: Query<Entity, With<FileSelectedMarker>>,
    mut selected_file: ResMut<SelectedFilePath>,
    mut drawer: ResMut<FileDrawerOpen>,
    mut open_writer: MessageWriter<crate::editor::tabs::OpenFileRequest>,
    mut commands: Commands,
) {
    for (entity, file, interaction) in q_files.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
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
        drawer.0 = false;
    }
}
/// 处理目录条目点击：展开/折叠切换，在子项容器 spawn/despawn 子条目。
///
/// 展开/折叠时替换目录行的折叠箭头图标（chevron-right/chevron-down，
/// ImageNode 不能旋转故用两枚图标），并 spawn/despawn 子项容器内容。
fn handle_dir_click(
    mut commands: Commands,
    mut q_dirs: Query<(&mut DirEntry, &Interaction, &ChildOf), Changed<Interaction>>,
    q_children: Query<&Children>,
    q_dir_children: Query<Entity, With<DirChildrenMarker>>,
    q_dir_rows: Query<Entity, With<DirEntry>>,
    mut q_icons: Query<&mut ImageNode>,
    selected_file: Res<SelectedFilePath>,
    theme: Res<Theme>,
    icons: Res<crate::kit::IconAssets>,
    mut expanded: ResMut<ExpandedDirs>,
) {
    for (mut dir, interaction, parent) in q_dirs.iter_mut() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // 拿外层 Column 的 children，找目录行 Button 与 DirChildrenMarker 子容器
        let Ok(col_children) = q_children.get(parent.0) else {
            continue;
        };
        let mut child_container = None;
        let mut dir_row = None;
        for &c in col_children {
            if q_dir_children.get(c).is_ok() {
                child_container = Some(c);
            } else if q_dir_rows.get(c).is_ok() {
                dir_row = Some(c);
            }
        }
        let Some(child_container) = child_container else {
            continue;
        };
        let Some(dir_row) = dir_row else {
            continue;
        };
        // 在目录行 Button 的子节点里找折叠箭头（首个 ImageNode 子节点）
        let Ok(row_children) = q_children.get(dir_row) else {
            continue;
        };
        let mut arrow_entity = None;
        for &c in row_children {
            if q_icons.get(c).is_ok() {
                arrow_entity = Some(c);
                break;
            }
        }

        if dir.expanded {
            // 折叠：清理该目录及所有子孙目录的展开记录，避免悬空路径残留
            dir.expanded = false;
            let collapsed_path = dir.path.clone();
            expanded.0.retain(|p| !p.starts_with(&collapsed_path));
            if let Some(e) = arrow_entity
                && let Ok(mut img) = q_icons.get_mut(e)
            {
                img.image = icons.get("chevron-right");
            }
            commands.entity(child_container).despawn_children();
        } else {
            // 展开：读子目录内容，spawn 到子容器
            dir.expanded = true;
            expanded.0.insert(dir.path.clone());
            if let Some(e) = arrow_entity
                && let Ok(mut img) = q_icons.get_mut(e)
            {
                img.image = icons.get("chevron-down");
            }
            let entries = list_dir(&dir.path);
            let expanded_set = &expanded.0;
            let selected_path = selected_file.0.clone();
            commands.entity(child_container).with_children(|p| {
                for entry in &entries {
                    spawn_entry(
                        p,
                        entry,
                        &theme,
                        &icons,
                        expanded_set,
                        selected_path.as_deref(),
                    );
                }
            });
        }
    }
}
/// 更新文件/目录条目背景色：选中态半透明 accent、悬停态更淡 accent、默认透明。
///
/// 条目 Button 在 spawn 时挂 `BackgroundColor(Color::NONE)`，本系统据
/// `FileSelectedMarker`（选中）与 `Interaction::Hovered`（悬停）改写背景色。
///
/// 优化：仅处理 Interaction 变化或 SelectedMarker 变化的条目，避免每帧全量遍历。
fn update_file_entry_style(
    q: Query<
        (Entity, Option<&FileSelectedMarker>, &Interaction),
        (
            Or<(With<FileEntry>, With<DirEntry>)>,
            Or<(Changed<Interaction>, Changed<FileSelectedMarker>)>,
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
        } else if *interaction == Interaction::Hovered {
            hover_color
        } else {
            none_color
        };
        if let Ok(mut bg) = q_bg.get_mut(entity) {
            if *bg != want {
                *bg = want;
            }
        }
    }
}
