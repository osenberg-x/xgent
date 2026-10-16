//! 设置面板的"显示真值 + 只写脏键 + 校验先于写入"回归。
//!
//! 覆盖验收项 A1（打开即显示当前值，重开不丢）、A2/A5（三态行显示解析后的策略、
//! 且三态互斥折算成两张列表）、A9（非法值一条都不写）、A6 的面板侧半段
//! （未设置显示内置生效值并标注未设置；留空 = 写 `0` 清除）。
//!
//! 面板字段的内容直接读组件（`EditableText` / `SettingsNumInput`），不经几何：
//! headless 装配不产出子节点坐标，见 `settings_input_focus.rs` 顶部说明。

mod common;

use bevy::prelude::*;
use bevy::text::EditableText;
use xgent_settings_core::ProjectConfig;
use xgent_settings_core::global::{ProviderConfig, ProviderKind};
use xgent_ui::settings_panel::{
    LanguageButton, NumMode, ParsedField, PolicyButton, PolicyDraft, PolicyRow, PolicyState,
    SaveSettingsMessage, SettingsPanelState, parse_num_field, policy_lists, project_num_fields,
    provider_num_fields,
};
use xui_i18n::StringSource;

/// 取本轮已写入的保存请求。
fn saved_messages(app: &App) -> Vec<SaveSettingsMessage> {
    let msgs = app.world().resource::<Messages<SaveSettingsMessage>>();
    let mut cursor = msgs.get_cursor();
    cursor.read(&msgs).cloned().collect()
}

