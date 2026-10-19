//! `spoilheap guard`: a PreToolUse hook. Exit 0 silently when there is room; exit 2
//! with the reason on stderr when free space is under the floor. Claude Code and Codex
//! both block the tool call on exit 2 and show the stderr text to the model.

use crate::dig::{self, Env, Opts, Verdict};
use crate::disk;
use crate::units::{self, Amount};
use std::io::{IsTerminal, Read};
use std::path::PathBuf;

pub fn hook_cwd() -> Option<PathBuf> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut s = String::new();
    // hook payloads are small JSON objects written once; never wait on an open pipe forever
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::stdin().take(1 << 20).read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    if let Ok(b) = rx.recv_timeout(std::time::Duration::from_millis(500)) {
        s = b;
    }
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v.get("cwd").and_then(|c| c.as_str()).map(PathBuf::from)
}

pub struct Check {
    pub path: PathBuf,
    pub space: disk::Space,
    pub floor: u64,
}

pub fn checks(env: &Env, extra: &[PathBuf], floor: Amount) -> Vec<Check> {
    let mut paths: Vec<PathBuf> = extra.to_vec();
    paths.push(env.home.clone());
    let mut seen = vec![];
    let mut out = vec![];
    for p in paths {
        let dev = disk::device(&p);
        if dev.is_some() && seen.contains(&dev) {
            continue;
        }
        seen.push(dev);
        if let Ok(s) = disk::space(&p) {
            out.push(Check { path: disk::existing_ancestor(&p), floor: floor.resolve(s.total), space: s });
        }
    }
    out
}

pub fn run(env: &Env, floor: Amount, paths: Vec<PathBuf>, quiet_ok: bool) -> i32 {
    let mut extra = paths;
    if extra.is_empty() {
        if let Some(c) = hook_cwd() {
            extra.push(c);
        } else if let Ok(c) = std::env::current_dir() {
            extra.push(c);
        }
    }
    let cs = checks(env, &extra, floor);
    let low: Vec<&Check> = cs.iter().filter(|c| c.space.free < c.floor).collect();
    if low.is_empty() {
        if !quiet_ok {
            for c in &cs {
                println!("ok  {} has {} free (floor {})", env.tilde(&c.path), units::bytes(c.space.free), units::bytes(c.floor));
            }
        }
        return 0;
    }
    let mut msg = String::new();
    for c in &low {
        msg.push_str(&format!(
            "spoilheap guard: only {} free on {} (floor {}). Stop and tell the user before writing anything else: a full disk corrupts agent databases (anomalyco/opencode#56829) and can stop even cleanup commands from starting (openai/codex#55038).\n",
            units::bytes(c.space.free),
            env.tilde(&c.path),
            floor.label()
        ));
    }
    let opts = Opts { older: std::time::Duration::from_secs(30 * 86400), roots: vec![], roots_given: false, now: std::time::SystemTime::now(), repo_depth: 0 };
    let rep = dig::dig(env, &opts);
    let mut safe: Vec<&dig::Item> = rep.items.iter().filter(|i| i.verdict == Verdict::Safe).collect();
    safe.sort_by(|a, b| b.reclaim.cmp(&a.reclaim));
    let total: u64 = safe.iter().map(|i| i.reclaim).sum();
    if total > 0 {
        msg.push_str(&format!("Agent leftovers that are SAFE to delete ({} in total):\n", units::bytes(total)));
        for i in safe.iter().take(3) {
            msg.push_str(&format!("  [{}] {} · {} · {}\n", i.id, i.label, units::bytes(i.reclaim), env.tilde(&i.path)));
        }
        msg.push_str("Ask the user to run: spoilheap clean --safe --yes\n");
    } else {
        msg.push_str("No agent leftovers are SAFE to delete; run `spoilheap` to see what is REVIEW.\n");
    }
    crate::out::save_ids(env, &rep);
    eprint!("{msg}");
    2
}

pub fn hook_snippet(target: &str, floor: &str) -> Result<String, String> {
    let cmd = format!("spoilheap guard --floor {floor}");
    match target {
        "claude" | "claude-code" => Ok(format!(
            r#"# Claude Code — add to ~/.claude/settings.json (or .claude/settings.json in a repo).
# Exit code 2 blocks the tool call and shows spoilheap's message to Claude.
{{
  "hooks": {{
    "PreToolUse": [
      {{
        "matcher": "Bash|Write|Edit|MultiEdit|NotebookEdit",
        "hooks": [{{ "type": "command", "command": "{cmd}", "timeout": 30 }}]
      }}
    ]
  }}
}}
"#
        )),
        "codex" => Ok(format!(
            r#"# Codex — add to ~/.codex/config.toml.
# Exit code 2 with a reason on stderr blocks the tool call (codex-rs/hooks/src/events/pre_tool_use.rs).
[features]
hooks = true

[[hooks.PreToolUse]]

[[hooks.PreToolUse.hooks]]
type = "command"
command = "{cmd}"
statusMessage = "checking free disk space"
"#
        )),
        other => Err(format!("unknown hook target '{other}'; use `spoilheap hook claude` or `spoilheap hook codex`")),
    }
}
