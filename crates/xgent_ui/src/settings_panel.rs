//! 设置面板：把两份配置里可改的键显示成可读、可改、可保存的表单。
//!
//! 面板由顶栏齿轮按钮、provider 标签或命令面板 `settings.open` 触发开关。
//! 面板自己**不**读写磁盘也不发 IPC：打开时发 [`RequestSettingsSnapshotMessage`]，
//! 由 `xgent_app` 经 daemon 取回当前值写进 [`GlobalConfigRes`] 与
//! [`ProjectConfigRes`]；保存时只把**改动过的键**装进 [`SaveSettingsMessage`]，
//! 写回结果经 [`SettingsSaveReportMessage`] 如实报告（哪些键已写入、哪个键失败及原因）。
//!
//! 四条贯穿全文件的判据：
//! - **显示的是生效值**。数值项有两种语义，面板必须分清：
//!   - [`NumMode::Sentinel`]（项目配置的全部数值键 + provider 的
//!     `context_window`/`max_output_tokens`/`first_event_timeout_secs`）：
//!     存 `0` 表示"未设置"，输入框显示内置缺省并标注未设置——`0` 不等于 0，
//!     也不等于"不限"（`max_parallel_tools = 0` 是 4）。
//!   - [`NumMode::Plain`]（`timeout_secs` 与两个重试延迟）：core 没给这些键定义
//!     缺省回落，存的就是生效的，因此显示原值；**留空表示不修改**。
//! - **只写脏键**：未改动的字段一个都不发，所以密钥框留空不会清空已存的 api_key。
//! - **先校验后写**：任何一个字段非法就一条都不写，并点出是哪个字段。
//! - **不猜策略**：工具三态行显示 [`ToolExecutor::resolved_policy`] 的解析结果，
//!   与执行时同一条判据（`denied` → `approved` → 动态升级 → tier 缺省）。
//!
//! 输入框使用官方 `EditableText`（光标/删除/IME 全交官方 text_input 系统）。

use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Button, TextInput};
use xgent_settings::{GlobalConfigRes, Localizer, ProjectConfigRes};
use xgent_settings_core::global::{ProviderConfig, ProviderKind};
use xgent_settings_core::project::ToolPolicyConfig;
use xgent_settings_core::{
    ProjectConfig, global::defaults as glob_defaults, project::defaults as proj_defaults,
};
use xgent_tools::{SecurityPolicy, ToolExecutor, ToolExecutorResource};

use crate::fonts::ui_text;
use crate::i18n::tr;
use crate::theme::{Density, Theme, px, space, type_scale};

/// 设置面板根节点标记。
#[derive(Component, Default)]
pub struct SettingsPanelMarker;

/// 设置面板遮罩（点击关闭；v8 M1）。
#[derive(Component, Default)]
pub struct SettingsScrimMarker;

/// Provider ID 输入框标记。
#[derive(Component, Default)]
pub struct ProviderIdInput;

/// API Base 输入框标记。
#[derive(Component, Default)]
pub struct ApiBaseInput;

/// API Key 输入框标记。
#[derive(Component, Default)]
pub struct ApiKeyInput;

/// Model 输入框标记。
#[derive(Component, Default)]
pub struct ModelInput;

/// 保存按钮标记。
#[derive(Component, Default)]
pub struct SettingsSaveButtonMarker;

/// 关闭按钮标记。
#[derive(Component, Default)]
pub struct SettingsCloseButtonMarker;

/// kind 选择按钮标记（携带它代表的 ProviderKind）。
#[derive(Component)]
pub struct KindButton {
    /// 该按钮代表的 provider 类型
    pub kind: ProviderKind,
    /// 是否当前选中
    pub selected: bool,
}

/// kind 选择器状态。
#[derive(Resource, Debug, Clone)]
pub struct KindSelector {
    /// 当前选中的 provider 类型
    pub current: ProviderKind,
}

impl Default for KindSelector {
    fn default() -> Self {
        Self {
            current: ProviderKind::OpenAiCompat,
        }
    }
}

/// 设置面板开关状态。
#[derive(Resource, Default)]
pub struct SettingsPanelState {
    pub open: bool,
}

/// 配置快照状态。
///
/// 面板自己没有读配置的通路（权威在 daemon），所以打开时要先要一份快照；
/// `tick` 由 `xgent_app` 在"权威值确实变了、且可以安全重建输入框"时 +1，
/// 面板据此重渲染（有键被 daemon 拒掉的保存不推进 `tick`，见
/// `xgent_app::settings_io::apply_save_result`）。
#[derive(Resource, Debug, Clone, Default)]
pub struct SettingsSnapshotReady {
    /// 已取到一份可用的配置快照
    pub ready: bool,
    /// 渲染计数：`xgent_app` 每落一份"可以安全按磁盘重建输入框"的权威快照就 +1，
    /// 面板按**增量**重渲染（见 `toggle_panel`）。
    pub tick: u32,
    /// 快照读取失败的原因（非空即显示，不把"读不到"演成"全是默认值"）
    pub error: String,
}

/// 请求取回当前配置快照（面板打开时 UI → `xgent_app`）。
#[derive(Message, Debug, Clone, Default)]
pub struct RequestSettingsSnapshotMessage;

/// 一条待写入的配置项（点分键 + 值）。
#[derive(Message, Debug, Clone)]
pub struct SaveSettingsMessage {
    /// 项目作用域的改动（写 `<project>/.xgent/config.toml`）
    pub project: Vec<ConfigItem>,
    /// 全局作用域的改动（写 `~/.xgent/agent/config.toml`）
    pub global: Vec<ConfigItem>,
}

/// 单个配置键值对。
#[derive(Debug, Clone)]
pub struct ConfigItem {
    /// 点分配置键
    pub key: String,
    /// 新值
    pub value: serde_json::Value,
}

/// 保存结果（`xgent_app` → 面板）。
#[derive(Message, Debug, Clone)]
pub struct SettingsSaveReportMessage {
    /// 已成功写入的键
    pub written: Vec<String>,
    /// 写入失败的键与 daemon 给的原因
    pub failed: Vec<(String, String)>,
}

/// 拉取模型列表（UI → `xgent_app`）。
#[derive(Message, Debug, Clone)]
pub struct FetchModelsMessage {
    /// provider id
    pub provider_id: String,
    /// api_base（用于构建 provider）
    pub api_base: String,
    /// api_key（编辑中的草稿凭据，不落盘）
    pub api_key: String,
    /// provider 类型
    pub kind: ProviderKind,
}

/// 模型列表结果（`xgent_app` → UI）。
#[derive(Message, Debug, Clone)]
pub struct ModelListResultMessage {
    /// 模型 id 列表
    pub models: Vec<String>,
    /// 错误信息（空表示成功）
    pub error: String,
}

/// 模型列表状态（缓存）。
#[derive(Resource, Default)]
pub struct ModelListState {
    /// 可用模型列表
    pub models: Vec<String>,
    /// 是否正在加载
    pub loading: bool,
    /// 错误信息
    pub error: String,
}

/// 刷新模型按钮标记。
#[derive(Component, Default)]
pub struct FetchModelsButtonMarker;

/// 模型列表项标记（携带模型 id）。
#[derive(Component)]
pub struct ModelItemMarker {
    pub model_id: String,
}

/// 模型列表容器标记。
#[derive(Component, Default)]
pub struct ModelListContainerMarker;

/// 语言选择按钮标记。
#[derive(Component, Clone)]
pub struct LanguageButton {
    /// 该按钮代表的语言代码
    pub lang: String,
}

