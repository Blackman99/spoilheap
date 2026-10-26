//! `spoilheap procs`: agent helper processes grouped by the agent that runs them,
//! orphans marked, each with a SAFE / REVIEW / KEEP verdict. It only reads: nothing
//! here sends a signal. SAFE items come with the command to stop them yourself.
//!
//! `classify` is a pure function over a `ptable::Snapshot`; tests feed it fixtures
//! from tests/fixtures/procs.

use crate::dig::{Env, Verdict};
use crate::ptable::{P, Snapshot, base};
use crate::render::{Style, clip};
use crate::units;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const AGENTS: &[&str] = &["Claude Code", "Codex", "OpenCode", "Gemini CLI", "Cline"];

/// Programs that run a script named in the next argument.
const RUNNERS: &[&str] = &[
    "node", "bun", "deno", "npx", "bunx", "pnpm", "python", "python3", "uv", "uvx", "pipx",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The agent process itself.
    Agent,
    /// Runs under a live agent process.
    Helper,
    /// Started by an agent, now without its parent.
    Orphan,
}

impl Role {
    pub fn word(self) -> &'static str {
        match self {
            Role::Agent => "agent",
            Role::Helper => "helper",
            Role::Orphan => "orphan",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub pid: u32,
    pub ppid: u32,
    pub agent: &'static str,
    pub role: Role,
    /// "snapshot shell", "MCP server", "app server", "app helper", "helper", "command", "agent"
    pub kind: &'static str,
    pub label: String,
    /// The agent process this runs under (helpers), or its own pid (agents).
    pub root: Option<u32>,
    /// Why it counts as an orphan ("PPID 1", "parent 4242 is gone", ...).
    pub orphan: Option<String>,
    pub cwd: Option<PathBuf>,
    pub rss: u64,
    pub cpu: f64,
    pub age_s: Option<u64>,
    /// Processes below an orphan, stopped with it in the usual case.
    pub children: Vec<u32>,
    pub tree_rss: u64,
    /// How many helpers under the same agent run this exact command (1 = unique).
    pub copies: usize,
    pub command: String,
    pub verdict: Verdict,
    pub reason: String,
    /// What to run to stop it (SAFE and REVIEW orphans). spoilheap never runs it.
    pub suggest: Option<String>,
}

#[derive(Debug, Default)]
pub struct Listing {
    /// Agents first, each followed by its helpers (biggest first), then orphans;
    /// grouped in AGENTS order.
    pub entries: Vec<Entry>,
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// argv[0] base name, and the script base name when argv[0] is a runner like node.
fn program(p: &P) -> (String, Option<String>) {
    let a0 = p.argv.first().map(|a| lower(base(a))).unwrap_or_default();
    let a0 = if a0.is_empty() { lower(&p.name) } else { a0 };
    let a0 = a0.trim_end_matches(".exe").to_string();
    let script = if RUNNERS
        .iter()
        .any(|r| a0 == *r || a0.starts_with("python3."))
    {
        p.argv
            .iter()
            .skip(1)
            .find(|a| !a.starts_with('-'))
            .map(|a| lower(base(a)))
    } else {
        None
    };
    (a0, script)
}

fn joined(p: &P) -> String {
    let mut s = p.argv.join(" ");
    if !p.exe.is_empty() {
        s.push(' ');
        s.push_str(&p.exe);
    }
    s.replace('\\', "/")
}

/// The agent this process *is*, from its executable and arguments only.
pub fn agent_of(p: &P) -> Option<&'static str> {
    let (a0, script) = program(p);
    let name = lower(&p.name);
    let all = joined(p);
    let is = |n: &str| a0 == n || name == n || script.as_deref() == Some(n);
    let sub = p.argv.get(1).map(|s| s.as_str()).unwrap_or("");
    if is("claude") || all.contains("@anthropic-ai/claude-code/") {
        if sub == "mcp" {
            return None; // `claude mcp serve` is a server some other client started
        }
        return Some("Claude Code");
    }
    // Codex desktop (macOS) is Codex.app; its helpers live inside the bundle
    if all.contains("/Codex.app/Contents/MacOS/Codex") && !all.contains("Helper") {
        return Some("Codex");
    }
    if (is("codex") || all.contains("@openai/codex/bin/codex"))
        && !matches!(sub, "app-server" | "mcp-server")
    {
        return Some("Codex");
    }
    if is("opencode") || all.contains("opencode-ai/bin/opencode") {
        return Some("OpenCode");
    }
    if is("gemini") || all.contains("@google/gemini-cli/") {
        return Some("Gemini CLI");
    }
    if is("cline") {
        return Some("Cline");
    }
    None
}

/// Agent helpers recognisable on their own, even with the agent gone.
pub fn helper_of(p: &P) -> Option<(&'static str, &'static str)> {
    let (a0, _) = program(p);
    let all = joined(p);
    let sub = p.argv.get(1).map(|s| s.as_str()).unwrap_or("");
    // Claude Code runs every Bash command in a shell that sources a snapshot of
    // your shell config: zsh -c -l "source ~/.claude/shell-snapshots/snapshot-zsh-….sh && eval …"
    // (anthropics/claude-code#103313: these can outlive Claude at PPID 1)
    if all.contains("shell-snapshots/snapshot-") {
        return Some(("Claude Code", "snapshot shell"));
    }
    if a0 == "codex" && sub == "app-server" {
        return Some(("Codex", "app server"));
    }
    if a0 == "codex" && sub == "mcp-server" {
        return Some(("Codex", "MCP server"));
    }
    if all.contains("/Codex.app/Contents/") {
        return Some(("Codex", "app helper"));
    }
    // Cline's own processes: the VS Code extension folder and the CLI's core/host
    if all.contains("saoudrizwan.claude-dev") || ["cline-core", "cline-host"].contains(&a0.as_str())
    {
        return Some(("Cline", "helper"));
    }
    None
}

/// The agent whose marker variable is in this process's environment.
pub fn env_agent(p: &P) -> Option<&'static str> {
    for e in &p.env {
        let a = match e.as_str() {
            "CLAUDECODE" => "Claude Code",
            "CODEX_THREAD_ID" | "CODEX_CI" | "CODEX_SANDBOX_NETWORK_DISABLED" => "Codex",
            "OPENCODE" | "OPENCODE_PID" => "OpenCode",
            "GEMINI_CLI" => "Gemini CLI",
            _ => continue,
        };
        return Some(a);
    }
    None
}

