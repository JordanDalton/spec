use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use notify::{EventKind, RecursiveMode, Watcher};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn path_to_uri(path: &Path) -> Option<String> {
    let path_str = path.to_string_lossy();
    // Normalise separators to '/'
    let normalised = path_str.replace('\\', "/");

    // Strip leading ".spec/sessions/" (with or without leading "./" or "/")
    let prefix = ".spec/sessions/";
    let stripped = if let Some(pos) = normalised.find(prefix) {
        &normalised[pos + prefix.len()..]
    } else {
        return None;
    };

    // Must end with ".json" and not be a tmp/lock file
    if !stripped.ends_with(".json")
        || stripped.ends_with(".tmp")
        || stripped.ends_with(".lock")
    {
        return None;
    }

    let without_ext = &stripped[..stripped.len() - ".json".len()];
    Some(format!("spec://session/{}", without_ext))
}

fn uri_to_session_path(uri: &str) -> Option<String> {
    let prefix = "spec://session/";
    if !uri.starts_with(prefix) {
        return None;
    }
    let rel = &uri[prefix.len()..];
    Some(format!(".spec/sessions/{}.json", rel))
}

// ---------------------------------------------------------------------------
// run_spec
// ---------------------------------------------------------------------------

async fn run_spec(args: &[&str], envs: &[(&str, &str)]) -> (String, bool) {
    let mut cmd = Command::new("spec");
    cmd.args(args);
    // spec-mcp runs inside an AI tool (Claude Code, Codex, etc.).
    // Default to claudecode so it works without any API key configuration.
    // Users can override by setting SPEC_PROVIDER in the MCP server's env block.
    if std::env::var("SPEC_PROVIDER").is_err() {
        cmd.env("SPEC_PROVIDER", "claudecode");
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    match cmd.output().await {
        Ok(output) => {
            let mut combined = String::new();
            combined.push_str(&String::from_utf8_lossy(&output.stdout));
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
            (combined, output.status.success())
        }
        Err(e) => (format!("Failed to run spec: {}", e), false),
    }
}

// ---------------------------------------------------------------------------
// Notification helper
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn send_notification(tx: &mpsc::Sender<String>, method: &str, params: Value) {
    let msg = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    let line = format!("{}\n", msg);
    // Non-blocking best-effort for sync contexts; use try_send
    let _ = tx.try_send(line);
}

// ---------------------------------------------------------------------------
// Handler implementations
// ---------------------------------------------------------------------------

async fn handle_initialize(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
            "serverInfo": {"name": "spec-mcp", "version": "0.1.0"},
            "capabilities": {
                "tools": {},
                "resources": {"subscribe": true}
            }
        }
    })
}

async fn handle_ping(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {}
    })
}

