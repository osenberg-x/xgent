//! 文件树刷新回归测试。
//!
//! 验证收到 `FileChangedEvent`（daemon 文件监听）后，文件树自动重建，
//! 新增文件出现在树中，且已展开目录状态在重建后恢复。

use std::fs;
use std::path::PathBuf;

use bevy::prelude::*;
use xgent_settings::Localizer;
use xgent_ui::editor::EditorPlugin;
use xgent_ui::editor::conflict::FileChangedEvent;
use xgent_ui::file_panel::{
    DirEntry, ExpandedDirs, FileEntry, FilePanelPlugin, FileTreeDirty, ProjectRoot,
};
use xgent_ui::layout::LayoutPlugin;
use xgent_ui::resize::ResizePlugin;
use xui::i18n_bridge::Strings;
use xui_i18n::StringSource;

/// 空 StringSource（测试不关心 i18n 文案）。
struct NoopStrings;
impl StringSource for NoopStrings {
    fn get(&self, key: &str, _args: &[(&str, String)]) -> String {
        key.to_string()
    }
    fn current_lang(&self) -> &str {
        "zh-CN"
    }
}

fn collect_file_entries(app: &mut App) -> Vec<PathBuf> {
    let mut q = app.world_mut().query::<&FileEntry>();
    q.iter(app.world()).map(|e| e.path.clone()).collect()
}

/// 收集文件树中所有 DirEntry 的路径。
fn collect_dir_entries(app: &mut App) -> Vec<PathBuf> {
    let mut q = app.world_mut().query::<&DirEntry>();
    q.iter(app.world()).map(|e| e.path.clone()).collect()
}

/// 文件树初始构建：项目根设置后，顶层文件应出现在树中。
#[test]
fn file_tree_shows_top_level_files() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("alpha.txt"), "hello").expect("写文件");
    fs::write(root.join("beta.rs"), "fn main() {}").expect("写文件");

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            xui::XuiPlugin,
            LayoutPlugin,
            ResizePlugin,
            EditorPlugin,
            FilePanelPlugin,
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>()
        .insert_resource(ProjectRoot { path: root.clone() });

    // 跑 Startup + 几帧 Update 让 rebuild_file_tree 执行
    for _ in 0..5 {
        app.update();
    }

    let files = collect_file_entries(&mut app);
    assert!(
        files.iter().any(|p| p == &root.join("alpha.txt")),
        "alpha.txt 应出现在文件树中，实际: {files:?}"
    );
    assert!(
        files.iter().any(|p| p == &root.join("beta.rs")),
        "beta.rs 应出现在文件树中，实际: {files:?}"
    );
}

/// 收到 FileChangedEvent 后文件树应重建，新增文件可见。
#[test]
fn file_tree_refreshes_on_file_changed_event() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("existing.txt"), "old").expect("写文件");

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            xui::XuiPlugin,
            LayoutPlugin,
            ResizePlugin,
            EditorPlugin,
            FilePanelPlugin,
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>()
        .insert_resource(ProjectRoot { path: root.clone() });

    for _ in 0..5 {
        app.update();
    }

    // 初始：只有 existing.txt
    let files = collect_file_entries(&mut app);
    assert_eq!(files.len(), 1, "初始应只有 1 个文件");

    // 在磁盘上新增文件
    fs::write(root.join("newcomer.txt"), "new").expect("写新文件");

    // 发 FileChangedEvent（模拟 daemon 通知）
    app.world_mut().write_message(FileChangedEvent {
        path: root.join("newcomer.txt"),
    });

    // 跑几帧让 mark_file_tree_dirty → rebuild_file_tree 执行
    // 跑足够帧让 debounce 倒计时 + rebuild_file_tree 执行
    for _ in 0..15 {
        app.update();
    }

    // 重建后应包含新文件
    let files = collect_file_entries(&mut app);
    assert!(
        files.iter().any(|p| p == &root.join("newcomer.txt")),
        "newcomer.txt 应在刷新后出现在文件树中，实际: {files:?}"
    );
    assert_eq!(files.len(), 2, "重建后应有 2 个文件");

    // dirty 标记应已清零
    let dirty = app.world().resource::<FileTreeDirty>();
    assert!(!dirty.0, "重建后 FileTreeDirty 应为 false");
}