fn is_mcp(p: &P) -> bool {
    p.argv.iter().any(|a| {
        let l = lower(a);
        l.contains("@modelcontextprotocol/") || lower(base(&l)).contains("mcp")
    })
}

/// Words in argv that look like secrets are replaced before anything is shown.
pub fn redact(argv: &[String]) -> Vec<String> {
    let secret = |k: &str| {
        let k = lower(k);
        ["key", "token", "secret", "password", "passwd", "auth"]
            .iter()
            .any(|w| k.contains(w))
    };
    let mut out = vec![];
    let mut hide_next = false;
    for a in argv {
        if hide_next {
            out.push("***".to_string());
            hide_next = false;
            continue;
        }
        if let Some((k, _)) = a.split_once('=') {
            if secret(k) {
                out.push(format!("{k}=***"));
                continue;
            }
        }
        if a.starts_with('-') && secret(a) {
            hide_next = true;
        }
        out.push(a.clone());
    }
    out
}

fn label(p: &P, kind: &str) -> String {
    let (a0, script) = program(p);
    let shown = |s: &str| s.trim_end_matches(".exe").to_string();
    match kind {
        "snapshot shell" => {
            // the command Claude ran in it: … && eval 'npm test' \< /dev/null && …
            let all = p.argv.join(" ");
            match all
                .split_once("eval '")
                .and_then(|(_, r)| r.split_once('\''))
                .map(|(c, _)| c.trim())
                .filter(|c| !c.is_empty())
            {
                Some(c) => format!("snapshot shell ({a0}): {c}"),
                None => format!("snapshot shell ({a0})"),
            }
        }
        "MCP server" => {
            let words: Vec<String> = p
                .argv
                .iter()
                .skip(1)
                .map(|a| a.replace('\\', "/"))
                .collect();
            let name = words
                .iter()
                .find_map(|a| {
                    a.find("@modelcontextprotocol/")
                        .map(|i| a[i + "@modelcontextprotocol/".len()..].to_string())
                })
                .or_else(|| {
                    let cands: Vec<&String> = words
                        .iter()
                        .filter(|a| !a.starts_with('-') && lower(a).contains("mcp"))
                        .collect();
                    cands
                        .iter()
                        .find(|a| a.contains('/') || a.contains('.'))
                        .or(cands.first())
                        .map(|a| {
                            // keep npm scopes (@playwright/mcp), drop folders
                            if a.starts_with('@') && a.matches('/').count() == 1 {
                                a.to_string()
                            } else {
                                base(a).to_string()
                            }
                        })
                })
                .unwrap_or(a0.clone());
            let name = match name.rfind('@') {
                Some(i) if i > 0 => name[..i].to_string(),
                _ => name,
            };
            if RUNNERS.contains(&a0.as_str()) && a0 != "node" {
                format!("MCP server {name} (via {a0})")
            } else {
                format!("MCP server {name}")
            }
        }
        "app server" => {
            let words: Vec<&str> = p
                .argv
                .iter()
                .skip(1)
                .take_while(|a| !a.starts_with('-'))
                .map(|a| a.as_str())
                .collect();
            format!("{a0} {}", words.join(" "))
        }
        "app helper" => {
            if p.exe.is_empty() {
                p.name.clone()
            } else {
                base(&p.exe).to_string()
            }
        }
        "agent" => match script {
            Some(sc) => sc
                .trim_end_matches(".js")
                .trim_end_matches(".mjs")
                .to_string(),
            None => shown(&a0),
        },
        _ => {
            let mut s = shown(&a0);
            if let Some(sc) = script {
                s = format!("{s} {sc}");
            } else if let Some(arg) = p.argv.iter().skip(1).find(|a| !a.starts_with('-')) {
                if arg.len() <= 24 && !arg.contains(' ') {
                    s = format!("{s} {}", base(arg));
                }
            }
            s
        }
    }
}

/// Processes that adopt orphans: init systems and container init shims.
const REAPERS: &[&str] = &[
    "systemd",
    "launchd",
    "init",
    "tini",
    "dumb-init",
    "catatonit",
    "s6-svscan",
    "runit",
];

/// Codex's shared app-server, which detaches on purpose (`--managed-daemon`) and
/// so never has a Codex parent, even while one uses it.
fn codex_daemon(p: &P) -> bool {
    let (a0, _) = program(p);
    a0 == "codex"
        && p.argv.get(1).is_some_and(|s| s == "app-server")
        && (p.argv.iter().any(|a| a == "--managed-daemon")
            || p.argv.get(2).is_some_and(|s| s == "daemon"))
}