async fn handle_tools_list(id: &Value) -> Value {
    let file_desc = "The actual source file path relative to the project root (e.g. 'app/Http/Controllers/HomeController.php'). This must be a real file path, not a description or a .md filename.";
    let agent_id_desc = "Your stable agent identity — a short lowercase name like 'alice' or 'claude-1'. Must be identical across all commands you run in this session (propose, respond, concede, agree). Do NOT use a model name, version string, or auto-generated value.";

    let tools = json!([
        {
            "name": "spec_state",
            "description": "Get machine-readable session status (no LLM call, instant). Returns exactly one line: STATUS: WAITING_FOR_REPLY | WAITING_FOR_AGREE | STUCK | LOCKED | NO_SESSION. STUCK means ≥3 rounds with no agreement — the mediator should intervene. Call this to decide your next move without burning tokens.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": file_desc}
                },
                "required": ["file"]
            }
        },
        {
            "name": "spec_log",
            "description": "Full session message history for a spec file — all proposals, responses, concessions, and agreements with timestamps and reasoning.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": file_desc}
                },
                "required": ["file"]
            }
        },
        {
            "name": "spec_status",
            "description": "Overall project status: all open and locked sessions, spec files found, agents involved, lesson count.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        },
        {
            "name": "spec_propose",
            "description": "Submit a spec proposal for a source file. Reads the file, queries past lessons, generates a concrete proposal, and records it in the session. After calling this, poll with spec_state until STATUS changes from WAITING_FOR_REPLY.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": file_desc},
                    "intent": {"type": "string", "description": "What change to make and why, in plain language. Be specific about behavior, not implementation."},
                    "knowledge": {"type": "string", "description": "Optional. Explicit assumptions, constraints, or prior knowledge to anchor the proposal. If omitted, the LLM infers from the source file."},
                    "agent_id": {"type": "string", "description": agent_id_desc}
                },
                "required": ["file", "intent", "agent_id"]
            }
        },
        {
            "name": "spec_respond",
            "description": "Respond to the latest proposal in the session. Reads the full session history and takes a stance (ACCEPT / REJECT / MODIFY). Use this after another agent has proposed and you need to review their proposal.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": file_desc},
                    "agent_id": {"type": "string", "description": agent_id_desc}
                },
                "required": ["file", "agent_id"]
            }
        },
        {
            "name": "spec_concede",
            "description": "Update or withdraw your position after reviewing the other agent's response. Use this when the other agent has raised valid points and you want to revise your stance before agreeing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": file_desc},
                    "agent_id": {"type": "string", "description": agent_id_desc}
                },
                "required": ["file", "agent_id"]
            }
        },
        {
            "name": "spec_agree",
            "description": "Sign off on the current spec. When all agents agree, the session locks and the spec file is written. Use solo=true to lock immediately without requiring a second agent (single-agent workflow).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": file_desc},
                    "agent_id": {"type": "string", "description": agent_id_desc},
                    "solo": {"type": "boolean", "description": "Set to true to lock without requiring a second agent. Use only when no second agent is available."}
                },
                "required": ["file", "agent_id"]
            }
        },
        {
            "name": "spec_clarify",
            "description": "Mediator tool: surface a semantic contradiction between competing proposals without taking a position. Call this when two agents are stuck and talking past each other.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": file_desc}
                },
                "required": ["file"]
            }
        },
        {
            "name": "spec_reframe",
            "description": "Mediator tool: find common ground between stuck agents and suggest a path to resolution. Call this after spec_clarify has surfaced the contradiction.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": file_desc}
                },
                "required": ["file"]
            }
        },
        {
            "name": "spec_build",
            "description": "Implementer tool: write code from the agreed and locked spec. Errors if consensus has not been reached. Only call this after STATUS is LOCKED.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": file_desc}
                },
                "required": ["file"]
            }
        }
    ]);

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"tools": tools}
    })
}

async fn handle_tools_call(id: &Value, params: &Value) -> Value {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32602, "message": "Missing tool name"}
            });
        }
    };

    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let get_str = |key: &str| -> Option<String> {
        args.get(key).and_then(|v| v.as_str()).map(|s| s.to_owned())
    };

    let (output, success) = match tool_name {
        "spec_state" => {
            let file = match get_str("file") {
                Some(f) => f,
                None => return invalid_params(id, "Missing required param: file"),
            };
            run_spec(&["state", &file], &[]).await
        }
        "spec_log" => {
            let file = match get_str("file") {
                Some(f) => f,
                None => return invalid_params(id, "Missing required param: file"),
            };
            run_spec(&["log", &file], &[]).await
        }
        "spec_status" => run_spec(&["status"], &[]).await,
        "spec_propose" => {
            let file = match get_str("file") {
                Some(f) => f,
                None => return invalid_params(id, "Missing required param: file"),
            };
            let intent = match get_str("intent") {
                Some(i) => i,
                None => return invalid_params(id, "Missing required param: intent"),
            };
            let agent_id = match get_str("agent_id") {
                Some(a) => a,
                None => return invalid_params(id, "Missing required param: agent_id"),
            };
            let knowledge = get_str("knowledge");

            if let Some(ref k) = knowledge {
                run_spec(
                    &["propose", &file, &intent, "--knowledge", k],
                    &[("SPEC_AGENT_ID", &agent_id)],
                )
                .await
            } else {
                run_spec(
                    &["propose", &file, &intent],
                    &[("SPEC_AGENT_ID", &agent_id)],
                )
                .await
            }
        }
        "spec_respond" => {
            let file = match get_str("file") {
                Some(f) => f,
                None => return invalid_params(id, "Missing required param: file"),
            };
            let agent_id = match get_str("agent_id") {
                Some(a) => a,
                None => return invalid_params(id, "Missing required param: agent_id"),
            };
            run_spec(&["respond", &file], &[("SPEC_AGENT_ID", &agent_id)]).await
        }
        "spec_concede" => {
            let file = match get_str("file") {
                Some(f) => f,
                None => return invalid_params(id, "Missing required param: file"),
            };
            let agent_id = match get_str("agent_id") {
                Some(a) => a,
                None => return invalid_params(id, "Missing required param: agent_id"),
            };
            run_spec(&["concede", &file], &[("SPEC_AGENT_ID", &agent_id)]).await
        }
        "spec_agree" => {
            let file = match get_str("file") {
                Some(f) => f,
                None => return invalid_params(id, "Missing required param: file"),
            };
            let agent_id = match get_str("agent_id") {
                Some(a) => a,
                None => return invalid_params(id, "Missing required param: agent_id"),
            };
            let solo = args.get("solo").and_then(|v| v.as_bool()).unwrap_or(false);
            if solo {
                run_spec(&["agree", &file, "--solo"], &[("SPEC_AGENT_ID", &agent_id)]).await
            } else {
                run_spec(&["agree", &file], &[("SPEC_AGENT_ID", &agent_id)]).await
            }
        }
        "spec_clarify" => {
            let file = match get_str("file") {
                Some(f) => f,
                None => return invalid_params(id, "Missing required param: file"),
            };
            run_spec(&["clarify", &file], &[]).await
        }
        "spec_reframe" => {
            let file = match get_str("file") {
                Some(f) => f,
                None => return invalid_params(id, "Missing required param: file"),
            };
            run_spec(&["reframe", &file], &[]).await
        }
        "spec_build" => {
            let file = match get_str("file") {
                Some(f) => f,
                None => return invalid_params(id, "Missing required param: file"),
            };
            run_spec(&["build", &file], &[]).await
        }
        _ => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("Unknown tool: {}", tool_name)}
            });
        }
    };

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{"type": "text", "text": output}],
            "isError": !success
        }
    })
}

