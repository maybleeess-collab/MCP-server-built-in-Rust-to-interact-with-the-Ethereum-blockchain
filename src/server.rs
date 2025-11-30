use crate::ethereum::EthereumClient;
use crate::tools::{balance::GetBalanceTool, price::GetTokenPriceTool, swap::SwapTokensTool, Tool};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead};
use tracing::{debug, error, info};

#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Option<Value>,
    id: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<JsonRpcError>,
    id: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcError {
    code: i32,
    message: String,
    data: Option<Value>,
}

pub async fn run(client: EthereumClient) -> Result<()> {
    let mut tools: HashMap<String, Box<dyn Tool>> = HashMap::new();

    // Register tools
    let balance_tool = GetBalanceTool;
    tools.insert(balance_tool.name().to_string(), Box::new(balance_tool));

    let price_tool = GetTokenPriceTool;
    tools.insert(price_tool.name().to_string(), Box::new(price_tool));

    let swap_tool = SwapTokensTool;
    tools.insert(swap_tool.name().to_string(), Box::new(swap_tool));

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    info!("MCP Server Ready. Waiting for JSON-RPC requests on stdin...");

    while let Some(Ok(line)) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }

        debug!("Received request: {}", line);

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to parse JSON-RPC request: {}", e);
                continue;
            }
        };

        let response = handle_request(&req, &client, &tools).await;

        let response_str = serde_json::to_string(&response)?;
        println!("{}", response_str);
    }

    Ok(())
}

async fn handle_request(
    req: &JsonRpcRequest,
    client: &EthereumClient,
    tools: &HashMap<String, Box<dyn Tool>>,
) -> JsonRpcResponse {
    match req.method.as_str() {
        "tools/list" => {
            let tool_list: Vec<Value> = tools
                .values()
                .map(|t| {
                    json!({
                        "name": t.name(),
                        "description": t.description(),
                        "inputSchema": t.schema()
                    })
                })
                .collect();

            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                result: Some(json!({ "tools": tool_list })),
                error: None,
                id: req.id.clone(),
            }
        }
        "tools/call" => {
            if let Some(params) = &req.params {
                let name = params.get("name").and_then(|v| v.as_str());
                let args = params.get("arguments").cloned().unwrap_or(json!({}));

                if let Some(tool_name) = name {
                    if let Some(tool) = tools.get(tool_name) {
                        match tool.call(client, args).await {
                            Ok(result) => JsonRpcResponse {
                                jsonrpc: "2.0".into(),
                                // Hybrid approach: Standard MCP 'content' for compatibility, plus 'data' for agents.
                                result: Some(json!({
                                    "content": [{
                                        "type": "text",
                                        "text": serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
                                    }],
                                    "data": result
                                })),
                                error: None,
                                id: req.id.clone(),
                            },
                            Err(e) => JsonRpcResponse {
                                jsonrpc: "2.0".into(),
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32603,
                                    message: format!("Tool execution failed: {}", e),
                                    data: None,
                                }),
                                id: req.id.clone(),
                            },
                        }
                    } else {
                        JsonRpcResponse {
                            jsonrpc: "2.0".into(),
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32601,
                                message: format!("Tool not found: {}", tool_name),
                                data: None,
                            }),
                            id: req.id.clone(),
                        }
                    }
                } else {
                    JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: "Missing 'name' parameter".into(),
                            data: None,
                        }),
                        id: req.id.clone(),
                    }
                }
            } else {
                JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Missing params".into(),
                        data: None,
                    }),
                    id: req.id.clone(),
                }
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "Method not found".into(),
                data: None,
            }),
            id: req.id.clone(),
        },
    }
}
