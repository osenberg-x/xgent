//! F-12 成本统计：当前会话累计、历史 JSONL 聚合与成本格式化。

use crossbeam_channel::{Receiver, TryRecvError, unbounded};
use std::collections::BTreeMap;
use std::path::Path;

use bevy::prelude::*;
use xgent_agent::Conversation;
use xgent_core::chat::AgentMessage;
use xgent_core::session::SessionEntry;
use xgent_settings::GlobalConfigRes;
use xgent_settings_core::pricing::PricingConfig;

/// 当前会话的 token/成本统计。
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct CostStats {
    /// 输入 token
    pub prompt: u64,
    /// 输出 token
    pub completion: u64,
    /// 已定价部分的成本；存在未定价消息时为 `None`
    pub cost: Option<f64>,
    /// 是否存在未定价消息
    pub unpriced: bool,
}

impl CostStats {
    /// 从会话消息重算。
    pub fn from_conversation(conversation: &Conversation, pricing: &PricingConfig) -> Self {
        let mut stats = Self::default();
        let mut priced = false;
        for message in &conversation.messages {
            let AgentMessage::Assistant(assistant) = message else {
                continue;
            };
            let Some(usage) = &assistant.usage else {
                continue;
            };
            stats.prompt += u64::from(usage.prompt);
            stats.completion += u64::from(usage.completion);
            let model = assistant.model.as_deref().unwrap_or("unknown");
            match pricing.cost(u64::from(usage.prompt), u64::from(usage.completion), model) {
                Some(cost) => {
                    priced = true;
                    stats.cost = Some(stats.cost.unwrap_or(0.0) + cost);
                }
                None => stats.unpriced = true,
            }
        }
        if stats.unpriced || !priced {
            stats.cost = None;
        }
        stats
    }

    /// 总 token。
    pub fn total_tokens(&self) -> u64 {
        self.prompt + self.completion
    }
}

/// 单个模型的历史累计。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelUsage {
    /// 模型名
    pub model: String,
    /// 输入 token
    pub prompt: u64,
    /// 输出 token
    pub completion: u64,
    /// 成本；未定价为 `None`
    pub cost: Option<f64>,
}

impl ModelUsage {
    /// 总 token。
    pub fn total_tokens(&self) -> u64 {
        self.prompt + self.completion
    }
}

/// 全部历史会话的聚合。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistoryUsage {
    /// 输入 token
    pub prompt: u64,
    /// 输出 token
    pub completion: u64,
    /// 总成本；存在未定价模型时为 `None`
    pub cost: Option<f64>,
    /// 是否存在未定价模型
    pub unpriced: bool,
    /// 按模型
    pub models: BTreeMap<String, ModelUsage>,
    /// 扫描的 jsonl 文件数
    pub sessions: usize,
    /// 跳过的损坏行
    pub skipped: usize,
}

impl HistoryUsage {
    /// 总 token。
    pub fn total_tokens(&self) -> u64 {
        self.prompt + self.completion
    }

    /// 按 token 降序、模型名升序的模型列表。
    pub fn models_by_usage(&self) -> Vec<&ModelUsage> {
        let mut models: Vec<&ModelUsage> = self.models.values().collect();
        models.sort_by(|a, b| {
            b.total_tokens()
                .cmp(&a.total_tokens())
                .then_with(|| a.model.cmp(&b.model))
        });
        models
    }
}

/// 成本展示：未定价 `—`；小于 1 美分保留 4 位。
pub fn format_cost(cost: Option<f64>) -> String {
    match cost {
        Some(cost) if cost.is_finite() && cost >= 0.0 => {
            if cost < 0.01 {
                format!("${cost:.4}")
            } else {
                format!("${cost:.2}")
            }
        }
        _ => "—".to_string(),
    }
}