/// Orphan check for something no live agent is above: (is it orphaned, why).
fn orphaned(p: &P, by_pid: &HashMap<u32, &P>) -> Option<String> {
    if p.ppid <= 1 {
        return Some("PPID 1".into());
    }
    match by_pid.get(&p.ppid) {
        None => Some(format!("parent {} is gone", p.ppid)),
        // orphans are re-parented to the nearest subreaper, often a user systemd
        Some(pp) if REAPERS.contains(&pp.name.as_str()) => {
            Some(format!("adopted by {}[{}]", pp.name, pp.pid))
        }
        Some(_) if codex_daemon(p) => Some("detached on purpose".into()),
        Some(_) => None,
    }
}

fn deleted(p: &Path) -> bool {
    p.to_string_lossy().ends_with(" (deleted)")
}

fn in_worktree<'a>(path: &Path, wts: &'a [PathBuf]) -> Option<&'a PathBuf> {
    if deleted(path) {
        return None;
    }
    wts.iter()
        .filter(|w| path.starts_with(w))
        .max_by_key(|w| w.components().count())
}

/// Pids whose working folder and open files the classification needs: agents
/// and every process in an orphan's tree. The live collector fills only these.
pub fn needs_files(snap: &Snapshot) -> Vec<u32> {
    let (roots, orphans, by_parent) = shape(snap);
    let mut v: Vec<u32> = roots.iter().map(|(p, _)| *p).collect();
    for (top, _, _) in &orphans {
        v.push(*top);
        v.extend(descendants(*top, &by_parent));
    }
    v.sort();
    v.dedup();
    v
}

