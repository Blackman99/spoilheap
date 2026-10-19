//! Which processes are running, and which of them have files open under a path.

use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
}

pub fn list() -> Vec<Proc> {
    #[cfg(unix)]
    {
        let Ok(out) = Command::new("ps").args(["-axo", "pid=,ppid=,comm="]).stdin(Stdio::null()).stderr(Stdio::null()).output() else {
            return vec![];
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                let pid = it.next()?.parse().ok()?;
                let ppid = it.next()?.parse().ok()?;
                let rest: Vec<&str> = it.collect();
                let full = rest.join(" ");
                let name = Path::new(&full).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(full);
                Some(Proc { pid, ppid, name })
            })
            .collect()
    }
    #[cfg(windows)]
    {
        let Ok(out) = Command::new("tasklist").args(["/fo", "csv", "/nh"]).stdin(Stdio::null()).stderr(Stdio::null()).output() else {
            return vec![];
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split("\",\"").map(|s| s.trim_matches('"')).collect();
                Some(Proc { pid: f.get(1)?.parse().ok()?, ppid: 0, name: f.first()?.to_string() })
            })
            .collect()
    }
}

/// Running agent apps whose data spoilheap reports, by display name.
pub fn running_agents(ps: &[Proc]) -> Vec<&'static str> {
    let mut v = vec![];
    let has = |pred: &dyn Fn(&str) -> bool| ps.iter().any(|p| pred(&p.name.to_ascii_lowercase()));
    if has(&|n| n == "codex" || n.starts_with("codex") || n == "chatgpt") {
        v.push("Codex");
    }
    if has(&|n| n == "claude" || n.starts_with("claude")) {
        v.push("Claude Code");
    }
    if has(&|n| n.starts_with("opencode")) {
        v.push("OpenCode");
    }
    if has(&|n| n.starts_with("gemini")) {
        v.push("Gemini CLI");
    }
    v
}

/// "git[48211] ← ChatGPT[4410]": a process and its ancestors (max 3).
pub fn chain(ps: &[Proc], pid: u32) -> String {
    let mut parts = vec![];
    let mut cur = pid;
    for _ in 0..3 {
        let Some(p) = ps.iter().find(|p| p.pid == cur) else { break };
        parts.push(format!("{}[{}]", p.name, p.pid));
        if p.ppid <= 1 {
            break;
        }
        cur = p.ppid;
    }
    parts.join(" ← ")
}

/// PIDs that hold a file open under `dir`. Linux reads /proc/*/fd; macOS asks lsof
/// about the newest files only (lsof +D over a big tree is too slow to be useful).
pub fn writers(dir: &Path, newest_files: &[std::path::PathBuf]) -> Vec<u32> {
    let mut pids = vec![];
    #[cfg(target_os = "linux")]
    {
        let _ = newest_files;
        let Ok(rd) = std::fs::read_dir("/proc") else { return pids };
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
            let Ok(fds) = std::fs::read_dir(e.path().join("fd")) else { continue };
            for fd in fds.flatten() {
                if let Ok(t) = std::fs::read_link(fd.path()) {
                    if t.starts_with(dir) {
                        pids.push(pid);
                        break;
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = dir;
        if newest_files.is_empty() {
            return pids;
        }
        if let Ok(out) = Command::new("lsof").arg("-t").args(newest_files).stdin(Stdio::null()).stderr(Stdio::null()).output() {
            for l in String::from_utf8_lossy(&out.stdout).lines() {
                if let Ok(p) = l.trim().parse() {
                    if !pids.contains(&p) {
                        pids.push(p);
                    }
                }
            }
        }
    }
    pids.sort();
    pids.dedup();
    pids
}

/// (pid, name, current working folder) of every process we can see.
pub fn cwds() -> Vec<(u32, String, std::path::PathBuf)> {
    let mut out = vec![];
    #[cfg(target_os = "linux")]
    {
        let Ok(rd) = std::fs::read_dir("/proc") else { return out };
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
            if pid == std::process::id() {
                continue;
            }
            let Ok(cwd) = std::fs::read_link(e.path().join("cwd")) else { continue };
            let name = std::fs::read_to_string(e.path().join("comm")).map(|s| s.trim().to_string()).unwrap_or_default();
            out.push((pid, name, cwd));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(o) = Command::new("lsof").args(["-a", "-d", "cwd", "-Fpcn"]).stdin(Stdio::null()).stderr(Stdio::null()).output() {
            let (mut pid, mut name) = (0u32, String::new());
            for l in String::from_utf8_lossy(&o.stdout).lines() {
                match l.split_at(1) {
                    ("p", v) => pid = v.parse().unwrap_or(0),
                    ("c", v) => name = v.to_string(),
                    ("n", v) if pid != std::process::id() => out.push((pid, name.clone(), std::path::PathBuf::from(v))),
                    _ => {}
                }
            }
        }
    }
    out
}
