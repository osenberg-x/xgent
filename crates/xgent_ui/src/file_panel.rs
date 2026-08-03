//! 文件面板：项目文件树预览 + 当前文件内容（MVP 只读）。
//!
//! 文件树从项目根遍历，按字母排序、目录优先。
//! 点击目录展开/折叠，点击文件读取内容在下方预览。
//! 忽略路径：MVP 硬编码匹配构建产物（`target/`、`node_modules/` 等）+ dotfile 白名单。
//! 文件系统变更时（daemon `FileChangedEvent`）自动重建文件树，保留已展开目录。

use std::collections::HashSet;
use std::path::PathBuf;

use parking_lot::Mutex;
use tokio::sync::oneshot;

use bevy::ecs::hierarchy::ChildOf;
use bevy::prelude::*;
use bevy::ui::ScrollPosition;

use crate::layout::FilePanelMarker;
use crate::theme::{Theme, space};

/// 文件树容器标记。
#[derive(Component, Default)]
pub struct FileTreeMarker;

/// 文件内容预览区标记。
#[derive(Component, Default)]
pub struct FilePreviewMarker;

/// 文件预览头路径文本标记（- 文件名前缀）。
#[derive(Component, Default)]
pub struct FilePreviewPathMarker;

/// 文件预览头元信息文本标记（字节数 · 只读预览）。
#[derive(Component, Default)]
pub struct FilePreviewMetaMarker;
/// 文件预览 ✕ 关闭按钮标记（收起分屏）。
#[derive(Component, Default)]
pub struct FilePreviewCloseMarker;

/// 文件预览内容区容器标记。
#[derive(Component, Default)]
pub struct FilePreviewBodyMarker;

/// 目录子项容器标记（展开时在此 spawn 子条目）。
#[derive(Component, Default)]
pub struct DirChildrenMarker;

/// 文件面板折叠按钮标记。
#[derive(Component, Default)]
pub struct FilePanelToggleMarker;

/// 目录行的箭头文本节点标记（> 折叠 / v 展开，点击展开/折叠时切换）。
#[derive(Component, Default)]
pub struct DirArrowMarker;

/// 目录行的图标文本节点标记（+ 折叠 / - 展开，展开/折叠时切换）。
#[derive(Component, Default)]
pub struct DirIconMarker;

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

/// 当前正在预览的文件路径（用于丢弃过期的异步读取结果）。
#[derive(Resource, Default)]
pub struct CurrentPreviewPath(pub Option<PathBuf>);

/// 当前选中的文件路径（重建/折叠恢复选中态，与 `FileSelectedMarker` 同步）。
#[derive(Resource, Default)]
pub struct SelectedFilePath(pub Option<PathBuf>);

/// 文件预览异步 IO runtime（由 xgent_app 注入 tokio handle）。
///
/// 若 `handle` 为 None，降级为同步 IO（小文件可用，大文件会卡帧）。
/// 对齐 [`crate::editor::io::EditorIoRuntime`] 的注入模式。
#[derive(Resource)]
pub struct PreviewIoRuntime {
    /// tokio runtime handle（可选，便于测试不依赖 runtime）
    pub handle: Option<tokio::runtime::Handle>,
    /// 待 poll 的预览读取结果 receiver 列表
    pending: Mutex<Vec<oneshot::Receiver<PreviewFileContent>>>,
}

impl Default for PreviewIoRuntime {
    fn default() -> Self {
        Self {
            handle: None,
            pending: Mutex::new(Vec::new()),
        }
    }
}

impl PreviewIoRuntime {
    /// 注入 handle。
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            handle: Some(handle),
            pending: Mutex::new(Vec::new()),
        }
    }
}

/// 异步读取的文件内容（预览用）。
struct PreviewFileContent {
    /// 文件路径（用于回传标识）
    path: PathBuf,
    /// 读取结果：Ok(字节数, 截断后文本) 或 Err(错误信息)
    content: Result<(usize, String), String>,
}

