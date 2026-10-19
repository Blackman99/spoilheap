//! End-to-end: build a fake home with real git worktrees and a real SQLite file,
//! run the binary, and check verdicts, clean, guard, hook and MCP.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_spoilheap"));
    c.env("NO_COLOR", "1")
        .env("SPOILHEAP_VACUUM_MIN", "100KB")
        .env("COLUMNS", "140")
        .env_remove("CODEX_HOME")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("XDG_DATA_HOME")
        .env_remove("SPOILHEAP_FAKE_FREE");
    c
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "advice.detachedHead=false",
        ])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?} in {}", dir.display());
}

fn write(p: &Path, bytes: usize, age_days: f64) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, vec![7u8; bytes]).unwrap();
    if age_days > 0.0 {
        let t = SystemTime::now() - Duration::from_secs_f64(age_days * 86400.0);
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }
}

fn age_tree(dir: &Path, days: f64) {
    let t = SystemTime::now() - Duration::from_secs_f64(days * 86400.0);
    fn rec(p: &Path, t: SystemTime) {
        for e in fs::read_dir(p).unwrap().flatten() {
            let ft = e.file_type().unwrap();
            if ft.is_dir() {
                if e.file_name() != ".git" {
                    rec(&e.path(), t);
                }
            } else if ft.is_file() && e.file_name() != ".git" {
                if let Ok(f) = fs::File::options().write(true).open(e.path()) {
                    let _ = f.set_modified(t);
                }
            }
        }
    }
    rec(dir, t);
}

struct Home {
    dir: PathBuf,
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn home(name: &str) -> Home {
    let dir = std::env::temp_dir().join(format!("spoilheap-it-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dir = fs::canonicalize(&dir).unwrap();
    let dir = PathBuf::from(
        dir.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string(),
    );
    let h = dir.clone();
    // Codex history
    write(
        &h.join(".codex/sessions/2026/06/01/rollout-old.jsonl"),
        300_000,
        90.0,
    );
    write(
        &h.join(".codex/sessions/2026/10/18/rollout-new.jsonl"),
        100_000,
        1.0,
    );
    // a real repo with real worktrees under ~/.codex/worktrees
    let repo = h.join("code/app");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init"]);
    fs::write(repo.join(".gitignore"), "target/\n").unwrap();
    fs::write(repo.join("main.rs"), "fn main(){}\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "init"]);
    let wts = h.join(".codex/worktrees");
    let merged = wts.join("a1/app");
    let unique = wts.join("b2/app");
    let dirty = wts.join("c3/app");
    fs::create_dir_all(merged.parent().unwrap()).unwrap();
    fs::create_dir_all(unique.parent().unwrap()).unwrap();
    fs::create_dir_all(dirty.parent().unwrap()).unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--detach",
            merged.to_str().unwrap(),
            "main",
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "codex/sig",
            unique.to_str().unwrap(),
            "main",
        ],
    );
    fs::write(unique.join("sig.rs"), "sig").unwrap();
    git(&unique, &["add", "-A"]);
    git(&unique, &["commit", "-m", "sig"]);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "codex/wip",
            dirty.to_str().unwrap(),
            "main",
        ],
    );
    fs::write(dirty.join("wip.rs"), "wip").unwrap();
    write(&merged.join("target/debug/big.rlib"), 2_000_000, 5.0);
    write(&dirty.join("target/debug/bin"), 1_500_000, 5.0);
    for w in [&merged, &unique, &dirty] {
        age_tree(w, 5.0);
    }
    // a half-written pack from a git that died
    write(
        &repo.join(".git/objects/pack/tmp_pack_dead01"),
        1_200_000,
        2.0,
    );
    // OpenCode: a real SQLite file whose rows were deleted but never vacuumed
    let db = h.join(".local/share/opencode/opencode.db");
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bloated.db"),
        &db,
    )
    .unwrap();
    // Claude Code
    write(&h.join(".claude/projects/-x/old.jsonl"), 250_000, 60.0);
    write(&h.join(".claude/shell-snapshots/s.sh"), 2_000_000, 3.0);
    Home { dir }
}