/// 已展开目录在文件树重建后应保持展开状态。
#[test]
fn expanded_dirs_preserved_across_rebuild() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();
    // 创建子目录 + 文件
    fs::create_dir(root.join("subdir")).expect("创建子目录");
    fs::write(root.join("subdir").join("inner.txt"), "inner").expect("写文件");
    fs::write(root.join("top.txt"), "top").expect("写文件");

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            xui::XuiPlugin,
            LayoutPlugin,
            ResizePlugin,
            EditorPlugin,
            FilePanelPlugin,
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>()
        .insert_resource(ProjectRoot { path: root.clone() });

    for _ in 0..5 {
        app.update();
    }

    // 手动标记子目录为展开（模拟用户点击展开）
    {
        let mut expanded = app.world_mut().resource_mut::<ExpandedDirs>();
        expanded.0.insert(root.join("subdir"));
    }
    // 设 dirty 触发重建
    {
        let mut dirty = app.world_mut().resource_mut::<FileTreeDirty>();
        dirty.0 = true;
        dirty.1 = 0;
    }

    for _ in 0..5 {
        app.update();
    }

    // 重建后子目录应仍标记为展开
    let expanded = app.world().resource::<ExpandedDirs>();
    assert!(
        expanded.0.contains(&root.join("subdir")),
        "重建后 subdir 应保持展开状态"
    );

    // 子目录内的文件应可见（因为预展开了）
    let files = collect_file_entries(&mut app);
    assert!(
        files
            .iter()
            .any(|p| p == &root.join("subdir").join("inner.txt")),
        "展开目录的子文件应在重建后可见，实际: {files:?}"
    );

    // DirEntry 的 expanded 字段也应为 true
    let mut q = app.world_mut().query::<&DirEntry>();
    let subdir_entry = q.iter(app.world()).find(|e| e.path == root.join("subdir"));
    assert!(subdir_entry.is_some(), "subdir 的 DirEntry 应存在");
    assert!(
        subdir_entry.unwrap().expanded,
        "subdir 的 DirEntry.expanded 应为 true"
    );
}

/// 忽略目录（target/.git 等）不应出现在文件树中。
#[test]
fn ignored_dirs_excluded_from_tree() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();
    fs::create_dir(root.join("target")).expect("创建 target");
    fs::write(root.join("target").join("binary.o"), "binary").expect("写文件");
    fs::create_dir(root.join(".git")).expect("创建 .git");
    fs::write(root.join(".git").join("config"), "config").expect("写文件");
    fs::write(root.join("visible.txt"), "visible").expect("写文件");

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            xui::XuiPlugin,
            LayoutPlugin,
            ResizePlugin,
            EditorPlugin,
            FilePanelPlugin,
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>()
        .insert_resource(ProjectRoot { path: root.clone() });

    for _ in 0..5 {
        app.update();
    }

    let dirs = collect_dir_entries(&mut app);
    assert!(
        !dirs.iter().any(|p| p.ends_with("target")),
        "target 目录不应出现在文件树中"
    );
    assert!(
        !dirs.iter().any(|p| p.ends_with(".git")),
        ".git 目录不应出现在文件树中"
    );

    let files = collect_file_entries(&mut app);
    assert!(
        files.iter().any(|p| p == &root.join("visible.txt")),
        "visible.txt 应出现在文件树中"
    );
    // target/.git 内的文件不应出现
    assert!(
        !files
            .iter()
            .any(|p| p.ends_with("binary.o") || p.ends_with("config")),
        "忽略目录内的文件不应出现在文件树中"
    );
}

/// 文件树重建后选中态应恢复：选中的文件条目仍挂 `FileSelectedMarker`。
#[test]
fn selected_marker_preserved_across_rebuild() {
    use xgent_ui::file_panel::FileSelectedMarker;

    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("alpha.txt"), "hello").expect("写文件");
    fs::write(root.join("beta.rs"), "fn main() {}").expect("写文件");

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            xui::XuiPlugin,
            LayoutPlugin,
            ResizePlugin,
            EditorPlugin,
            FilePanelPlugin,
        ))
        .insert_resource(Strings(Box::new(NoopStrings)))
        .init_resource::<Localizer>()
        .insert_resource(ProjectRoot { path: root.clone() });

    for _ in 0..5 {
        app.update();
    }

    // 找到 alpha.txt 的 entity 并标记为选中
    {
        let mut q = app.world_mut().query::<(Entity, &FileEntry)>();
        let alpha_entity = q
            .iter(app.world())
            .find(|(_, e)| e.path == root.join("alpha.txt"))
            .map(|(e, _)| e)
            .expect("alpha.txt 应在树中");
        app.world_mut()
            .entity_mut(alpha_entity)
            .insert(FileSelectedMarker);
        app.world_mut()
            .insert_resource(xgent_ui::file_panel::SelectedFilePath(Some(
                root.join("alpha.txt")
            )));
    }

    // 触发重建（设 dirty）
    {
        let mut dirty = app.world_mut().resource_mut::<FileTreeDirty>();
        dirty.0 = true;
        dirty.1 = 0;
    }
    for _ in 0..5 {
        app.update();
    }

    // 重建后 alpha.txt 应仍挂 FileSelectedMarker
    let mut q = app
        .world_mut()
        .query::<(&FileEntry, Option<&FileSelectedMarker>)>();
    let alpha = q
        .iter(app.world())
        .find(|(e, _)| e.path == root.join("alpha.txt"))
        .expect("alpha.txt 应在重建后的树中");
    assert!(
        alpha.1.is_some(),
        "重建后 alpha.txt 应仍挂 FileSelectedMarker"
    );

    // 其他文件不应有 FileSelectedMarker
    let beta = q
        .iter(app.world())
        .find(|(e, _)| e.path == root.join("beta.rs"))
        .expect("beta.rs 应在重建后的树中");
    assert!(
        beta.1.is_none(),
        "重建后 beta.rs 不应挂 FileSelectedMarker"
    );
}