/// 造一份"什么都配过"的项目配置。
fn configured_project() -> ProjectConfig {
    ProjectConfig {
        agent: xgent_settings_core::AgentConfig {
            max_parallel_tools: 2,
            retrieval_max_tokens: 5_000,
            max_events_per_frame: 0,
        },
        tools: xgent_settings_core::ToolConfig {
            confirm_timeout_secs: 90,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn configured_global() -> xgent_settings_core::GlobalConfig {
    let mut g = xgent_settings_core::GlobalConfig::default();
    g.default_provider = "openai".into();
    g.providers.insert(
        "openai".into(),
        ProviderConfig {
            kind: ProviderKind::OpenAiCompat,
            api_base: "https://example.test/v1".into(),
            api_key: "sk-stored".into(),
            model_overrides: [("default".to_string(), "gpt-4o".to_string())].into(),
            context_window: 32_000,
            ..Default::default()
        },
    );
    g
}

/// A1：打开面板显示的是配置里的真实值，不是空白框。
#[test]
fn panel_opens_with_the_stored_values() {
    let mut app = common::panel_app();
    common::set_global(&mut app, configured_global());
    common::set_project(&mut app, configured_project());
    common::open_panel(&mut app);

    let mut read = |key: &str| common::num_field_text(&mut app, key);
    assert_eq!(read("agent.max_parallel_tools"), "2", "配置值应原样显示");
    assert_eq!(read("agent.retrieval_max_tokens"), "5000");
    assert_eq!(read("tools.confirm_timeout_secs"), "90");
    assert_eq!(read("context_window"), "32000");

    let base = field_text::<xgent_ui::settings_panel::ApiBaseInput>(&mut app);
    assert_eq!(base, "https://example.test/v1");
    let key = field_text::<xgent_ui::settings_panel::ApiKeyInput>(&mut app);
    assert_eq!(key, "sk-stored", "密钥框回填已存值，用户不必重打");
    let model = field_text::<xgent_ui::settings_panel::ModelInput>(&mut app);
    assert_eq!(model, "gpt-4o");
}

/// 取某个文本框的内容。
fn field_text<M: Component>(app: &mut App) -> String {
    let e = common::one_entity::<M>(app);
    app.world()
        .get::<EditableText>(e)
        .map(|v| v.value().to_string())
        .unwrap_or_default()
}

/// A1 的后半段：关掉再打开，显示值仍然是配置里的值（此前重开是四个空框）。
#[test]
fn reopening_the_panel_redraws_the_same_values() {
    let mut app = common::panel_app();
    common::set_global(&mut app, configured_global());
    common::set_project(&mut app, configured_project());
    common::open_panel(&mut app);
    common::reopen_panel(&mut app);

    assert_eq!(
        common::num_field_text(&mut app, "agent.max_parallel_tools"),
        "2"
    );
    assert_eq!(
        field_text::<xgent_ui::settings_panel::ApiBaseInput>(&mut app),
        "https://example.test/v1"
    );
    assert!(
        app.world().resource::<SettingsPanelState>().open.eq(&true),
        "重开后面板应处于打开态"
    );
}

/// A6 的面板侧半段：未设置的字段显示内置生效值，并说明"未设置"。
#[test]
fn unset_field_shows_effective_default_and_says_so() {
    let mut app = common::panel_app();
    common::set_global(&mut app, xgent_settings_core::GlobalConfig::default());
    common::set_project(&mut app, ProjectConfig::default());
    common::open_panel(&mut app);

    // 缺省 4（不是 0，也不是"不限"）
    assert_eq!(
        common::num_field_text(&mut app, "agent.max_parallel_tools"),
        "4"
    );
    assert_eq!(
        common::num_field_text(&mut app, "agent.retrieval_max_tokens"),
        "8000"
    );
    let unset_hint = tr_key("settings-field-unset");
    let labels = visible_texts(&mut app);
    assert!(
        labels.iter().any(|l| l == &unset_hint),
        "未设置的字段应带一条说明文案，实际文案 {labels:?}"
    );
}

/// 取当前语言下的文案（面板与测试同源）。
fn tr_key(key: &str) -> String {
    xgent_settings::Localizer::load("zh-CN").get(key, &[])
}

/// 面板上全部可见文本。
fn visible_texts(app: &mut App) -> Vec<String> {
    let mut q = app.world_mut().query::<&Text>();
    q.iter(app.world()).map(|t| t.0.clone()).collect()
}

/// 快照读不到（项目 TOML 坏了 / daemon 连不上）时，面板要说原因，
/// 而不是演成"配置是空的、还在加载"。
///
/// 原因文本有 daemon 侧的 `corrupt_project_toml_reports_the_file_path` 守着，
/// 这条守的是显示侧：把 `error_line` 整块删掉，这条会红。
#[test]
fn snapshot_read_failure_is_reported_instead_of_a_default_config() {
    let mut app = common::panel_app();
    common::set_global(&mut app, xgent_settings_core::GlobalConfig::default());
    common::set_project(&mut app, ProjectConfig::default());
    {
        let mut snap = app
            .world_mut()
            .resource_mut::<xgent_ui::settings_panel::SettingsSnapshotReady>();
        snap.error = "解析 /proj/.xgent/config.toml 失败".into();
        snap.ready = true;
    }
    common::open_panel(&mut app);

    let texts = visible_texts(&mut app);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("解析 /proj/.xgent/config.toml 失败")),
        "读失败的原因应出现在面板上，实际文案 {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == &tr_key("settings-loading")),
        "已经拿到失败结果就不该再显示加载占位，实际文案 {texts:?}"
    );
}

/// A9：留空在 Sentinel 语义下是"清除为未设置"，在 Plain 语义下是"不改"。
#[test]
fn empty_input_means_unset_or_unchanged_by_mode() {
    assert_eq!(
        parse_num_field("", 64, NumMode::Sentinel),
        Ok(ParsedField::Value(0)),
        "Sentinel 留空应折算成 0（未设置）"
    );
    assert_eq!(
        parse_num_field("  ", 86_400, NumMode::Plain),
        Ok(ParsedField::Unchanged),
        "Plain 留空不得写 0——那会让 timeout 直接变成 0 秒"
    );
    assert_eq!(
        parse_num_field("0", 64, NumMode::Plain),
        Ok(ParsedField::Value(0))
    );
    assert!(parse_num_field("abc", 64, NumMode::Sentinel).is_err());
    assert_eq!(
        parse_num_field("65", 64, NumMode::Sentinel),
        Err(xgent_ui::settings_panel::FieldError::OutOfRange { max: 64 })
    );
    // 上界本身要能填进去（判据是"超过上限"而不是"达到上限"）
    assert_eq!(
        parse_num_field("64", 64, NumMode::Sentinel),
        Ok(ParsedField::Value(64))
    );
}

/// A9 的写入侧半段：任一字段非法 → 一条都不写，并把原因写进状态行。
#[test]
fn invalid_number_blocks_the_whole_save() {
    let mut app = common::panel_app();
    common::set_global(&mut app, configured_global());
    common::set_project(&mut app, configured_project());
    common::open_panel(&mut app);

    // 一个合法改动 + 一个非法值：合法的也不能落盘
    common::set_num_field_text(&mut app, "context.max_keywords", "9");
    common::set_num_field_text(&mut app, "agent.max_parallel_tools", "many");
    common::press_save(&mut app);

    assert!(
        saved_messages(&mut app).is_empty(),
        "校验失败时不应发出任何写入请求"
    );
    let status = common::status_text(&mut app);
    assert!(
        status.contains(&tr_key("settings-error-not-a-number")),
        "状态行应点出是哪个字段非法，实际文案: {status}"
    );
    let label = app
        .world()
        .resource::<xgent_settings::Localizer>()
        .get("settings-max-parallel-tools", &[]);
    assert!(
        status.contains(label.as_str()),
        "状态行应带字段名，实际文案: {status}"
    );
}

/// A5 + A2：三态折算成两张列表，且与默认一致的选择不写进配置。
#[test]
fn policy_rows_fold_into_two_lists_without_redundant_entries() {
    let rows = vec![
        PolicyRow {
            tool_id: "read_file".into(),
            state: PolicyState::Auto,
        },
        PolicyRow {
            tool_id: "write_file".into(),
            state: PolicyState::Ask,
        },
        PolicyRow {
            tool_id: "run_command".into(),
            state: PolicyState::Deny,
        },
        PolicyRow {
            tool_id: "editor".into(),
            state: PolicyState::Auto,
        },
    ];
    // editor 是 UiOnly：不写配置也自动执行，所以不该进 approved
    let default_of = |id: &str| {
        if id == "editor" {
            PolicyState::Auto
        } else {
            PolicyState::Ask
        }
    };
    let (approved, denied) = policy_lists(&rows, default_of);
    assert_eq!(approved, vec!["read_file".to_string()]);
    assert_eq!(denied, vec!["run_command".to_string()]);

    // 同一工具改了三态后不会同时出现在两张表里
    let moved = vec![PolicyRow {
        tool_id: "run_command".into(),
        state: PolicyState::Deny,
    }];
    let (a2, d2) = policy_lists(&moved, default_of);
    assert!(a2.is_empty() && d2 == vec!["run_command".to_string()]);
}

/// 丢掉命令的 sink：只为把 `editor` 工具挂进执行器，让面板列出它。
struct NullSink;
impl xgent_tools::EditorCommandSink for NullSink {
    fn emit(&self, _req: xgent_tools::EditorCommandRequest) -> Result<(), String> {
        Ok(())
    }
}

/// A2：默认配置下 `read_file` 显示询问、`editor`（UiOnly）显示自动执行。
#[test]
fn policy_rows_display_the_resolved_answer() {
    use xgent_settings_core::project::ToolPolicyConfig;
    use xgent_tools::ToolExecutor;
    use xgent_ui::settings_panel::policy_rows;

    let executor = ToolExecutor::with_defaults();
    // xgent_app 启动时就是这么把编辑器工具注册进同一个执行器的
    executor.register(std::sync::Arc::new(xgent_tools::EditorTool::new(
        std::sync::Arc::new(NullSink),
    )));
    let rows = policy_rows(
        &executor,
        &ToolPolicyConfig::default(),
        &PolicyDraft::default(),
    );
    let state_of = |id: &str| {
        rows.iter()
            .find(|r| r.tool_id == id)
            .map(|r| r.state)
            .unwrap_or_else(|| panic!("行缺失: {id}"))
    };
    assert_eq!(state_of("read_file"), PolicyState::Ask, "默认需确认");
    assert_eq!(state_of("write_file"), PolicyState::Ask);
    assert_eq!(
        state_of(xgent_tools::EditorTool::ID),
        PolicyState::Auto,
        "UiOnly 由 resolve_policy 无条件放行，面板必须如实显示"
    );

    // 配了 denied 之后同一行显示拒绝
    let denied = ToolPolicyConfig {
        approved: vec![],
        denied: vec!["read_file".into()],
    };
    let rows2 = policy_rows(&executor, &denied, &PolicyDraft::default());
    assert_eq!(
        rows2
            .iter()
            .find(|r| r.tool_id == "read_file")
            .unwrap()
            .state,
        PolicyState::Deny
    );
}

/// A5 的 UI 侧：点"拒绝"后再保存，写出的两张表互斥。
#[test]
fn choosing_a_state_clears_the_other_lists() {
    let mut app = common::panel_app();
    common::set_global(&mut app, configured_global());
    let mut project = configured_project();
    project.tool_policy.approved = vec!["read_file".into()];
    common::set_project(&mut app, project);
    common::open_panel(&mut app);

    common::press::<PolicyButton>(&mut app, |b| {
        b.tool_id == "read_file" && b.state == PolicyState::Deny
    });
    common::press_save(&mut app);

    let msgs = saved_messages(&app);
    assert_eq!(msgs.len(), 1, "应恰好发出一次保存请求");
    let items = &msgs[0].project;
    let approved = items
        .iter()
        .find(|i| i.key == "tool_policy.approved")
        .map(|i| i.value.clone())
        .expect("approved 列表变了应被写入");
    let denied = items
        .iter()
        .find(|i| i.key == "tool_policy.denied")
        .map(|i| i.value.clone())
        .expect("denied 列表变了应被写入");
    assert_eq!(approved, serde_json::json!([] as [String; 0]));
    assert_eq!(denied, serde_json::json!(["read_file"]));
}

/// A6/A7/A8 的键映射：项目键原样发出，provider 键带 `providers.<id>.` 前缀。
#[test]
fn changed_fields_are_written_with_their_daemon_keys() {
    let mut app = common::panel_app();
    common::set_global(&mut app, configured_global());
    common::set_project(&mut app, configured_project());
    common::open_panel(&mut app);

    common::set_num_field_text(&mut app, "context.tree_max_depth", "1");
    common::set_num_field_text(&mut app, "context_window", "64000");
    // 留空在 Sentinel 语义下是"清除为未设置"：发出去的值必须是 0，
    // 不能是空串，也不能是刚才显示着的内置缺省
    common::set_num_field_text(&mut app, "tools.confirm_timeout_secs", "");
    common::press_save(&mut app);

    let msgs = saved_messages(&app);
    assert_eq!(msgs.len(), 1);
    let project_keys: Vec<&str> = msgs[0].project.iter().map(|i| i.key.as_str()).collect();
    assert!(
        project_keys.contains(&"context.tree_max_depth"),
        "项目键应原样发出: {project_keys:?}"
    );
    assert!(
        msgs[0]
            .project
            .iter()
            .any(|i| i.key == "tools.confirm_timeout_secs" && i.value == serde_json::json!(0)),
        "留空应写成 0（未设置哨兵）: {:?}",
        msgs[0].project
    );
    let global = &msgs[0].global;
    assert!(
        global
            .iter()
            .any(|i| i.key == "providers.openai.context_window"
                && i.value == serde_json::json!(64000)),
        "provider 键应拼成 providers.<id>.<field>: {global:?}"
    );
}

/// 「只写脏键」：不改任何东西时点保存，一个键都不发。
#[test]
fn untouched_fields_produce_no_writes() {
    let mut app = common::panel_app();
    common::set_global(&mut app, configured_global());
    common::set_project(&mut app, configured_project());
    common::open_panel(&mut app);

    common::press_save(&mut app);
    assert!(
        saved_messages(&mut app).is_empty(),
        "未改动任何字段时不应发出写请求——否则留空的密钥框会清掉已存凭据"
    );
    assert!(
        app.world().resource::<SettingsPanelState>().open,
        "没写任何东西时面板保持打开"
    );
}

/// 密钥框留空不会被当成"把密钥清空"（脏检查的直接后果）。
#[test]
fn empty_api_key_field_does_not_wipe_the_stored_key() {
    let mut app = common::panel_app();
    common::set_global(&mut app, configured_global());
    common::set_project(&mut app, ProjectConfig::default());
    common::open_panel(&mut app);

    let e = common::one_entity::<xgent_ui::settings_panel::ApiKeyInput>(&mut app);
    {
        let mut edit = app.world_mut().get_mut::<EditableText>(e).unwrap();
        *edit = EditableText::new("");
    }
    common::press_save(&mut app);
    let msgs = saved_messages(&app);
    let wipes = msgs
        .iter()
        .flat_map(|m| m.global.iter())
        .any(|i| i.key.ends_with(".api_key"));
    assert!(!wipes, "清空显示框不应把已存 api_key 写没");
}

/// 语言按钮只在真的换语言时才发写请求。
#[test]
fn language_button_writes_only_on_change() {
    let mut app = common::panel_app();
    let mut g = configured_global();
    g.preferences.language = "zh-CN".into();
    common::set_global(&mut app, g);
    common::set_project(&mut app, ProjectConfig::default());
    common::open_panel(&mut app);

    common::press::<LanguageButton>(&mut app, |b| b.lang == "zh-CN");
    assert_eq!(
        app.world()
            .resource::<Messages<xgent_ui::settings_panel::SaveLanguageMessage>>()
            .len(),
        0,
        "点当前语言不该发写请求"
    );
    common::press::<LanguageButton>(&mut app, |b| b.lang == "en-US");
    assert_eq!(
        app.world()
            .resource::<Messages<xgent_ui::settings_panel::SaveLanguageMessage>>()
            .len(),
        1,
        "换成另一种语言应发一条持久化消息"
    );
}

/// 字段表与 core 的缺省一致（面板不得自带一套数字）。
#[test]
fn field_table_mirrors_core_defaults() {
    use xgent_settings_core::project::defaults as d;
    let f = project_num_fields(&ProjectConfig::default());
    let get = |key: &str| {
        f.iter()
            .find(|x| x.key == key)
            .unwrap_or_else(|| panic!("缺字段 {key}"))
    };
    assert_eq!(
        get("agent.max_parallel_tools").effective,
        d::MAX_PARALLEL_TOOLS as u64
    );
    assert_eq!(
        get("agent.retrieval_max_tokens").effective,
        d::RETRIEVAL_MAX_TOKENS as u64
    );
    assert_eq!(
        get("tools.confirm_timeout_secs").effective,
        d::CONFIRM_TIMEOUT_SECS
    );
    assert_eq!(
        get("context.tree_max_depth").effective,
        d::TREE_MAX_DEPTH as u64
    );
    assert_eq!(
        get("context.max_keywords").effective,
        d::MAX_KEYWORDS as u64
    );
    // 项目侧全是 Sentinel（core 给每个键都定义了回落缺省）
    assert!(
        f.iter().all(|x| x.mode == NumMode::Sentinel),
        "项目数值键都该是 Sentinel 语义"
    );
    let p = provider_num_fields(&ProviderConfig::default());
    let by = |k: &str| p.iter().find(|x| x.key == k).unwrap();
    assert_eq!(by("timeout_secs").mode, NumMode::Plain);
    // 只在 Anthropic 下生效的那个框必须自己说清楚，否则用户以为配上了
    assert_eq!(
        by("max_output_tokens").note_key,
        Some("settings-field-anthropic-only")
    );
    assert_eq!(by("context_window").mode, NumMode::Sentinel);
    assert_eq!(
        by("context_window").effective,
        xgent_settings_core::global::defaults::CONTEXT_WINDOW as u64
    );
}

/// A9 报告文案：已写入的键与失败的键都出现在状态行里，面板不关。
#[test]
fn partial_save_reports_written_and_failed_keys() {
    let mut app = common::panel_app();
    common::set_global(&mut app, configured_global());
    common::set_project(&mut app, ProjectConfig::default());
    common::open_panel(&mut app);

    // 直接投一条报告（app 侧的逐键结果在这里以消息形式回流）
    app.world_mut()
        .write_message(xgent_ui::settings_panel::SettingsSaveReportMessage {
            written: vec![
                "tool_policy.denied".into(),
                "agent.max_parallel_tools".into(),
            ],
            failed: vec![("context.max_keywords".into(), "未知配置 key".into())],
        });
    app.update();

    let status = common::status_text(&mut app);
    // 已落盘的键要逐个点名——只报"已保存 2 项"就看不出哪几个进去了
    assert!(
        status.contains("tool_policy.denied") && status.contains("agent.max_parallel_tools"),
        "已写入的键应被点名: {status}"
    );
    assert!(!status.contains('2'), "不该退回只报个数的形态: {status}");
    assert!(
        status.contains("未知配置 key"),
        "应带 daemon 给的原因: {status}"
    );
    assert!(
        status.contains("context.max_keywords"),
        "应点出失败的键: {status}"
    );
    assert!(
        app.world().resource::<SettingsPanelState>().open,
        "有失败时面板必须保持打开"
    );
}

/// A9 的另一半：daemon 拒了某个键之后，用户填的那个值必须还在框里。
///
/// 挡住重建的判据在 app 侧——有键被拒时不推进 `tick`，连那份资源都不碰
/// （`settings_io::drain_save`，整条帧序由该模块的
/// `a_failed_save_leaves_the_rejected_value_in_the_real_panel_frame` 端到端守着）。
/// 这条用例钉的是**面板的规则**：只认 `tick` 的增量，绝不认 `is_changed()`。前两轮
/// 面板挂在 `is_changed()` 上，而 `ResMut` 的 `deref_mut` 无条件标脏、`tick > 0` 在
/// 第一次快照落地后恒成立，被拒的值就在下一帧被磁盘值换掉了。所以这里刻意造出
/// "资源被标脏、磁盘值被改写、tick 未动"的一帧：面板必须无动于衷。
///
/// 之后一次合法的 `tick` 推进（对端改了配置）必须把框重建回磁盘值——不能因为刚
/// 失败过一次就永久不刷新。
#[test]
fn failed_save_keeps_the_rejected_value_on_screen() {
    let mut app = common::panel_app();
    common::set_global(&mut app, xgent_settings_core::GlobalConfig::default());
    common::set_project(&mut app, ProjectConfig::default());
    common::open_panel(&mut app);
    // 真实次序：打开面板 → app 取回快照并推进 tick → 面板按磁盘值重建过一次
    app.world_mut()
        .resource_mut::<xgent_ui::settings_panel::SettingsSnapshotReady>()
        .tick += 1;
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        common::num_field_text(&mut app, "context.tree_max_depth"),
        "4",
        "前置条件：快照推进过一次后框里是磁盘值"
    );

    // 用户把深度改成 7（合法数字，但 daemon 说这个键不认）
    common::set_num_field_text(&mut app, "context.tree_max_depth", "7");
    app.world_mut()
        .write_message(xgent_ui::settings_panel::SettingsSaveReportMessage {
            written: vec!["agent.max_parallel_tools".into()],
            failed: vec![("context.tree_max_depth".into(), "未知配置 key".into())],
        });
    // 比 app 现在做的更严：把资源标脏、把磁盘值回填进 ProjectConfigRes，tick 仍不动
    {
        let mut snap = app
            .world_mut()
            .resource_mut::<xgent_ui::settings_panel::SettingsSnapshotReady>();
        snap.error.clear();
    }
    let mut disk = ProjectConfig::default();
    disk.context.tree_max_depth = 4;
    common::set_project(&mut app, disk);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        common::num_field_text(&mut app, "context.tree_max_depth"),
        "7",
        "资源被写脏但 tick 没动，被拒的值应留在框里，而不是被磁盘上的 4 换掉"
    );
    assert!(
        common::status_text(&mut app).contains("context.tree_max_depth"),
        "状态行要点出是哪个键失败"
    );
    assert!(
        app.world().resource::<SettingsPanelState>().open,
        "有失败时面板保持打开"
    );

    // 对端真的改了配置（app 推进 tick）：显示必须跟上，不能停在刚失败的那份编辑态
    let mut peer = ProjectConfig::default();
    peer.context.tree_max_depth = 6;
    common::set_project(&mut app, peer);
    app.world_mut()
        .resource_mut::<xgent_ui::settings_panel::SettingsSnapshotReady>()
        .tick += 1;
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        common::num_field_text(&mut app, "context.tree_max_depth"),
        "6",
        "合法的快照刷新不该被上一次失败挡住"
    );

    // 关闭再打开：同样回到磁盘值
    common::reopen_panel(&mut app);
    assert_eq!(
        common::num_field_text(&mut app, "context.tree_max_depth"),
        "6",
        "重开面板后应显示磁盘上的生效值"
    );
}

