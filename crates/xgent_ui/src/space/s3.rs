//! F-20 S3：3D 时间轴数据、只读回放与 HUD scrubber。
//!
//! 数据口径（见 `docs/openspec/changes/f20-s3-timeline/design.md`）：
//! - agent 回合取当前会话用户消息时间戳；检查点清单的 `Created` 作为该轮新建文件。
//! - git 提交取 `log` + `ls-tree` 的树文件集，剥掉项目在仓库内的前缀。
//! - 只回放**当前 L1 图已有节点**：历史里存在但当前图没有的文件不重建星体。
//! - 相机不写；进入历史时保存选中并清空，回到现在恢复。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::InputFocus;
use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart, PointerPress};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{Pressed, UiGlobalTransform};
use bevy::ui_widgets::{Button, observe};
use crossbeam_channel::{Receiver, TryRecvError, unbounded};
use xgent_agent::Conversation;
use xgent_core::chat::AgentMessage;
use xgent_core::spatial::{SpatialNodeId, SpatialNodeKind};
use xgent_settings::Localizer;
use xgent_tools::TurnManifest;
use xgent_tools::checkpoint::EntryKind;
use xui::Ellipsis;

use crate::file_panel::ProjectRoot;
use crate::fonts::{ellipsis_text, ui_text};
use crate::i18n::{tr, tr_with};
use crate::modal::ModalStack;
use crate::panels::{PanelRegistry, SlotLayout, ids};
use crate::space::s2::SpaceSearchState;
use crate::space::view::{SpaceEdge, SpaceGraph, SpaceLoadState, SpacePickState, SpatialStar};
use crate::theme::{Theme, px, radius, space, type_scale};

const MAX_TURNS: usize = 20;
const MAX_COMMITS: usize = 50;

/// 时间轴锚点类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineAnchorKind {
    /// 当前会话的一轮。
    Turn {
        /// 会话号
        session: u64,
        /// 轮号（与检查点清单一致）
        turn: u64,
    },
    /// git 提交。
    Commit {
        /// 完整 hash
        hash: String,
    },
}

/// 一个时间轴锚点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineAnchor {
    /// 时间戳（ms）
    pub at_ms: i64,
    /// 锚点类型
    pub kind: TimelineAnchorKind,
    /// 提交标题；回合锚点留空，由 UI 按 i18n 生成
    pub label: String,
}

/// 时间轴数据。
#[derive(Debug, Clone, Default)]
pub struct TimelineData {
    /// 按时间排序的锚点
    pub anchors: Vec<TimelineAnchor>,
    /// commit hash → 树内文件（相对项目根，`/` 分隔）
    pub commit_trees: BTreeMap<String, BTreeSet<String>>,
    /// turn → 该轮新建文件
    pub turn_created: BTreeMap<u64, BTreeSet<String>>,
    /// 当前图文件集
    pub current_files: BTreeSet<String>,
    /// 当前图目录集
    pub current_dirs: BTreeSet<String>,
    /// 是否成功读取 git
    pub has_git: bool,
    /// 降级/数据问题
    pub warnings: Vec<String>,
}

/// 回放可见集（`files`/`dirs` 已与当前图取交集）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibilityPlan {
    /// 可见文件
    pub files: BTreeSet<String>,
    /// 可见目录
    pub dirs: BTreeSet<String>,
    /// 历史中存在但当前图缺失、因此无法回放的文件数
    pub missing: usize,
}

/// 时间轴状态：`position == None` 表示“现在”。
#[derive(Resource, Default)]
pub struct TimelineState {
    /// 已加载数据
    pub data: Option<TimelineData>,
    /// `None` = 现在；`Some(i)` = 第 i 个锚点
    pub position: Option<usize>,
    /// 是否正在后台加载
    pub loading: bool,
    /// 线程创建等硬失败
    pub error: Option<String>,
    /// 数据代数（UI 重建标记）
    revision: u64,
    saved_selected: Option<SpatialNodeId>,
}

impl TimelineState {
    /// 锚点数量。
    pub fn anchor_count(&self) -> usize {
        self.data.as_ref().map_or(0, |data| data.anchors.len())
    }

    /// 是否有可回放锚点。
    pub fn has_anchors(&self) -> bool {
        self.anchor_count() > 0
    }

    /// 当前历史锚点。
    pub fn current(&self) -> Option<&TimelineAnchor> {
        let data = self.data.as_ref()?;
        data.anchors.get(self.position?)
    }

    /// 切到指定槽位（`slot == anchor_count` 表示现在）。
    pub fn go_to_slot(&mut self, slot: usize, pick: &mut SpacePickState) {
        let count = self.anchor_count();
        if count == 0 {
            return;
        }
        if slot >= count {
            self.return_now(pick);
            return;
        }
        if self.position == Some(slot) {
            return;
        }
        if self.position.is_none() {
            self.saved_selected = pick.selected.take();
        }
        self.position = Some(slot);
    }

    /// 相对当前位置切换：负数往更早，正数往现在。
    pub fn step(&mut self, delta: i32, pick: &mut SpacePickState) {
        let count = self.anchor_count();
        if count == 0 {
            return;
        }
        let slot = self.position.unwrap_or(count);
        let next = if delta < 0 {
            slot.saturating_sub(1)
        } else {
            (slot + 1).min(count)
        };
        self.go_to_slot(next, pick);
    }

    /// 回到现在：恢复进入历史前的选中。
    pub fn return_now(&mut self, pick: &mut SpacePickState) {
        if self.position.take().is_some() {
            pick.selected = self.saved_selected.take();
        }
    }

    /// 应用新数据；历史位置按锚点身份尽量保留，找不到则回 now。
    pub fn apply_data(&mut self, data: TimelineData, pick: &mut SpacePickState) {
        let previous = self
            .data
            .as_ref()
            .zip(self.position)
            .and_then(|(data, index)| data.anchors.get(index))
            .map(|anchor| anchor.kind.clone());
        self.data = Some(data);
        self.revision = self.revision.wrapping_add(1);
        self.loading = false;
        self.error = None;
        let Some(previous) = previous else {
            self.position = None;
            return;
        };
        let new_index = self
            .data
            .as_ref()
            .and_then(|data| data.anchors.iter().position(|a| a.kind == previous));
        match new_index {
            Some(index) => self.position = Some(index),
            None => self.return_now(pick),
        }
    }
}

/// 后台加载任务。
#[derive(Resource, Default)]
pub struct TimelineLoader {
    rx: Option<Receiver<TimelineData>>,
    current: Option<TimelineSignature>,
    applied: Option<TimelineSignature>,
    pending: Option<TimelineJobInput>,
}

/// 加载输入签名；任一变化触发重载。
#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineSignature {
    root: PathBuf,
    session: u64,
    turns: usize,
    scan_revision: u64,
}

/// 一次加载所需的完整输入快照。
struct TimelineJobInput {
    signature: TimelineSignature,
    turn_times: Vec<i64>,
    current_files: BTreeSet<String>,
    current_dirs: BTreeSet<String>,
}