type Shape = (
    Vec<(u32, &'static str)>,
    Vec<(u32, &'static str, &'static str)>,
    HashMap<u32, Vec<u32>>,
);

fn descendants(pid: u32, by_parent: &HashMap<u32, Vec<u32>>) -> Vec<u32> {
    let mut out = vec![];
    let mut stack = vec![pid];
    while let Some(c) = stack.pop() {
        for k in by_parent.get(&c).into_iter().flatten() {
            if !out.contains(k) && *k != pid {
                out.push(*k);
                stack.push(*k);
            }
        }
    }
    out
}

/// Agent roots, orphan tops (pid, agent, kind), and the child map.
fn shape(snap: &Snapshot) -> Shape {
    let by_pid: HashMap<u32, &P> = snap.procs.iter().map(|p| (p.pid, p)).collect();
    let mut by_parent: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in &snap.procs {
        if p.ppid != p.pid {
            by_parent.entry(p.ppid).or_default().push(p.pid);
        }
    }
    // nearest ancestor that is an agent process (not counting the process itself)
    let ancestor_agent = |p: &P| -> Option<(u32, &'static str)> {
        let mut cur = p.ppid;
        for _ in 0..64 {
            let q = by_pid.get(&cur)?;
            if let Some(a) = agent_of(q) {
                return Some((q.pid, a));
            }
            if q.ppid == q.pid || q.ppid == 0 {
                return None;
            }
            cur = q.ppid;
        }
        None
    };
    let mut roots = vec![];
    for p in &snap.procs {
        if let Some(a) = agent_of(p) {
            // the npm wrapper of Codex starts the real codex; Gemini CLI relaunches
            // itself: same agent below the same agent is a helper, not a second agent
            let under_same = {
                let mut cur = p.ppid;
                let mut found = false;
                for _ in 0..64 {
                    let Some(q) = by_pid.get(&cur) else { break };
                    if agent_of(q) == Some(a) {
                        found = true;
                        break;
                    }
                    if agent_of(q).is_some() || q.ppid == q.pid {
                        break;
                    }
                    cur = q.ppid;
                }
                found
            };
            if !under_same {
                roots.push((p.pid, a));
            }
        }
    }
    let mut orphans = vec![];
    for p in &snap.procs {
        if agent_of(p).is_some() || ancestor_agent(p).is_some() {
            continue;
        }
        let (agent, kind) = match (helper_of(p), env_agent(p)) {
            (Some((a, k)), _) => (a, k),
            (None, Some(a)) if is_mcp(p) => (a, "MCP server"),
            (None, Some(a)) => (a, "command"),
            (None, None) => continue,
        };
        if orphaned(p, &by_pid).is_some() {
            orphans.push((p.pid, agent, kind));
        }
    }
    (roots, orphans, by_parent)
}

pub fn classify(snap: &Snapshot) -> Listing {
    let by_pid: HashMap<u32, &P> = snap.procs.iter().map(|p| (p.pid, p)).collect();
    let (roots, orphans, by_parent) = shape(snap);
    let root_pids: Vec<u32> = roots.iter().map(|r| r.0).collect();
    let mut entries = vec![];
    let cmd = |p: &P| redact(&p.argv).join(" ");
    for agent in AGENTS {
        for &(rpid, _) in roots.iter().filter(|r| r.1 == *agent) {
            let rp = by_pid[&rpid];
            // helpers: everything below, down to (not into) another agent
            let mut helpers: Vec<&P> = vec![];
            let mut stack = vec![rpid];
            while let Some(c) = stack.pop() {
                for k in by_parent.get(&c).into_iter().flatten() {
                    if root_pids.contains(k) || helpers.iter().any(|h| h.pid == *k) {
                        continue;
                    }
                    helpers.push(by_pid[k]);
                    stack.push(*k);
                }
            }
            let orphan = orphaned(rp, &by_pid);
            let all = joined(rp);
            let service = all.contains("/Codex.app/")
                || rp.argv.iter().skip(1).any(|a| {
                    ["serve", "service", "daemon", "app-server", "--daemon"].contains(&a.as_str())
                });
            let (verdict, reason) = match (&orphan, service) {
                (Some(why), false) => (
                    Verdict::Review,
                    format!(
                        "{agent} itself, but nothing started it any more ({why}): a session whose terminal closed. Stop it if no session should be running there"
                    ),
                ),
                _ => (
                    Verdict::Keep,
                    match helpers.len() {
                        0 => format!("{agent} itself, running"),
                        1 => format!("{agent} itself, running with 1 helper process"),
                        n => format!("{agent} itself, running with {n} helper processes"),
                    },
                ),
            };
            entries.push(Entry {
                pid: rpid,
                ppid: rp.ppid,
                agent,
                role: Role::Agent,
                kind: "agent",
                label: label(rp, "agent"),
                root: Some(rpid),
                orphan,
                cwd: rp.cwd.clone(),
                rss: rp.rss,
                cpu: rp.cpu,
                age_s: rp.age_s,
                children: vec![],
                tree_rss: rp.rss,
                copies: 1,
                command: cmd(rp),
                suggest: (verdict == Verdict::Review).then(|| format!("kill -TERM {rpid}")),
                verdict,
                reason,
            });
            let mut counts: HashMap<String, usize> = HashMap::new();
            for h in &helpers {
                *counts.entry(h.argv.join("\u{0}")).or_default() += 1;
            }
            helpers.sort_by(|a, b| b.rss.cmp(&a.rss).then(a.pid.cmp(&b.pid)));
            for h in helpers {
                let kind = helper_of(h)
                    .map(|x| x.1)
                    .or_else(|| is_mcp(h).then_some("MCP server"))
                    .unwrap_or("helper");
                let copies = counts[&h.argv.join("\u{0}")];
                let parent = by_pid
                    .get(&h.ppid)
                    .map(|q| format!("{}[{}]", q.name, q.pid))
                    .unwrap_or_default();
                let mut reason = format!(
                    "runs under {}[{rpid}]{}, which is running",
                    label(rp, "agent"),
                    if h.ppid == rpid {
                        String::new()
                    } else {
                        format!(" (via {parent})")
                    }
                );
                if copies > 1 && !h.argv.is_empty() {
                    reason.push_str(&format!(
                        "; {copies} copies of this command run under it (seen after restarts: openai/codex#55141)"
                    ));
                }
                entries.push(Entry {
                    pid: h.pid,
                    ppid: h.ppid,
                    agent,
                    role: Role::Helper,
                    kind,
                    label: label(h, kind),
                    root: Some(rpid),
                    orphan: None,
                    cwd: h.cwd.clone(),
                    rss: h.rss,
                    cpu: h.cpu,
                    age_s: h.age_s,
                    children: vec![],
                    tree_rss: h.rss,
                    copies,
                    command: cmd(h),
                    verdict: Verdict::Keep,
                    reason,
                    suggest: None,
                });
            }
        }
        let mut mine: Vec<&(u32, &'static str, &'static str)> =
            orphans.iter().filter(|o| o.1 == *agent).collect();
        mine.sort_by_key(|o| std::cmp::Reverse(by_pid[&o.0].rss));
        for &&(pid, _, kind) in &mine {
            let p = by_pid[&pid];
            let why = orphaned(p, &by_pid).unwrap_or_default();
            let kids = descendants(pid, &by_parent);
            let tree: Vec<&P> = std::iter::once(p)
                .chain(kids.iter().filter_map(|k| by_pid.get(k).copied()))
                .collect();
            let tree_rss = tree.iter().map(|q| q.rss).sum();
            let rel = |f: &Path, w: &Path| {
                clip(&f.strip_prefix(w).unwrap_or(f).display().to_string(), 48)
            };
            let hit = tree.iter().find_map(|q| {
                if let Some(w) = q
                    .cwd
                    .as_deref()
                    .and_then(|c| in_worktree(c, &snap.worktrees))
                {
                    return Some((q, "working folder".to_string(), w));
                }
                q.files.iter().find_map(|f| {
                    in_worktree(f, &snap.worktrees)
                        .map(|w| (q, format!("open file {}", rel(f, w)), w))
                })
            });
            let gone_dir = if p.cwd.as_deref().is_some_and(deleted) {
                ", its folder was deleted"
            } else {
                ""
            };
            let started = match kind {
                "snapshot shell" => format!("{agent} exited and left this shell behind"),
                "MCP server" => format!("the {agent} session that started it has ended"),
                "app server" => format!("no {agent} is running to use this app-server"),
                "command" => format!("{agent} started it and is gone"),
                _ => format!("its {agent} is gone"),
            };
            let kids_note = if kids.is_empty() {
                String::new()
            } else {
                format!(
                    "; {} child process{} below it",
                    kids.len(),
                    if kids.len() == 1 { "" } else { "es" }
                )
            };
            let (verdict, reason) = if let Some((q, what, w)) = hit {
                let whose = if q.pid == pid {
                    format!("its {what}")
                } else {
                    format!("the {what} of {}[{}]", q.name, q.pid)
                };
                (
                    Verdict::Review,
                    format!(
                        "{why}, but {whose} is in the live worktree {}: it may still be working there{kids_note}",
                        w.display()
                    ),
                )
            } else if codex_daemon(p) && roots.iter().any(|r| r.1 == "Codex") {
                (
                    Verdict::Review,
                    format!(
                        "Codex's shared app-server detaches by design, and a Codex is running: it may be the one serving it{kids_note}"
                    ),
                )
            } else if p.cwd.is_none() {
                (
                    Verdict::Review,
                    format!(
                        "{why}; its working folder could not be read, so a live worktree cannot be ruled out{kids_note}"
                    ),
                )
            } else if kind == "command" {
                (
                    Verdict::Review,
                    format!(
                        "{why}: {started}. It may be a server or watcher you asked for{kids_note}"
                    ),
                )
            } else {
                (
                    Verdict::Safe,
                    format!(
                        "{why}: {started}; nothing of it is in a live worktree{gone_dir}{kids_note}"
                    ),
                )
            };
            entries.push(Entry {
                pid,
                ppid: p.ppid,
                agent,
                role: Role::Orphan,
                kind,
                label: label(p, kind),
                root: None,
                orphan: Some(why),
                cwd: p.cwd.clone(),
                rss: p.rss,
                cpu: tree.iter().map(|q| q.cpu).sum(),
                age_s: p.age_s,
                children: kids,
                tree_rss,
                copies: 1,
                command: cmd(p),
                suggest: Some(format!("kill -TERM {pid}")),
                verdict,
                reason,
            });
        }
    }
    Listing { entries }
}

pub fn json(l: &Listing, supported: bool) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let count = |v: Verdict| l.entries.iter().filter(|e| e.verdict == v).count();
    json!({
        "tool": "spoilheap",
        "version": env!("CARGO_PKG_VERSION"),
        "generated_at": units::iso8601(now),
        "supported": supported,
        "totals": {
            "agents": l.entries.iter().filter(|e| e.role == Role::Agent).count(),
            "processes": l.entries.iter().map(|e| 1 + e.children.len()).sum::<usize>(),
            "orphans": l.entries.iter().filter(|e| e.role == Role::Orphan).count(),
            "rss_bytes": l.entries.iter().map(|e| e.tree_rss).sum::<u64>(),
            "safe": count(Verdict::Safe),
            "review": count(Verdict::Review),
            "keep": count(Verdict::Keep),
        },
        "processes": l.entries.iter().map(|e| json!({
            "pid": e.pid,
            "ppid": e.ppid,
            "agent": e.agent,
            "role": e.role.word(),
            "kind": e.kind,
            "label": e.label,
            "agent_pid": e.root,
            "orphan": e.orphan.is_some(),
            "orphan_reason": e.orphan,
            "cwd": e.cwd.as_ref().map(|c| c.display().to_string()),
            "rss_bytes": e.rss,
            "tree_rss_bytes": e.tree_rss,
            "cpu_percent": (e.cpu * 10.0).round() / 10.0,
            "age_s": e.age_s,
            "children": e.children,
            "copies": e.copies,
            "command": e.command,
            "verdict": e.verdict.word(),
            "reason": e.reason,
            "suggested_command": e.suggest,
        })).collect::<Vec<_>>(),
    })
}

