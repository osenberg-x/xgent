use crate::*;
use eventsource_stream::Eventsource;
use futures_core::StreamExt;

/// OpenAI 兼容协议适配器
pub struct OpenAICompatibleAdapter {
    id: ProviderId,
    client: reqwest::Client,
    api_base: String,
    api_key: String,
    default_model: String,
}

impl OpenAICompatibleAdapter {
    pub fn new(
        id: impl Info<String>,
        api_base: impl Info<String>,
        api_key: impl Info<String>,
        default_model: impl Info<String>,
    ) -> Self {
        Self {
            id: ProviderId(id.into()),
            client: reqwest::Client::new(),
            api_base: api_base.into(),
            api_key: api_key.into(),
            default_model: default_model.into(),
        }
    }
}

#[async_trait::async_trait]
impl LLMProvider for OpenAICompatibleAdapter {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: ModelId(self.default_model.clone()),
            display_name: self.default_model.clone(),
            content_window: 128_000,
        }]
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream, ProviderError> {
        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));

        let mut body = serde_json::to_value(&request)
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        // 确保 stream: true
        body["stream"] = serde_json::Value::Bool(true);

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api{
                status: status.as_u16(),
                message: message,
            });
        }

        /// 解析 SSE 流
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            // 使用 eventsource-stream 解析 SSE
            let mut stream = response.bytes_stream().eventsource();

            while let Some(event) = stream.next().await {
                match event {
                    Ok(event) => {
                        if event.data == "[DONE]" {
                            let _ = tx.send(Ok(ChatStreamEvent::Done{
                                usage: TokenUsage::default(),
                            })).await;
                            break;
                        }

                        // 解析 OpenAI SSE data JSON
                        match serde_json::from_str::<serde_json::Value>(&event.data) {
                            // 解析 choices[0].delta
                                                            if let Some(choices) = data.get("choices").and_then(|c| c.as_array()) {
                                                                if let Some(choice) = choices.first() {
                                                                    if let Some(delta) = choice.get("delta") {
                                                                        // 文本内容
                                                                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                                                            if !content.is_empty() {
                                                                                if tx.send(Ok(ChatStreamEvent::Delta {
                                                                                    content: content.to_string(),
                                                                                })).await.is_err() {
                                                                                    break;
                                                                                }
                                                                            }
                                                                        }
                                                                        // 工具调用
                                                                        if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                                                            for tc in tool_calls {
                                                                                let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                                                                                let function = tc.get("function");
                                                                                let name = function.and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or("").to_string();
                                                                                let arguments = function.and_then(|f| f.get("arguments")).and_then(|a| a.as_str()).unwrap_or("{}").to_string();
                                                                                if !name.is_empty() {
                                                                                    if tx.send(Ok(ChatStreamEvent::ToolCall { id, name, arguments })).await.is_err() {
                                                                                        break;
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }                           Ok(data) => {
                            }
                            Err(e) => {
                                return Err(ProviderError::Api{
                                    status: 500,
                                    message: e.to_string(),
                                });
                            }
                        }
                    }
                    Err(e) => {
                        return Err(ProviderError::Api{
                            status: 500,
                            message: e.to_string(),
                        });
                    }
                }
            }
        });

        Ok(ChatStream::new(tx))
    }
}