/// 密度档位按钮标记（v8 M2b）。
#[derive(Component, Clone, Copy)]
pub struct DensityButton {
    /// 该按钮代表的档位
    pub density: Density,
}

/// 3D 首页开关按钮（MVP-2）。
#[derive(Component, Clone, Copy)]
pub struct ThreeDHomeButton {
    /// 是否启用 3D 首页
    pub enabled: bool,
}

/// 数值设置输入框标记。
#[derive(Component, Clone)]
pub struct SettingsNumInput {
    /// 配置键（provider 字段是短名，保存时再拼成 `providers.<id>.<key>`）
    pub key: &'static str,
    /// 打开面板时的存储值（脏检查基准）
    pub stored: u64,
    /// 打开面板时显示的生效值（未设置时即内置缺省）
    pub effective: u64,
    /// 该字段属于全局（provider）作用域
    pub global: bool,
    /// 语义：`0`/留空 是否表示"未设置"
    pub mode: NumMode,
    /// 允许上限
    pub max: u64,
}

/// 数值字段的语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumMode {
    /// `0` = 未设置，回落到内置缺省（显示缺省值，留空即写 `0` 清除）
    Sentinel,
    /// 存储值即生效值（留空 = 不修改）
    Plain,
}

/// 数值字段校验失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldError {
    /// 不是非负整数
    NotANumber,
    /// 超出该字段允许的上限
    OutOfRange {
        /// 允许上限
        max: u64,
    },
}

/// 解析后的输入值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsedField {
    /// 未修改（Plain 模式下留空）
    Unchanged,
    /// 新存储值
    Value(u64),
}

/// 一个数值设置字段（渲染 + 校验 + 脏检查的数据）。
#[derive(Debug, Clone)]
pub struct NumField {
    /// 配置键
    pub key: &'static str,
    /// 标签的 i18n 键
    pub label_key: &'static str,
    /// 存储值
    pub stored: u64,
    /// 生效值（Sentinel 且未设置时为内置缺省）
    pub effective: u64,
    /// 允许上限
    pub max: u64,
    /// 语义
    pub mode: NumMode,
    /// 该值只在特定条件下生效时的说明文案键（`None` = 无保留条件）
    pub note_key: Option<&'static str>,
}

impl NumField {
    /// 输入框应显示的数字。
    pub fn display_value(&self) -> u64 {
        match self.mode {
            NumMode::Sentinel => self.effective,
            NumMode::Plain => self.stored,
        }
    }

    /// 是否处于"未设置"状态（仅 Sentinel 语义有此概念）。
    pub fn is_unset(&self) -> bool {
        matches!(self.mode, NumMode::Sentinel) && self.stored == 0
    }
}

/// 解析输入框文本为待写值。
///
/// - [`NumMode::Sentinel`]：空串 = 未设置 = 写 `0`；
/// - [`NumMode::Plain`]：空串 = 不修改（写 `0` 对这类键有真实含义，不能由留空触发）。
pub fn parse_num_field(text: &str, max: u64, mode: NumMode) -> Result<ParsedField, FieldError> {
    let t = text.trim();
    if t.is_empty() {
        return match mode {
            NumMode::Sentinel => Ok(ParsedField::Value(0)),
            NumMode::Plain => Ok(ParsedField::Unchanged),
        };
    }
    let v: u64 = t.parse().map_err(|_| FieldError::NotANumber)?;
    if v > max {
        return Err(FieldError::OutOfRange { max });
    }
    Ok(ParsedField::Value(v))
}

/// 项目作用域数值字段（顺序即面板顺序，也是校验/标签的查找表）。
///
/// 缺省值与上限都取自 `xgent_settings_core`，本文件不写死数字。
pub fn project_num_fields(cfg: &ProjectConfig) -> Vec<NumField> {
    let rows = vec![
        NumField {
            key: "agent.retrieval_max_tokens",
            label_key: "settings-retrieval-max-tokens",
            stored: cfg.agent.retrieval_max_tokens as u64,
            effective: cfg.agent.effective_retrieval_max_tokens() as u64,
            max: 1_000_000,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "agent.max_parallel_tools",
            label_key: "settings-max-parallel-tools",
            stored: cfg.agent.max_parallel_tools as u64,
            effective: cfg.agent.effective_max_parallel_tools() as u64,
            // 上限刻意很小：这一闸挡的是子进程/socket/fd 数量，4 已是常见批次的量级
            max: 64,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "agent.max_events_per_frame",
            label_key: "settings-max-events-per-frame",
            stored: cfg.agent.max_events_per_frame as u64,
            effective: cfg.agent.effective_max_events_per_frame() as u64,
            max: 10_000,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "tools.confirm_timeout_secs",
            label_key: "settings-confirm-timeout",
            stored: cfg.tools.confirm_timeout_secs,
            effective: cfg.tools.effective_confirm_timeout_secs(),
            max: 86_400,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "tools.command_timeout_secs",
            label_key: "settings-command-timeout",
            stored: cfg.tools.command_timeout_secs,
            effective: cfg.tools.effective_command_timeout_secs(),
            max: 86_400,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "tools.command_max_output_bytes",
            label_key: "settings-command-max-output",
            stored: cfg.tools.command_max_output_bytes as u64,
            effective: cfg.tools.effective_command_max_output_bytes() as u64,
            max: 16 * 1024 * 1024,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "context.tree_max_depth",
            label_key: "settings-tree-max-depth",
            stored: cfg.context.tree_max_depth as u64,
            effective: cfg.context.effective_tree_max_depth() as u64,
            max: 64,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "context.tree_max_entries",
            label_key: "settings-tree-max-entries",
            stored: cfg.context.tree_max_entries as u64,
            effective: cfg.context.effective_tree_max_entries() as u64,
            max: 100_000,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "context.rg_timeout_secs",
            label_key: "settings-rg-timeout",
            stored: cfg.context.rg_timeout_secs,
            effective: cfg.context.effective_rg_timeout_secs(),
            max: 3_600,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "context.max_search_files",
            label_key: "settings-max-search-files",
            stored: cfg.context.max_search_files as u64,
            effective: cfg.context.effective_max_search_files() as u64,
            max: 10_000,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "context.max_keywords",
            label_key: "settings-max-keywords",
            stored: cfg.context.max_keywords as u64,
            effective: cfg.context.effective_max_keywords() as u64,
            max: 256,
            mode: NumMode::Sentinel,
            note_key: None,
        },
    ];
    rows
}

/// provider（全局作用域）数值字段。
pub fn provider_num_fields(pc: &ProviderConfig) -> Vec<NumField> {
    vec![
        NumField {
            key: "context_window",
            label_key: "settings-context-window",
            stored: pc.context_window as u64,
            effective: pc.resolve_context_window("").0 as u64,
            max: 1_000_000_000,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "max_output_tokens",
            label_key: "settings-max-output-tokens",
            stored: pc.max_output_tokens as u64,
            effective: pc.effective_max_output_tokens() as u64,
            max: 1_000_000_000,
            mode: NumMode::Sentinel,
            // build_provider 只在 Anthropic 分支发送 max_tokens；OpenAI 兼容下
            // 这个框改了也不影响请求，所以必须写明，不能让人以为配上了
            note_key: Some("settings-field-anthropic-only"),
        },
        NumField {
            key: "first_event_timeout_secs",
            label_key: "settings-first-event-timeout",
            stored: pc.first_event_timeout_secs,
            effective: pc.effective_first_event_timeout_secs(),
            max: 3_600,
            mode: NumMode::Sentinel,
            note_key: None,
        },
        NumField {
            key: "timeout_secs",
            label_key: "settings-request-timeout",
            stored: pc.timeout_secs,
            effective: pc.timeout_secs,
            max: 86_400,
            // core 里 0 就是"请求立刻超时"，没有回落缺省一说 → 不按未设置处理
            mode: NumMode::Plain,
            note_key: None,
        },
        NumField {
            key: "retry_initial_delay_ms",
            label_key: "settings-retry-initial-delay",
            stored: pc.retry_initial_delay_ms,
            effective: pc.retry_initial_delay_ms,
            max: 600_000,
            mode: NumMode::Plain,
            note_key: None,
        },
        NumField {
            key: "retry_max_delay_ms",
            label_key: "settings-retry-max-delay",
            stored: pc.retry_max_delay_ms,
            effective: pc.retry_max_delay_ms,
            max: 3_600_000,
            mode: NumMode::Plain,
            note_key: None,
        },
        NumField {
            key: "max_retries",
            label_key: "settings-max-retries",
            stored: pc.max_retries.unwrap_or(0) as u64,
            effective: pc.max_retries.unwrap_or(0) as u64,
            max: 20,
            // `None`（无限重试）在面板里不显示成数字：留空 = 不修改
            mode: NumMode::Plain,
            note_key: None,
        },
    ]
}