fn cpu_label(st: &Style, cpu: f64) -> String {
    let s = if cpu < 0.05 {
        "0% CPU".to_string()
    } else if cpu >= 10.0 {
        format!("{cpu:.0}% CPU")
    } else {
        format!("{cpu:.1}% CPU")
    };
    if cpu >= 50.0 { st.red(&s) } else { st.dim(&s) }
}

/// How many helpers each agent shows without --all.
const SHOWN_HELPERS: usize = 5;

pub fn render(env: &Env, l: &Listing, st: &Style, all: bool) -> String {
    let mut o = String::new();
    let w = st.width;
    o.push_str(&format!(
        "{}  {}\n",
        st.bold("spoilheap procs"),
        st.dim("agent processes on this machine · read-only, nothing is signalled")
    ));
    if l.entries.is_empty() {
        o.push_str("\nNo agent processes running.\n");
        o.push_str(&st.dim(
            "Looked for Claude Code, Codex (CLI and desktop), OpenCode, Gemini CLI and Cline,\ntheir MCP servers and helpers, and Claude Code snapshot shells.\n",
        ));
        return o;
    }
    let n = |v: Verdict| l.entries.iter().filter(|e| e.verdict == v).count();
    let agents = l.entries.iter().filter(|e| e.role == Role::Agent).count();
    let orphans = l.entries.iter().filter(|e| e.role == Role::Orphan).count();
    let procs: usize = l.entries.iter().map(|e| 1 + e.children.len()).sum();
    let rss: u64 = l.entries.iter().map(|e| e.tree_rss).sum();
    let mut head = format!(
        "{} · {} process{} · {} memory",
        if agents == 0 {
            "no agent running".to_string()
        } else {
            format!(
                "{} agent{} running",
                st.bold(&agents.to_string()),
                if agents == 1 { "" } else { "s" }
            )
        },
        procs,
        if procs == 1 { "" } else { "es" },
        st.bold(&units::bytes(rss)),
    );
    if orphans > 0 {
        head.push_str(&format!(
            " · {} orphan{}",
            st.bold(&orphans.to_string()),
            if orphans == 1 { "" } else { "s" }
        ));
    }
    o.push_str(&format!(
        "{head} · {} {} · {} {} · {} {}\n",
        st.verdict_word(Verdict::Safe),
        n(Verdict::Safe),
        st.verdict_word(Verdict::Review),
        n(Verdict::Review),
        st.verdict_word(Verdict::Keep),
        n(Verdict::Keep),
    ));
    let size_w = 9;
    let meta_w = 18;
    // "  1234567 REVIEW " = 17 columns
    let ind = 17;
    let label_w = w.saturating_sub(ind + 2 + size_w + 2 + meta_w).max(16);
    let line = |o: &mut String, e: &Entry, lead: &str, extra: &str| {
        let mut lab = format!("{lead}{}{extra}", e.label);
        if e.role == Role::Orphan {
            lab = format!("orphan · {lab}");
        }
        let lab = clip(&lab, label_w);
        let pad = label_w.saturating_sub(lab.chars().count());
        let mut meta = cpu_label(st, e.cpu);
        if let Some(a) = e.age_s {
            meta.push_str(&st.dim(&format!(" · {}", units::age(a))));
        }
        o.push_str(&format!(
            "  {} {} {}{}  {:>size_w$}  {meta}\n",
            st.dim(&format!("{:>7}", e.pid)),
            st.verdict(e.verdict),
            lab,
            " ".repeat(pad),
            units::bytes(e.tree_rss),
        ));
    };
    let detail = |o: &mut String, e: &Entry| {
        let mut text = String::new();
        if let Some(c) = &e.cwd {
            text.push_str(&clip(&env.tilde(c), (w.saturating_sub(ind)).max(20) / 2));
            text.push_str(" — ");
        }
        let home = env.home.display().to_string();
        if home.len() > 1 {
            text.push_str(&e.reason.replace(&format!("{home}/"), "~/"));
        } else {
            text.push_str(&e.reason);
        }
        for l in crate::render::wrap(&text, w.saturating_sub(ind).max(30), ind) {
            o.push_str(&format!("{}{}\n", " ".repeat(ind), st.dim(l.trim_start())));
        }
    };
    let mut dup_lines = 0;
    for agent in AGENTS {
        let group: Vec<&Entry> = l.entries.iter().filter(|e| e.agent == *agent).collect();
        if group.is_empty() {
            continue;
        }
        let gp: usize = group.iter().map(|e| 1 + e.children.len()).sum();
        let grss: u64 = group.iter().map(|e| e.tree_rss).sum();
        o.push('\n');
        o.push_str(&format!(
            "{}  {}\n",
            st.bold(&agent.to_uppercase()),
            st.dim(&format!(
                "{gp} process{} · {}",
                if gp == 1 { "" } else { "es" },
                units::bytes(grss)
            ))
        ));
        for e in group.iter().filter(|e| e.role == Role::Agent) {
            line(&mut o, e, "", "");
            detail(&mut o, e);
            let helpers: Vec<&&Entry> = group
                .iter()
                .filter(|h| h.role == Role::Helper && h.root == Some(e.pid))
                .collect();
            let mut seen: Vec<&str> = vec![];
            let mut shown = 0;
            let mut hidden = (0usize, 0u64);
            for h in &helpers {
                if !all {
                    if h.copies > 1 {
                        if seen.contains(&h.command.as_str()) {
                            continue;
                        }
                        seen.push(&h.command);
                    }
                    if shown >= SHOWN_HELPERS {
                        hidden.0 += h.copies;
                        hidden.1 += h.rss * h.copies as u64;
                        continue;
                    }
                }
                shown += 1;
                let extra = if h.copies > 1 && !all {
                    dup_lines += 1;
                    format!(" ×{}", h.copies)
                } else {
                    String::new()
                };
                let mut h: Entry = Entry::clone(h);
                if !all {
                    h.tree_rss = h.rss * h.copies as u64;
                }
                line(&mut o, &h, "↳ ", &extra);
            }
            if hidden.0 > 0 {
                o.push_str(&format!(
                    "{}{}\n",
                    " ".repeat(ind),
                    st.dim(&format!(
                        "↳ {} more helper{}, {} (--all lists them)",
                        hidden.0,
                        if hidden.0 == 1 { "" } else { "s" },
                        units::bytes(hidden.1)
                    ))
                ));
            }
        }
        for e in group.iter().filter(|e| e.role == Role::Orphan) {
            line(&mut o, e, "", "");
            detail(&mut o, e);
        }
    }
    if dup_lines > 0 {
        let text = "×N: that many helpers run the same command under one agent. Codex restarts have doubled them (openai/codex#55141); restarting the agent clears them";
        o.push('\n');
        for (i, l) in crate::render::wrap(text, w.saturating_sub(7).max(30), 5)
            .into_iter()
            .enumerate()
        {
            if i == 0 {
                o.push_str(&format!("{} {l}\n", st.yellow("note")));
            } else {
                o.push_str(&format!("{}\n", st.dim(&l)));
            }
        }
    }
    o.push_str(&format!("\n{}\n", st.bold("next")));
    let pids = |v: Verdict| -> Vec<String> {
        l.entries
            .iter()
            .filter(|e| e.verdict == v && e.suggest.is_some())
            .map(|e| e.pid.to_string())
            .collect()
    };
    let safe = pids(Verdict::Safe);
    let review = pids(Verdict::Review);
    if safe.is_empty() && review.is_empty() {
        o.push_str("  nothing to stop: every agent process has its agent running above it\n");
        return o;
    }
    if !safe.is_empty() {
        let c = format!("kill -TERM {}", safe.join(" "));
        let c = if c.len() > 26 { c } else { format!("{c:<26}") };
        o.push_str(&format!(
            "  {}  stops {} SAFE orphan{} (SIGTERM lets {} exit cleanly)\n",
            st.green(&c),
            safe.len(),
            if safe.len() == 1 { "" } else { "s" },
            if safe.len() == 1 { "it" } else { "them" }
        ));
    }
    if !review.is_empty() {
        let c = format!("kill -TERM {}", review.join(" "));
        let c = if c.len() > 26 { c } else { format!("{c:<26}") };
        o.push_str(&format!(
            "  {}  REVIEW: only once you know {} not yours\n",
            st.yellow(&c),
            if review.len() == 1 {
                "it is"
            } else {
                "they are"
            }
        ));
    }
    o.push_str(&st.dim(
        "  spoilheap only lists; run these yourself, then `spoilheap procs` again to check\n",
    ));
    o
}