/// rail 拖动起点（比例 0..=1）。
#[derive(Resource, Default)]
pub struct TimelineDrag {
    start_fraction: Option<f32>,
}

/// HUD 时间轴根。
#[derive(Component)]
pub struct SpaceTimelineRootMarker;
/// 当前状态/锚点标签。
#[derive(Component)]
pub struct SpaceTimelineLabelText;
/// rail 本体（点击/拖动目标）。
#[derive(Component)]
pub struct SpaceTimelineRailMarker;
/// 标记点容器。
#[derive(Component)]
pub struct SpaceTimelineTrackMarker;
/// 单个锚点/现在端点标记。
#[derive(Component, Debug, Clone, Copy)]
pub struct SpaceTimelineMarker {
    /// 0..=anchor_count（`anchor_count` 为“现在”）。
    pub slot: usize,
}
/// 回到现在按钮。
#[derive(Component)]
pub struct SpaceTimelineReturnMarker;
/// 回到现在按钮文本。
#[derive(Component)]
pub struct SpaceTimelineReturnText;
/// 历史视图边框。
#[derive(Component)]
pub struct SpaceTimelineFrameMarker;

/// 从当前 L1 图收集文件/目录路径。
pub fn current_paths(graph: &SpaceGraph) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut files = BTreeSet::new();
    let mut dirs = BTreeSet::new();
    for node in &graph.0.nodes {
        let Some(path) = &node.path else {
            continue;
        };
        let path = normalize(&path.display().to_string());
        match node.kind {
            SpatialNodeKind::File => {
                files.insert(path);
            }
            SpatialNodeKind::Module => {
                dirs.insert(path);
            }
            _ => {}
        }
    }
    (files, dirs)
}

/// 合并锚点：时间升序；同一时刻 commit 在 turn 前；只保留最近上限。
pub fn merge_anchors(
    mut turns: Vec<TimelineAnchor>,
    mut commits: Vec<TimelineAnchor>,
) -> Vec<TimelineAnchor> {
    turns.sort_by_key(|anchor| (anchor.at_ms, 1u8));
    commits.sort_by_key(|anchor| (anchor.at_ms, 0u8));
    if turns.len() > MAX_TURNS {
        turns.drain(..turns.len() - MAX_TURNS);
    }
    if commits.len() > MAX_COMMITS {
        commits.drain(..commits.len() - MAX_COMMITS);
    }
    let mut all = turns;
    all.extend(commits);
    all.sort_by(|a, b| {
        a.at_ms
            .cmp(&b.at_ms)
            .then_with(|| kind_rank(&a.kind).cmp(&kind_rank(&b.kind)))
    });
    all
}

fn kind_rank(kind: &TimelineAnchorKind) -> u8 {
    match kind {
        TimelineAnchorKind::Commit { .. } => 0,
        TimelineAnchorKind::Turn { .. } => 1,
    }
}

/// 加载时间轴（同步；调用方负责放后台线程）。
pub fn load_timeline(
    project_root: &Path,
    session: u64,
    turn_times_ms: &[i64],
    current_files: BTreeSet<String>,
    current_dirs: BTreeSet<String>,
) -> TimelineData {
    let mut data = TimelineData {
        current_files,
        current_dirs,
        ..Default::default()
    };
    let mut turn_anchors = Vec::new();
    let start = turn_times_ms.len().saturating_sub(MAX_TURNS);
    for (index, at_ms) in turn_times_ms.iter().enumerate().skip(start) {
        let turn = (index + 1) as u64;
        let created = read_checkpoint_created(project_root, session, turn, &mut data.warnings);
        data.turn_created.insert(turn, created);
        turn_anchors.push(TimelineAnchor {
            at_ms: *at_ms,
            kind: TimelineAnchorKind::Turn { session, turn },
            label: String::new(),
        });
    }

    let mut commit_anchors = Vec::new();
    match git_prefix(project_root) {
        Ok(prefix) => match git_log(project_root) {
            Ok(commits) => {
                data.has_git = true;
                for (hash, seconds, subject) in commits.into_iter().take(MAX_COMMITS) {
                    match git_tree(project_root, &hash, &prefix) {
                        Ok(tree) => {
                            data.commit_trees.insert(hash.clone(), tree);
                            commit_anchors.push(TimelineAnchor {
                                at_ms: seconds.saturating_mul(1000),
                                kind: TimelineAnchorKind::Commit { hash },
                                label: subject,
                            });
                        }
                        Err(error) => data.warnings.push(format!("git ls-tree {hash}: {error}")),
                    }
                }
            }
            Err(error) => data.warnings.push(format!("git log: {error}")),
        },
        Err(error) => data.warnings.push(format!("git 不可用: {error}")),
    }

    data.anchors = merge_anchors(turn_anchors, commit_anchors);
    data
}

/// 锚点的历史文件集（未与当前图取交集）。
pub fn visible_files(anchor: &TimelineAnchor, data: &TimelineData) -> BTreeSet<String> {
    match &anchor.kind {
        TimelineAnchorKind::Commit { hash } => data
            .commit_trees
            .get(hash)
            .cloned()
            .unwrap_or_else(|| data.current_files.clone()),
        TimelineAnchorKind::Turn { turn, .. } => {
            let nearest = data
                .anchors
                .iter()
                .filter_map(|candidate| match &candidate.kind {
                    TimelineAnchorKind::Commit { hash } if candidate.at_ms <= anchor.at_ms => data
                        .commit_trees
                        .get(hash)
                        .map(|tree| (candidate.at_ms, tree)),
                    _ => None,
                })
                .max_by_key(|(at_ms, _)| *at_ms)
                .map(|(_, tree)| tree.clone());
            match nearest {
                Some(mut files) => {
                    for (turn_index, created) in &data.turn_created {
                        if *turn_index <= *turn {
                            files.extend(created.iter().cloned());
                        }
                    }
                    files
                }
                None => {
                    let mut files = data.current_files.clone();
                    for (turn_index, created) in &data.turn_created {
                        if *turn_index > *turn {
                            for path in created {
                                files.remove(path);
                            }
                        }
                    }
                    files
                }
            }
        }
    }
}

/// 由可见文件推导可见目录（当前图目录中至少有一个可见后代）。
pub fn visible_dirs(data: &TimelineData, files: &BTreeSet<String>) -> BTreeSet<String> {
    data.current_dirs
        .iter()
        .filter(|dir| {
            let prefix = format!("{dir}/");
            files.iter().any(|file| file.starts_with(&prefix))
        })
        .cloned()
        .collect()
}

/// 历史锚点的回放计划；`None` 表示“现在”，调用方应显示全部节点。
pub fn plan_visibility(data: &TimelineData, position: Option<usize>) -> Option<VisibilityPlan> {
    let anchor = data.anchors.get(position?)?;
    let history = visible_files(anchor, data);
    let files: BTreeSet<String> = history.intersection(&data.current_files).cloned().collect();
    let missing = history.len().saturating_sub(files.len());
    let dirs = visible_dirs(data, &files);
    Some(VisibilityPlan {
        files,
        dirs,
        missing,
    })
}