/// 快照推进（对端改了配置）时，尚未保存的策略草稿不能被吞掉。
///
/// 重渲染若用一份新草稿，屏幕上的高亮会回到磁盘值，而保存读的仍是真实草稿——
/// 显示与写入分叉，用户会以为自己点的那下没生效。
#[test]
fn snapshot_refresh_keeps_the_pending_policy_choice() {
    let mut app = common::panel_app();
    common::set_global(&mut app, xgent_settings_core::GlobalConfig::default());
    common::set_project(&mut app, ProjectConfig::default());
    common::open_panel(&mut app);

    common::press::<PolicyButton>(&mut app, |b| {
        b.tool_id == "read_file" && b.state == PolicyState::Auto
    });
    {
        let mut snap = app
            .world_mut()
            .resource_mut::<xgent_ui::settings_panel::SettingsSnapshotReady>();
        snap.tick += 1;
    }
    for _ in 0..3 {
        app.update();
    }

    let theme = app.world().resource::<xgent_ui::theme::Theme>();
    let accent = theme.accent;
    let rows: Vec<(PolicyState, Color)> = {
        let mut q = app.world_mut().query::<(&PolicyButton, &BackgroundColor)>();
        let mut out: Vec<(PolicyState, Color)> = q
            .iter(app.world())
            .filter(|(b, _)| b.tool_id == "read_file")
            .map(|(b, c)| (b.state, c.0))
            .collect();
        out.sort_by_key(|(st, _)| format!("{st:?}"));
        out
    };
    let selected: Vec<PolicyState> = rows
        .iter()
        .filter(|(_, c)| *c == accent)
        .map(|(st, _)| *st)
        .collect();
    assert_eq!(
        selected,
        vec![PolicyState::Auto],
        "重渲染后 read_file 的高亮应仍是未保存的自动执行，实际 {rows:?}"
    );
}