async fn handle_resources_list(id: &Value) -> Value {
    let mut resources = vec![json!({
        "uri": "spec://status",
        "name": "Project Status",
        "mimeType": "text/plain"
    })];

    let sessions_dir = Path::new(".spec/sessions");
    if sessions_dir.exists() {
        collect_session_resources(sessions_dir, &mut resources);
    }

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"resources": resources}
    })
}

fn collect_session_resources(dir: &Path, resources: &mut Vec<Value>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_session_resources(&path, resources);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".tmp") || name.ends_with(".lock") {
                continue;
            }
            if name.ends_with(".json") {
                if let Some(uri) = path_to_uri(&path) {
                    let display = uri
                        .strip_prefix("spec://session/")
                        .unwrap_or(&uri)
                        .to_owned();
                    resources.push(json!({
                        "uri": uri,
                        "name": display,
                        "mimeType": "application/json"
                    }));
                }
            }
        }
    }
}

async fn handle_resources_read(id: &Value, params: &Value) -> Value {
    let uri = match params.get("uri").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return invalid_params(id, "Missing required param: uri"),
    };

    if uri == "spec://status" {
        let (text, _) = run_spec(&["status"], &[]).await;
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "contents": [{"uri": uri, "text": text, "mimeType": "text/plain"}]
            }
        });
    }

    match uri_to_session_path(uri) {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(content) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "contents": [{"uri": uri, "text": content, "mimeType": "application/json"}]
                }
            }),
            Err(e) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32603, "message": format!("Failed to read file: {}", e)}
            }),
        },
        None => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32602, "message": format!("Unrecognised URI: {}", uri)}
        }),
    }
}

async fn handle_resources_subscribe(
    id: &Value,
    params: &Value,
    subscriptions: Arc<Mutex<HashSet<String>>>,
) -> Value {
    let uri = match params.get("uri").and_then(|v| v.as_str()) {
        Some(u) => u.to_owned(),
        None => return invalid_params(id, "Missing required param: uri"),
    };
    subscriptions.lock().unwrap().insert(uri);
    json!({"jsonrpc": "2.0", "id": id, "result": {}})
}

async fn handle_resources_unsubscribe(
    id: &Value,
    params: &Value,
    subscriptions: Arc<Mutex<HashSet<String>>>,
) -> Value {
    let uri = match params.get("uri").and_then(|v| v.as_str()) {
        Some(u) => u.to_owned(),
        None => return invalid_params(id, "Missing required param: uri"),
    };
    subscriptions.lock().unwrap().remove(&uri);
    json!({"jsonrpc": "2.0", "id": id, "result": {}})
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn invalid_params(id: &Value, msg: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": -32602, "message": msg}
    })
}