/// 字段键 → 定义（校验与标签都从这张表取，避免两处数字漂移）。
///
/// 两张表都查：provider 字段的键是短名（`context_window`），保存时才拼成
/// `providers.<id>.<key>`，而错误文案要按短名找标签。
pub fn num_spec(key: &str) -> Option<NumField> {
    project_num_fields(&ProjectConfig::default())
        .into_iter()
        .chain(provider_num_fields(&ProviderConfig::default()))
        .find(|f| f.key == key)
}

/// 内置缺省（测试断言"未设置显示的是 core 的缺省"时用）。
pub const DEFAULT_MAX_PARALLEL_TOOLS: u64 = proj_defaults::MAX_PARALLEL_TOOLS as u64;
/// 内置上下文窗口缺省。
pub const DEFAULT_CONTEXT_WINDOW: u64 = glob_defaults::CONTEXT_WINDOW as u64;

/// 工具策略三态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyState {
    /// 自动执行
    Auto,
    /// 每次询问
    Ask,
    /// 拒绝
    Deny,
}

impl PolicyState {
    /// 由解析后的安全策略映射成三态。
    pub fn from_policy(p: SecurityPolicy) -> Self {
        match p {
            SecurityPolicy::Approved => Self::Auto,
            SecurityPolicy::NeedsConfirmation => Self::Ask,
            SecurityPolicy::Denied => Self::Deny,
        }
    }

    /// 该状态对应的按钮文案 i18n 键。
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Auto => "settings-policy-auto",
            Self::Ask => "settings-policy-ask",
            Self::Deny => "settings-policy-deny",
        }
    }
}

/// 一行工具策略。
#[derive(Debug, Clone)]
pub struct PolicyRow {
    /// 工具 id
    pub tool_id: String,
    /// 当前显示的状态
    pub state: PolicyState,
}

/// 未保存的策略草稿（点击即改显示，保存才落盘）。
#[derive(Resource, Debug, Clone, Default)]
pub struct PolicyDraft {
    /// tool_id → 用户新选的状态
    pub changes: std::collections::HashMap<String, PolicyState>,
}

/// 策略按钮标记。
#[derive(Component, Clone)]
pub struct PolicyButton {
    /// 工具 id
    pub tool_id: String,
    /// 该按钮代表的状态
    pub state: PolicyState,
}

/// 面板状态提示行标记（保存结果写在这里）。
#[derive(Component, Default)]
pub struct SettingsStatusMarker;

/// 设置面板插件。
pub struct SettingsPanelPlugin;

impl Plugin for SettingsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<RequestSettingsSnapshotMessage>()
            .add_message::<SaveSettingsMessage>()
            .add_message::<SettingsSaveReportMessage>()
            .add_message::<FetchModelsMessage>()
            .add_message::<ModelListResultMessage>()
            .add_message::<SaveLanguageMessage>()
            .add_message::<SaveDensityMessage>()
            .add_message::<Save3DHomeMessage>()
            .insert_resource(SettingsPanelState::default())
            .insert_resource(KindSelector::default())
            .insert_resource(ModelListState::default())
            .insert_resource(SettingsSnapshotReady::default())
            .insert_resource(PolicyDraft::default())
            .add_systems(
                Update,
                (
                    toggle_panel,
                    push_settings_modal_on_spawn,
                    dismiss_on_modal_request,
                    close_on_scrim_click,
                    handle_kind_button,
                    handle_panel_dismiss,
                    handle_density_choice,
                    handle_3d_home_choice,
                    handle_policy_button,
                    handle_save_button,
                    handle_fetch_models,
                    handle_model_list_results,
                    handle_model_click,
                    apply_save_report,
                )
                    .chain(),
            );
    }
}

/// 保存语言偏好消息（UI → `xgent_app`，命令面板与设置面板共用）。
#[derive(Message, Debug, Clone)]
pub struct SaveLanguageMessage {
    /// 语言代码，如 "zh-CN" / "en-US"
    pub language: String,
}

/// 保存密度偏好消息（UI → `xgent_app`，v8 M2b）。
///
/// 与语言同形态：点击即生效、不进「保存」通路，写盘失败时由
/// [`SettingsSaveReportMessage`] 如实报告。
#[derive(Message, Debug, Clone)]
pub struct SaveDensityMessage {
    /// 目标档位
    pub density: Density,
}

/// 保存 3D 首页偏好（MVP-2）：点击即生效，不进「保存」通路。
#[derive(Message, Debug, Clone, Copy)]
pub struct Save3DHomeMessage {
    /// 是否启用 3D 首页
    pub enabled: bool,
}

/// 根据开关状态 spawn/despawn 面板，并在快照更新时重渲染。
///
/// 重渲染只认 `tick` 的**增量**，不认 `is_changed()`：`ResMut` 的 `deref_mut` 会无条件
/// 标脏，把重建挂在脏标记上等于"任何碰到这份资源的帧都要重画一次"——被 daemon 拒掉
/// 的那个值前两轮就是这样在下一帧被磁盘值换掉的。`xgent_app` 那边则有键被拒时连这份
/// 资源都不碰，两边各留一道。
fn toggle_panel(
    state: Res<SettingsPanelState>,
    mut commands: Commands,
    q_panel: Query<Entity, With<SettingsPanelMarker>>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    global: Res<GlobalConfigRes>,
    project: Res<ProjectConfigRes>,
    snapshot: Res<SettingsSnapshotReady>,
    executor: Res<ToolExecutorResource>,
    provider_info: Res<xgent_agent::ProviderInfo>,
    draft: Res<PolicyDraft>,
    mut writer: MessageWriter<RequestSettingsSnapshotMessage>,
    mut modal_pop: Option<MessageWriter<crate::modal::ModalPop>>,
    mut last_rendered: Local<u32>,
) {
    let panel_exists = q_panel.single().is_ok();

    if state.open && !panel_exists {
        *last_rendered = snapshot.tick;
        writer.write(RequestSettingsSnapshotMessage);
        spawn_panel(
            &mut commands,
            &theme,
            &loc,
            &global,
            &project,
            &executor,
            &snapshot,
            &provider_info,
            &draft,
        );
    } else if state.open && panel_exists && snapshot.tick > *last_rendered {
        // 权威快照真的推进了：重渲染，让显示跟上磁盘
        *last_rendered = snapshot.tick;
        if let Ok(entity) = q_panel.single() {
            if let Some(modal_pop) = modal_pop.as_mut() {
                modal_pop.write(crate::modal::ModalPop {
                    id: Some("settings"),
                });
            }
            commands.entity(entity).despawn();
        }
        spawn_panel(
            &mut commands,
            &theme,
            &loc,
            &global,
            &project,
            &executor,
            &snapshot,
            &provider_info,
            &draft,
        );
    } else if !state.open
        && panel_exists
        && let Ok(entity) = q_panel.single()
    {
        {
            if let Some(modal_pop) = modal_pop.as_mut() {
                modal_pop.write(crate::modal::ModalPop {
                    id: Some("settings"),
                });
            }
            commands.entity(entity).despawn();
        }
    }
}