/// 时间轴比例（0..=1）映射到槽位；空锚点返回 `None`。
pub fn slot_from_fraction(fraction: f32, anchor_count: usize) -> Option<usize> {
    if anchor_count == 0 {
        return None;
    }
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some((fraction * anchor_count as f32).round() as usize)
}

fn read_checkpoint_created(
    project_root: &Path,
    session: u64,
    turn: u64,
    warnings: &mut Vec<String>,
) -> BTreeSet<String> {
    let path = checkpoint_manifest_path(project_root, session, turn);
    let Ok(bytes) = std::fs::read(&path) else {
        return BTreeSet::new();
    };
    match serde_json::from_slice::<TurnManifest>(&bytes) {
        Ok(manifest) => manifest
            .entries
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::Created)
            .map(|entry| normalize(&entry.path))
            .collect(),
        Err(error) => {
            warnings.push(format!("检查点 {}: {error}", path.display()));
            BTreeSet::new()
        }
    }
}

fn checkpoint_manifest_path(project_root: &Path, session: u64, turn: u64) -> PathBuf {
    project_root
        .join(".xgent")
        .join("checkpoints")
        .join("turns")
        .join(session.to_string())
        .join(format!("{turn}.json"))
}

fn git_prefix(project_root: &Path) -> Result<String, String> {
    let output = run_git(project_root, &["rev-parse", "--show-prefix"])?;
    Ok(normalize(output.trim()))
}

fn git_log(project_root: &Path) -> Result<Vec<(String, i64, String)>, String> {
    let output = run_git(
        project_root,
        &["log", "-n", "50", "--format=%H%x09%ct%x09%s"],
    )?;
    let mut commits = Vec::new();
    for line in output.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(hash), Some(seconds), Some(subject)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Ok(seconds) = seconds.parse::<i64>() else {
            continue;
        };
        commits.push((hash.to_string(), seconds, subject.to_string()));
    }
    Ok(commits)
}

fn git_tree(project_root: &Path, hash: &str, prefix: &str) -> Result<BTreeSet<String>, String> {
    let output = run_git(
        project_root,
        &["ls-tree", "-r", "--name-only", "--full-tree", hash],
    )?;
    let mut files = BTreeSet::new();
    for line in output.lines() {
        let path = normalize(line);
        let path = path.strip_prefix(prefix).unwrap_or(&path);
        if !path.is_empty() {
            files.insert(path.to_string());
        }
    }
    Ok(files)
}

fn run_git(project_root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/")
}

/// 注册 S3 资源与系统（由 `SpacePlugin` 调用）。
pub fn register(app: &mut App) {
    app.init_resource::<TimelineState>()
        .init_resource::<TimelineLoader>()
        .init_resource::<TimelineDrag>()
        .add_systems(
            Update,
            (
                poll_timeline_job,
                drive_timeline_loader,
                handle_timeline_keys,
                handle_timeline_return_press,
                apply_timeline_visibility,
                sync_timeline_ui,
            )
                .chain(),
        );
}

/// 在 HUD 内创建时间轴 rail 与历史边框（初始隐藏/空态）。
pub fn spawn_timeline_nodes(hud: &mut ChildSpawnerCommands, theme: &Theme, loc: &Localizer) {
    hud.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(12.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        SpaceTimelineRootMarker,
    ))
    .with_children(|wrapper| {
        wrapper
            .spawn((
                Node {
                    width: Val::Px(560.0),
                    max_width: Val::Percent(90.0),
                    height: Val::Px(40.0),
                    padding: UiRect::horizontal(px(space::SM)),
                    column_gap: px(space::SM),
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(radius::CTRL)),
                    ..default()
                },
                BackgroundColor(theme.elevated),
                BorderColor::all(theme.border),
            ))
            .with_children(|root| {
                root.spawn((
                    Node {
                        width: Val::Px(210.0),
                        flex_shrink: 0.0,
                        min_width: Val::ZERO,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ellipsis_text(
                        "",
                        type_scale::MICRO,
                        400,
                        theme.text_muted,
                        type_scale::line_height::UI,
                    ),
                    SpaceTimelineLabelText,
                ));

                let drag_start = observe(
                    |event: On<PointerDragStart>,
                     rail: Query<
                        (&ComputedNode, &UiGlobalTransform),
                        With<SpaceTimelineRailMarker>,
                    >,
                     mut drag: ResMut<TimelineDrag>| {
                        let Ok((computed, transform)) = rail.get(event.entity) else {
                            return;
                        };
                        drag.start_fraction =
                            fraction_at_point(event.pointer.position, computed, transform);
                    },
                );
                let dragging = observe(
                    |event: On<PointerDrag>,
                     rail: Query<(&ComputedNode,), With<SpaceTimelineRailMarker>>,
                     drag: ResMut<TimelineDrag>,
                     mut timeline: ResMut<TimelineState>,
                     mut pick: ResMut<SpacePickState>| {
                        let Some(start) = drag.start_fraction else {
                            return;
                        };
                        let Ok((computed,)) = rail.get(event.entity) else {
                            return;
                        };
                        let width = computed.size().x;
                        if width <= 0.0 {
                            return;
                        }
                        let fraction = (start + event.distance.x / width).clamp(0.0, 1.0);
                        let count = timeline.anchor_count();
                        if let Some(slot) = slot_from_fraction(fraction, count) {
                            timeline.go_to_slot(slot, &mut pick);
                        }
                    },
                );
                let drag_end = observe(
                    |_event: On<PointerDragEnd>, mut drag: ResMut<TimelineDrag>| {
                        drag.start_fraction = None;
                    },
                );
                let pressing = observe(
                    |event: On<PointerPress>,
                     rail: Query<
                        (&ComputedNode, &UiGlobalTransform),
                        With<SpaceTimelineRailMarker>,
                    >,
                     mut timeline: ResMut<TimelineState>,
                     mut pick: ResMut<SpacePickState>| {
                        if event.button != PointerButton::Primary {
                            return;
                        }
                        let Ok((computed, transform)) = rail.get(event.entity) else {
                            return;
                        };
                        let Some(fraction) =
                            fraction_at_point(event.pointer.position, computed, transform)
                        else {
                            return;
                        };
                        let count = timeline.anchor_count();
                        if let Some(slot) = slot_from_fraction(fraction, count) {
                            timeline.go_to_slot(slot, &mut pick);
                        }
                    },
                );

                root.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        flex_grow: 1.0,
                        height: Val::Px(20.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    SpaceTimelineRailMarker,
                    drag_start,
                    dragging,
                    drag_end,
                    pressing,
                ))
                .with_children(|rail| {
                    rail.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(7.0),
                            right: Val::Px(7.0),
                            top: Val::Percent(50.0),
                            height: Val::Px(1.0),
                            ..default()
                        },
                        BackgroundColor(theme.border),
                    ));
                    rail.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            right: Val::Px(0.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceBetween,
                            ..default()
                        },
                        SpaceTimelineTrackMarker,
                    ));
                });

                root.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        padding: UiRect::axes(px(space::SM), px(space::XS)),
                        border_radius: BorderRadius::all(px(radius::SMALL)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(theme.surface),
                    SpaceTimelineReturnMarker,
                    Visibility::Hidden,
                ))
                .with_children(|button| {
                    button.spawn((
                        ui_text(
                            tr(loc, "space-timeline-return-now"),
                            type_scale::MICRO,
                            400,
                            theme.text_dim,
                            type_scale::line_height::UI,
                        ),
                        SpaceTimelineReturnText,
                    ));
                });
            });
    });

    hud.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(8.0),
            top: Val::Px(8.0),
            right: Val::Px(8.0),
            bottom: Val::Px(8.0),
            border: UiRect::all(px(2.0)),
            ..default()
        },
        BorderColor::all(theme.warm),
        SpaceTimelineFrameMarker,
        Visibility::Hidden,
    ));
}