fn dig_json(h: &Path) -> serde_json::Value {
    let out = bin()
        .args(["--json", "--all", "--home"])
        .arg(h)
        .arg("--root")
        .arg(h.join("code"))
        .env("XDG_CACHE_HOME", h.join(".cache"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn find<'a>(v: &'a serde_json::Value, kind: &str, path_end: &str) -> &'a serde_json::Value {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| {
            i["kind"] == kind
                && i["path"]
                    .as_str()
                    .unwrap()
                    .replace('\\', "/")
                    .ends_with(path_end)
        })
        .unwrap_or_else(|| panic!("no {kind} item ending in {path_end}: {v:#}"))
}

#[test]
fn verdicts_follow_git_and_sqlite() {
    let h = home("verdicts");
    let v = dig_json(&h.dir);
    assert_eq!(
        find(&v, "worktree", "a1/app")["verdict"],
        "SAFE",
        "clean + merged"
    );
    let u = find(&v, "worktree", "b2/app");
    assert_eq!(u["verdict"], "REVIEW");
    assert!(
        u["reason"]
            .as_str()
            .unwrap()
            .contains("1 commit exists only here"),
        "{u}"
    );
    let d = find(&v, "worktree", "c3/app");
    assert_eq!(d["verdict"], "KEEP");
    assert!(d["reason"].as_str().unwrap().contains("uncommitted"));
    // build output inside the dirty worktree is still offered, separately
    let b = find(&v, "build-output", "c3/app/target");
    assert_eq!(b["verdict"], "SAFE");
    assert_eq!(find(&v, "tmp-pack", ".git/objects/pack")["verdict"], "SAFE");
    let db = find(&v, "database", "opencode.db");
    assert_eq!(db["verdict"], "REVIEW");
    assert!(db["reclaimable_bytes"].as_u64().unwrap() >= 300_000, "{db}");
    assert!(db["action"]["command"].as_str().unwrap().contains("VACUUM"));
    assert_eq!(find(&v, "sessions", ".codex/sessions")["verdict"], "REVIEW");
    assert_eq!(find(&v, "recent", ".codex/sessions")["verdict"], "KEEP");
    assert_eq!(
        find(&v, "shell-snapshots", "shell-snapshots")["verdict"],
        "SAFE"
    );
    // ids are sequential from 101
    assert_eq!(v["items"][0]["id"], 101);
}