/// spawn 设置面板 overlay。
fn spawn_panel(
    commands: &mut Commands,
    theme: &Theme,
    loc: &Localizer,
    global: &GlobalConfigRes,
    project: &ProjectConfigRes,
    executor_res: &ToolExecutorResource,
    snapshot: &SettingsSnapshotReady,
    provider_info: &xgent_agent::ProviderInfo,
    draft: &PolicyDraft,
) {
    let font = theme.font_size;
    // 面板编辑的是"当前正在用的 provider"，缺省回落 config 的 default_provider
    let pid = if provider_info.id.is_empty() {
        global.default_provider.clone()
    } else {
        provider_info.id.clone()
    };
    let pc = global.providers.get(&pid).cloned().unwrap_or_default();

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(theme.overlay),
            SettingsPanelMarker,
        ))
        .with_children(|p| {
            // 遮罩点击层：铺满 overlay、透明、位于卡片之下。
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: px(0.0),
                    left: px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                Button,
                SettingsScrimMarker,
            ));
            // 卡片本身是可滚动容器：设置项一多就超过视口高度，
            // 用 xui 的 ScrollFrame（与对话区/终端同一套）而不是裸 Overflow——
            // 裸 Overflow 在根节点无 UiRoot 时量不出尺寸，子节点会全部叠在原点。
            let mut area = xui::ScrollFrame::vertical();
            area.style().width = Val::Px(crate::theme::size::OVERLAY_SETTINGS_W);
            area.style().height = Val::Percent(92.0);
            area.style().padding = UiRect::all(px(space::LG));
            area.style().row_gap = px(space::SM);
            area.style().border = UiRect::all(px(1.0));
            p.spawn((
                area,
                BackgroundColor(theme.elevated),
                BorderColor::all(theme.border),
                // 卡片挡住遮罩：点卡片空白处不触发「点外关闭」。
                Pickable::default(),
            ))
            .with_children(|card| {
                card.spawn(ui_text(
                    tr(loc, "settings-title"),
                    type_scale::H3,
                    400,
                    theme.text,
                    type_scale::line_height::UI,
                ));

                if let Some(line) = error_line(snapshot) {
                    card.spawn(ui_text(
                        line,
                        type_scale::BODY_SM,
                        400,
                        theme.st_fail,
                        type_scale::line_height::UI,
                    ));
                }
                if !snapshot.ready {
                    card.spawn(ui_text(
                        tr(loc, "settings-loading"),
                        type_scale::BODY,
                        400,
                        theme.text_dim,
                        type_scale::line_height::UI,
                    ));
                    spawn_button_row(card, theme, loc);
                    return;
                }

                // ── Provider ─────────────────────────────────
                section(card, theme, loc, "settings-section-provider");
                // v8 M7-T2：TabIndex 由布局顺序派生（间隔 10）
                let mut tab_counter = 10i32;
                spawn_labelled_input(
                    card,
                    theme,
                    loc,
                    font,
                    "settings-provider-id",
                    &pid,
                    ProviderIdInput,
                    false,
                    tab_counter,
                );
                tab_counter += 10;
                spawn_kind_row(card, theme, loc, &pc.kind);
                spawn_labelled_input(
                    card,
                    theme,
                    loc,
                    font,
                    "settings-api-base",
                    &pc.api_base,
                    ApiBaseInput,
                    false,
                    tab_counter,
                );
                tab_counter += 10;
                spawn_labelled_input(
                    card,
                    theme,
                    loc,
                    font,
                    "settings-api-key",
                    &pc.api_key,
                    ApiKeyInput,
                    true,
                    tab_counter,
                );
                tab_counter += 10;
                let model = pc
                    .model_overrides
                    .get("default")
                    .cloned()
                    .unwrap_or_else(|| global.default_model.clone());
                spawn_labelled_input(
                    card,
                    theme,
                    loc,
                    font,
                    "settings-model",
                    &model,
                    ModelInput,
                    false,
                    tab_counter,
                );
                tab_counter += 10;
                // 模型列表：v8 M3-T2——此前只有 max_height:150 且完全没有
                // ScrollPosition，条目一多就被硬裁（审计 C2：可达末项）。
                let mut model_list = xui::ScrollFrame::vertical();
                model_list.content_sized();
                model_list.style().max_height = px(150.0);
                card.spawn((model_list, ModelListContainerMarker));
                for f in provider_num_fields(&pc) {
                    spawn_num_field(card, theme, loc, font, &f, true, tab_counter);
                    tab_counter += 10;
                }

                // ── 工具策略 ─────────────────────────────────
                section(card, theme, loc, "settings-section-tool-policy");
                card.spawn(ui_text(
                    tr(loc, "settings-tool-policy-hint"),
                    type_scale::BODY_SM,
                    400,
                    theme.text_muted,
                    type_scale::line_height::UI,
                ));
                for row in policy_rows(&executor_res.0, &project.tool_policy, draft) {
                    spawn_policy_row(card, theme, loc, &row);
                }

                // ── agent / tools / context ──────────────────
                for (section_key, prefix) in [
                    ("settings-section-agent", "agent."),
                    ("settings-section-tools", "tools."),
                    ("settings-section-context", "context."),
                ] {
                    section(card, theme, loc, section_key);
                    for f in project_num_fields(project)
                        .into_iter()
                        .filter(|f| f.key.starts_with(prefix))
                    {
                        spawn_num_field(card, theme, loc, font, &f, false, tab_counter);
                        tab_counter += 10;
                    }
                }

                // ── 语言 ────────────────────────────────────
                section(card, theme, loc, "settings-section-language");
                card.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(space::SM),
                    ..default()
                },))
                    .with_children(|row| {
                        for lang in ["zh-CN", "en-US"] {
                            let selected = global.preferences.language == lang;
                            segment_button(
                                row,
                                theme,
                                loc,
                                &format!("settings-lang-{lang}"),
                                selected,
                                LanguageButton {
                                    lang: lang.to_string(),
                                },
                            );
                        }
                    });

                // ── 密度（v8 M2b）───────────────────────────
                section(card, theme, loc, "settings-section-density");
                card.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(space::SM),
                    ..default()
                },))
                    .with_children(|row| {
                        for (density, key) in [
                            (Density::Comfortable, "settings-density-comfortable"),
                            (Density::Compact, "settings-density-compact"),
                        ] {
                            let selected = global.preferences.density == density;
                            segment_button(
                                row,
                                theme,
                                loc,
                                key,
                                selected,
                                DensityButton { density },
                            );
                        }
                    });

                // ── 3D 首页（MVP-2）─────────────────────────
                section(card, theme, loc, "settings-section-3d-home");
                card.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(space::SM),
                    ..default()
                },))
                    .with_children(|row| {
                        for (enabled, key) in [
                            (true, "settings-3d-home-on"),
                            (false, "settings-3d-home-off"),
                        ] {
                            let selected = global.preferences.three_d_home == enabled;
                            segment_button(
                                row,
                                theme,
                                loc,
                                key,
                                selected,
                                ThreeDHomeButton { enabled },
                            );
                        }
                    });

                spawn_button_row(card, theme, loc);
                card.spawn(ui_text(
                    "",
                    type_scale::BODY_SM,
                    400,
                    theme.text_dim,
                    type_scale::line_height::UI,
                ))
                .insert(SettingsStatusMarker);
            });
        });
}