/// 只改项目作用域的项时，"没填 Provider ID"不该拦住保存
/// （此前 provider id 为空直接 early-return，工具策略一类改动根本存不下去）。
#[test]
fn project_only_save_works_without_provider_id() {
    let mut app = common::panel_app();
    // 空全局配置：default_provider 为空 → 面板的 provider id 框也是空的
    common::set_global(&mut app, xgent_settings_core::GlobalConfig::default());
    common::set_project(&mut app, ProjectConfig::default());
    common::open_panel(&mut app);

    common::press::<PolicyButton>(&mut app, |b| {
        b.tool_id == "read_file" && b.state == PolicyState::Deny
    });
    common::press_save(&mut app);

    let msgs = saved_messages(&app);
    assert_eq!(msgs.len(), 1, "只改策略也应发出保存请求");
    assert!(msgs[0].global.is_empty(), "不该带任何全局项");
    assert!(
        msgs[0]
            .project
            .iter()
            .any(|i| i.key == "tool_policy.denied" && i.value == serde_json::json!(["read_file"])),
        "策略改动应落进项目项: {:?}",
        msgs[0].project
    );
}

/// provider 项被改过却没填 id：整批不发，并说明原因。
#[test]
fn provider_changes_without_id_block_the_batch() {
    let mut app = common::panel_app();
    common::set_global(&mut app, xgent_settings_core::GlobalConfig::default());
    common::set_project(&mut app, ProjectConfig::default());
    common::open_panel(&mut app);

    // 改 provider 的上下文窗口（全局项），但 provider id 框是空的
    common::set_num_field_text(&mut app, "context_window", "64000");
    common::press_save(&mut app);

    assert!(
        saved_messages(&app).is_empty(),
        "没地方落盘的 provider 项不该被发出"
    );
    let status = common::status_text(&mut app);
    assert!(
        status.contains(&tr_key("settings-needs-provider-id")),
        "应说明缺 Provider ID: {status}"
    );
}
