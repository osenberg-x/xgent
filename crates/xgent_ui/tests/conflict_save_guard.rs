//! 冲突态保存保护测试：buffer 处于 `ConflictDetected` 时，`EditorSaveRequested`
//! 不应产生 `FileWriteRequest`，避免静默覆盖外部修改（设计 §3.6）。
//!
//! 用独立 `Schedule` 只跑 `handle_editor_save_requests`，绕过 `app.update()`
//! 帧尾的 `Messages::update` 清理，直接断言 message queue 长度。
use std::path::PathBuf;

use bevy::ecs::message::Messages;
use bevy::ecs::schedule::Schedule;
use bevy::prelude::*;
use xgent_ui::editor::buffer::{BufferState, EditorBuffer};
use xgent_ui::editor::handle_editor_save_requests;
use xgent_ui::editor::io::FileWriteRequest;
use xui::{EditorSaveRequested, TextEditor};

/// 构造最小 world：只注册被测 message 与必要 resource，不跑完整插件链。
fn make_world() -> World {
    let mut world = World::new();
    world.init_resource::<Messages<EditorSaveRequested>>();
    world.init_resource::<Messages<FileWriteRequest>>();
    world
}

/// spawn 一个指定状态的 buffer 实体，返回其 entity。
fn spawn_buffer(world: &mut World, state: BufferState) -> Entity {
    let mut buf = EditorBuffer::from_disk(PathBuf::from("/tmp/conflict.rs"), String::new());
    buf.state = state;
    world.spawn((buf, TextEditor::default())).id()
}

/// 注入保存请求。
fn request_save(world: &mut World, entity: Entity) {
    world
        .resource_mut::<Messages<EditorSaveRequested>>()
        .write(EditorSaveRequested { entity });
}

/// 只跑被测系统并返回产生的 FileWriteRequest 数量。
fn count_writes_after_save(world: &mut World) -> usize {
    let mut schedule = Schedule::default();
    schedule.add_systems(handle_editor_save_requests);
    schedule.run(world);
    world
        .resource::<Messages<FileWriteRequest>>()
        .len()
}

/// ConflictDetected 态按 Cmd+S 不应落盘。
#[test]
fn save_blocked_in_conflict_state() {
    let mut world = make_world();
    let entity = spawn_buffer(&mut world, BufferState::ConflictDetected);
    request_save(&mut world, entity);
    assert_eq!(
        count_writes_after_save(&mut world),
        0,
        "ConflictDetected 态不应触发文件写入"
    );
}

/// Dirty 态按 Cmd+S 应正常落盘（对照基线，确保守卫未误伤正常保存）。
#[test]
fn save_allowed_in_dirty_state() {
    let mut world = make_world();
    let entity = spawn_buffer(&mut world, BufferState::Dirty);
    request_save(&mut world, entity);
    assert_eq!(
        count_writes_after_save(&mut world),
        1,
        "Dirty 态应正常产生文件写入请求"
    );
}

/// LocalPreferred 态应允许保存（用户已明确选择保留本地，下次保存覆盖外部）。
#[test]
fn save_allowed_in_local_preferred_state() {
    let mut world = make_world();
    let entity = spawn_buffer(&mut world, BufferState::LocalPreferred);
    request_save(&mut world, entity);
    assert_eq!(
        count_writes_after_save(&mut world),
        1,
        "LocalPreferred 态应允许保存覆盖外部"
    );
}

/// Clean 态保存应产生写请求（无内容变化也允许落盘）。
#[test]
fn save_allowed_in_clean_state() {
    let mut world = make_world();
    let entity = spawn_buffer(&mut world, BufferState::Clean);
    request_save(&mut world, entity);
    assert_eq!(
        count_writes_after_save(&mut world),
        1,
        "Clean 态保存应正常产生写入请求"
    );
}