fn method_not_found(id: &Value, method: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": -32601, "message": format!("Method not found: {}", method)}
    })
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // 1. mpsc channel — all output goes through here
    let (tx, mut rx) = mpsc::channel::<String>(64);

    // 2. Writer task: drain channel → stdout
    tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(line) = rx.recv().await {
            if let Err(e) = stdout.write_all(line.as_bytes()).await {
                eprintln!("[spec-mcp] stdout write error: {}", e);
                break;
            }
            let _ = stdout.flush().await;
        }
    });

    // 3. Subscriptions
    let subscriptions: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    // 4. File watcher
    let sessions_dir = Path::new(".spec/sessions");
    let _watcher: Option<Box<dyn Watcher + Send>> = if sessions_dir.exists() {
        let tx_watch = tx.clone();
        let subs_watch = Arc::clone(&subscriptions);

        let mut watcher = match notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    let interesting = matches!(
                        event.kind,
                        EventKind::Create(_) | EventKind::Modify(_)
                    );
                    if !interesting {
                        return;
                    }
                    for path in &event.paths {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.ends_with(".tmp") || name.ends_with(".lock") {
                                continue;
                            }
                            if !name.ends_with(".json") {
                                continue;
                            }
                        } else {
                            continue;
                        }

                        if let Some(uri) = path_to_uri(path) {
                            let subscribed = subs_watch.lock().unwrap().contains(&uri);
                            if subscribed {
                                let msg = format!(
                                    "{}\n",
                                    json!({
                                        "jsonrpc": "2.0",
                                        "method": "notifications/resources/updated",
                                        "params": {"uri": uri}
                                    })
                                );
                                let _ = tx_watch.blocking_send(msg);
                            }
                        }
                    }
                }
                Err(e) => eprintln!("[spec-mcp] watcher error: {}", e),
            },
        ) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[spec-mcp] failed to create watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(sessions_dir, RecursiveMode::Recursive) {
            eprintln!("[spec-mcp] failed to watch .spec/sessions/: {}", e);
        } else {
            eprintln!("[spec-mcp] watching .spec/sessions/ for changes");
        }

        Some(Box::new(watcher))
    } else {
        eprintln!("[spec-mcp] warning: .spec/sessions/ not found, file watcher disabled");
        None
    };

    // 5. Stdin loop
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_owned();
        if line.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[spec-mcp] malformed JSON: {} — {:?}", e, line);
                continue;
            }
        };

        let method = match msg.get("method").and_then(|v| v.as_str()) {
            Some(m) => m.to_owned(),
            None => {
                eprintln!("[spec-mcp] message has no method, skipping");
                continue;
            }
        };

        // Notifications from client have no "id" — don't respond
        let id_opt = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(json!({}));

        let response = match method.as_str() {
            "initialize" => {
                if let Some(ref id) = id_opt {
                    Some(handle_initialize(id).await)
                } else {
                    None
                }
            }
            "ping" => {
                if let Some(ref id) = id_opt {
                    Some(handle_ping(id).await)
                } else {
                    None
                }
            }
            "tools/list" => {
                if let Some(ref id) = id_opt {
                    Some(handle_tools_list(id).await)
                } else {
                    None
                }
            }
            "tools/call" => {
                if let Some(ref id) = id_opt {
                    Some(handle_tools_call(id, &params).await)
                } else {
                    None
                }
            }
            "resources/list" => {
                if let Some(ref id) = id_opt {
                    Some(handle_resources_list(id).await)
                } else {
                    None
                }
            }
            "resources/read" => {
                if let Some(ref id) = id_opt {
                    Some(handle_resources_read(id, &params).await)
                } else {
                    None
                }
            }
            "resources/subscribe" => {
                if let Some(ref id) = id_opt {
                    Some(
                        handle_resources_subscribe(id, &params, Arc::clone(&subscriptions))
                            .await,
                    )
                } else {
                    None
                }
            }
            "resources/unsubscribe" => {
                if let Some(ref id) = id_opt {
                    Some(
                        handle_resources_unsubscribe(id, &params, Arc::clone(&subscriptions))
                            .await,
                    )
                } else {
                    None
                }
            }
            // Client notifications — acknowledge but don't respond
            "notifications/initialized" | "notifications/cancelled" => None,
            _ => {
                if let Some(ref id) = id_opt {
                    Some(method_not_found(id, &method))
                } else {
                    None
                }
            }
        };

        if let Some(resp) = response {
            let line_out = format!("{}\n", resp);
            if let Err(e) = tx.send(line_out).await {
                eprintln!("[spec-mcp] channel send error: {}", e);
                break;
            }
        }
    }

    eprintln!("[spec-mcp] stdin closed, exiting");
}