/// 快照读取失败时的提示行（成功时 `None`）。
fn error_line(snapshot: &SettingsSnapshotReady) -> Option<String> {
    if snapshot.error.is_empty() {
        None
    } else {
        Some(snapshot.error.clone())
    }
}

/// 保存 + 关闭按钮行。
fn spawn_button_row(
    card: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    theme: &Theme,
    loc: &Localizer,
) {
    card.spawn((Node {
        flex_direction: FlexDirection::Row,
        column_gap: px(space::MD),
        ..default()
    },))
        .with_children(|btns| {
            btns.spawn((
                Button,
                Hovered::default(),
                Node {
                    padding: UiRect {
                        left: px(space::LG),
                        right: px(space::LG),
                        top: px(space::SM + 1.0),
                        bottom: px(space::SM + 1.0),
                    },
                    border_radius: BorderRadius::all(px(crate::theme::radius::CTRL)),
                    ..default()
                },
                BackgroundColor(theme.accent),
                ui_text(
                    tr(loc, "settings-save"),
                    type_scale::BODY_SM,
                    510,
                    theme.accent_text,
                    type_scale::line_height::CTRL,
                ),
                SettingsSaveButtonMarker,
            ));
            btns.spawn((
                Button,
                Hovered::default(),
                Node {
                    padding: UiRect::all(px(space::SM)),
                    border_radius: BorderRadius::all(px(crate::theme::radius::CTRL)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                ui_text(
                    tr(loc, "settings-close"),
                    type_scale::BODY_SM,
                    510,
                    theme.text_dim,
                    type_scale::line_height::CTRL,
                ),
                SettingsCloseButtonMarker,
            ));
        });
}

/// 一枚分段按钮（语言 / 密度共用）：选中态 `accent` 底 + 白字 + 600 字重。
///
/// `extra` 是各分段自己的标记组件（[`LanguageButton`] / [`DensityButton`]），
/// 点击处理按标记分派。
fn segment_button(
    row: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    theme: &Theme,
    loc: &Localizer,
    label_key: &str,
    selected: bool,
    extra: impl Bundle,
) {
    row.spawn((
        Button,
        Hovered::default(),
        Node {
            padding: UiRect {
                left: px(space::MD),
                right: px(space::MD),
                top: px(2.0),
                bottom: px(2.0),
            },
            border_radius: BorderRadius::all(px(crate::theme::radius::CTRL)),
            ..default()
        },
        BackgroundColor(if selected { theme.accent } else { theme.subtle }),
        ui_text(
            tr(loc, label_key),
            type_scale::BODY_SM,
            if selected { 600 } else { 400 },
            if selected {
                theme.accent_text
            } else {
                theme.text_dim
            },
            type_scale::line_height::CTRL,
        ),
        extra,
    ));
}

/// provider kind 选择行（MVP 暴露 4 个变体，`Custom` 不给按钮）。
fn spawn_kind_row(
    card: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    theme: &Theme,
    loc: &Localizer,
    current: &ProviderKind,
) {
    card.spawn(ui_text(
        tr(loc, "settings-kind"),
        type_scale::BODY,
        400,
        theme.text_dim,
        type_scale::line_height::UI,
    ));
    card.spawn((Node {
        flex_direction: FlexDirection::Row,
        column_gap: px(space::SM),
        ..default()
    },))
        .with_children(|row| {
            let kinds = [
                (ProviderKind::OpenAiCompat, "settings-kind-openai-compat"),
                (ProviderKind::ResponseApi, "settings-kind-response-api"),
                (ProviderKind::Anthropic, "settings-kind-anthropic"),
                (ProviderKind::Ollama, "settings-kind-ollama"),
            ];
            for (kind, label_key) in kinds {
                let selected = *current == kind;
                row.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        padding: UiRect {
                            left: px(space::MD),
                            right: px(space::MD),
                            top: px(2.0),
                            bottom: px(2.0),
                        },
                        border_radius: BorderRadius::all(px(crate::theme::radius::CTRL)),
                        ..default()
                    },
                    BackgroundColor(if selected { theme.accent } else { theme.subtle }),
                    ui_text(
                        tr(loc, label_key),
                        type_scale::BODY_SM,
                        if selected { 600 } else { 400 },
                        if selected {
                            theme.accent_text
                        } else {
                            theme.text_dim
                        },
                        type_scale::line_height::CTRL,
                    ),
                    KindButton { kind, selected },
                ));
            }
        });
}

/// 小节标题。
fn section(
    p: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    theme: &Theme,
    loc: &Localizer,
    label_key: &str,
) {
    p.spawn(ui_text(
        tr(loc, label_key),
        type_scale::BODY,
        600,
        theme.text,
        type_scale::line_height::UI,
    ));
}

/// 「标签 + 输入框」一行（文本字段，初值来自当前配置）。
fn spawn_labelled_input(
    p: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    theme: &Theme,
    loc: &Localizer,
    font: f32,
    label_key: &str,
    value: &str,
    marker: impl Bundle,
    with_fetch: bool,
    tab_index: i32,
) {
    p.spawn(ui_text(
        tr(loc, label_key),
        type_scale::BODY,
        400,
        theme.text_dim,
        type_scale::line_height::UI,
    ));
    if with_fetch {
        // 模型字段带一个 ↻ 按钮：拉取列表是临时凭据探测，不写配置
        p.spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(space::SM),
            ..default()
        },))
            .with_children(|row| {
                row.spawn(text_input_node(theme, font, value, marker, tab_index));
                row.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        padding: UiRect::all(px(space::SM)),
                        ..default()
                    },
                    BackgroundColor(theme.surface),
                    ui_text(
                        "↻",
                        type_scale::BODY,
                        400,
                        theme.text_dim,
                        type_scale::line_height::UI,
                    ),
                    FetchModelsButtonMarker,
                ));
            });
    } else {
        p.spawn(text_input_node(theme, font, value, marker, tab_index));
    }
}

/// 一行数值字段：标签 + 预填输入框 +（未设置时）提示。
fn spawn_num_field(
    p: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    theme: &Theme,
    loc: &Localizer,
    font: f32,
    f: &NumField,
    global: bool,
    tab_index: i32,
) {
    p.spawn(ui_text(
        tr(loc, f.label_key),
        type_scale::BODY,
        400,
        theme.text_dim,
        type_scale::line_height::UI,
    ));
    let shown = if matches!(f.mode, NumMode::Plain) && f.stored == 0 {
        String::new()
    } else {
        f.display_value().to_string()
    };
    p.spawn(text_input_node(
        theme,
        font,
        &shown,
        SettingsNumInput {
            key: f.key,
            stored: f.stored,
            effective: f.effective,
            global,
            mode: f.mode,
            max: f.max,
        },
        tab_index,
    ));
    if let Some(note) = f.note_key {
        p.spawn(ui_text(
            tr(loc, note),
            type_scale::MICRO,
            400,
            theme.text_muted,
            type_scale::line_height::UI,
        ));
    }
    if f.is_unset() {
        // 只显示数字会让人以为这就是自己配的值；把"这是内置缺省"写在旁边
        p.spawn(ui_text(
            tr(loc, "settings-field-unset"),
            type_scale::MICRO,
            400,
            theme.text_muted,
            type_scale::line_height::UI,
        ));
    }
}

