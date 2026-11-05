//! 测试用 MCP stdio Server fixture；不参与产品运行。
//!
//! 支持 initialize / tools/list / tools/call；`--mode crash-after-init` 用于
//! 验证初始化后的失败隔离。

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "normal".to_string());
    eprintln!("xgent mcp fixture started ({mode})");
    if mode == "stderr-heavy" {
        for index in 0..20_000 {
            eprintln!(
                "fixture stderr line {index} ................................................"
            );
        }
    }
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "serverInfo": {"name": "xgent-fixture", "version": "0.1.0"},
                    "capabilities": {"tools": {}},
                },
            }),
            "tools/list" => {
                if mode == "no-tools" {
                    json!({"jsonrpc": "2.0", "id": id, "result": {"tools": []}})
                } else if mode == "duplicates" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {"tools": [
                            {"name": "dup.one", "description": "first", "inputSchema": {"type": "object"}},
                            {"name": "dup_one", "description": "second", "inputSchema": {"type": "object"}},
                        ]},
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {"tools": [
                            {
                                "name": "echo",
                                "description": "Echo text",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {"text": {"type": "string"}},
                                    "required": ["text"],
                                },
                            },
                            {
                                "name": "fail",
                                "description": "Always fails",
                                "inputSchema": {"type": "object"},
                            },
                        ]},
                    })
                }
            }
            "tools/call" => {
                let name = message
                    .get("params")
                    .and_then(|params| params.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let text = message
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .and_then(|args| args.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if name == "echo" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {"content": [{"type": "text", "text": format!("echo:{text}")}]},
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{"type": "text", "text": format!("no such tool: {name}")}],
                            "isError": true,
                        },
                    })
                }
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("Method not found: {method}")},
            }),
        };
        let mut encoded = serde_json::to_vec(&response).expect("序列化响应");
        encoded.push(b'\n');
        if stdout.write_all(&encoded).await.is_err() {
            break;
        }
        let _ = stdout.flush().await;
        if method == "initialize" && mode == "crash-after-init" {
            std::process::exit(0);
        }
    }
}