#[test]
fn clean_is_a_dry_run_and_never_touches_keep() {
    let h = home("clean");
    let cache = h.dir.join(".cache");
    let root = h.dir.join("code");
    let run = |args: &[&str]| {
        let out = bin()
            .args(args)
            .arg("--home")
            .arg(&h.dir)
            .arg("--root")
            .arg(&root)
            .env("XDG_CACHE_HOME", &cache)
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let (code, _, err) = run(&["clean"]);
    assert_eq!(code, 64);
    assert!(err.contains("say what to clear"));
    let (code, out, _) = run(&["clean", "--safe"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("dry run"), "{out}");
    assert!(
        h.dir.join(".codex/worktrees/a1/app").exists(),
        "dry run deleted something"
    );
    let v = dig_json(&h.dir);
    let keep = find(&v, "worktree", "c3/app")["id"]
        .as_u64()
        .unwrap()
        .to_string();
    let (code, out, _) = run(&["clean", "--safe", &keep, "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("KEEP is never touched"), "{out}");
    assert!(
        !h.dir.join(".codex/worktrees/a1").exists(),
        "merged worktree should be removed with its empty container: {out}"
    );
    assert!(
        h.dir.join(".codex/worktrees/c3/app/wip.rs").exists(),
        "KEEP worktree touched"
    );
    assert!(
        !h.dir.join(".codex/worktrees/c3/app/target").exists(),
        "SAFE build output kept"
    );
    assert!(
        h.dir.join(".codex/worktrees/b2/app/sig.rs").exists(),
        "REVIEW item touched without being named"
    );
    assert!(
        !h.dir
            .join("code/app/.git/objects/pack/tmp_pack_dead01")
            .exists()
    );
    assert!(
        h.dir.join(".local/share/opencode/opencode.db").exists(),
        "databases are never edited"
    );
    let wl = Command::new("git")
        .arg("-C")
        .arg(root.join("app"))
        .args(["worktree", "list"])
        .output()
        .unwrap();
    assert!(
        // match the worktree's own path segment: a random temp dir can contain "a1"
        !String::from_utf8_lossy(&wl.stdout).contains("/worktrees/a1/"),
        "git still lists the removed worktree"
    );
}

#[test]
fn guard_blocks_under_the_floor_and_names_leftovers() {
    let h = home("guard");
    let ok = bin()
        .args(["guard", "--floor", "1KB", "--home"])
        .arg(&h.dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(ok.status.code(), Some(0));
    let mut c = bin();
    c.args(["guard", "--floor", "10GB", "--home"])
        .arg(&h.dir)
        .env("SPOILHEAP_FAKE_FREE", "3GB/500GB")
        .env("XDG_CACHE_HOME", h.dir.join(".cache"));
    c.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"PreToolUse","cwd":"/","tool_name":"Bash"}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(2), "exit 2 blocks the tool call");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("only 3.0 GB free"), "{err}");
    assert!(err.contains("SAFE to delete"), "{err}");
    assert!(err.contains("spoilheap clean --safe --yes"), "{err}");
}

#[test]
fn hook_snippets_and_errors() {
    let o = bin().args(["hook", "claude"]).output().unwrap();
    let s = String::from_utf8_lossy(&o.stdout);
    assert!(s.contains("\"PreToolUse\"") && s.contains("spoilheap guard --floor 10GB"));
    let o = bin()
        .args(["hook", "codex", "--floor", "5%"])
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&o.stdout);
    assert!(
        s.contains("[[hooks.PreToolUse]]")
            && s.contains("hooks = true")
            && s.contains("--floor 5%")
    );
    let o = bin().args(["hook", "vim"]).output().unwrap();
    assert_eq!(o.status.code(), Some(64));
    let o = bin().args(["--flor", "1GB"]).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("did you mean --floor"));
    let o = bin().args(["clean", "abc"]).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("not an item number"));
}

#[test]
fn mcp_lists_read_only_tools_and_answers() {
    let h = home("mcp");
    let mut c = bin();
    c.arg("mcp")
        .arg("--home")
        .arg(&h.dir)
        .env("XDG_CACHE_HOME", h.dir.join(".cache"));
    c.stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = c.spawn().unwrap();
    {
        use std::io::Write;
        let mut i = child.stdin.take().unwrap();
        writeln!(i, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"t","version":"1"}}}}}}"#).unwrap();
        writeln!(
            i,
            r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
        )
        .unwrap();
        writeln!(i, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#).unwrap();
        writeln!(i, r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"disk_leftovers","arguments":{{"min_bytes":0}}}}}}"#).unwrap();
        writeln!(i, r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"disk_status","arguments":{{"floor":"1KB"}}}}}}"#).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let lines: Vec<serde_json::Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 4, "notifications get no reply");
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "spoilheap");
    let names: Vec<&str> = lines[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["disk_status", "disk_leftovers"]);
    assert!(
        lines[1]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["annotations"]["readOnlyHint"] == true)
    );
    assert!(
        lines[2]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("SAFE")
    );
    assert_eq!(lines[3]["result"]["structuredContent"]["ok"], true);
}

#[test]
fn empty_home_says_so() {
    let dir = std::env::temp_dir().join(format!("spoilheap-empty-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let o = bin()
        .arg("--home")
        .arg(&dir)
        .arg("--root")
        .arg(&dir)
        .env("XDG_CACHE_HOME", dir.join(".c"))
        .output()
        .unwrap();
    assert!(o.status.success());
    let s = String::from_utf8_lossy(&o.stdout);
    assert!(s.contains("Nothing found"), "{s}");
    let _ = fs::remove_dir_all(&dir);
}