/// 一行工具策略：工具名 + 三个状态按钮。
fn spawn_policy_row(
    p: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    theme: &Theme,
    loc: &Localizer,
    row: &PolicyRow,
) {
    p.spawn((Node {
        flex_direction: FlexDirection::Row,
        column_gap: px(space::SM),
        align_items: AlignItems::Center,
        ..default()
    },))
        .with_children(|line| {
            line.spawn(ui_text(
                row.tool_id.clone(),
                type_scale::BODY_SM,
                400,
                theme.text,
                type_scale::line_height::UI,
            ));
            for state in [PolicyState::Auto, PolicyState::Ask, PolicyState::Deny] {
                let selected = state == row.state;
                line.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        padding: UiRect {
                            left: px(space::SM),
                            right: px(space::SM),
                            top: px(2.0),
                            bottom: px(2.0),
                        },
                        border_radius: BorderRadius::all(px(crate::theme::radius::CTRL)),
                        ..default()
                    },
                    BackgroundColor(if selected { theme.accent } else { theme.subtle }),
                    ui_text(
                        tr(loc, state.label_key()),
                        type_scale::MICRO,
                        if selected { 600 } else { 400 },
                        if selected {
                            theme.accent_text
                        } else {
                            theme.text_dim
                        },
                        type_scale::line_height::CTRL,
                    ),
                    PolicyButton {
                        tool_id: row.tool_id.clone(),
                        state,
                    },
                ));
            }
        });
}

/// 由执行器与已存策略（叠加草稿）算出各行的显示状态。
pub fn policy_rows(
    executor: &ToolExecutor,
    stored: &ToolPolicyConfig,
    draft: &PolicyDraft,
) -> Vec<PolicyRow> {
    executor
        .policy_targets()
        .into_iter()
        .map(|(tool_id, _tier)| {
            let base = stored_state(executor, &tool_id, stored);
            PolicyRow {
                state: draft.changes.get(&tool_id).copied().unwrap_or(base),
                tool_id,
            }
        })
        .collect()
}

/// 已存配置解析出的三态（走执行器的同一条判据）。
fn stored_state(executor: &ToolExecutor, tool_id: &str, stored: &ToolPolicyConfig) -> PolicyState {
    PolicyState::from_policy(
        executor
            .resolved_policy(tool_id, stored)
            .unwrap_or(SecurityPolicy::NeedsConfirmation),
    )
}

/// 创建一个 `EditableText` 输入框节点。
///
/// 例外：输入框无 `Text` 组件（内容存于 [`EditableText`] 编辑器），若改用
/// [`ui_text`] 会额外插入 `Text::new("")`，被 `measure_text_system` 当静态
/// 文本节点测量，与输入框自身的 `TextInputMeasure` 冲突。故保留显式
/// `TextFont::from_font_size`（构造器式写法，非裸字面量）。
fn text_input_node(
    theme: &Theme,
    font: f32,
    value: &str,
    marker: impl Bundle,
    tab_index: i32,
) -> impl Bundle {
    (
        Node {
            padding: UiRect::all(px(space::SM)),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(crate::theme::radius::CTRL)),
            min_height: px(font + 8.0),
            ..default()
        },
        BackgroundColor(theme.input_bg),
        BorderColor::all(theme.border),
        TextFont::from_font_size(font),
        marker,
        // 文本输入 → 竖线 I-beam 指针
        crate::cursor::CursorStyle::Text,
        crate::cursor::CursorHit::default(),
        // 0.20 起点击聚焦只认 TabIndex（PointerFocusPlugin 的 AcquireFocus 解析器）：
        // 缺它则点击输入框会把焦点清到窗口，键盘事件无处投递 → 无法输入
        // v8 M7-T2：由布局顺序派生（间隔 10），废除全 0
        TabIndex(tab_index),
        EditableText::new(value),
        TextInput,
    )
}

/// px 辅助。
/// 处理 kind 按钮点击：切换当前选中 kind，更新按钮高亮。
fn handle_kind_button(
    q_kind: Query<&KindButton, Added<Pressed>>,
    mut selector: ResMut<KindSelector>,
    mut commands: Commands,
    q_all: Query<(Entity, &KindButton)>,
    theme: Res<Theme>,
) {
    for kb in q_kind.iter() {
        selector.current = kb.kind;
        for (e, kb) in q_all.iter() {
            let selected = kb.kind == selector.current;
            commands.entity(e).insert((
                KindButton {
                    kind: kb.kind,
                    selected,
                },
                BackgroundColor(if selected { theme.accent } else { theme.subtle }),
            ));
        }
    }
}

/// 处理策略按钮点击：记进草稿并即时高亮（保存才落盘）。
fn handle_policy_button(
    q_hit: Query<&PolicyButton, Added<Pressed>>,
    mut draft: ResMut<PolicyDraft>,
    mut commands: Commands,
    q_all: Query<(Entity, &PolicyButton)>,
    theme: Res<Theme>,
) {
    for hit in q_hit.iter() {
        draft.changes.insert(hit.tool_id.clone(), hit.state);
        for (e, btn) in q_all.iter() {
            if btn.tool_id != hit.tool_id {
                continue;
            }
            let selected = btn.state == hit.state;
            commands.entity(e).insert(BackgroundColor(if selected {
                theme.accent
            } else {
                theme.subtle
            }));
        }
    }
}

/// 面板实体一出现就入模态栈（z 与 Esc 交给 ModalStack 统一裁决）。
fn push_settings_modal_on_spawn(
    q: Query<Entity, Added<SettingsPanelMarker>>,
    push: Option<MessageWriter<crate::modal::ModalPush>>,
    focus: Option<Res<InputFocus>>,
) {
    let Some(mut push) = push else {
        return;
    };
    for entity in &q {
        push.write(crate::modal::ModalPush {
            id: "settings",
            entity,
            dismiss: crate::modal::Dismiss::EscAndScrim,
            opener_focus: focus.as_deref().and_then(|focus| focus.get()),
        });
    }
}

/// 通用 dismiss 请求（Esc）→ 关闭面板；真正的出栈由 `toggle_panel` 的 despawn 路径完成。
fn dismiss_on_modal_request(
    reader: Option<MessageReader<crate::modal::ModalDismissRequested>>,
    mut state: ResMut<SettingsPanelState>,
) {
    let Some(mut reader) = reader else {
        return;
    };
    for request in reader.read() {
        if request.id == "settings" {
            state.open = false;
        }
    }
}

/// 点击遮罩关闭（L-2：只有模态带 scrim）。
fn close_on_scrim_click(
    q: Query<(), (With<SettingsScrimMarker>, Added<Pressed>)>,
    mut state: ResMut<SettingsPanelState>,
) {
    for _ in q.iter() {
        state.open = false;
    }
}

/// 处理关闭与语言选择：两者都不经「保存」通路（语言有既有的持久化消息）。
fn handle_panel_dismiss(
    q_close: Query<(), (With<SettingsCloseButtonMarker>, Added<Pressed>)>,
    q_lang: Query<&LanguageButton, Added<Pressed>>,
    global: Res<GlobalConfigRes>,
    mut draft: ResMut<PolicyDraft>,
    mut state: ResMut<SettingsPanelState>,
    mut lang_writer: MessageWriter<SaveLanguageMessage>,
) {
    for _ in q_close.iter() {
        state.open = false;
        draft.changes.clear();
    }
    for hit in q_lang.iter() {
        if global.preferences.language != hit.lang {
            lang_writer.write(SaveLanguageMessage {
                language: hit.lang.clone(),
            });
        }
    }
}

