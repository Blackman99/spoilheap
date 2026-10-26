//! A snapshot of the process table, taken without changing anything.
//!
//! `collect` reads the table (Linux: /proc, macOS: ps), `fill` adds working folders
//! and open files for the few processes that need them, and `load` reads the same
//! snapshot from a JSON fixture so the classification in `agentprocs` can be tested
//! (and demoed) without real agent processes.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Environment variables the agents set in the processes they start. Only the
/// presence of the key is recorded; values are never kept or printed.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub const ENV_MARKERS: &[&str] = &[
    // Claude Code: Bash/PowerShell tools, hooks and (since 2.1.154) stdio MCP servers
    // https://code.claude.com/docs/en/env-vars
    "CLAUDECODE",
    // Codex: every shell command it runs (codex-rs/protocol/src/shell_environment.rs)
    "CODEX_THREAD_ID",
    "CODEX_CI",
    "CODEX_SANDBOX_NETWORK_DISABLED",
    // OpenCode: bash tool and MCP servers (anomalyco/opencode#35925)
    "OPENCODE",
    "OPENCODE_PID",
    // Gemini CLI: run_shell_command (google-gemini/gemini-cli#4791)
    "GEMINI_CLI",
];

#[derive(Debug, Clone, Default)]
pub struct P {
    pub pid: u32,
    pub ppid: u32,
    /// Short process name (Linux comm, macOS executable file name).
    pub name: String,
    /// Executable path when known.
    pub exe: String,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Regular files it holds open (only filled for orphan candidates).
    pub files: Vec<PathBuf>,
    /// Resident memory in bytes.
    pub rss: u64,
    /// CPU in percent of one core, over a short sample.
    pub cpu: f64,
    pub age_s: Option<u64>,
    /// Which of ENV_MARKERS are set in its environment.
    pub env: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub procs: Vec<P>,
    /// Git working trees (main or linked) that exist on disk and contain a
    /// working folder or an open file of some listed process.
    pub worktrees: Vec<PathBuf>,
}

/// Test and demo hook: SPOILHEAP_FAKE_PROCS=<file.json> reads the snapshot from a
/// fixture instead of the live process table.
pub fn fake() -> Option<Result<Snapshot, String>> {
    let p = std::env::var_os("SPOILHEAP_FAKE_PROCS")?;
    Some(load(Path::new(&p)))
}

pub fn load(path: &Path) -> Result<Snapshot, String> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| format!("SPOILHEAP_FAKE_PROCS {}: {e}", path.display()))?;
    parse(&s).map_err(|e| format!("SPOILHEAP_FAKE_PROCS {}: {e}", path.display()))
}

pub fn parse(s: &str) -> Result<Snapshot, String> {
    let v: Value = serde_json::from_str(s).map_err(|e| e.to_string())?;
    let strs = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut snap = Snapshot {
        worktrees: strs(&v["worktrees"])
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        ..Default::default()
    };
    for p in v["procs"].as_array().ok_or("no \"procs\" array")? {
        let argv = strs(&p["argv"]);
        snap.procs.push(P {
            pid: p["pid"].as_u64().ok_or("a process without \"pid\"")? as u32,
            ppid: p["ppid"].as_u64().unwrap_or(0) as u32,
            name: p["name"].as_str().map(String::from).unwrap_or_else(|| {
                argv.first()
                    .map(|a| base(a).to_string())
                    .unwrap_or_default()
            }),
            exe: p["exe"].as_str().unwrap_or_default().to_string(),
            argv,
            cwd: p["cwd"].as_str().map(PathBuf::from),
            files: strs(&p["files"]).into_iter().map(PathBuf::from).collect(),
            rss: p["rss"].as_u64().unwrap_or(0),
            cpu: p["cpu"].as_f64().unwrap_or(0.0),
            age_s: p["age_s"].as_u64(),
            env: strs(&p["env"]),
        });
    }
    Ok(snap)
}

/// File name part of a path or argv word, for either separator.
pub fn base(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or(s)
}

/// Read the live process table. Linux samples CPU over `sample`; macOS takes ps's
/// recent-average %CPU.
#[allow(unused_variables)]
pub fn collect(sample: std::time::Duration) -> Result<Snapshot, String> {
    #[cfg(target_os = "linux")]
    {
        Ok(linux::collect(sample))
    }
    #[cfg(target_os = "macos")]
    {
        macos::collect()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err("unsupported".into())
    }
}

