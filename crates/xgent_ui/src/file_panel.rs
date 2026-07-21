//! 文件面板：项目文件树预览 + 当前文件内容（MVP 只读）。
//!
//! 文件树从项目根遍历，按字母排序、目录优先。
//! 点击目录展开/折叠，点击文件读取内容在下方预览。
//! `.gitignore` 忽略路径：MVP 简单匹配 `target/`、`.git/`、`node_modules/`。

use std::path::PathBuf;

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

/// 文件预览头路径文本标记（📄 + 文件名）。
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

/// 目录行的箭头文本节点标记（▸/▾，点击展开/折叠时切换）。
#[derive(Component, Default)]
pub struct DirArrowMarker;

/// 目录行的图标文本节点标记（📁/📂，展开/折叠时切换）。
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

/// 文件面板插件。
pub struct FilePanelPlugin;

impl Plugin for FilePanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProjectRoot>()
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
                    update_file_entry_style,
                ),
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
                Text::new("◀"),
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
            // fv-head：📄 路径 + spacer + ✕ 关闭
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
                // 📄 路径文本
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
                    Text::new("✕"),
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
    let _ = font;
    commands.entity(side).add_child(preview);
}

/// 目录或文件内容（一次遍历的一层条目）。
struct DirContent {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// 判断路径是否被忽略（MVP 简单匹配）。
fn is_ignored(name: &str) -> bool {
    matches!(
        name,
        "target" | ".git" | "node_modules" | ".xgent" | "__pycache__" | ".next" | "dist" | "build"
    )
}
/// 列出目录下的一层条目（目录优先，字母排序）。
fn list_dir(dir: &PathBuf) -> Vec<DirContent> {
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
        let is_dir = path.is_dir();
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
/// 目录节点 = 外层 Column 容器 + 目录行 Button(row: 箭头 + 图标 + 名称) + 子容器 Node。
/// 文件节点 = Button(row: 图标 + 名称)。
/// 箭头/图标/名称分离为独立 Text 子节点，便于展开/折叠时单独切换，
/// 且支持选中/悬停态（由独立系统据 `FileSelectedMarker`/`Interaction` 设背景色）。
/// 子项缩进由子容器的左 padding 累积（每层 `space::LG` = 16px）。
fn spawn_entry(parent: &mut ChildSpawnerCommands, entry: &DirContent, theme: &Theme, font: f32) {
    let font_size = FontSize::Px(font);
    if entry.is_dir {
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
                        expanded: false,
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|row| {
                    // 箭头（▸ 折叠 / ▾ 展开）
                    row.spawn((
                        Node {
                            width: px(10.0),
                            ..default()
                        },
                        Text::new("▸"),
                        TextFont {
                            font_size,
                            ..default()
                        },
                        TextColor(theme.text_dim),
                        DirArrowMarker,
                    ));
                    // 图标（📁 折叠 / 📂 展开）
                    row.spawn((
                        Node {
                            width: px(14.0),
                            ..default()
                        },
                        Text::new("📁"),
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
                // 子项容器（折叠态空，展开时 spawn 子条目）
                col.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::left(px(space::LG)),
                        ..default()
                    },
                    DirChildrenMarker,
                ));
            });
    } else {
        parent
            .spawn((
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
            ))
            .with_children(|row| {
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
                    Text::new("📄"),
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
/// 根据项目根路径构建文件树（每次重建）。
fn rebuild_file_tree(
    root: Res<ProjectRoot>,
    q_tree: Query<Entity, With<FileTreeMarker>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    // 仅在项目根路径变化时重建
    if !root.is_changed() && !root.is_added() {
        return;
    }
    if root.path.as_os_str().is_empty() {
        return;
    }
    let Ok(tree) = q_tree.single() else {
        return;
    };
    // 清除旧条目
    commands.entity(tree).despawn_children();
    let font = theme.font_size;
    let entries = list_dir(&root.path);
    commands.entity(tree).with_children(|p| {
        for entry in &entries {
            spawn_entry(p, entry, &theme, font);
        }
    });
}

/// 处理文件条目点击：代码文件打开编辑器，其他文件在右侧分屏预览区显示。
///
/// 两种情况都展开右侧分屏（`SideViewCollapsed=false`）并设 `SideViewContent`：
/// - 代码文件 → `SideViewContent::Editor` + 发 `OpenFileRequest`；
/// - 非代码文件 → `SideViewContent::Preview` + 填充预览内容。
///
/// 显隐（`EditorViewMarker`/`FilePreviewMarker` 的 `display`）由
/// [`crate::editor::apply_editor_view_visibility`] 统一应用，本系统不直接写
/// `&mut Node` 以避免 B0001 query 冲突。
fn handle_file_click(
    q_files: Query<(Entity, &FileEntry, &Interaction), Changed<Interaction>>,
    q_preview: Query<Entity, With<FilePreviewMarker>>,
    mut q_path: Query<&mut Text, With<FilePreviewPathMarker>>,
    mut q_meta: Query<&mut Text, With<FilePreviewMetaMarker>>,
    q_body: Query<Entity, With<FilePreviewBodyMarker>>,
    q_selected: Query<Entity, With<FileSelectedMarker>>,
    mut side_collapsed: ResMut<crate::layout::SideViewCollapsed>,
    mut content: ResMut<crate::editor::SideViewContent>,
    theme: Res<Theme>,
    mut commands: Commands,
    mut open_writer: MessageWriter<crate::editor::tabs::OpenFileRequest>,
) {
    let Ok(preview) = q_preview.single() else {
        return;
    };
    let _ = preview;
    let font = theme.font_size;
    for (entity, file, interaction) in q_files.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // 选中态：清除旧选中，标记当前
        for old in q_selected.iter() {
            commands.entity(old).remove::<FileSelectedMarker>();
        }
        commands.entity(entity).insert(FileSelectedMarker);
        // 展开右侧分屏
        side_collapsed.0 = false;
        // 代码文件 → 编辑器视图（编辑器层接管显隐）
        if is_code_file(&file.path) {
            *content = crate::editor::SideViewContent::Editor;
            open_writer.write(crate::editor::tabs::OpenFileRequest {
                path: file.path.clone(),
                line: None,
            });
            continue;
        }
        // 非代码文件 → 预览视图 + 填充 fv-head 路径 + fv-body 内容
        *content = crate::editor::SideViewContent::Preview;
        let name = file
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        // 更新 fv-head 路径文本
        if let Ok(mut path_text) = q_path.single_mut() {
            path_text.0 = format!("📄 {}", name);
        }
        // 读取文件内容（字节 + 文本）
        let (bytes_len, text) = match std::fs::read(&file.path) {
            Ok(b) => {
                let len = b.len();
                let text = String::from_utf8_lossy(&b).to_string();
                (len, text)
            }
            Err(e) => (0, format!("读取失败: {e}")),
        };
        // 更新 fv-head 元信息：字节数 · 只读预览
        if let Ok(mut meta_text) = q_meta.single_mut() {
            meta_text.0 = format!("· {} 字节 · 只读预览", bytes_len);
        }
        // 填充 fv-body 内容（Rust 语法高亮，其余纯文本）
        if let Ok(body) = q_body.single() {
            let truncated: String = text.lines().take(1000).collect::<Vec<_>>().join("\n");
            commands.entity(body).despawn_children();
            commands.entity(body).with_children(|p| {
                let mono = FontSize::Px(font - 2.0);
                if let Some(lang) = preview_language(&file.path) {
                    // Rust：tree-sitter 高亮，按 span spawn Text 节点
                    let spans = xui::highlight(&truncated, lang);
                    for span in spans {
                        let end = span.end.min(truncated.len());
                        if end <= span.start {
                            continue;
                        }
                        let slice = &truncated[span.start..end];
                        let color = xui::span_color_for(span.kind);
                        p.spawn((
                            Node { ..default() },
                            Text::new(slice.to_string()),
                            TextFont { font_size: mono, ..default() },
                            TextColor(color),
                        ));
                    }
                } else {
                    // 非 Rust：纯文本
                    p.spawn((
                        Node { ..default() },
                        Text::new(truncated),
                        TextFont { font_size: mono, ..default() },
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
                | "ts"
                | "py"
                | "go"
                | "c"
                | "cpp"
                | "h"
                | "yml"
                | "yaml"
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
/// 展开/折叠时单独切换 `DirArrowMarker`（▸/▾）与 `DirIconMarker`（📁/📂）
/// 子节点文本，而非重写整行文本——因目录行现为 row 容器含分离的子节点。
fn handle_dir_click(
    mut commands: Commands,
    mut q_dirs: Query<(&mut DirEntry, &Interaction, &ChildOf), Changed<Interaction>>,
    q_children: Query<&Children>,
    q_dir_children: Query<Entity, With<DirChildrenMarker>>,
    mut q_text: ParamSet<(
        Query<&mut Text, With<DirArrowMarker>>,
        Query<&mut Text, With<DirIconMarker>>,
    )>,
    theme: Res<Theme>,
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
            } else {
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
            // 折叠
            dir.expanded = false;
            if let Some(e) = arrow_entity {
                if let Ok(mut t) = q_text.p0().get_mut(e) {
                    *t = Text::new("▸");
                }
            }
            if let Some(e) = icon_entity {
                if let Ok(mut t) = q_text.p1().get_mut(e) {
                    *t = Text::new("📁");
                }
            }
            commands.entity(child_container).despawn_children();
        } else {
            // 展开：读子目录内容，spawn 到子容器
            dir.expanded = true;
            if let Some(e) = arrow_entity {
                if let Ok(mut t) = q_text.p0().get_mut(e) {
                    *t = Text::new("▾");
                }
            }
            if let Some(e) = icon_entity {
                if let Ok(mut t) = q_text.p1().get_mut(e) {
                    *t = Text::new("📂");
                }
            }
            let entries = list_dir(&dir.path);
            commands.entity(child_container).with_children(|p| {
                for entry in &entries {
                    spawn_entry(p, entry, &theme, font);
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
    mut side_collapsed: ResMut<crate::layout::SideViewCollapsed>,
    mut content: ResMut<crate::editor::SideViewContent>,
) {
    for interaction in q_btn.iter() {
        if *interaction == Interaction::Pressed {
            side_collapsed.0 = true;
            *content = crate::editor::SideViewContent::None;
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