/// 预览读取完成消息（异步任务完成后发回 ECS）。
#[derive(Message, Debug, Clone)]
pub struct PreviewReadResult {
    /// 文件路径
    pub path: PathBuf,
    /// 读取结果：Ok(字节数, 截断后文本) 或 Err(错误信息)
    pub content: Result<(usize, String), String>,
}

/// 文件面板插件。
pub struct FilePanelPlugin;

impl Plugin for FilePanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProjectRoot>()
            .init_resource::<SelectedFilePath>()
            .init_resource::<FileTreeDirty>()
            .init_resource::<ExpandedDirs>()
            .init_resource::<PreviewIoRuntime>()
            .init_resource::<CurrentPreviewPath>()
            // FileChangedEvent 由 EditorPlugin 注册，此处幂等再注册确保独立可用
            .add_message::<crate::editor::conflict::FileChangedEvent>()
            .add_message::<PreviewReadResult>()
            .add_systems(Startup, spawn_file_panel.after(crate::layout::spawn_layout))
            .add_systems(
                Startup,
                spawn_file_preview.after(crate::layout::spawn_layout),
            )
            .add_systems(
                Update,
                (
                    handle_file_click,
                    handle_dir_click,
                    rebuild_file_tree,
                    handle_file_panel_toggle,
                    handle_file_preview_close,
                    mark_file_tree_dirty_on_fs_change,
                    poll_preview_read_results,
                    apply_preview_read_result,
                )
                    .chain()
                    .before(update_file_entry_style),
            );
    }
}

/// 启动时在文件面板内 spawn 标题头 + 文件树（预览区移至右侧分屏，见 [`spawn_file_preview`]）。
fn spawn_file_panel(
    mut commands: Commands,
    q: Query<Entity, With<FilePanelMarker>>,
    theme: Res<Theme>,
    loc: Res<xgent_settings::Localizer>,
) {
    let Ok(entity) = q.single() else {
        return;
    };
    let font = theme.font_size;
    commands.entity(entity).with_children(|p| {
        // 标题头：资源管理器 + 折叠按钮◀（点击切 FilePanelCollapsed）
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
            BackgroundColor(theme.bar),
            BorderColor::all(theme.border),
        ))
        .with_children(|head| {
            // 标题（资源管理器，大写小字体、字间距）
            head.spawn((
                Text::new(crate::i18n::tr(&loc, "file-panel-title").to_uppercase()),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme.text_dim),
            ));
            // 折叠按钮◀
            head.spawn((
                Button,
                Node {
                    width: px(24.0),
                    height: px(24.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                Text::new("<"),
                TextFont {
                    font_size: FontSize::Px(font),
                    ..default()
                },
                TextColor(theme.text_dim),
                FilePanelToggleMarker,
            ));
        });
        // 文件树区（可滚动，独占文件面板）
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
}

/// 启动时在右侧分屏容器内 spawn 文件预览区（初始隐藏）。
fn spawn_file_preview(
    mut commands: Commands,
    q_side: Query<Entity, With<crate::layout::SideViewMarker>>,
    theme: Res<Theme>,
) {
    let Ok(side) = q_side.single() else {
        return;
    };
    let font = theme.font_size;
    // 预览区容器（Column：fv-head + fv-body），初始隐藏
    let preview = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme.bg),
            FilePreviewMarker,
        ))
        .with_children(|p| {
            // fv-head：路径 + spacer + ✕ 关闭
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(space::SM),
                    padding: UiRect::all(px(space::MD)),
                    border: UiRect::bottom(px(1.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(theme.bar),
                BorderColor::all(theme.border),
            ))
            .with_children(|head| {
                // 路径文本
                head.spawn((
                    Text::new(String::new()),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                    FilePreviewPathMarker,
                ));
                // · 元信息（字节数 · 只读预览）
                head.spawn((
                    Text::new(String::new()),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                    FilePreviewMetaMarker,
                ));
                // spacer
                head.spawn((Node {
                    flex_grow: 1.0,
                    ..default()
                },));
                // ✕ 关闭按钮
                head.spawn((
                    Button,
                    Node {
                        width: px(28.0),
                        height: px(28.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    Text::new("x"),
                    TextFont {
                        font_size: FontSize::Px(font),
                        ..default()
                    },
                    TextColor(theme.text_dim),
                    FilePreviewCloseMarker,
                ));
            });
            // fv-body：可滚动内容区
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip_y(),
                    ..default()
                },
                ScrollPosition::default(),
                FilePreviewBodyMarker,
            ));
        })
        .id();
    commands.entity(side).add_child(preview);
}