/// 扫描 sessions 目录并聚合；损坏行/缺 usage/无 model 跳过。
pub fn scan_history(sessions_dir: &Path, pricing: &PricingConfig) -> HistoryUsage {
    let mut history = HistoryUsage::default();
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        return history;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        history.sessions += 1;
        let Ok(source) = std::fs::read_to_string(&path) else {
            history.skipped += 1;
            continue;
        };
        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<SessionEntry>(line) {
                Ok(SessionEntry::Message(message)) => {
                    let AgentMessage::Assistant(assistant) = message.message else {
                        continue;
                    };
                    let Some(usage) = assistant.usage else {
                        continue;
                    };
                    let model = assistant.model.unwrap_or_else(|| "unknown".to_string());
                    let prompt = u64::from(usage.prompt);
                    let completion = u64::from(usage.completion);
                    history.prompt += prompt;
                    history.completion += completion;
                    let entry = history
                        .models
                        .entry(model.clone())
                        .or_insert_with(|| ModelUsage {
                            model,
                            ..Default::default()
                        });
                    entry.prompt += prompt;
                    entry.completion += completion;
                    match pricing.cost(prompt, completion, &entry.model) {
                        Some(cost) => {
                            entry.cost = Some(entry.cost.unwrap_or(0.0) + cost);
                        }
                        None => {
                            entry.cost = None;
                            history.unpriced = true;
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => history.skipped += 1,
            }
        }
    }
    if !history.unpriced {
        let mut total = 0.0;
        let mut priced = false;
        for model in history.models.values() {
            if let Some(cost) = model.cost {
                total += cost;
                priced = true;
            }
        }
        if priced {
            history.cost = Some(total);
        }
    }
    history
}

/// 成本面板开关。
#[derive(Resource, Debug, Default)]
pub struct CostPanelState {
    /// 是否打开
    pub open: bool,
}

/// 历史扫描状态（线程 + mpsc，不阻塞 UI）。
#[derive(Resource, Default)]
pub struct CostHistoryState {
    /// 是否正在扫描
    pub loading: bool,
    /// 扫描结果
    pub result: Option<HistoryUsage>,
    /// 错误
    pub error: Option<String>,
    rx: Option<Receiver<Result<HistoryUsage, String>>>,
}

impl CostHistoryState {
    /// 启动一次后台扫描；正在扫描时不重复启动。
    pub fn start_scan(&mut self, sessions_dir: std::path::PathBuf, pricing: PricingConfig) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.result = None;
        self.error = None;
        let (tx, rx) = unbounded();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scan_history(&sessions_dir, &pricing)
            }))
            .map_err(|_| "历史扫描失败".to_string());
            let _ = tx.send(result);
        });
    }

    /// 轮询后台结果。
    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.loading = false;
                self.result = Some(result);
                self.rx = None;
            }
            Ok(Err(error)) => {
                self.loading = false;
                self.error = Some(error);
                self.rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.loading = false;
                self.error = Some("历史扫描任务已结束".to_string());
                self.rx = None;
            }
        }
    }
}

/// 成本统计插件：当前会话重算资源。
pub struct CostPlugin;

impl Plugin for CostPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CostStats>()
            .init_resource::<CostPanelState>()
            .init_resource::<CostHistoryState>()
            .add_systems(Update, recompute_cost_stats);
    }
}

