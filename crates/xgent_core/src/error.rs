use thiserror::Error;

/// XGent 统一错误类型
#[derive(Debug, Error)]
pub enum XgentError {
    #[error("ipc error: {0}")]
    Ipc(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("tool error: {0}")]
    Tool(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type XgentResult<T> = Result<T, XgentError>;