/// 密度选择（v8 M2b）：点击即改显示并发写入消息，与语言同形态、不进「保存」通路。
///
/// 就地写 `GlobalConfigRes.preferences.density` 是**有意**的：`sync_theme_from_config`
/// 会在下一次 Update 把它带到 `ThemeHandle`，于是本帧或下一帧就重排，不必等 IPC 往返；
/// 落盘结果仍由 `xgent_app` 经配置通路确认，失败会写进面板状态行。
fn handle_density_choice(
    q: Query<&DensityButton, Added<Pressed>>,
    mut global: ResMut<GlobalConfigRes>,
    mut writer: MessageWriter<SaveDensityMessage>,
) {
    for hit in q.iter() {
        if global.preferences.density != hit.density {
            global.preferences.density = hit.density;
            writer.write(SaveDensityMessage {
                density: hit.density,
            });
        }
    }
}

/// 3D 首页开关：点击即改 `GlobalConfigRes` 并发写入消息；布局在下一帧切换。
fn handle_3d_home_choice(
    q: Query<&ThreeDHomeButton, Added<Pressed>>,
    mut global: ResMut<GlobalConfigRes>,
    mut writer: MessageWriter<Save3DHomeMessage>,
) {
    for hit in q.iter() {
        if global.preferences.three_d_home != hit.enabled {
            global.preferences.three_d_home = hit.enabled;
            writer.write(Save3DHomeMessage {
                enabled: hit.enabled,
            });
        }
    }
}

/// 处理保存按钮点击。
///
/// 保存只发脏键，且：
///
/// - 任一数值字段非法 → 一条也不写，报出是哪个字段；
/// - provider id 为空 → 不写、不关面板（此前会静默关面板，让用户以为已存）。
fn handle_save_button(
    q_save: Query<(), (With<SettingsSaveButtonMarker>, Added<Pressed>)>,
    q_id: Query<&EditableText, With<ProviderIdInput>>,
    q_base: Query<&EditableText, With<ApiBaseInput>>,
    q_key: Query<&EditableText, With<ApiKeyInput>>,
    q_model: Query<&EditableText, With<ModelInput>>,
    q_nums: Query<(&EditableText, &SettingsNumInput)>,
    mut q_status: Query<&mut Text, With<SettingsStatusMarker>>,
    kind_selector: Res<KindSelector>,
    global: Res<GlobalConfigRes>,
    project: Res<ProjectConfigRes>,
    executor_res: Res<ToolExecutorResource>,
    mut draft: ResMut<PolicyDraft>,
    mut writer: MessageWriter<SaveSettingsMessage>,
    loc: Res<Localizer>,
    theme: Res<Theme>,
) {
    for _ in q_save.iter() {
        let provider_id = trim_text(&q_id);
        let mut msg = SaveSettingsMessage {
            project: Vec::new(),
            global: Vec::new(),
        };

        // 1. 数值字段：先全部校验，任何一个非法都不写
        let mut rejected: Vec<String> = Vec::new();
        for (text, meta) in q_nums.iter() {
            let raw = text.value().to_string();
            match parse_num_field(&raw, meta.max, meta.mode) {
                Ok(ParsedField::Unchanged) => {}
                // 未设置的字段显示的是内置缺省：原样保存回来不算改动，
                // 否则打开一次面板就会把缺省值写进用户的配置文件
                Ok(ParsedField::Value(v))
                    if matches!(meta.mode, NumMode::Sentinel)
                        && meta.stored == 0
                        && v == meta.effective => {}
                Ok(ParsedField::Value(v)) if v != meta.stored => {
                    let key = if meta.global {
                        format!("providers.{provider_id}.{}", meta.key)
                    } else {
                        meta.key.to_string()
                    };
                    let item = ConfigItem {
                        key,
                        value: serde_json::json!(v),
                    };
                    if meta.global {
                        msg.global.push(item);
                    } else {
                        msg.project.push(item);
                    }
                }
                Ok(ParsedField::Value(_)) => {}
                Err(e) => rejected.push(crate::i18n::tr_with(
                    &loc,
                    "settings-failed-item",
                    &[
                        ("key", field_label_text(&loc, meta.key)),
                        ("reason", field_error_text(&loc, &e)),
                    ],
                )),
            }
        }
        if !rejected.is_empty() {
            set_status(
                &mut q_status,
                &theme,
                &rejected.join(&tr(&loc, "settings-list-sep")),
            );
            return;
        }

        // 2. provider 文本字段（同样只发变化过的）
        let current = global
            .providers
            .get(&provider_id)
            .cloned()
            .unwrap_or_default();
        let base = trim_text(&q_base);
        if base != current.api_base {
            msg.global.push(ConfigItem {
                key: format!("providers.{provider_id}.api_base"),
                value: serde_json::Value::String(base),
            });
        }
        let api_key = trim_text(&q_key);
        if !api_key.is_empty() && api_key != current.api_key {
            msg.global.push(ConfigItem {
                key: format!("providers.{provider_id}.api_key"),
                value: serde_json::Value::String(api_key),
            });
        }
        let model = trim_text(&q_model);
        let stored_model = current
            .model_overrides
            .get("default")
            .cloned()
            .unwrap_or_default();
        if model != stored_model {
            msg.global.push(ConfigItem {
                key: format!("providers.{provider_id}.model_overrides"),
                value: serde_json::Value::String(model),
            });
        }
        if kind_selector.current != current.kind {
            msg.global.push(ConfigItem {
                key: format!("providers.{provider_id}.kind"),
                value: serde_json::to_value(kind_selector.current)
                    .unwrap_or(serde_json::Value::Null),
            });
        }
        if global.default_provider != provider_id {
            msg.global.push(ConfigItem {
                key: "default_provider".into(),
                value: serde_json::Value::String(provider_id.clone()),
            });
        }

        // 3. 工具策略两张列表
        let executor: &ToolExecutor = &executor_res.0;
        let rows: Vec<PolicyRow> = executor
            .policy_targets()
            .into_iter()
            .map(|(tool_id, _)| {
                let base = stored_state(executor, &tool_id, &project.tool_policy);
                PolicyRow {
                    state: draft.changes.get(&tool_id).copied().unwrap_or(base),
                    tool_id,
                }
            })
            .collect();
        // 空配置下的状态：不写覆盖时本来就会自动执行的工具（UiOnly）不进列表
        let defaults = |id: &str| {
            PolicyState::from_policy(
                executor
                    .resolved_policy(id, &ToolPolicyConfig::default())
                    .unwrap_or(SecurityPolicy::NeedsConfirmation),
            )
        };
        let (approved, denied) = policy_lists(&rows, defaults);
        if approved != project.tool_policy.approved {
            msg.project.push(ConfigItem {
                key: "tool_policy.approved".into(),
                value: serde_json::json!(approved),
            });
        }
        if denied != project.tool_policy.denied {
            msg.project.push(ConfigItem {
                key: "tool_policy.denied".into(),
                value: serde_json::json!(denied),
            });
        }

        // provider 项要写进 `providers.<id>.*`，id 为空就没地方落——这时整批不发，
        // 但只改项目作用域的项不该被"没填 provider id"挡住（此前会）
        if !msg.global.is_empty() && provider_id.is_empty() {
            set_status(
                &mut q_status,
                &theme,
                &tr(&loc, "settings-needs-provider-id"),
            );
            return;
        }
        if msg.project.is_empty() && msg.global.is_empty() {
            set_status(&mut q_status, &theme, &tr(&loc, "settings-nothing-to-save"));
            return;
        }
        writer.write(msg);
        draft.changes.clear();
    }
}