/// Add working folder and open files for `pids`, then the live worktrees they are in.
pub fn fill(snap: &mut Snapshot, pids: &[u32]) {
    #[cfg(target_os = "linux")]
    linux::fill(snap, pids);
    #[cfg(target_os = "macos")]
    macos::fill(snap, pids);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = pids;
    let mut wts: Vec<PathBuf> = vec![];
    for p in snap.procs.iter().filter(|p| pids.contains(&p.pid)) {
        for path in p.cwd.iter().chain(p.files.iter()) {
            if let Some(w) = worktree_of(path) {
                if !wts.contains(&w) {
                    wts.push(w);
                }
            }
        }
    }
    snap.worktrees = wts;
}

/// The git working tree that holds `path`, if it still exists.
fn worktree_of(path: &Path) -> Option<PathBuf> {
    if path.to_string_lossy().ends_with(" (deleted)") {
        return None;
    }
    let mut cur = Some(path);
    while let Some(d) = cur {
        if d.join(".git").exists() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{ENV_MARKERS, P, Snapshot};

    struct Stat {
        ppid: u32,
        comm: String,
        ticks: u64,
        start: u64,
    }

    fn stat(pid: u32) -> Option<Stat> {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // "pid (comm with spaces) S ppid ..." — comm ends at the last ')'
        let open = s.find('(')?;
        let close = s.rfind(')')?;
        let comm = s[open + 1..close].to_string();
        let f: Vec<&str> = s[close + 2..].split(' ').collect();
        // fields after comm start at index 3 (state); utime=14, stime=15, starttime=22
        let n = |i: usize| f.get(i - 3).and_then(|x| x.parse::<u64>().ok());
        Some(Stat {
            ppid: n(4)? as u32,
            comm,
            ticks: n(14)? + n(15)?,
            start: n(22)?,
        })
    }

    fn pids() -> Vec<u32> {
        std::fs::read_dir("/proc")
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_string_lossy().parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn collect(sample: std::time::Duration) -> Snapshot {
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
        let uptime: f64 = std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse().ok())
            .unwrap_or(0.0);
        let me = std::process::id();
        let mut first: Vec<(P, u64)> = vec![];
        for pid in pids() {
            if pid == me {
                continue;
            }
            let Some(st) = stat(pid) else { continue };
            let argv: Vec<String> = std::fs::read(format!("/proc/{pid}/cmdline"))
                .map(|b| {
                    b.split(|c| *c == 0)
                        .filter(|a| !a.is_empty())
                        .map(|a| String::from_utf8_lossy(a).into_owned())
                        .collect()
                })
                .unwrap_or_default();
            let rss = std::fs::read_to_string(format!("/proc/{pid}/statm"))
                .ok()
                .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
                .unwrap_or(0)
                * page;
            let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let env = std::fs::read(format!("/proc/{pid}/environ"))
                .map(|b| {
                    let mut keys = vec![];
                    for kv in b.split(|c| *c == 0) {
                        let k = kv.split(|c| *c == b'=').next().unwrap_or_default();
                        if let Some(m) = ENV_MARKERS.iter().find(|m| m.as_bytes() == k) {
                            keys.push(m.to_string());
                        }
                    }
                    keys
                })
                .unwrap_or_default();
            let age = (uptime - st.start as f64 / hz).max(0.0) as u64;
            first.push((
                P {
                    pid,
                    ppid: st.ppid,
                    name: st.comm,
                    exe,
                    argv,
                    rss,
                    age_s: Some(age),
                    env,
                    ..Default::default()
                },
                st.ticks,
            ));
        }
        let t0 = std::time::Instant::now();
        std::thread::sleep(sample);
        let dt = t0.elapsed().as_secs_f64().max(0.001);
        let mut procs = vec![];
        for (mut p, t) in first {
            if let Some(st) = stat(p.pid) {
                p.cpu = (st.ticks.saturating_sub(t) as f64 / hz) / dt * 100.0;
            }
            procs.push(p);
        }
        Snapshot {
            procs,
            worktrees: vec![],
        }
    }

    pub fn fill(snap: &mut Snapshot, pids: &[u32]) {
        for p in snap.procs.iter_mut().filter(|p| pids.contains(&p.pid)) {
            p.cwd = std::fs::read_link(format!("/proc/{}/cwd", p.pid)).ok();
            if let Ok(rd) = std::fs::read_dir(format!("/proc/{}/fd", p.pid)) {
                for fd in rd.flatten() {
                    if let Ok(t) = std::fs::read_link(fd.path()) {
                        if t.is_absolute()
                            && !t.starts_with("/dev")
                            && !t.starts_with("/proc")
                            && !p.files.contains(&t)
                        {
                            p.files.push(t);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{ENV_MARKERS, P, Snapshot, base};
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    fn ps(args: &[&str]) -> Result<String, String> {
        let out = Command::new("ps")
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| format!("could not run ps: {e}"))?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// "[[dd-]hh:]mm:ss" → seconds
    fn etime(s: &str) -> Option<u64> {
        let (days, rest) = match s.split_once('-') {
            Some((d, r)) => (d.parse::<u64>().ok()?, r),
            None => (0, s),
        };
        let mut secs = 0u64;
        for part in rest.split(':') {
            secs = secs * 60 + part.parse::<u64>().ok()?;
        }
        Some(days * 86400 + secs)
    }

    fn pid_rest(l: &str) -> Option<(u32, &str)> {
        let l = l.trim_start();
        let (pid, rest) = l.split_once(char::is_whitespace)?;
        Some((pid.parse().ok()?, rest.trim_start()))
    }

    pub fn collect() -> Result<Snapshot, String> {
        let me = std::process::id();
        // comm is the executable path on macOS and may hold spaces, so it goes last
        let table = ps(&["-axww", "-o", "pid=,ppid=,rss=,%cpu=,etime=,comm="])?;
        let args = ps(&["-axww", "-o", "pid=,command="])?;
        // -E appends the environment of the user's own processes after the arguments
        let with_env = ps(&["-axwwE", "-o", "pid=,command="])?;
        let mut procs = vec![];
        for l in table.lines() {
            let mut it = l.split_whitespace();
            let (Some(pid), Some(ppid), Some(rss), Some(cpu), Some(et)) =
                (it.next(), it.next(), it.next(), it.next(), it.next())
            else {
                continue;
            };
            let Ok(pid) = pid.parse::<u32>() else {
                continue;
            };
            if pid == me {
                continue;
            }
            // the rest of the line after five fields, spaces and all
            let mut exe = l.trim_start();
            for _ in 0..5 {
                exe = exe
                    .split_once(char::is_whitespace)
                    .map(|(_, r)| r.trim_start())
                    .unwrap_or("");
            }
            let exe = exe.trim_end().to_string();
            procs.push(P {
                pid,
                ppid: ppid.parse().unwrap_or(0),
                name: base(&exe).to_string(),
                exe,
                rss: rss.parse::<u64>().unwrap_or(0) * 1024,
                cpu: cpu.replace(',', ".").parse().unwrap_or(0.0),
                age_s: etime(et),
                ..Default::default()
            });
        }
        let mut cmd: std::collections::HashMap<u32, String> = Default::default();
        for l in args.lines() {
            if let Some((pid, rest)) = pid_rest(l) {
                cmd.insert(pid, rest.trim_end().to_string());
            }
        }
        for l in with_env.lines() {
            let Some((pid, rest)) = pid_rest(l) else {
                continue;
            };
            let Some(p) = procs.iter_mut().find(|p| p.pid == pid) else {
                continue;
            };
            let clean = cmd.get(&pid).cloned().unwrap_or_default();
            // ps joins argv with spaces; keep an executable path that has spaces
            // in it ("Codex Helper (Renderer)") as one word
            p.argv = match clean.strip_prefix(p.exe.as_str()) {
                Some(rest) if !p.exe.is_empty() && p.exe.contains(' ') => {
                    std::iter::once(p.exe.clone())
                        .chain(rest.split_whitespace().map(String::from))
                        .collect()
                }
                _ => clean.split_whitespace().map(String::from).collect(),
            };
            if let Some(env) = rest.strip_prefix(clean.as_str()) {
                for kv in env.split_whitespace() {
                    let k = kv.split('=').next().unwrap_or_default();
                    if ENV_MARKERS.contains(&k) && !p.env.iter().any(|e| e == k) {
                        p.env.push(k.to_string());
                    }
                }
            }
        }
        Ok(Snapshot {
            procs,
            worktrees: vec![],
        })
    }

    pub fn fill(snap: &mut Snapshot, pids: &[u32]) {
        if pids.is_empty() {
            return;
        }
        let list: Vec<String> = pids.iter().map(|p| p.to_string()).collect();
        let Ok(out) = Command::new("lsof")
            .args(["-w", "-n", "-P", "-p", &list.join(","), "-Fpfn"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        else {
            return;
        };
        let (mut pid, mut fd) = (0u32, String::new());
        for l in String::from_utf8_lossy(&out.stdout).lines() {
            if l.is_empty() {
                continue;
            }
            let (k, v) = l.split_at(1);
            match k {
                "p" => pid = v.parse().unwrap_or(0),
                "f" => fd = v.to_string(),
                "n" => {
                    let Some(p) = snap.procs.iter_mut().find(|p| p.pid == pid) else {
                        continue;
                    };
                    let path = PathBuf::from(v);
                    if fd == "cwd" {
                        p.cwd = Some(path);
                    } else if fd.chars().next().is_some_and(|c| c.is_ascii_digit())
                        && path.is_absolute()
                        && !path.starts_with("/dev")
                        && !p.files.contains(&path)
                    {
                        p.files.push(path);
                    }
                }
                _ => {}
            }
        }
    }
}