fn fraction_at_point(
    point: Vec2,
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
) -> Option<f32> {
    let size = computed.size();
    if size.x <= 0.0 {
        return None;
    }
    let center = transform.affine().translation;
    let left = center.x - size.x * 0.5;
    Some(((point.x - left) / size.x).clamp(0.0, 1.0))
}

/// 轮询后台任务（系统用）。
fn poll_timeline_job(
    mut loader: ResMut<TimelineLoader>,
    mut timeline: ResMut<TimelineState>,
    mut pick: ResMut<SpacePickState>,
) {
    poll_loader(&mut loader, &mut timeline, &mut pick);
}

/// 后台任务轮询（可单测）。
fn poll_loader(
    loader: &mut TimelineLoader,
    timeline: &mut TimelineState,
    pick: &mut SpacePickState,
) {
    let Some(rx) = loader.rx.as_ref() else {
        return;
    };
    match rx.try_recv() {
        Ok(data) => {
            loader.rx = None;
            loader.applied = loader.current.take();
            timeline.apply_data(data, pick);
            if let Some(next) = loader.pending.take() {
                start_job(loader, timeline, next);
            } else {
                timeline.loading = false;
            }
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            loader.rx = None;
            loader.current = None;
            timeline.loading = false;
            timeline.error = Some("时间轴加载线程异常退出".to_string());
        }
    }
}

/// 根据输入签名启动/排队后台加载。
fn drive_timeline_loader(
    mut loader: ResMut<TimelineLoader>,
    mut timeline: ResMut<TimelineState>,
    root: Option<Res<ProjectRoot>>,
    conv: Option<Res<Conversation>>,
    graph: Res<SpaceGraph>,
    load_state: Res<SpaceLoadState>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
) {
    if !space_visible(registry.as_deref(), layout.as_deref()) || !load_state.is_loaded() {
        return;
    }
    let (Some(root), Some(conv)) = (root, conv) else {
        return;
    };
    let turn_times: Vec<i64> = conv
        .messages
        .iter()
        .filter_map(|message| match message {
            AgentMessage::User(user) => Some(user.timestamp as i64),
            _ => None,
        })
        .collect();
    let signature = TimelineSignature {
        root: root.path.clone(),
        session: conv.id.0,
        turns: turn_times.len(),
        scan_revision: load_state.revision(),
    };
    if loader.current.as_ref() == Some(&signature)
        || (loader.rx.is_none() && loader.applied.as_ref() == Some(&signature))
    {
        return;
    }
    let (current_files, current_dirs) = current_paths(&graph);
    let input = TimelineJobInput {
        signature,
        turn_times,
        current_files,
        current_dirs,
    };
    if loader.rx.is_some() {
        let same = loader
            .pending
            .as_ref()
            .is_some_and(|pending| pending.signature == input.signature);
        if !same {
            loader.pending = Some(input);
        }
        return;
    }
    start_job(&mut loader, &mut timeline, input);
}

fn start_job(loader: &mut TimelineLoader, timeline: &mut TimelineState, input: TimelineJobInput) {
    let TimelineJobInput {
        signature,
        turn_times,
        current_files,
        current_dirs,
    } = input;
    let root = signature.root.clone();
    let session = signature.session;
    let job_signature = signature.clone();
    let (tx, rx) = unbounded();
    let spawned = std::thread::Builder::new()
        .name(format!("xgent-timeline-{session}"))
        .spawn(move || {
            let data = load_timeline(&root, session, &turn_times, current_files, current_dirs);
            let _ = tx.send(data);
        });
    match spawned {
        Ok(_handle) => {
            loader.rx = Some(rx);
            loader.current = Some(job_signature);
            timeline.loading = true;
            timeline.error = None;
        }
        Err(error) => {
            loader.applied = Some(job_signature);
            timeline.loading = false;
            timeline.error = Some(format!("无法启动时间轴加载线程: {error}"));
        }
    }
}

/// 左/右切换历史位置（相机 yaw 在时间轴可用时让位）。
fn handle_timeline_keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    registry: Option<Res<PanelRegistry>>,
    layout: Option<Res<SlotLayout>>,
    modal: Option<Res<ModalStack>>,
    input_focus: Option<Res<InputFocus>>,
    text: Query<(), With<EditableText>>,
    search: Option<Res<SpaceSearchState>>,
    mut timeline: ResMut<TimelineState>,
    mut pick: ResMut<SpacePickState>,
) {
    if !space_visible(registry.as_deref(), layout.as_deref()) {
        return;
    }
    if search.as_deref().is_some_and(|search| search.open) {
        return;
    }
    if modal.as_deref().is_some_and(|stack| !stack.is_empty()) {
        return;
    }
    let typing = input_focus
        .as_deref()
        .and_then(InputFocus::get)
        .is_some_and(|entity| text.get(entity).is_ok());
    if typing {
        return;
    }
    let Some(keys) = keys.as_ref() else {
        return;
    };
    let delta = if keys.just_pressed(KeyCode::ArrowLeft) {
        -1
    } else if keys.just_pressed(KeyCode::ArrowRight) {
        1
    } else {
        return;
    };
    timeline.step(delta, &mut pick);
}

/// 点击“回到现在”。
fn handle_timeline_return_press(
    q: Query<(), (With<SpaceTimelineReturnMarker>, Added<Pressed>)>,
    mut timeline: ResMut<TimelineState>,
    mut pick: ResMut<SpacePickState>,
) {
    if q.iter().next().is_some() {
        timeline.return_now(&mut pick);
    }
}

