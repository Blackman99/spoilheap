//! `spoilheap mcp`: a read-only MCP server on stdio. Agents can ask how much room is
//! left and what is safe to clear; there is deliberately no tool that deletes.

use crate::dig::{self, Env, Opts, Verdict};
use crate::units;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

fn tools() -> Value {
    json!([
        {
            "name": "disk_status",
            "description": "Free space on the filesystem holding a path (default: home), and whether it is under a floor. Call before large builds, downloads or test runs.",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string", "description": "Any path on the filesystem to check"},
                "floor": {"type": "string", "description": "Minimum free space, e.g. 10GB or 5% (default 10GB)"}
            }},
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "disk_leftovers",
            "description": "What coding agents (Codex, Claude Code, OpenCode, Gemini CLI, Cline) left on disk, each with a SAFE / REVIEW / KEEP verdict and the reason. Read-only: tell the user to run `spoilheap clean` themselves.",
            "inputSchema": {"type": "object", "properties": {
                "min_bytes": {"type": "integer", "description": "Hide items smaller than this (default 1000000)"}
            }},
            "annotations": {"readOnlyHint": true}
        }
    ])
}

fn call(env: &Env, name: &str, args: &Value) -> Result<(String, Value), String> {
    match name {
        "disk_status" => {
            let floor = crate::units::parse_amount(
                args.get("floor").and_then(|v| v.as_str()).unwrap_or("10GB"),
            )?;
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| env.home.clone());
            let s = crate::disk::space(&path)
                .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
            let f = floor.resolve(s.total);
            let ok = s.free >= f;
            let text = format!(
                "{} free of {} on {} — {}",
                units::bytes(s.free),
                units::bytes(s.total),
                env.tilde(&crate::disk::existing_ancestor(&path)),
                if ok {
                    format!("above the {} floor", units::bytes(f))
                } else {
                    format!(
                        "UNDER the {} floor: stop writing and ask the user to free space (spoilheap clean --safe)",
                        units::bytes(f)
                    )
                }
            );
            Ok((
                text,
                json!({"free_bytes": s.free, "total_bytes": s.total, "floor_bytes": f, "ok": ok}),
            ))
        }
        "disk_leftovers" => {
            let min = args
                .get("min_bytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(1_000_000);
            let opts = Opts {
                older: std::time::Duration::from_secs(30 * 86400),
                roots: vec![],
                now: std::time::SystemTime::now(),
                repo_depth: 0,
            };
            let rep = dig::dig(env, &opts);
            crate::out::save_ids(env, &rep);
            let mut lines = vec![];
            let (all, safe, _, _) = crate::render::totals(&rep.items);
            lines.push(format!(
                "Agents left {} in {} places; {} is SAFE to clear.",
                units::bytes(all),
                rep.items.len(),
                units::bytes(safe)
            ));
            for i in rep.items.iter().filter(|i| i.bytes >= min) {
                lines.push(format!(
                    "[{}] {} {} · {} · {} · {}",
                    i.id,
                    i.verdict.word(),
                    i.harness,
                    i.label,
                    units::bytes(i.bytes),
                    i.reason
                ));
            }
            if rep.items.iter().any(|i| i.verdict == Verdict::Safe) {
                lines.push(
                    "The user can clear SAFE items with: spoilheap clean --safe --yes".into(),
                );
            }
            let now = std::time::SystemTime::now();
            Ok((
                lines.join("\n"),
                json!({"items": rep.items.iter().filter(|i| i.bytes >= min).map(|i| crate::out::item(i, now)).collect::<Vec<_>>()}),
            ))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

pub fn serve(env: &Env) -> i32 {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            let _ = writeln!(
                out,
                "{}",
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}})
            );
            continue;
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let Some(id) = id else { continue }; // notifications need no answer
        let resp = match method {
            "initialize" => json!({"jsonrpc": "2.0", "id": id, "result": {
                "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "spoilheap", "version": env!("CARGO_PKG_VERSION")}
            }}),
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "tools/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"tools": tools()}}),
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                let args = req["params"].get("arguments").cloned().unwrap_or(json!({}));
                match call(env, name, &args) {
                    Ok((text, data)) => {
                        json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": text}], "structuredContent": data, "isError": false}})
                    }
                    Err(e) => {
                        json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": e}], "isError": true}})
                    }
                }
            }
            _ => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("method not found: {method}")}})
            }
        };
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
    0
}