/// `spoilheap procs`. Exit 0 whatever it finds (listing is not a failure);
/// 1 only when the process table cannot be read.
pub fn run(env: &Env, st: &Style, as_json: bool, all: bool) -> i32 {
    let snap = match crate::ptable::fake() {
        Some(r) => r,
        // SPOILHEAP_OS=windows shows the Windows message on any OS (for tests)
        None if env.os == "windows" => Err("unsupported".to_string()),
        None => match crate::ptable::collect(std::time::Duration::from_millis(250)) {
            Ok(mut s) => {
                let need = needs_files(&s);
                crate::ptable::fill(&mut s, &need);
                Ok(s)
            }
            Err(e) => Err(e),
        },
    };
    let snap = match snap {
        Ok(s) => s,
        Err(e) if e == "unsupported" => {
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json(&Listing::default(), false)).unwrap()
                );
            } else {
                println!(
                    "{}  {}\n\n`spoilheap procs` is not supported on Windows yet (planned for v0.3).\n{}",
                    st.bold("spoilheap procs"),
                    st.dim("agent processes on this machine"),
                    st.dim(
                        "Task Manager > Details, sorted by name, shows node.exe and codex.exe helpers in the meantime."
                    )
                );
            }
            return 0;
        }
        Err(e) => {
            eprintln!("{} {e}", st.red("spoilheap:"));
            return 1;
        }
    };
    let l = classify(&snap);
    if as_json {
        println!("{}", serde_json::to_string_pretty(&json(&l, true)).unwrap());
    } else {
        print!("{}", render(env, &l, st, all));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ptable::parse;

    fn fixture(name: &str) -> Listing {
        let s = match name {
            "live-agent" => include_str!("../tests/fixtures/procs/live-agent.json"),
            "orphan-snapshot-shell" => {
                include_str!("../tests/fixtures/procs/orphan-snapshot-shell.json")
            }
            "orphan-mcp-parent-gone" => {
                include_str!("../tests/fixtures/procs/orphan-mcp-parent-gone.json")
            }
            "orphan-in-worktree" => include_str!("../tests/fixtures/procs/orphan-in-worktree.json"),
            "unrelated-node" => include_str!("../tests/fixtures/procs/unrelated-node.json"),
            "demo" => include_str!("../tests/fixtures/procs/demo.json"),
            _ => unreachable!(),
        };
        classify(&parse(s).unwrap())
    }

    fn get(l: &Listing, pid: u32) -> &Entry {
        l.entries
            .iter()
            .find(|e| e.pid == pid)
            .unwrap_or_else(|| panic!("pid {pid} not listed: {:#?}", l.entries))
    }

    fn inline(procs: &str, worktrees: &str) -> Listing {
        classify(
            &parse(&format!(
                r#"{{"worktrees":[{worktrees}],"procs":[{procs}]}}"#
            ))
            .unwrap(),
        )
    }

    #[test]
    fn live_agent_and_its_helpers_are_keep() {
        let l = fixture("live-agent");
        let root = get(&l, 4242);
        assert_eq!((root.role, root.agent), (Role::Agent, "Claude Code"));
        assert_eq!(root.verdict, Verdict::Keep);
        for pid in [4300, 4301, 4302, 4410, 4411] {
            let h = get(&l, pid);
            assert_eq!(h.role, Role::Helper, "{pid}");
            assert_eq!(h.root, Some(4242));
            assert_eq!(h.verdict, Verdict::Keep);
            assert!(h.suggest.is_none());
        }
        assert_eq!(get(&l, 4410).kind, "snapshot shell");
        assert_eq!(get(&l, 4300).label, "MCP server mcp-server-filesystem");
        assert_eq!(get(&l, 4301).label, "MCP server @playwright/mcp");
        assert!(
            get(&l, 4302)
                .reason
                .contains("(via npm exec @playwright/mcp@latest[4301])")
        );
        // the login shell above Claude is not an agent process
        assert!(l.entries.iter().all(|e| e.pid != 900 && e.pid != 1));
    }

    #[test]
    fn orphaned_snapshot_shell_at_ppid_1_is_safe() {
        let l = fixture("orphan-snapshot-shell");
        assert_eq!(l.entries.len(), 1, "the tail below it is folded in");
        let e = get(&l, 7731);
        assert_eq!((e.role, e.kind), (Role::Orphan, "snapshot shell"));
        assert_eq!(e.orphan.as_deref(), Some("PPID 1"));
        assert_eq!(e.verdict, Verdict::Safe);
        assert_eq!(e.children, vec![7732]);
        assert_eq!(e.tree_rss, 6_300_000);
        assert_eq!(e.suggest.as_deref(), Some("kill -TERM 7731"));
    }

    #[test]
    fn orphaned_mcp_server_whose_parent_is_gone_is_safe() {
        let l = fixture("orphan-mcp-parent-gone");
        let e = get(&l, 8810);
        assert_eq!((e.agent, e.kind), ("Claude Code", "MCP server"));
        assert_eq!(e.orphan.as_deref(), Some("parent 8800 is gone"));
        assert_eq!(e.verdict, Verdict::Safe);
        assert!(e.reason.contains("its folder was deleted"), "{}", e.reason);
        assert!(!e.command.contains("ghp_example"), "{}", e.command);
        // adopted by a user systemd counts as orphaned too
        let g = get(&l, 8820);
        assert_eq!((g.agent, g.verdict), ("Gemini CLI", Verdict::Safe));
        assert_eq!(g.orphan.as_deref(), Some("adopted by systemd[501]"));
        assert_eq!(g.label, "MCP server notes_mcp.py (via uv)");
    }

    #[test]
    fn orphan_working_in_a_live_worktree_is_review() {
        let l = fixture("orphan-in-worktree");
        let e = get(&l, 6100);
        assert_eq!(e.verdict, Verdict::Review);
        assert!(
            e.reason.contains("live worktree /home/dev/code/shop"),
            "{}",
            e.reason
        );
        // the shell itself sits in /tmp, but the vite below it works in a Codex worktree
        let s = get(&l, 6200);
        assert_eq!(s.verdict, Verdict::Review);
        assert!(s.reason.contains("node[6201]"), "{}", s.reason);
        assert!(s.reason.contains(".codex/worktrees/a1b2/shop"));
        let c = get(&l, 6300);
        assert_eq!(
            (c.agent, c.kind, c.verdict),
            ("Codex", "command", Verdict::Review)
        );
        assert!(l.entries.iter().all(|e| e.verdict != Verdict::Safe));
    }

    #[test]
    fn unrelated_node_processes_are_not_listed() {
        assert!(fixture("unrelated-node").entries.is_empty());
    }

    #[test]
    fn nested_and_duplicate_helpers() {
        let l = fixture("demo");
        let agents: Vec<u32> = l
            .entries
            .iter()
            .filter(|e| e.role == Role::Agent)
            .map(|e| e.pid)
            .collect();
        assert_eq!(
            agents,
            vec![4242, 5100],
            "the native codex under codex.js is a helper"
        );
        assert_eq!(get(&l, 5100).label, "codex");
        assert_eq!(get(&l, 5101).root, Some(5100));
        assert_eq!(get(&l, 5110).copies, 2);
        assert!(get(&l, 5110).reason.contains("openai/codex#55141"));
        assert!(get(&l, 8810).command.ends_with("--password ***"));
        assert!(l.entries.iter().all(|e| e.pid != 3100));
        let out = render(
            &Env::from_process(Some("/home/dev".into())),
            &l,
            &Style {
                color: false,
                width: 100,
            },
            false,
        );
        assert!(out.contains("MCP server mcp-server-github ×2"), "{out}");
        assert!(out.contains("kill -TERM 8810 7731"), "{out}");
        assert_eq!(out.matches("mcp-server-github").count(), 1, "{out}");
    }

    #[test]
    fn unknown_working_folder_is_review() {
        let l = inline(
            r#"{"pid":70,"ppid":1,"argv":["node","/x/mcp-server-git"],"env":["CLAUDECODE"]}"#,
            "",
        );
        assert_eq!(get(&l, 70).verdict, Verdict::Review);
    }

    #[test]
    fn agent_roots_without_a_parent() {
        let l = inline(
            r#"{"pid":10,"ppid":1,"argv":["claude"],"cwd":"/w"},
               {"pid":11,"ppid":1,"argv":["/Applications/Codex.app/Contents/MacOS/Codex"],"cwd":"/"},
               {"pid":12,"ppid":11,"argv":["/Applications/Codex.app/Contents/Frameworks/Codex Helper.app/Contents/MacOS/Codex Helper"],"exe":"/Applications/Codex.app/Contents/Frameworks/Codex Helper.app/Contents/MacOS/Codex Helper"}"#,
            "",
        );
        // a CLI agent whose terminal is gone is yours to judge; the desktop app is not orphaned
        assert_eq!(get(&l, 10).verdict, Verdict::Review);
        assert_eq!(get(&l, 11).verdict, Verdict::Keep);
        assert_eq!(get(&l, 12).root, Some(11));
        assert_eq!(get(&l, 12).kind, "app helper");
    }

    #[test]
    fn codex_app_server() {
        // started by an editor extension: not an orphan, not listed
        let ext = r#"{"pid":20,"ppid":19,"argv":["/x/codex","app-server"],"cwd":"/w"},
                     {"pid":19,"ppid":1,"name":"Code Helper (Plugin)","argv":["Code Helper (Plugin)"]}"#;
        assert!(inline(ext, "").entries.is_empty());
        // the managed daemon detaches on purpose
        let daemon = r#"{"pid":30,"ppid":19,"argv":["/x/codex","app-server","--listen","unix://","--managed-daemon"],"cwd":"/gone (deleted)"},
                        {"pid":19,"ppid":1,"name":"sh","argv":["sh"]}"#;
        let l = inline(daemon, "");
        assert_eq!(get(&l, 30).verdict, Verdict::Safe);
        assert_eq!(get(&l, 30).label, "codex app-server");
        let with_codex = format!(r#"{daemon},{{"pid":40,"ppid":19,"argv":["codex"],"cwd":"/w"}}"#);
        assert_eq!(get(&inline(&with_codex, ""), 30).verdict, Verdict::Review);
    }

    #[test]
    fn markers_stay_narrow() {
        let p = |argv: &[&str]| P {
            argv: argv.iter().map(|s| s.to_string()).collect(),
            name: base(argv[0]).to_string(),
            ..Default::default()
        };
        assert_eq!(agent_of(&p(&["/usr/bin/claude"])), Some("Claude Code"));
        assert_eq!(
            agent_of(&p(&["node", "/n/@google/gemini-cli/dist/index.js"])),
            Some("Gemini CLI")
        );
        assert_eq!(
            agent_of(&p(&["node", "/n/opencode-ai/bin/opencode"])),
            Some("OpenCode")
        );
        assert_eq!(agent_of(&p(&["claude", "mcp", "serve"])), None);
        assert_eq!(agent_of(&p(&["node", "server.js"])), None);
        assert_eq!(agent_of(&p(&["claude-monitor"])), None);
        assert_eq!(agent_of(&p(&["vim", "claude.md"])), None);
        assert_eq!(helper_of(&p(&["node", "index.js"])), None);
    }

    #[test]
    fn redaction() {
        let r = redact(&[
            "x".into(),
            "--api-key".into(),
            "sk-1".into(),
            "GITHUB_TOKEN=ghp".into(),
            "--port".into(),
            "80".into(),
        ]);
        assert_eq!(
            r,
            ["x", "--api-key", "***", "GITHUB_TOKEN=***", "--port", "80"]
        );
    }
}