/// 目录或文件内容（一次遍历的一层条目）。
struct DirContent {
    name: String,
    path: PathBuf,
    is_dir: bool,
}
/// 判断路径是否被忽略（MVP 简单匹配）。
///
/// 过滤构建产物、VCS 元数据、IDE 配置及系统临时文件（如 `.DS_Store`）。
/// 隐藏文件（`.` 开头）除显式允许的外均过滤，避免树被噪声淹没。
fn is_ignored(name: &str) -> bool {
    if name.starts_with('.') {
        // 允许 `.env`、`.gitignore` 等用户可能需要查看的 dotfile
        return !matches!(
            name,
            ".env" | ".gitignore" | ".gitattributes" | ".editorconfig"
        );
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

/// 将文件字节截断为预览文本（最多 1000 行 / 256KB 原始字节）。
///
/// 先按字节上限截断（回退到 UTF-8 字符边界），再取前 1000 行。
/// 返回 `(原始字节数, 截断后文本)`——`原始字节数` 反映文件真实大小。
fn truncate_preview(bytes: &[u8]) -> (usize, String) {
    const MAX_BYTES: usize = 256 * 1024;
    let len = bytes.len();
    let capped = if bytes.len() > MAX_BYTES {
        // 截断到 MAX_BYTES，回退到最后一个 UTF-8 字符边界
        let mut end = MAX_BYTES;
        while end > 0 && matches!(bytes.get(end), Some(b) if (*b & 0xC0) == 0x80) {
            end -= 1;
        }
        &bytes[..end]
    } else {
        bytes
    };
    let text = String::from_utf8_lossy(capped);
    let truncated: String = text.lines().take(1000).collect::<Vec<_>>().join("\n");
    (len, truncated)
}

/// spawn 一个文件树条目（目录或文件）。
///
/// 目录节点 = 外层 Column 容器 + 目录行 Button(row: 箭头 + 图标 + 名称) + 子容器 Node。
/// 文件节点 = Button(row: 图标 + 名称)。
/// 箭头/图标/名称分离为独立 Text 子节点，便于展开/折叠时单独切换，
/// 且支持选中/悬停态（由独立系统据 `FileSelectedMarker`/`Interaction` 设背景色）。
/// 子项缩进由子容器的左 padding 累积（每层 `space::LG` = 16px）。
///
/// `expanded_dirs` 记录已展开目录路径，预展开的目录在 spawn 时即填充子项（递归）。
/// `selected_path` 非空时，匹配的文件条目在 spawn 时即挂 `FileSelectedMarker`（重建恢复选中态）。
fn spawn_entry(
    parent: &mut ChildSpawnerCommands,
    entry: &DirContent,
    theme: &Theme,
    font: f32,
    expanded_dirs: &HashSet<PathBuf>,
    selected_path: Option<&std::path::Path>,
) {
    let font_size = FontSize::Px(font);
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
                        padding: UiRect::vertical(px(2.0)),
                        ..default()
                    },
                    DirEntry {
                        path: entry.path.clone(),
                        expanded: is_expanded,
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|row| {
                    // 箭头（> 折叠 / v 展开）
                    row.spawn((
                        Node {
                            width: px(10.0),
                            ..default()
                        },
                        Text::new(if is_expanded { "v" } else { ">" }),
                        TextFont {
                            font_size,
                            ..default()
                        },
                        TextColor(theme.text_dim),
                        DirArrowMarker,
                    ));
                    // 图标（+ 折叠 / - 展开）
                    row.spawn((
                        Node {
                            width: px(14.0),
                            ..default()
                        },
                        Text::new(if is_expanded { "-" } else { "+" }),
                        TextFont {
                            font_size,
                            ..default()
                        },
                        TextColor(theme.text),
                        DirIconMarker,
                    ));
                    // 名称
                    row.spawn((
                        Text::new(entry.name.clone()),
                        TextFont {
                            font_size,
                            ..default()
                        },
                        TextColor(theme.text),
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
                            spawn_entry(cc, child, theme, font, expanded_dirs, selected_path);
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
                padding: UiRect::vertical(px(2.0)),
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
            // 图标占位（对齐目录行的箭头宽度）
            row.spawn((
                Node {
                    width: px(10.0),
                    ..default()
                },
                Text::new(""),
                TextFont {
                    font_size,
                    ..default()
                },
            ));
            // 文件图标
            row.spawn((
                Node {
                    width: px(14.0),
                    ..default()
                },
                Text::new("-"),
                TextFont {
                    font_size,
                    ..default()
                },
                TextColor(theme.text),
            ));
            // 名称
            row.spawn((
                Text::new(entry.name.clone()),
                TextFont {
                    font_size,
                    ..default()
                },
                TextColor(theme.text),
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
    let font = theme.font_size;
    let entries = list_dir(&root.path);
    let expanded = &expanded.0;
    commands.entity(tree).with_children(|p| {
        for entry in &entries {
            spawn_entry(p, entry, &theme, font, expanded, selected_path.as_deref());
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

/// 处理文件条目点击：代码文件打开编辑器，其他文件在右侧分屏预览区显示。
///
/// 两种情况都展开右侧分屏（`SideViewCollapsed=false`）并设 `SideViewContent`：
/// - 代码文件 → `SideViewContent::Editor` + 发 `OpenFileRequest`；
/// - 非代码文件 → `SideViewContent::Preview` + 异步读取文件内容（结果由 [`apply_preview_read_result`] 填充）。
///
/// 显隐（`EditorViewMarker`/`FilePreviewMarker` 的 `display`）由
/// [`crate::editor::apply_editor_view_visibility`] 统一应用，本系统不直接写
/// `&mut Node` 以避免 B0001 query 冲突。
fn handle_file_click(
    q_files: Query<(Entity, &FileEntry, &Interaction), Changed<Interaction>>,
    q_preview: Query<Entity, With<FilePreviewMarker>>,
    // q_path/q_meta 都 &mut Text，With<Marker> 之间 Bevy 无法证明不相交
    // （With 不隐含 Without），同系统内会触发 B0001，故用 ParamSet 串行化。
    mut q_texts: ParamSet<(
        Query<&mut Text, With<FilePreviewPathMarker>>,
        Query<&mut Text, With<FilePreviewMetaMarker>>,
    )>,
    q_selected: Query<Entity, With<FileSelectedMarker>>,
    mut selected_file: ResMut<SelectedFilePath>,
    mut side_collapsed: ResMut<crate::layout::SideViewCollapsed>,
    mut content: ResMut<crate::editor::SideViewContent>,
    mut commands: Commands,
    mut open_writer: MessageWriter<crate::editor::tabs::OpenFileRequest>,
    io_rt: Res<PreviewIoRuntime>,
    mut current_preview: ResMut<CurrentPreviewPath>,
    loc: Res<xgent_settings::Localizer>,
) {
    let has_preview = q_preview.single().is_ok();
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
        // 展开右侧分屏
        side_collapsed.0 = false;
        // 代码文件 → 编辑器视图（编辑器层接管显隐）
        if is_code_file(&file.path) {
            *content = crate::editor::SideViewContent::Editor;
            *current_preview = CurrentPreviewPath(None);
            // 清空预览头文本，避免从非代码文件切来时残留旧路径/元信息
            if let Ok(mut t) = q_texts.p0().single_mut() {
                t.0 = String::new();
            }
            if let Ok(mut t) = q_texts.p1().single_mut() {
                t.0 = String::new();
            }
            open_writer.write(crate::editor::tabs::OpenFileRequest {
                path: file.path.clone(),
                line: None,
            });
            continue;
        }
        // 非代码文件 → 预览视图 + 更新 fv-head 路径 + 异步读取文件内容
        // 预览区不存在时跳过（代码文件路径不受影响，已在上方 continue）
        if !has_preview {
            continue;
        }
        *current_preview = CurrentPreviewPath(Some(file.path.clone()));
        *content = crate::editor::SideViewContent::Preview;
        let name = file
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        // 更新 fv-head 路径文本
        if let Ok(mut path_text) = q_texts.p0().single_mut() {
            path_text.0 = format!("- {}", name);
        }
        // 更新 fv-head 元信息：加载中
        if let Ok(mut meta_text) = q_texts.p1().single_mut() {
            meta_text.0 = crate::i18n::tr(&loc, "preview-loading");
        }
        // 异步读取文件内容（tokio task），结果经 oneshot channel 回 ECS
        let path = file.path.clone();
        if let Some(handle) = io_rt.handle.clone() {
            let (tx, rx) = oneshot::channel::<PreviewFileContent>();
            handle.spawn(async move {
                let result = tokio::fs::read(&path)
                    .await
                    .map_err(|e| e.to_string())
                    .map(|b| truncate_preview(&b));
                let _ = tx.send(PreviewFileContent {
                    path,
                    content: result,
                });
            });
            io_rt.pending.lock().push(rx);
        } else {
            // 降级同步 IO（无 runtime，小文件可用）
            let result = std::fs::read(&file.path)
                .map_err(|e| e.to_string())
                .map(|b| truncate_preview(&b));
            commands.write_message(PreviewReadResult {
                path: file.path.clone(),
                content: result,
            });
        }
    }
}

/// 每帧非阻塞 poll pending 预览读取 receiver，就绪的发 [`PreviewReadResult`] 消息。
///
/// 未就绪的保留到下一帧，避免 `blocking_recv` 卡 ECS 帧循环。
/// 同步降级路径（无 runtime）直接由 `handle_file_click` 发消息，不经此系统。
fn poll_preview_read_results(
    io_rt: Res<PreviewIoRuntime>,
    mut writer: MessageWriter<PreviewReadResult>,
) {
    let mut pending = io_rt.pending.lock();
    let mut still_pending = Vec::with_capacity(pending.len());
    for mut rx in pending.drain(..) {
        match rx.try_recv() {
            Ok(result) => {
                writer.write(PreviewReadResult {
                    path: result.path,
                    content: result.content,
                });
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                still_pending.push(rx);
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                // 任务被取消，静默丢弃
            }
        }
    }
    *pending = still_pending;
}

/// 订阅 [`PreviewReadResult`]，把读取成功的文本填充到 fv-body + 更新元信息。
///
/// 高亮逻辑与原同步路径一致：Rust 文件用 `xui::highlight` 按 span spawn Text 节点，
/// 其余纯文本。
fn apply_preview_read_result(
    mut reader: MessageReader<PreviewReadResult>,
    content: Res<crate::editor::SideViewContent>,
    current_preview: Res<CurrentPreviewPath>,
    q_body: Query<Entity, With<FilePreviewBodyMarker>>,
    // q_path/q_meta 都 &mut Text，With<Marker> 之间 Bevy 无法证明不相交
    // （With 不隐含 Without），同系统内会触发 B0001，故用 ParamSet 串行化。
    mut q_texts: ParamSet<(
        Query<&mut Text, With<FilePreviewPathMarker>>,
        Query<&mut Text, With<FilePreviewMetaMarker>>,
    )>,
    theme: Res<Theme>,
    mut commands: Commands,
    loc: Res<xgent_settings::Localizer>,
) {
    let font = theme.font_size;
    for result in reader.read() {
        // 丢弃过期结果：用户已切换到其他文件/视图
        if *content != crate::editor::SideViewContent::Preview
            || current_preview.0.as_ref() != Some(&result.path)
        {
            continue;
        }
        let (bytes_len, truncated) = match &result.content {
            Ok((len, text)) => (*len, text.clone()),
            Err(e) => {
                // 读取失败：更新元信息显示错误
                if let Ok(mut meta_text) = q_texts.p1().single_mut() {
                    meta_text.0 = crate::i18n::tr_with(
                        &loc,
                        "preview-read-error",
                        &[("error", e.clone())],
                    );
                }
                continue;
            }
        };
        // 更新 fv-head 元信息：字节数 · 只读预览
        if let Ok(mut meta_text) = q_texts.p1().single_mut() {
            meta_text.0 = crate::i18n::tr_with(
                &loc,
                "preview-bytes",
                &[("bytes", bytes_len.to_string())],
            );
        }
        // 填充 fv-body 内容（Rust 语法高亮，其余纯文本）
        if let Ok(body) = q_body.single() {
            commands.entity(body).despawn_children();
            commands.entity(body).with_children(|p| {
                let mono = FontSize::Px(font - 2.0);
                if let Some(lang) = preview_language(&result.path) {
                    // Rust：tree-sitter 高亮，按 span spawn Text 节点
                    let spans = xui::highlight(&truncated, lang);
                    for span in spans {
                        let start = span.start.min(truncated.len());
                        let end = span.end.min(truncated.len());
                        if end <= start {
                            continue;
                        }
                        // 确保字节偏移对齐 UTF-8 字符边界，防止切片 panic
                        if !truncated.is_char_boundary(start) || !truncated.is_char_boundary(end) {
                            continue;
                        }
                        let slice = &truncated[start..end];
                        let color = xui::span_color_for(span.kind);
                        p.spawn((
                            Node { ..default() },
                            Text::new(slice.to_string()),
                            TextFont {
                                font_size: mono,
                                ..default()
                            },
                            TextColor(color),
                        ));
                    }
                } else {
                    // 非 Rust：纯文本
                    p.spawn((
                        Node { ..default() },
                        Text::new(truncated),
                        TextFont {
                            font_size: mono,
                            ..default()
                        },
                        TextColor(theme.text_dim),
                    ));
                }
            });
        }
    }
}
/// 判断是否为代码文件（按扩展名）。
fn is_code_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some(
            "rs" | "toml"
                | "json"
                | "md"
                | "txt"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "py"
                | "go"
                | "c"
                | "cpp"
                | "h"
                | "yml"
                | "yaml"
                | "sh"
                | "rb"
                | "java"
                | "css"
                | "html"
                | "sql"
        )
    )
}

/// 预览用的语法高亮语言：MVP 仅 Rust（tree-sitter grammar 随二进制，D-06）。
/// 非 Rust 文件返回 None，调用方渲染纯文本。
fn preview_language(path: &std::path::Path) -> Option<xui::Language> {
    if path.extension().and_then(|e| e.to_str()) == Some("rs") {
        Some(xui::Language::Rust)
    } else {
        None
    }
}

/// 处理目录条目点击：展开/折叠切换，在子项容器 spawn/despawn 子条目。
///
/// 展开/折叠时单独切换 `DirArrowMarker`（>/v）与 `DirIconMarker`（+/-）
/// 子节点文本，而非重写整行文本——因目录行现为 row 容器含分离的子节点。
fn handle_dir_click(
    mut commands: Commands,
    mut q_dirs: Query<(&mut DirEntry, &Interaction, &ChildOf), Changed<Interaction>>,
    q_children: Query<&Children>,
    q_dir_children: Query<Entity, With<DirChildrenMarker>>,
    q_dir_rows: Query<Entity, With<DirEntry>>,
    selected_file: Res<SelectedFilePath>,
    mut q_text: ParamSet<(
        Query<&mut Text, With<DirArrowMarker>>,
        Query<&mut Text, With<DirIconMarker>>,
    )>,
    theme: Res<Theme>,
    mut expanded: ResMut<ExpandedDirs>,
) {
    let font = theme.font_size;
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
        // 在目录行 Button 的子节点里找箭头与图标
        let Ok(row_children) = q_children.get(dir_row) else {
            continue;
        };
        let mut arrow_entity = None;
        let mut icon_entity = None;
        for &c in row_children {
            if q_text.p0().get(c).is_ok() {
                arrow_entity = Some(c);
            } else if q_text.p1().get(c).is_ok() {
                icon_entity = Some(c);
            }
        }

        if dir.expanded {
            // 折叠：清理该目录及所有子孙目录的展开记录，避免悬空路径残留
            dir.expanded = false;
            let collapsed_path = dir.path.clone();
            expanded.0.retain(|p| !p.starts_with(&collapsed_path));
            if let Some(e) = arrow_entity {
                if let Ok(mut t) = q_text.p0().get_mut(e) {
                    *t = Text::new(">");
                }
            }
            if let Some(e) = icon_entity {
                if let Ok(mut t) = q_text.p1().get_mut(e) {
                    *t = Text::new("+");
                }
            }
            commands.entity(child_container).despawn_children();
        } else {
            // 展开：读子目录内容，spawn 到子容器
            dir.expanded = true;
            expanded.0.insert(dir.path.clone());
            if let Some(e) = arrow_entity {
                if let Ok(mut t) = q_text.p0().get_mut(e) {
                    *t = Text::new("v");
                }
            }
            if let Some(e) = icon_entity {
                if let Ok(mut t) = q_text.p1().get_mut(e) {
                    *t = Text::new("-");
                }
            }
            let entries = list_dir(&dir.path);
            let expanded_set = &expanded.0;
            let selected_path = selected_file.0.clone();
            commands.entity(child_container).with_children(|p| {
                for entry in &entries {
                    spawn_entry(p, entry, &theme, font, expanded_set, selected_path.as_deref());
                }
            });
        }
    }
}
/// 处理文件面板折叠按钮点击：切换 `FilePanelCollapsed`。
fn handle_file_panel_toggle(
    q_btn: Query<&Interaction, (With<FilePanelToggleMarker>, Changed<Interaction>)>,
    mut collapsed: ResMut<crate::layout::FilePanelCollapsed>,
) {
    for interaction in q_btn.iter() {
        if *interaction == Interaction::Pressed {
            collapsed.0 = !collapsed.0;
        }
    }
}
/// 处理文件预览 ✕ 关闭按钮点击：收起右侧分屏 + 清空内容。
fn handle_file_preview_close(
    q_btn: Query<&Interaction, (With<FilePreviewCloseMarker>, Changed<Interaction>)>,
    q_body: Query<Entity, With<FilePreviewBodyMarker>>,
    mut q_texts: ParamSet<(
        Query<&mut Text, With<FilePreviewPathMarker>>,
        Query<&mut Text, With<FilePreviewMetaMarker>>,
    )>,
    mut side_collapsed: ResMut<crate::layout::SideViewCollapsed>,
    mut content: ResMut<crate::editor::SideViewContent>,
    mut current_preview: ResMut<CurrentPreviewPath>,
    mut commands: Commands,
) {
    for interaction in q_btn.iter() {
        if *interaction == Interaction::Pressed {
            side_collapsed.0 = true;
            *content = crate::editor::SideViewContent::None;
            *current_preview = CurrentPreviewPath(None);
            // 清空 fv-body 子节点 + 预览头文本，避免残留旧内容
            if let Ok(body) = q_body.single() {
                commands.entity(body).despawn_children();
            }
            if let Ok(mut t) = q_texts.p0().single_mut() {
                t.0 = String::new();
            }
            if let Ok(mut t) = q_texts.p1().single_mut() {
                t.0 = String::new();
            }
        }
    }
}
/// 更新文件/目录条目背景色：选中态半透明 accent、悬停态更淡 accent、默认透明。
///
/// 条目 Button 在 spawn 时挂 `BackgroundColor(Color::NONE)`，本系统每帧据
/// `FileSelectedMarker`（选中）与 `Interaction::Hovered`（悬停）改写背景色。
fn update_file_entry_style(
    q: Query<
        (Entity, Option<&FileSelectedMarker>, &Interaction),
        Or<(With<FileEntry>, With<DirEntry>)>,
    >,
    mut q_bg: Query<&mut BackgroundColor>,
) {
    let sel_color = BackgroundColor(Color::srgba(0.36, 0.62, 0.92, 0.22));
    let hover_color = BackgroundColor(Color::srgba(0.36, 0.62, 0.92, 0.12));
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