/// 按历史锚点回放当前图节点/目录/边的可见性；now 全显示。
fn apply_timeline_visibility(
    timeline: Res<TimelineState>,
    graph: Res<SpaceGraph>,
    added: Query<(), Added<SpatialStar>>,
    mut stars: Query<(&SpatialStar, &mut Visibility), Without<SpaceEdge>>,
    mut edges: Query<(&SpaceEdge, &mut Visibility), Without<SpatialStar>>,
) {
    if !timeline.is_changed() && !graph.is_changed() && added.is_empty() {
        return;
    }
    let plan = timeline
        .data
        .as_ref()
        .and_then(|data| plan_visibility(data, timeline.position));
    let Some(plan) = plan else {
        for (_, mut visibility) in &mut stars {
            if *visibility != Visibility::Visible {
                *visibility = Visibility::Visible;
            }
        }
        for (_, mut visibility) in &mut edges {
            if *visibility != Visibility::Visible {
                *visibility = Visibility::Visible;
            }
        }
        return;
    };
    if plan.missing > 0 {
        tracing::debug!(
            missing = plan.missing,
            "时间轴回放只覆盖当前图已有节点，历史文件不会重建星体"
        );
    }
    let mut visible_ids = BTreeSet::new();
    for (star, mut visibility) in &mut stars {
        let path = star.path.as_deref().map(normalize);
        let visible = match star.kind {
            SpatialNodeKind::File => path
                .as_deref()
                .is_some_and(|path| plan.files.contains(path)),
            SpatialNodeKind::Module => path.as_deref().is_some_and(|path| plan.dirs.contains(path)),
            _ => true,
        };
        let wanted = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if visible {
            visible_ids.insert(star.id);
        }
    }
    for (edge, mut visibility) in &mut edges {
        let visible = visible_ids.contains(&edge.from) && visible_ids.contains(&edge.to);
        let wanted = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

fn space_visible(registry: Option<&PanelRegistry>, layout: Option<&SlotLayout>) -> bool {
    match (registry, layout) {
        (Some(registry), Some(layout)) => layout.is_panel_visible(registry, ids::SPACE),
        _ => false,
    }
}

fn anchor_label(loc: &Localizer, anchor: &TimelineAnchor) -> String {
    match &anchor.kind {
        TimelineAnchorKind::Turn { turn, .. } => {
            tr_with(loc, "space-timeline-turn", &[("turn", turn.to_string())])
        }
        TimelineAnchorKind::Commit { hash } => {
            let short: String = hash.chars().take(7).collect();
            let subject = anchor.label.trim();
            if subject.is_empty() {
                tr_with(loc, "space-timeline-commit-short", &[("hash", short)])
            } else {
                tr_with(
                    loc,
                    "space-timeline-commit",
                    &[("hash", short), ("subject", subject.to_string())],
                )
            }
        }
    }
}

/// 同步时间轴标签、标记、回到现在与历史边框。
fn sync_timeline_ui(
    mut commands: Commands,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    timeline: Res<TimelineState>,
    markers: Query<Entity, With<SpaceTimelineMarker>>,
    tracks: Query<Entity, With<SpaceTimelineTrackMarker>>,
    mut texts: Query<(
        &mut Text,
        Option<&mut Ellipsis>,
        Has<SpaceTimelineLabelText>,
        Has<SpaceTimelineReturnText>,
    )>,
    mut targets: Query<(
        &mut Visibility,
        Has<SpaceTimelineFrameMarker>,
        Has<SpaceTimelineReturnMarker>,
        Has<SpaceTimelineRailMarker>,
    )>,
    mut last: Local<Option<(u64, Option<usize>)>>,
) {
    if !timeline.is_changed() && !loc.is_changed() && !theme.is_changed() {
        return;
    }
    let count = timeline.anchor_count();
    let label = if let Some(error) = &timeline.error {
        format!("{} · {error}", tr(&loc, "space-timeline-error"))
    } else if let Some(data) = &timeline.data {
        if data.anchors.is_empty() {
            tr(&loc, "space-timeline-empty")
        } else {
            let base = match timeline.position.and_then(|index| data.anchors.get(index)) {
                Some(anchor) => anchor_label(&loc, anchor),
                None => tr(&loc, "space-timeline-now"),
            };
            if data.warnings.is_empty() {
                base
            } else {
                format!(
                    "{base} · {}",
                    tr_with(
                        &loc,
                        "space-timeline-degraded",
                        &[("count", data.warnings.len().to_string())]
                    )
                )
            }
        }
    } else {
        tr(&loc, "space-timeline-loading")
    };

    for (mut text, ellipsis, is_label, is_return) in &mut texts {
        if is_label {
            if text.0 != label {
                text.0 = label.clone();
            }
            if let Some(mut ellipsis) = ellipsis
                && ellipsis.source != label
            {
                ellipsis.source = label.clone();
            }
        } else if is_return {
            let value = tr(&loc, "space-timeline-return-now");
            if text.0 != value {
                text.0 = value;
            }
        }
    }

    let history = timeline.position.is_some() && count > 0;
    for (mut visibility, is_frame, is_return, is_rail) in &mut targets {
        let wanted = if is_frame || is_return {
            if history {
                Visibility::Visible
            } else {
                Visibility::Hidden
            }
        } else if is_rail {
            if count > 0 {
                Visibility::Visible
            } else {
                Visibility::Hidden
            }
        } else {
            continue;
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }

    let key = (timeline.revision, timeline.position);
    if *last == Some(key) && !theme.is_changed() {
        return;
    }
    *last = Some(key);
    for entity in &markers {
        commands.entity(entity).despawn();
    }
    if count == 0 {
        return;
    }
    let Ok(track) = tracks.single() else {
        return;
    };
    let selected = timeline.position.unwrap_or(count);
    commands.entity(track).with_children(|track| {
        for slot in 0..=count {
            let color = if slot == count {
                theme.st_ok
            } else if slot == selected {
                theme.warm
            } else {
                theme.accent
            };
            let size = if slot == selected { 12.0 } else { 8.0 };
            track
                .spawn((
                    Node {
                        width: Val::Px(14.0),
                        height: Val::Px(14.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    SpaceTimelineMarker { slot },
                ))
                .with_children(|marker| {
                    marker.spawn((
                        Node {
                            width: Val::Px(size),
                            height: Val::Px(size),
                            border_radius: BorderRadius::all(px(size * 0.5)),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use xgent_core::spatial::{ClusterId, SpatialGraph, SpatialNode, SpatialStatus};
    use xgent_tools::CheckpointEntry;
    use xui_i18n::StringSource;

    fn turn_anchor(at_ms: i64, turn: u64) -> TimelineAnchor {
        TimelineAnchor {
            at_ms,
            kind: TimelineAnchorKind::Turn { session: 1, turn },
            label: String::new(),
        }
    }

    fn commit_anchor(at_ms: i64, hash: &str) -> TimelineAnchor {
        TimelineAnchor {
            at_ms,
            kind: TimelineAnchorKind::Commit {
                hash: hash.to_string(),
            },
            label: "commit".into(),
        }
    }

    #[test]
    fn merge_orders_by_time_and_commit_first() {
        let turns = vec![turn_anchor(200, 2), turn_anchor(100, 1)];
        let commits = vec![commit_anchor(100, "a"), commit_anchor(300, "b")];
        let merged = merge_anchors(turns, commits);
        assert_eq!(merged.len(), 4);
        assert!(matches!(merged[0].kind, TimelineAnchorKind::Commit { .. }));
        assert_eq!(merged[1].at_ms, 100);
        assert_eq!(merged[3].at_ms, 300);
    }

    #[test]
    fn merge_handles_empty() {
        assert!(merge_anchors(Vec::new(), Vec::new()).is_empty());
    }

    #[test]
    fn merge_keeps_most_recent_turns() {
        let turns: Vec<TimelineAnchor> = (0..25)
            .map(|index| turn_anchor(index, index as u64))
            .collect();
        let merged = merge_anchors(turns, Vec::new());
        assert_eq!(merged.len(), MAX_TURNS);
        assert_eq!(merged[0].at_ms, 5, "应丢弃最旧的 5 个回合");
    }

    #[test]
    fn checkpoint_created_and_visible_sets() {
        let temp = tempfile::tempdir().expect("临时目录");
        let manifest = TurnManifest {
            session: 1,
            turn: 1,
            entries: vec![
                CheckpointEntry {
                    path: "src/new.rs".into(),
                    kind: EntryKind::Created,
                    pre_hash: None,
                    post_hash: None,
                    blob: None,
                },
                CheckpointEntry {
                    path: "src/old.rs".into(),
                    kind: EntryKind::Modified,
                    pre_hash: None,
                    post_hash: None,
                    blob: None,
                },
            ],
            uncovered: Vec::new(),
        };
        let path = checkpoint_manifest_path(temp.path(), 1, 1);
        fs::create_dir_all(path.parent().unwrap()).expect("目录");
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).expect("清单");

        let data = load_timeline(
            temp.path(),
            1,
            &[100],
            BTreeSet::from(["src/new.rs".to_string(), "src/old.rs".to_string()]),
            BTreeSet::from(["src".to_string()]),
        );
        assert_eq!(data.turn_created.get(&1).unwrap().len(), 1);
        let visible = visible_files(&data.anchors[0], &data);
        assert!(visible.contains("src/new.rs"));
        assert!(visible.contains("src/old.rs"));
        assert_eq!(visible_dirs(&data, &visible).len(), 1);
    }

    #[test]
    fn git_timeline_reads_tree_and_degrades_without_git() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let temp = tempfile::tempdir().expect("临时目录");
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(temp.path())
                .args(args)
                .output()
                .expect("git");
            assert!(output.status.success(), "git {args:?} 失败");
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "test"]);
        fs::write(temp.path().join("a.rs"), "fn a() {}").expect("a");
        git(&["add", "."]);
        git(&["commit", "-m", "first"]);
        fs::write(temp.path().join("b.rs"), "fn b() {}").expect("b");
        git(&["add", "."]);
        git(&["commit", "-m", "second"]);

        let data = load_timeline(temp.path(), 1, &[], BTreeSet::new(), BTreeSet::new());
        assert!(data.has_git);
        assert_eq!(data.anchors.len(), 2);
        assert!(
            data.anchors
                .iter()
                .all(|anchor| matches!(anchor.kind, TimelineAnchorKind::Commit { .. }))
        );
        // 两次提交可能落在同一秒，锚点顺序不保证“最后=最新”；只验证各自树内容。
        assert!(
            data.commit_trees.values().any(|tree| tree.contains("a.rs")),
            "第一次提交的树应可读取"
        );
        assert!(
            data.commit_trees.values().any(|tree| tree.contains("b.rs")),
            "第二次提交的树应可读取"
        );
    }

    #[test]
    fn plan_intersects_current_graph_and_filters_dirs() {
        let mut data = TimelineData {
            current_files: BTreeSet::from([
                "src/a.rs".to_string(),
                "src/b.rs".to_string(),
                "docs/x.md".to_string(),
            ]),
            current_dirs: BTreeSet::from([
                "src".to_string(),
                "docs".to_string(),
                "empty".to_string(),
            ]),
            ..Default::default()
        };
        data.commit_trees.insert(
            "h".to_string(),
            BTreeSet::from([
                "src/a.rs".to_string(),
                "src/gone.rs".to_string(),
                "docs/x.md".to_string(),
            ]),
        );
        data.anchors = vec![commit_anchor(0, "h")];
        let plan = plan_visibility(&data, Some(0)).expect("历史计划");
        assert!(plan.files.contains("src/a.rs"));
        assert!(plan.files.contains("docs/x.md"));
        assert!(!plan.files.contains("src/b.rs"));
        assert!(
            !plan.files.contains("src/gone.rs"),
            "当前图缺失的文件不回放"
        );
        assert_eq!(plan.missing, 1);
        assert!(plan.dirs.contains("src"));
        assert!(!plan.dirs.contains("empty"));
        assert!(plan.dirs.contains("docs"), "docs/x.md 可见，应保留 docs");
    }

    #[test]
    fn turn_visible_files_use_commit_prefix_plus_created() {
        let mut data = TimelineData::default();
        data.commit_trees
            .insert("h".to_string(), BTreeSet::from(["src/old.rs".to_string()]));
        data.anchors = vec![commit_anchor(1_000, "h"), turn_anchor(2_000, 1)];
        data.turn_created
            .insert(1, BTreeSet::from(["src/new.rs".to_string()]));
        data.turn_created
            .insert(2, BTreeSet::from(["src/later.rs".to_string()]));
        let visible = visible_files(&data.anchors[1], &data);
        assert!(visible.contains("src/old.rs"));
        assert!(visible.contains("src/new.rs"));
        assert!(!visible.contains("src/later.rs"));
    }

    #[test]
    fn no_git_turn_visible_removes_later_created() {
        let mut data = TimelineData {
            current_files: BTreeSet::from(["a.rs".to_string(), "b.rs".to_string()]),
            ..Default::default()
        };
        data.anchors = vec![turn_anchor(1_000, 1), turn_anchor(2_000, 2)];
        data.turn_created
            .insert(1, BTreeSet::from(["a.rs".to_string()]));
        data.turn_created
            .insert(2, BTreeSet::from(["b.rs".to_string()]));
        let visible = visible_files(&data.anchors[0], &data);
        assert!(visible.contains("a.rs"));
        assert!(!visible.contains("b.rs"));
    }

    #[test]
    fn slot_mapping_rounds_to_nearest_endpoint() {
        assert_eq!(slot_from_fraction(0.0, 3), Some(0));
        assert_eq!(slot_from_fraction(0.4, 3), Some(1));
        assert_eq!(slot_from_fraction(0.9, 3), Some(3));
        assert_eq!(slot_from_fraction(1.5, 3), Some(3));
        assert_eq!(slot_from_fraction(f32::NAN, 3), Some(0));
        assert_eq!(slot_from_fraction(0.5, 0), None);
    }

    #[test]
    fn state_saves_and_restores_selection() {
        let mut data = TimelineData::default();
        data.anchors = vec![turn_anchor(1, 1)];
        let mut state = TimelineState {
            data: Some(data),
            ..Default::default()
        };
        let mut pick = SpacePickState::default();
        pick.selected = Some(SpatialNodeId(7));
        state.go_to_slot(0, &mut pick);
        assert_eq!(state.position, Some(0));
        assert_eq!(pick.selected, None, "进入历史应清空选中");
        state.return_now(&mut pick);
        assert_eq!(state.position, None);
        assert_eq!(pick.selected, Some(SpatialNodeId(7)));
    }

    #[test]
    fn step_walks_anchors_and_returns_now() {
        let mut data = TimelineData::default();
        data.anchors = vec![turn_anchor(1, 1), turn_anchor(2, 2)];
        let mut state = TimelineState {
            data: Some(data),
            ..Default::default()
        };
        let mut pick = SpacePickState::default();
        state.step(-1, &mut pick);
        assert_eq!(state.position, Some(1), "从 now 向左进入最新历史");
        state.step(-1, &mut pick);
        assert_eq!(state.position, Some(0));
        state.step(1, &mut pick);
        assert_eq!(state.position, Some(1));
        state.step(1, &mut pick);
        assert_eq!(state.position, None, "最右端回到现在");
    }

    #[test]
    fn apply_data_keeps_anchor_identity() {
        let mut old = TimelineData::default();
        old.anchors = vec![commit_anchor(1, "a"), commit_anchor(2, "b")];
        let mut state = TimelineState {
            data: Some(old),
            position: Some(1),
            ..Default::default()
        };
        let mut pick = SpacePickState::default();
        pick.selected = Some(SpatialNodeId(3));
        let mut new = TimelineData::default();
        new.anchors = vec![commit_anchor(0, "c"), commit_anchor(2, "b")];
        state.apply_data(new, &mut pick);
        assert_eq!(state.position, Some(1), "同一提交应保持选中");
        assert_eq!(pick.selected, Some(SpatialNodeId(3)));
    }

    #[test]
    fn poll_loader_applies_then_clears_loading() {
        let (tx, rx) = unbounded();
        let mut loader = TimelineLoader {
            rx: Some(rx),
            current: Some(TimelineSignature {
                root: PathBuf::from("."),
                session: 1,
                turns: 0,
                scan_revision: 0,
            }),
            ..Default::default()
        };
        let mut timeline = TimelineState {
            loading: true,
            ..Default::default()
        };
        let mut pick = SpacePickState::default();
        tx.send(TimelineData::default()).expect("发送");
        poll_loader(&mut loader, &mut timeline, &mut pick);
        assert!(timeline.data.is_some());
        assert!(!timeline.loading);
        assert!(loader.rx.is_none());
    }

    #[test]
    fn poll_loader_reports_thread_failure() {
        let (tx, rx) = unbounded::<TimelineData>();
        drop(tx);
        let mut loader = TimelineLoader {
            rx: Some(rx),
            ..Default::default()
        };
        let mut timeline = TimelineState::default();
        let mut pick = SpacePickState::default();
        poll_loader(&mut loader, &mut timeline, &mut pick);
        assert!(timeline.error.is_some());
        assert!(!timeline.loading);
    }

    fn node(id: u32, kind: SpatialNodeKind, path: &str) -> SpatialNode {
        SpatialNode {
            id: SpatialNodeId(id),
            kind,
            label: path.to_string(),
            path: Some(PathBuf::from(path)),
            cluster: ClusterId(0),
            weight: 1.0,
            status: SpatialStatus::Idle,
            anchor: Some([0.0, 0.0, 0.0]),
        }
    }

    fn test_graph() -> SpaceGraph {
        let mut graph = SpatialGraph::default();
        graph.nodes.push(node(0, SpatialNodeKind::File, "src/a.rs"));
        graph.nodes.push(node(1, SpatialNodeKind::File, "src/b.rs"));
        graph.nodes.push(node(2, SpatialNodeKind::Module, "src"));
        SpaceGraph(graph)
    }

    fn test_star(id: u32, kind: SpatialNodeKind, path: &str) -> SpatialStar {
        SpatialStar {
            id: SpatialNodeId(id),
            kind,
            label: path.to_string(),
            path: Some(path.to_string()),
            base_scale: 1.0,
        }
    }

    #[test]
    fn visibility_system_replays_nodes_edges_and_keeps_camera() {
        let mut app = App::new();
        app.init_resource::<TimelineState>()
            .insert_resource(test_graph())
            .add_systems(Update, apply_timeline_visibility);

        let star_a = app
            .world_mut()
            .spawn((
                test_star(0, SpatialNodeKind::File, "src/a.rs"),
                Visibility::Visible,
            ))
            .id();
        let star_b = app
            .world_mut()
            .spawn((
                test_star(1, SpatialNodeKind::File, "src/b.rs"),
                Visibility::Visible,
            ))
            .id();
        let dir = app
            .world_mut()
            .spawn((
                test_star(2, SpatialNodeKind::Module, "src"),
                Visibility::Visible,
            ))
            .id();
        let edge_ab = app
            .world_mut()
            .spawn((
                SpaceEdge {
                    from: SpatialNodeId(0),
                    to: SpatialNodeId(1),
                },
                Visibility::Visible,
            ))
            .id();
        let edge_ad = app
            .world_mut()
            .spawn((
                SpaceEdge {
                    from: SpatialNodeId(0),
                    to: SpatialNodeId(2),
                },
                Visibility::Visible,
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                crate::space::view::SpaceCamera,
                Transform::default(),
                crate::space::view::OrbitCamera {
                    target: Vec3::ZERO,
                    yaw: 0.5,
                    pitch: 0.25,
                    dist: 12.0,
                },
            ))
            .id();

        let mut data = TimelineData {
            current_files: BTreeSet::from(["src/a.rs".to_string(), "src/b.rs".to_string()]),
            current_dirs: BTreeSet::from(["src".to_string()]),
            ..Default::default()
        };
        data.commit_trees
            .insert("h".to_string(), BTreeSet::from(["src/a.rs".to_string()]));
        data.anchors = vec![commit_anchor(1, "h")];
        *app.world_mut().resource_mut::<TimelineState>() = TimelineState {
            data: Some(data),
            position: Some(0),
            ..Default::default()
        };
        app.update();

        assert_eq!(
            app.world().get::<Visibility>(star_a),
            Some(&Visibility::Visible)
        );
        assert_eq!(
            app.world().get::<Visibility>(star_b),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(dir),
            Some(&Visibility::Visible)
        );
        assert_eq!(
            app.world().get::<Visibility>(edge_ab),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(edge_ad),
            Some(&Visibility::Visible)
        );
        let orbit = app
            .world()
            .get::<crate::space::view::OrbitCamera>(camera)
            .expect("相机");
        assert_eq!(orbit.yaw, 0.5);
        assert_eq!(orbit.pitch, 0.25);
        assert_eq!(orbit.dist, 12.0);
        assert_eq!(orbit.target, Vec3::ZERO);

        app.world_mut()
            .resource_mut::<TimelineState>()
            .return_now(&mut SpacePickState::default());
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(star_b),
            Some(&Visibility::Visible)
        );
        assert_eq!(
            app.world().get::<Visibility>(edge_ab),
            Some(&Visibility::Visible)
        );
    }

    fn visible_space_panels() -> (PanelRegistry, SlotLayout) {
        let mut registry = PanelRegistry::default();
        registry.register(
            crate::panels::PanelDescriptor::new(
                ids::SPACE,
                crate::panels::Slot::Center,
                "panel-space",
                "panel-right",
                10,
            )
            .with_default_visible(),
            None,
        );
        (registry, SlotLayout::default())
    }

    #[test]
    fn arrow_keys_step_history_only_when_space_visible() {
        let mut data = TimelineData::default();
        data.anchors = vec![turn_anchor(1, 1), turn_anchor(2, 2)];
        let mut app = App::new();
        let (registry, layout) = visible_space_panels();
        app.insert_resource(registry)
            .insert_resource(layout)
            .insert_resource(TimelineState {
                data: Some(data),
                ..Default::default()
            })
            .init_resource::<SpacePickState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, handle_timeline_keys);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowLeft);
        app.update();
        assert_eq!(
            app.world().resource::<TimelineState>().position,
            Some(1),
            "左箭头从 now 进入最新历史"
        );

        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.press(KeyCode::ArrowRight);
        app.update();
        assert_eq!(
            app.world().resource::<TimelineState>().position,
            None,
            "右箭头回到现在"
        );
    }

    #[test]
    fn return_button_restores_selection() {
        let mut data = TimelineData::default();
        data.anchors = vec![turn_anchor(1, 1)];
        let mut app = App::new();
        let mut timeline = TimelineState {
            data: Some(data),
            ..Default::default()
        };
        let mut pick = SpacePickState::default();
        pick.selected = Some(SpatialNodeId(11));
        timeline.go_to_slot(0, &mut pick);
        assert_eq!(pick.selected, None);
        app.insert_resource(timeline)
            .insert_resource(pick)
            .add_systems(Update, handle_timeline_return_press);
        app.world_mut().spawn((SpaceTimelineReturnMarker, Pressed));
        app.update();
        assert_eq!(app.world().resource::<TimelineState>().position, None);
        assert_eq!(
            app.world().resource::<SpacePickState>().selected,
            Some(SpatialNodeId(11))
        );
    }

    #[test]
    fn escape_exits_history_before_selection() {
        let mut data = TimelineData::default();
        data.anchors = vec![turn_anchor(1, 1)];
        let mut timeline = TimelineState {
            data: Some(data),
            ..Default::default()
        };
        let mut pick = SpacePickState::default();
        pick.selected = Some(SpatialNodeId(5));
        timeline.go_to_slot(0, &mut pick);
        assert_eq!(pick.selected, None);

        let mut app = App::new();
        app.insert_resource(timeline)
            .insert_resource(pick)
            .init_resource::<crate::space::s2::SpaceSearchState>()
            .init_resource::<crate::space::s2::SpaceEscConsumed>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, crate::space::s2::consume_space_escape);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert_eq!(app.world().resource::<TimelineState>().position, None);
        assert_eq!(
            app.world().resource::<SpacePickState>().selected,
            Some(SpatialNodeId(5))
        );
        assert!(
            app.world()
                .resource::<crate::space::s2::SpaceEscConsumed>()
                .0
        );
    }

    #[test]
    fn ui_shows_empty_then_history_label_and_border() {
        let mut app = App::new();
        app.insert_resource(Theme::default())
            .insert_resource(Localizer::default())
            .insert_resource(TimelineState {
                data: Some(TimelineData::default()),
                ..Default::default()
            })
            .add_systems(
                Startup,
                |mut commands: Commands, theme: Res<Theme>, loc: Res<Localizer>| {
                    let root = commands.spawn_empty().id();
                    commands.entity(root).with_children(|hud| {
                        spawn_timeline_nodes(hud, &theme, &loc);
                    });
                },
            )
            .add_systems(Update, sync_timeline_ui);
        app.update();

        let mut label_query = app
            .world_mut()
            .query_filtered::<&Text, With<SpaceTimelineLabelText>>();
        let empty = label_query
            .single(app.world())
            .map(|text| text.0.clone())
            .expect("标签");
        assert_eq!(empty, "没有可回放的历史");
        let mut frame = app
            .world_mut()
            .query_filtered::<&Visibility, With<SpaceTimelineFrameMarker>>();
        assert_eq!(
            frame.single(app.world()).ok(),
            Some(&Visibility::Hidden),
            "现在不应显示历史边框"
        );

        let mut data = TimelineData::default();
        data.anchors = vec![commit_anchor(1, "abcdef123456")];
        app.world_mut()
            .resource_mut::<TimelineState>()
            .apply_data(data, &mut SpacePickState::default());
        app.world_mut()
            .resource_mut::<TimelineState>()
            .go_to_slot(0, &mut SpacePickState::default());
        app.update();

        let mut label_query = app
            .world_mut()
            .query_filtered::<&Text, With<SpaceTimelineLabelText>>();
        let label = label_query
            .single(app.world())
            .map(|text| text.0.clone())
            .expect("标签");
        assert!(label.contains("abcdef1"), "历史标签应显示短 hash：{label}");
        let mut frame = app
            .world_mut()
            .query_filtered::<&Visibility, With<SpaceTimelineFrameMarker>>();
        assert_eq!(
            frame.single(app.world()).ok(),
            Some(&Visibility::Visible),
            "历史应显示边框"
        );
        let mut markers = app.world_mut().query_filtered::<&SpaceTimelineMarker, ()>();
        assert_eq!(markers.iter(app.world()).count(), 2, "1 个锚点 + 现在端点");
    }

    #[test]
    fn spawns_timeline_structure_and_i18n() {
        let zh = Localizer::load("zh-CN");
        let en = Localizer::load("en-US");
        for key in [
            "space-timeline-now",
            "space-timeline-turn",
            "space-timeline-commit",
            "space-timeline-commit-short",
            "space-timeline-loading",
            "space-timeline-empty",
            "space-timeline-error",
            "space-timeline-degraded",
            "space-timeline-return-now",
        ] {
            assert_ne!(zh.get(key, &[]), key, "zh-CN 缺少 {key}");
            assert_ne!(en.get(key, &[]), key, "en-US 缺少 {key}");
        }

        let mut app = App::new();
        app.insert_resource(Theme::default())
            .insert_resource(zh)
            .add_systems(
                Startup,
                |mut commands: Commands, theme: Res<Theme>, loc: Res<Localizer>| {
                    let root = commands.spawn_empty().id();
                    commands.entity(root).with_children(|hud| {
                        spawn_timeline_nodes(hud, &theme, &loc);
                    });
                },
            );
        app.update();
        let world = app.world_mut();
        let mut rail = world.query_filtered::<(), With<SpaceTimelineRailMarker>>();
        assert_eq!(rail.iter(world).count(), 1);
        let mut frame = world.query_filtered::<(), With<SpaceTimelineFrameMarker>>();
        assert_eq!(frame.iter(world).count(), 1);
        let mut label = world.query_filtered::<(), With<SpaceTimelineLabelText>>();
        assert_eq!(label.iter(world).count(), 1);
        let mut back = world.query_filtered::<(), With<SpaceTimelineReturnMarker>>();
        assert_eq!(back.iter(world).count(), 1);
    }
}
