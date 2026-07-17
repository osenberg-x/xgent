//! 文件面板：项目文件树预览 + 当前文件内容（MVP 只读）。
//!
//! 文件树从项目根遍历，按字母排序、目录优先。
//! 点击目录展开/折叠，点击文件读取内容在下方预览。
//! `.gitignore` 忽略路径：MVP 简单匹配 `target/`、`.git/`、`node_modules/`。

use std::path::PathBuf;

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
            .add_systems(Update, (handle_file_click, rebuild_file_tree));
    }
}

/// 启动时在文件面板内 spawn 文件树 + 预览区。
fn spawn_file_panel(
    mut commands: Commands,
    q: Query<Entity, With<FilePanelMarker>>,
    theme: Res<Theme>,
    root: Res<ProjectRoot>,
) {
    let Ok(entity) = q.single() else {
        return;
    };
    let font = theme.font_size;
    commands.entity(entity).with_children(|p| {
        // 文件树区（上，可滚动）
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
        // 文件预览区（下，高 40%，可滚动）
        p.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(40.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip_y(),
                border: UiRect::top(px(1.0)),
                padding: UiRect::all(px(space::SM)),
                ..default()
            },
            BackgroundColor(theme.bg),
            BorderColor::all(theme.border),
            ScrollPosition::default(),
            FilePreviewMarker,
        ));
    });
    let _ = font;
    let _ = root;
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

/// 判断路径是否被忽略（MVP 简单匹配）。
fn is_ignored(name: &str) -> bool {
    matches!(
        name,
        "target" | ".git" | "node_modules" | ".xgent" | "__pycache__" | ".next" | "dist" | "build"
    )
}

/// 目录或文件内容。
struct DirContent {
    name: String,
    path: PathBuf,
    is_dir: bool,
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

/// spawn 一个文件树条目（目录或文件）。
fn spawn_entry(parent: &mut ChildSpawnerCommands, entry: &DirContent, theme: &Theme, font: f32) {
    let icon = if entry.is_dir { "📁" } else { "📄" };
    if entry.is_dir {
        parent.spawn((
            Button,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::left(px(space::SM)),
                ..default()
            },
            Text::new(format!("{} {}", icon, entry.name)),
            TextFont {
                font_size: FontSize::Px(font),
                ..default()
            },
            TextColor(theme.text),
            DirEntry {
                path: entry.path.clone(),
                expanded: false,
            },
        ));
    } else {
        parent.spawn((
            Button,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::left(px(space::SM)),
                ..default()
            },
            Text::new(format!("{} {}", icon, entry.name)),
            TextFont {
                font_size: FontSize::Px(font),
                ..default()
            },
            TextColor(theme.text),
            FileEntry {
                path: entry.path.clone(),
            },
        ));
    }
}

/// 处理文件条目点击（读取内容预览）。
fn handle_file_click(
    q_files: Query<(&FileEntry, &Interaction), Changed<Interaction>>,
    q_preview: Query<Entity, With<FilePreviewMarker>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(preview) = q_preview.single() else {
        return;
    };
    let font = theme.font_size;
    for (file, interaction) in q_files.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // 读取文件内容（同步，MVP 简化）
        let content = match std::fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(e) => format!("读取失败: {e}"),
        };
        // 截断到前 1000 行
        let truncated: String = content.lines().take(1000).collect::<Vec<_>>().join("\n");
        // 清除旧预览内容并写入新内容
        commands.entity(preview).despawn_children();
        commands.entity(preview).with_children(|p| {
            p.spawn((
                Node { ..default() },
                Text::new(truncated),
                TextFont {
                    font_size: FontSize::Px(font - 2.0),
                    ..default()
                },
                TextColor(theme.text_dim),
            ));
        });
    }
}