fn recompute_cost_stats(
    conversation: Res<Conversation>,
    global: Res<GlobalConfigRes>,
    mut stats: ResMut<CostStats>,
) {
    if !conversation.is_changed() && !global.is_changed() && !stats.is_added() {
        return;
    }
    let next = CostStats::from_conversation(&conversation, &global.0.pricing);
    if *stats != next {
        *stats = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use xgent_core::chat::{AssistantMessage, TokenUsage, UserMessage};

    fn conversation(usages: &[Option<(u32, u32, &str)>]) -> Conversation {
        let mut conversation = Conversation::default();
        for (index, usage) in usages.iter().enumerate() {
            if let Some((prompt, completion, model)) = usage {
                conversation
                    .messages
                    .push(AgentMessage::Assistant(AssistantMessage {
                        content: Vec::new(),
                        model: Some((*model).to_string()),
                        usage: Some(TokenUsage {
                            prompt: *prompt,
                            completion: *completion,
                        }),
                        timestamp: index as u64,
                    }));
            } else {
                conversation.messages.push(AgentMessage::User(UserMessage {
                    content: Vec::new(),
                    timestamp: index as u64,
                }));
            }
        }
        conversation
    }

    fn pricing() -> PricingConfig {
        PricingConfig {
            input_per_million: 2.0,
            output_per_million: 10.0,
            models: BTreeMap::from([(
                "mini".to_string(),
                xgent_settings_core::ModelPrice {
                    input_per_million: 1.0,
                    output_per_million: 2.0,
                },
            )]),
        }
    }

    #[test]
    fn current_session_recomputes_and_skips_missing_usage() {
        let conv = conversation(&[
            Some((1_000_000, 1_000_000, "mini")),
            None,
            Some((500_000, 0, "mini")),
        ]);
        let stats = CostStats::from_conversation(&conv, &pricing());
        assert_eq!(stats.prompt, 1_500_000);
        assert_eq!(stats.completion, 1_000_000);
        assert!(!stats.unpriced);
        assert!((stats.cost.unwrap() - 3.5).abs() < 1e-9);
    }

    #[test]
    fn unpriced_model_has_no_cost() {
        let conv = conversation(&[Some((100, 100, "unknown"))]);
        let stats = CostStats::from_conversation(&conv, &PricingConfig::default());
        assert_eq!(stats.total_tokens(), 200);
        assert!(stats.cost.is_none());
        assert!(stats.unpriced);
    }

    #[test]
    fn history_aggregates_and_skips_bad_lines() {
        let temp = tempfile::tempdir().expect("临时目录");
        let write = |name: &str, lines: &[String]| {
            fs::write(temp.path().join(name), lines.join("\n")).expect("写会话");
        };
        let message = |model: &str, prompt: u32, completion: u32| {
            serde_json::to_string(&SessionEntry::Message(
                xgent_core::session::SessionMessage {
                    id: format!("m-{model}-{prompt}"),
                    parent_id: None,
                    timestamp: 1,
                    message: AgentMessage::Assistant(AssistantMessage {
                        content: Vec::new(),
                        model: Some(model.to_string()),
                        usage: Some(TokenUsage { prompt, completion }),
                        timestamp: 1,
                    }),
                },
            ))
            .expect("序列化")
        };
        write(
            "a.jsonl",
            &[
                message("mini", 1_000_000, 0),
                "not-json".to_string(),
                message("unknown", 500_000, 0),
            ],
        );
        write(
            "b.jsonl",
            &[
                message("mini", 0, 1_000_000),
                "{\"type\":\"message\"}".to_string(),
            ],
        );
        let before_a = fs::read(temp.path().join("a.jsonl")).expect("读前");
        let before_b = fs::read(temp.path().join("b.jsonl")).expect("读前");
        let history = scan_history(temp.path(), &pricing());
        assert_eq!(
            fs::read(temp.path().join("a.jsonl")).expect("读后"),
            before_a
        );
        assert_eq!(
            fs::read(temp.path().join("b.jsonl")).expect("读后"),
            before_b
        );
        assert_eq!(history.sessions, 2);
        assert_eq!(history.prompt, 1_500_000);
        assert_eq!(history.completion, 1_000_000);
        assert_eq!(history.skipped, 2);
        assert!(!history.unpriced);
        assert!((history.cost.unwrap() - 4.0).abs() < 1e-9);
        let models = history.models_by_usage();
        assert_eq!(models[0].model, "mini");
        assert_eq!(models[0].total_tokens(), 2_000_000);
    }

    #[test]
    fn cost_formatting_and_background_scan() {
        assert_eq!(format_cost(None), "—");
        assert_eq!(format_cost(Some(0.0042)), "$0.0042");
        assert_eq!(format_cost(Some(1.234)), "$1.23");

        let temp = tempfile::tempdir().expect("临时目录");
        let mut state = CostHistoryState::default();
        state.start_scan(temp.path().to_path_buf(), pricing());
        for _ in 0..100 {
            state.poll();
            if !state.loading {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!state.loading);
        assert!(state.error.is_none());
        assert_eq!(state.result.expect("结果").sessions, 0);
    }

    #[test]
    fn history_reads_no_files_returns_empty() {
        let temp = tempfile::tempdir().expect("临时目录");
        let history = scan_history(temp.path(), &pricing());
        assert_eq!(history.sessions, 0);
        assert!(history.models.is_empty());
        assert!(history.cost.is_none());
    }
}