/// 把三态选择折算成 `approved` / `denied` 两份列表。
///
/// 某工具若**不写配置时就已经**是自动执行（`UiOnly` 的 tier 缺省），不为它写
/// `approved` 条目——列表里只留用户真正改变过的东西。
pub fn policy_lists(
    rows: &[PolicyRow],
    default_state: impl Fn(&str) -> PolicyState,
) -> (Vec<String>, Vec<String>) {
    let mut approved = Vec::new();
    let mut denied = Vec::new();
    for row in rows {
        match row.state {
            PolicyState::Deny => denied.push(row.tool_id.clone()),
            PolicyState::Auto => {
                if default_state(&row.tool_id) != PolicyState::Auto {
                    approved.push(row.tool_id.clone());
                }
            }
            PolicyState::Ask => {}
        }
    }
    approved.sort();
    approved.dedup();
    denied.sort();
    denied.dedup();
    (approved, denied)
}

/// 取输入框文本（去首尾空白）。
fn trim_text<F: bevy::ecs::query::QueryFilter>(q: &Query<&EditableText, F>) -> String {
    q.single()
        .map(|e| e.value().to_string().trim().to_string())
        .unwrap_or_default()
}

/// 字段标签文案（找不到定义时如实报"未知字段"，不静默显示空标签）。
fn field_label_text(loc: &Localizer, key: &str) -> String {
    match num_spec(key) {
        Some(f) => tr(loc, f.label_key),
        None => key.to_string(),
    }
}

/// 校验错误的本地化文案（数字经 fluent 参数交给语料，不在 Rust 侧拼标点）。
fn field_error_text(loc: &Localizer, e: &FieldError) -> String {
    match e {
        FieldError::NotANumber => tr(loc, "settings-error-not-a-number"),
        FieldError::OutOfRange { max } => crate::i18n::tr_with(
            loc,
            "settings-error-out-of-range",
            &[("max", max.to_string())],
        ),
    }
}

/// 把提示写进面板状态行。
fn set_status(
    q_status: &mut Query<&mut Text, With<SettingsStatusMarker>>,
    _theme: &Theme,
    text: &str,
) {
    if let Ok(mut t) = q_status.single_mut() {
        t.0 = text.to_string();
    }
}

/// 处理刷新模型按钮点击：发 FetchModelsMessage。
fn handle_fetch_models(
    q_fetch: Query<(), (With<FetchModelsButtonMarker>, Added<Pressed>)>,
    q_id: Query<&EditableText, With<ProviderIdInput>>,
    q_base: Query<&EditableText, With<ApiBaseInput>>,
    q_key: Query<&EditableText, With<ApiKeyInput>>,
    kind_selector: Res<KindSelector>,
    mut fetch_writer: MessageWriter<FetchModelsMessage>,
    mut model_state: ResMut<ModelListState>,
    loc: Res<Localizer>,
) {
    for _ in q_fetch.iter() {
        let provider_id = trim_text(&q_id);
        let api_base = trim_text(&q_base);
        let api_key = trim_text(&q_key);
        if provider_id.is_empty() || api_base.is_empty() {
            model_state.error = tr(&loc, "settings-fetch-models-needs-fields");
            model_state.loading = false;
            continue;
        }
        model_state.loading = true;
        model_state.error.clear();
        fetch_writer.write(FetchModelsMessage {
            provider_id,
            api_base,
            api_key,
            kind: kind_selector.current,
        });
    }
}

/// 订阅 ModelListResultMessage，更新 ModelListState 并重建列表 UI。
fn handle_model_list_results(
    mut reader: MessageReader<ModelListResultMessage>,
    mut model_state: ResMut<ModelListState>,
    mut commands: Commands,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    q_container: Query<Entity, With<ModelListContainerMarker>>,
) {
    for ev in reader.read() {
        model_state.loading = false;
        model_state.error = ev.error.clone();
        model_state.models = ev.models.clone();

        let Ok(container) = q_container.single() else {
            continue;
        };
        commands.entity(container).despawn_related::<Children>();
        let lines: Vec<(String, Color)> = if !ev.error.is_empty() {
            vec![(ev.error.clone(), theme.st_fail)]
        } else if ev.models.is_empty() {
            vec![(tr(&loc, "settings-model-list-empty"), theme.text_muted)]
        } else {
            ev.models
                .iter()
                .map(|m| (m.clone(), theme.text_dim))
                .collect()
        };
        let models = ev.models.clone();
        commands.entity(container).with_children(|c| {
            for (text, color) in lines {
                let model_id = if models.contains(&text) {
                    Some(text.clone())
                } else {
                    None
                };
                if let Some(model_id) = model_id {
                    c.spawn((
                        Button,
                        Hovered::default(),
                        Node {
                            padding: UiRect::all(px(space::XS)),
                            border: UiRect::bottom(px(1.0)),
                            ..default()
                        },
                        BorderColor::all(theme.line),
                        ui_text(
                            text,
                            type_scale::BODY,
                            400,
                            color,
                            type_scale::line_height::UI,
                        ),
                        ModelItemMarker { model_id },
                    ));
                } else {
                    c.spawn(ui_text(
                        text,
                        type_scale::MICRO,
                        400,
                        color,
                        type_scale::line_height::UI,
                    ));
                }
            }
        });
    }
}

/// 处理模型列表项点击：填充到 Model 输入框。
fn handle_model_click(
    q_items: Query<&ModelItemMarker, Added<Pressed>>,
    mut q_model: Query<&mut EditableText, With<ModelInput>>,
) {
    let Some(item) = q_items.iter().next() else {
        return;
    };
    if let Ok(mut model_input) = q_model.single_mut() {
        model_input.clear();
        model_input.queue_edit(bevy::text::TextEdit::Insert(item.model_id.clone().into()));
    }
}

/// 把保存结果如实报出去：成功就收起面板并弹一条 toast（面板马上要消失，
/// 只写进它内部的状态行等于没写），失败则保持打开并把逐键原因写进状态行。
fn apply_save_report(
    mut reader: MessageReader<SettingsSaveReportMessage>,
    mut q_status: Query<&mut Text, With<SettingsStatusMarker>>,
    mut state: ResMut<SettingsPanelState>,
    theme: Res<Theme>,
    loc: Res<Localizer>,
    mut toast: MessageWriter<crate::kit::ToastMessage>,
) {
    for ev in reader.read() {
        let text = if ev.failed.is_empty() {
            state.open = false;
            let done = tr_with_saved(&loc, ev.written.len());
            toast.write(crate::kit::ToastMessage { text: done.clone() });
            done
        } else {
            // 分隔符与冒号都来自语料：英文串里不该出现中文顿号或分号
            let sep = tr(&loc, "settings-report-sep");
            let list_sep = tr(&loc, "settings-list-sep");
            let mut parts: Vec<String> = Vec::new();
            if !ev.written.is_empty() {
                // 落盘的键要**点名**，只报个数用户就看不出"哪几个进去了、哪个没进"
                parts.push(crate::i18n::tr_with(
                    &loc,
                    "settings-written-keys",
                    &[("keys", ev.written.join(&list_sep))],
                ));
            }
            parts.push(tr(&loc, "settings-failed-prefix"));
            for (key, why) in &ev.failed {
                parts.push(crate::i18n::tr_with(
                    &loc,
                    "settings-failed-item",
                    &[("key", key.clone()), ("reason", why.clone())],
                ));
            }
            parts.join(&sep)
        };
        set_status(&mut q_status, &theme, &text);
    }
}

/// 「已保存 N 项」文案（0 项时说不成，退成"无改动"）。
fn tr_with_saved(loc: &Localizer, n: usize) -> String {
    if n == 0 {
        tr(loc, "settings-nothing-to-save")
    } else {
        crate::i18n::tr_with(loc, "settings-saved-count", &[("count", n.to_string())])
    }
}
