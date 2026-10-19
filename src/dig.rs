//! Find what each coding agent left on disk and give every item a verdict.

use crate::git;
use crate::sqlite;
use crate::units;
use crate::walk::{self, Measure, Walker};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    Safe,
    Review,
    Keep,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Safe => "SAFE",
            Verdict::Review => "REVIEW",
            Verdict::Keep => "KEEP",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Delete these files/folders, then remove folders left empty under `root`.
    Remove {
        paths: Vec<PathBuf>,
        root: PathBuf,
    },
    /// `git -C <repo> worktree remove <worktree>` (never --force).
    WorktreeRemove {
        repo: PathBuf,
        worktree: PathBuf,
        container: Option<PathBuf>,
    },
    /// `git -C <repo> worktree prune`
    WorktreePrune {
        repo: PathBuf,
    },
    /// spoilheap will not do this itself; it prints the command.
    Manual(String),
    None,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: u32,
    pub harness: &'static str,
    pub kind: &'static str,
    pub label: String,
    pub path: PathBuf,
    pub bytes: u64,
    /// What clearing it gives back (free pages for a database, 0 for KEEP).
    pub reclaim: u64,
    pub files: u64,
    pub newest: Option<SystemTime>,
    pub oldest: Option<SystemTime>,
    pub verdict: Verdict,
    pub reason: String,
    pub action: Action,
    pub parent: Option<usize>,
}

impl Item {
    fn new(
        harness: &'static str,
        kind: &'static str,
        label: impl Into<String>,
        path: &Path,
        m: &Measure,
    ) -> Item {
        Item {
            id: 0,
            harness,
            kind,
            label: label.into(),
            path: path.to_path_buf(),
            bytes: m.bytes,
            reclaim: 0,
            files: m.files,
            newest: m.newest,
            oldest: m.oldest,
            verdict: Verdict::Keep,
            reason: String::new(),
            action: Action::None,
            parent: None,
        }
    }
    fn verdict(mut self, v: Verdict, reason: impl Into<String>, action: Action) -> Item {
        self.verdict = v;
        self.reason = reason.into();
        self.reclaim = if v == Verdict::Keep || matches!(action, Action::None) {
            0
        } else {
            self.bytes
        };
        self.action = action;
        self
    }
    pub fn key(&self) -> String {
        format!("{}|{}", self.kind, self.path.display())
    }
}

#[derive(Debug, Clone)]
pub struct Env {
    pub home: PathBuf,
    pub os: &'static str,
    pub vars: Vec<(String, String)>,
}

impl Env {
    pub fn from_process(home_override: Option<PathBuf>) -> Env {
        let vars: Vec<(String, String)> = std::env::vars().collect();
        let get = |k: &str| {
            vars.iter()
                .find(|(a, _)| a == k)
                .map(|(_, b)| b.clone())
                .filter(|s| !s.is_empty())
        };
        let home = home_override
            .or_else(|| get("SPOILHEAP_HOME").map(PathBuf::from))
            .or_else(|| {
                if cfg!(windows) {
                    get("USERPROFILE")
                } else {
                    get("HOME")
                }
                .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let os = if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(windows) {
            "windows"
        } else {
            "linux"
        };
        let os = get("SPOILHEAP_OS")
            .map(|s| match s.as_str() {
                "macos" => "macos",
                "windows" => "windows",
                _ => "linux",
            })
            .unwrap_or(os);
        Env { home, os, vars }
    }
    pub fn var(&self, k: &str) -> Option<String> {
        self.vars
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, b)| b.clone())
            .filter(|s| !s.is_empty())
    }
    pub fn h(&self, rel: &str) -> PathBuf {
        let mut p = self.home.clone();
        for part in rel.split('/') {
            p.push(part);
        }
        p
    }
    pub fn codex(&self) -> PathBuf {
        self.var("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.h(".codex"))
    }
    pub fn claude(&self) -> PathBuf {
        self.var("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.h(".claude"))
    }
    pub fn xdg_data(&self) -> PathBuf {
        self.var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.h(".local/share"))
    }
    pub fn xdg_config(&self) -> PathBuf {
        self.var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.h(".config"))
    }
    /// Where Electron apps keep per-user data.
    pub fn app_support(&self) -> PathBuf {
        match self.os {
            "macos" => self.h("Library/Application Support"),
            "windows" => self
                .var("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| self.h("AppData/Roaming")),
            _ => self.xdg_config(),
        }
    }
    pub fn cache_dir(&self) -> PathBuf {
        match self.os {
            "macos" => self.h("Library/Caches/spoilheap"),
            "windows" => self
                .var("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| self.h("AppData/Local"))
                .join("spoilheap"),
            _ => self
                .var("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| self.h(".cache"))
                .join("spoilheap"),
        }
    }
    /// "~/.codex/sessions" for display.
    pub fn tilde(&self, p: &Path) -> String {
        match p.strip_prefix(&self.home) {
            Ok(r) if r.as_os_str().is_empty() => "~".into(),
            Ok(r) => format!("~{}{}", std::path::MAIN_SEPARATOR, r.display()),
            Err(_) => p.display().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Opts {
    /// History older than this is REVIEW; newer is KEEP.
    pub older: Duration,
    pub roots: Vec<PathBuf>,
    pub now: SystemTime,
    pub repo_depth: usize,
}

#[derive(Debug, Default)]
pub struct Report {
    pub items: Vec<Item>,
    pub roots: Vec<PathBuf>,
    pub repos: usize,
    pub notes: Vec<String>,
}

pub const BUILD_DIRS: &[(&str, &str)] = &[
    ("target", "cargo build"),
    ("node_modules", "npm install"),
    (".next", "next build"),
    (".turbo", "turbo"),
    ("dist", "your build"),
    ("build", "your build"),
    (".venv", "uv sync / pip install"),
    ("venv", "pip install"),
    (".gradle", "gradle"),
    ("DerivedData", "Xcode"),
    (".build", "swift build"),
    ("__pycache__", "python"),
];

const DAY: Duration = Duration::from_secs(86400);
const HOUR: Duration = Duration::from_secs(3600);

fn is_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p)
        .map(|m| m.is_dir())
        .unwrap_or(false)
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| is_dir(p))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn older_than(t: Option<SystemTime>, now: SystemTime, d: Duration) -> bool {
    t.is_some_and(|t| now.duration_since(t).unwrap_or_default() > d)
}

struct Ctx<'a> {
    env: &'a Env,
    opts: &'a Opts,
    items: Vec<Item>,
    repos: Vec<PathBuf>,
}

impl Ctx<'_> {
    fn push(&mut self, it: Item) -> usize {
        self.items.push(it);
        self.items.len() - 1
    }

    /// Split a history folder into files older than `older` (REVIEW) and newer (KEEP).
    fn split_by_age(
        &mut self,
        harness: &'static str,
        kind: &'static str,
        dir: &Path,
        what: &str,
        why_old: &str,
        why_new: &str,
    ) {
        if !is_dir(dir) {
            return;
        }
        let now = self.opts.now;
        let cutoff = self.opts.older;
        let mut old = Measure::default();
        let mut new = Measure::default();
        let mut old_files = vec![];
        Walker::new().each_file(dir, &mut |p, md| {
            let t = md.modified().ok();
            if older_than(t, now, cutoff) {
                old.add_file(walk::allocated(md), t);
                old_files.push(p.to_path_buf());
            } else {
                new.add_file(walk::allocated(md), t);
            }
        });
        let age = units::duration_label(cutoff);
        if old.files > 0 {
            let it = Item::new(harness, kind, format!("{what} older than {age}"), dir, &old)
                .verdict(
                    Verdict::Review,
                    why_old.to_string(),
                    Action::Remove {
                        paths: old_files,
                        root: dir.to_path_buf(),
                    },
                );
            self.push(it);
        }
        if new.files > 0 {
            let it = Item::new(harness, "recent", format!("{what}, last {age}"), dir, &new)
                .verdict(Verdict::Keep, why_new.to_string(), Action::None);
            self.push(it);
        }
    }

    /// Files older than `min_age` are SAFE to delete; newer ones are left alone.
    fn safe_files(
        &mut self,
        harness: &'static str,
        kind: &'static str,
        dir: &Path,
        label: &str,
        reason: &str,
        min_age: Duration,
    ) {
        if !is_dir(dir) {
            return;
        }
        let now = self.opts.now;
        let mut m = Measure::default();
        let mut files = vec![];
        Walker::new().each_file(dir, &mut |p, md| {
            let t = md.modified().ok();
            if older_than(t, now, min_age) {
                m.add_file(walk::allocated(md), t);
                files.push(p.to_path_buf());
            }
        });
        if m.files > 0 {
            let it = Item::new(harness, kind, label, dir, &m).verdict(
                Verdict::Safe,
                reason,
                Action::Remove {
                    paths: files,
                    root: dir.to_path_buf(),
                },
            );
            self.push(it);
        }
    }

    fn codex(&mut self) {
        let c = self.env.codex();
        if !is_dir(&c) {
            return;
        }
        self.split_by_age(
            "Codex",
            "sessions",
            &c.join("sessions"),
            "sessions",
            "chat rollouts; `codex resume` can reopen them, nothing else reads them",
            "recent chat rollouts",
        );
        let arch = c.join("archived_sessions");
        if is_dir(&arch) {
            let m = walk::measure(&arch);
            if m.files > 0 {
                let it = Item::new("Codex", "archived-sessions", "archived sessions", &arch, &m)
                    .verdict(
                        Verdict::Review,
                        "threads you archived in Codex; they stay on disk until deleted",
                        Action::Remove {
                            paths: vec![arch.clone()],
                            root: c.clone(),
                        },
                    );
                self.push(it);
            }
        }
        let wts = c.join("worktrees");
        self.worktree_container("Codex", &wts);
        self.safe_files(
            "Codex",
            "logs",
            &c.join("log"),
            "logs older than 1 d",
            "log files; Codex writes new ones as it goes",
            DAY,
        );
        for e in std::fs::read_dir(&c)
            .map(|r| r.flatten().map(|e| e.path()).collect::<Vec<_>>())
            .unwrap_or_default()
        {
            let n = e
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if n.starts_with("logs_") && n.ends_with(".sqlite")
                || n.starts_with("state_") && n.ends_with(".sqlite")
            {
                self.sqlite_db("Codex", &e, "quit Codex");
            }
        }
        let tmp = c.join(".tmp");
        if is_dir(&tmp) {
            let m = walk::measure(&tmp);
            if m.bytes > 0 {
                let fresh = !older_than(m.newest, self.opts.now, DAY);
                let it = Item::new("Codex", "tmp", "plugin temp clones (.tmp)", &tmp, &m);
                let it = if fresh {
                    it.verdict(
                        Verdict::Review,
                        "written in the last day: a plugin refresh may be running",
                        Action::Remove {
                            paths: subdirs(&tmp),
                            root: tmp.clone(),
                        },
                    )
                } else {
                    it.verdict(
                        Verdict::Safe,
                        "leftover plugin/marketplace clones (openai/codex#55068)",
                        Action::Remove {
                            paths: subdirs(&tmp),
                            root: tmp.clone(),
                        },
                    )
                };
                self.push(it);
            }
        }
        // bun keeps every Codex build it ever installed
        let bun = self.env.h(".bun/install/cache/@openai");
        if is_dir(&bun) {
            let mut builds: Vec<(PathBuf, Measure)> = subdirs(&bun)
                .into_iter()
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("codex"))
                })
                .map(|p| {
                    let m = walk::measure(&p);
                    (p, m)
                })
                .collect();
            builds.sort_by_key(|(_, m)| std::cmp::Reverse(m.newest));
            if builds.len() > 1 {
                let mut m = Measure::default();
                let mut paths = vec![];
                for (p, mm) in builds.iter().skip(1) {
                    m.merge(mm);
                    paths.push(p.clone());
                }
                let it = Item::new(
                    "Codex",
                    "bun-cache",
                    format!("{} old Codex builds in bun's cache", paths.len()),
                    &bun,
                    &m,
                )
                .verdict(
                    Verdict::Safe,
                    "the newest build is kept; bun downloads again if one is needed",
                    Action::Remove {
                        paths,
                        root: bun.clone(),
                    },
                );
                self.push(it);
            }
        }
    }

    fn crash_dumps(&mut self) {
        let apps = [
            ("Codex", "Codex"),
            ("ChatGPT", "Codex"),
            ("Claude", "Claude Code"),
            ("Cursor", "Cursor"),
        ];
        for (app, harness) in apps {
            let base = self.env.app_support().join(app);
            for sub in ["Crashpad", "Crashpad/pending", "Crash Reports"] {
                let d = base.join(sub);
                if sub == "Crashpad/pending" || !is_dir(&d) {
                    continue;
                }
                let now = self.opts.now;
                let mut recent = 0u64;
                let mut files = vec![];
                // count only the dumps: Crashpad's settings.dat / metadata stay and are not reported
                let mut m = crate::walk::Measure::default();
                Walker::new().each_file(&d, &mut |p, md| {
                    let n = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    if n == "settings.dat" || n == "metadata" {
                        return; // Crashpad's own state, not a dump
                    }
                    if !older_than(md.modified().ok(), now, HOUR) {
                        recent += 1;
                    }
                    m.add_file(crate::walk::allocated(md), md.modified().ok());
                    files.push(p.to_path_buf());
                });
                if files.is_empty() {
                    continue;
                }
                let mut reason =
                    "crash reports; nothing reads them after they are written".to_string();
                if recent >= 30 {
                    reason = format!(
                        "crash loop: {} dumps in the last hour (openai/codex#25921). Quit {app} first or the pile grows straight back",
                        units::thousands(recent)
                    );
                }
                let label = format!("{app} crash dumps");
                let it = Item::new(harness, "crash-dumps", label, &d, &m).verdict(
                    Verdict::Safe,
                    reason,
                    Action::Remove {
                        paths: files,
                        root: d.clone(),
                    },
                );
                self.push(it);
            }
        }
    }

    fn claude(&mut self) {
        let c = self.env.claude();
        if !is_dir(&c) {
            return;
        }
        self.split_by_age(
            "Claude Code",
            "transcripts",
            &c.join("projects"),
            "transcripts",
            "session transcripts; Claude Code deletes these after cleanupPeriodDays (default 30), so cleanup is off or raised here",
            "recent session transcripts",
        );
        self.split_by_age(
            "Claude Code",
            "file-history",
            &c.join("file-history"),
            "file history",
            "pre-edit copies behind /rewind for old sessions",
            "pre-edit copies behind /rewind",
        );
        self.safe_files(
            "Claude Code",
            "shell-snapshots",
            &c.join("shell-snapshots"),
            "shell snapshots older than 1 d",
            "captured shell environments; each new session writes a fresh one",
            DAY,
        );
        self.safe_files(
            "Claude Code",
            "logs",
            &c.join("debug"),
            "debug logs older than 1 d",
            "debug logs",
            DAY,
        );
    }

    fn opencode(&mut self) {
        let d = self.env.xdg_data().join("opencode");
        if !is_dir(&d) {
            return;
        }
        let mut dbs: Vec<PathBuf> = std::fs::read_dir(&d)
            .map(|r| r.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        dbs.retain(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with("opencode") && n.ends_with(".db")
            })
        });
        if let Some(v) = self.env.var("OPENCODE_DB") {
            let p = PathBuf::from(v);
            if p.exists() && !dbs.contains(&p) {
                dbs.push(p);
            }
        }
        dbs.sort();
        for db in dbs {
            self.sqlite_db("OpenCode", &db, "quit every OpenCode instance");
        }
        let snap = d.join("snapshot");
        for proj in subdirs(&snap) {
            let m = walk::measure(&proj);
            if m.bytes == 0 {
                continue;
            }
            let id = proj
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let short: String = id.chars().take(10).collect();
            let label = format!("undo snapshots · project {short}");
            let it = Item::new("OpenCode", "snapshots", label, &proj, &m);
            let it = if older_than(m.newest, self.opts.now, self.opts.older) {
                it.verdict(
                    Verdict::Review,
                    format!(
                        "git snapshots behind /undo; untouched for {}+ (anomalyco/opencode#55010)",
                        units::duration_label(self.opts.older)
                    ),
                    Action::Remove {
                        paths: vec![proj.clone()],
                        root: snap.clone(),
                    },
                )
            } else {
                it.verdict(
                    Verdict::Keep,
                    "git snapshots behind /undo for a project you used recently",
                    Action::None,
                )
            };
            self.push(it);
        }
        self.split_by_age(
            "OpenCode",
            "storage",
            &d.join("storage"),
            "session files (storage/)",
            "per-session JSON files from before the SQLite store",
            "per-session JSON files",
        );
        self.safe_files(
            "OpenCode",
            "logs",
            &d.join("log"),
            "logs older than 1 d",
            "log files",
            DAY,
        );
    }

    fn sqlite_db(&mut self, harness: &'static str, db: &Path, quit: &str) {
        let Ok(md) = std::fs::metadata(db) else {
            return;
        };
        let mut m = Measure::default();
        m.add_file(walk::allocated(&md), md.modified().ok());
        let wal = PathBuf::from(format!("{}-wal", db.display()));
        let wal_bytes = std::fs::metadata(&wal)
            .map(|m| walk::allocated(&m))
            .unwrap_or(0);
        if wal_bytes > 0 {
            m.add_file(wal_bytes, None);
        }
        let name = db
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(h) = sqlite::header(db) else {
            let it = Item::new(harness, "database", name, db, &m).verdict(
                Verdict::Keep,
                "database (header unreadable)",
                Action::None,
            );
            self.push(it);
            return;
        };
        let free = h.free_bytes();
        let size = md.len();
        let mut detail = String::new();
        if let Some(t) = sqlite::top_tables(db) {
            let parts: Vec<String> = t
                .iter()
                .take(2)
                .map(|(n, b)| format!("{n} {}", units::bytes(*b)))
                .collect();
            detail = format!("; biggest: {}", parts.join(", "));
        }
        let wal_note = if wal_bytes > 1_000_000_000 {
            format!(
                "; its WAL is {} (a long reader is starving checkpoints, anomalyco/opencode#56827)",
                units::bytes(wal_bytes)
            )
        } else {
            String::new()
        };
        // SPOILHEAP_VACUUM_MIN lowers the "worth a VACUUM" bar (default 50 MB and 20%, or 500 MB)
        let min = std::env::var("SPOILHEAP_VACUUM_MIN")
            .ok()
            .and_then(|v| units::parse_amount(&v).ok())
            .map(|a| a.resolve(size))
            .unwrap_or(50_000_000);
        let worth = free >= 500_000_000 || (size > 0 && free * 5 >= size && free >= min);
        if worth {
            let label = format!(
                "{name}: {} of {} is free pages",
                units::bytes(free),
                units::bytes(size)
            );
            let mut it = Item::new(harness, "database", label, db, &m).verdict(
                Verdict::Review,
                format!(
                    "SQLite never shrinks on DELETE; VACUUM gives the free pages back. It needs ~{} free while it runs{detail}{wal_note}",
                    units::bytes(size.saturating_sub(free))
                ),
                Action::Manual(format!("{quit}, then: sqlite3 '{}' 'VACUUM;'", db.display())),
            );
            it.reclaim = free;
            self.push(it);
        } else {
            let it = Item::new(harness, "database", name, db, &m).verdict(
                Verdict::Keep,
                format!(
                    "live database, {} free pages{detail}{wal_note}",
                    units::bytes(free)
                ),
                Action::None,
            );
            self.push(it);
        }
    }

    fn gemini(&mut self) {
        for base in [self.env.h(".gemini/tmp")] {
            for proj in subdirs(&base) {
                let m = walk::measure(&proj);
                if m.bytes == 0 {
                    continue;
                }
                let id: String = proj
                    .file_name()
                    .map(|n| n.to_string_lossy().chars().take(10).collect())
                    .unwrap_or_default();
                let it = Item::new(
                    "Gemini CLI",
                    "gemini-tmp",
                    format!("chats & checkpoints · project {id}"),
                    &proj,
                    &m,
                );
                let it = if older_than(m.newest, self.opts.now, self.opts.older) {
                    it.verdict(
                        Verdict::Review,
                        format!(
                            "saved chats, checkpoints and logs; untouched for {}+",
                            units::duration_label(self.opts.older)
                        ),
                        Action::Remove {
                            paths: vec![proj.clone()],
                            root: base.clone(),
                        },
                    )
                } else {
                    it.verdict(
                        Verdict::Keep,
                        "chats and checkpoints of a project you used recently",
                        Action::None,
                    )
                };
                self.push(it);
            }
        }
    }

    fn cline(&mut self) {
        let mut bases = vec![self.env.h(".cline/data")];
        for ed in ["Code", "Cursor", "Windsurf", "VSCodium"] {
            bases.push(
                self.env
                    .app_support()
                    .join(ed)
                    .join("User/globalStorage/saoudrizwan.claude-dev"),
            );
        }
        for b in bases {
            for (sub, what) in [
                ("tasks", "task histories"),
                ("checkpoints", "checkpoint repos"),
            ] {
                let d = b.join(sub);
                if !is_dir(&d) {
                    continue;
                }
                let mut old = Measure::default();
                let mut new = Measure::default();
                let mut old_paths = vec![];
                for t in subdirs(&d) {
                    let m = walk::measure(&t);
                    if older_than(m.newest, self.opts.now, self.opts.older) {
                        old.merge(&m);
                        old_paths.push(t);
                    } else {
                        new.merge(&m);
                    }
                }
                let age = units::duration_label(self.opts.older);
                if !old_paths.is_empty() {
                    let it = Item::new("Cline", "cline", format!("{what} older than {age}"), &d, &old).verdict(
                        Verdict::Review,
                        format!("{} folder{} Cline keeps per task; those tasks can no longer be reopened once removed", old_paths.len(), plural(old_paths.len())),
                        Action::Remove { paths: old_paths, root: d.clone() },
                    );
                    self.push(it);
                }
                if new.files > 0 {
                    let it = Item::new("Cline", "recent", format!("{what}, last {age}"), &d, &new)
                        .verdict(Verdict::Keep, "recent tasks", Action::None);
                    self.push(it);
                }
            }
        }
    }

    /// `~/.codex/worktrees/<id>/<repo>` or `<repo>/.claude/worktrees/<name>`.
    fn worktree_container(&mut self, harness: &'static str, container: &Path) {
        if !is_dir(container) {
            return;
        }
        for d in subdirs(container) {
            if git::is_linked_worktree(&d) || git::is_main_repo(&d) {
                self.worktree(harness, &d, container);
                continue;
            }
            let inner: Vec<PathBuf> = subdirs(&d)
                .into_iter()
                .filter(|p| git::is_linked_worktree(p))
                .collect();
            if !inner.is_empty() {
                for w in inner {
                    self.worktree(harness, &w, container);
                }
                // anything else left beside the worktrees is counted with them
                continue;
            }
            let m = walk::measure(&d);
            if m.bytes == 0 {
                continue;
            }
            let it = Item::new(harness, "worktree", "folder in the worktree container (not a git worktree)", &d, &m).verdict(
                Verdict::Review,
                "no .git link inside: git removed the worktree but left files, or something else wrote here",
                Action::Remove { paths: vec![d.clone()], root: container.to_path_buf() },
            );
            self.push(it);
        }
    }

    fn worktree(&mut self, harness: &'static str, wt: &Path, container: &Path) {
        let info = git::analyze(wt);
        // rebuildable output inside it, measured on its own
        let mut walker = Walker::new();
        let mut builds: Vec<(PathBuf, &str, Measure)> = vec![];
        if !info.orphaned && info.error.is_none() {
            for (p, how) in find_build_dirs(wt, 3) {
                let rel = p
                    .strip_prefix(wt)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                if git::is_ignored(wt, &rel) {
                    let m = walk::measure(&p);
                    walker.skip.insert(p.clone());
                    builds.push((p, how, m));
                }
            }
        }
        walker.skip.insert(wt.join(".git"));
        let m = walker.measure(wt);
        let in_use: Vec<String> = crate::procs::cwds()
            .into_iter()
            .filter(|(_, _, c)| c.starts_with(wt))
            .map(|(p, n, _)| format!("{n}[{p}]"))
            .collect();
        let base = wt
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = if info.orphaned {
            format!("former worktree {base}")
        } else {
            match &info.branch {
                Some(b) => format!("worktree {b}"),
                None => format!("worktree {base} (detached)"),
            }
        };
        let fresh = !older_than(m.newest, self.opts.now, HOUR);
        let it = Item::new(harness, "worktree", name, wt, &m);
        let remove = Action::WorktreeRemove {
            repo: info.main_repo.clone().unwrap_or_else(|| wt.to_path_buf()),
            worktree: wt.to_path_buf(),
            container: Some(container.to_path_buf()),
        };
        let it = if info.orphaned {
            it.verdict(
                Verdict::Review,
                "git no longer treats this as a worktree (its repo was moved, pruned, or this is a copy); the files are a plain folder now",
                Action::Remove { paths: vec![wt.to_path_buf()], root: container.to_path_buf() },
            )
        } else if let Some(e) = &info.error {
            it.verdict(
                Verdict::Keep,
                format!("git could not read it: {}", first_line(e)),
                Action::None,
            )
        } else if !in_use.is_empty() {
            it.verdict(
                Verdict::Keep,
                format!(
                    "in use right now: {} has its working folder here",
                    in_use.join(", ")
                ),
                Action::None,
            )
        } else if info.dirty > 0 {
            it.verdict(
                Verdict::Keep,
                format!(
                    "{} uncommitted change{}: real work",
                    info.dirty,
                    plural(info.dirty)
                ),
                Action::None,
            )
        } else if info.unique > 0 {
            it.verdict(
                Verdict::Review,
                format!(
                    "clean, but {} commit{} exist{} only here; push or merge first",
                    info.unique,
                    plural(info.unique),
                    if info.unique == 1 { "s" } else { "" }
                ),
                remove,
            )
        } else if fresh {
            let a = units::age(units::age_secs(
                m.newest.unwrap_or(self.opts.now),
                self.opts.now,
            ));
            let when = if a == "just now" {
                a
            } else {
                format!("{a} ago")
            };
            it.verdict(Verdict::Review, format!("clean and merged, but a file changed {when}: an agent may still be working in it"), remove)
        } else {
            it.verdict(
                Verdict::Safe,
                "clean, and every commit is also on another branch or remote",
                remove,
            )
        };
        let idx = self.push(it);
        if let Some(r) = &info.main_repo {
            let r = &crate::walk::canon(r).unwrap_or_else(|_| r.clone());
            if !self.repos.contains(r) {
                self.repos.push(r.clone());
            }
        }
        for (p, how, bm) in builds {
            if bm.bytes < 1_000_000 {
                continue;
            }
            let dn = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut b = Item::new(
                harness,
                "build-output",
                format!("build output {dn}/"),
                &p,
                &bm,
            )
            .verdict(
                Verdict::Safe,
                format!("git-ignored and rebuildable ({how}); the worktree itself is untouched"),
                Action::Remove {
                    paths: vec![p.clone()],
                    root: wt.to_path_buf(),
                },
            );
            b.parent = Some(idx);
            self.push(b);
        }
    }

    fn repo(&mut self, repo: &Path) {
        let pack = repo.join(".git/objects/pack");
        let now = self.opts.now;
        let mut stale = Measure::default();
        let mut fresh = Measure::default();
        let mut stale_paths = vec![];
        for e in std::fs::read_dir(&pack)
            .map(|r| r.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            let n = e.file_name().to_string_lossy().into_owned();
            if !(n.starts_with("tmp_pack_") || n.starts_with("tmp_idx_") || n.starts_with(".tmp-"))
            {
                continue;
            }
            let Ok(md) = e.path().symlink_metadata() else {
                continue;
            };
            let t = md.modified().ok();
            if older_than(t, now, HOUR) {
                stale.add_file(walk::allocated(&md), t);
                stale_paths.push(e.path());
            } else {
                fresh.add_file(walk::allocated(&md), t);
            }
        }
        let objs = repo.join(".git/objects");
        for e in std::fs::read_dir(&objs)
            .map(|r| r.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("incoming-")
                && older_than(e.metadata().ok().and_then(|m| m.modified().ok()), now, HOUR)
            {
                let m = walk::measure(&e.path());
                stale.merge(&m);
                stale_paths.push(e.path());
            }
        }
        let rname = repo
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if stale.files > 0 {
            let it = Item::new("Repos", "tmp-pack", format!("{rname}: {} half-written git packs", units::thousands(stale.files)), &pack, &stale).verdict(
                Verdict::Safe,
                "tmp_pack_* from a git that died mid-write (openai/codex#54955); untouched for 1 h+",
                Action::Remove { paths: stale_paths, root: pack.clone() },
            );
            self.push(it);
        }
        if fresh.files > 0 {
            let it = Item::new("Repos", "tmp-pack", format!("{rname}: git packs being written now"), &pack, &fresh).verdict(
                Verdict::Keep,
                "tmp_pack_* written in the last hour: a fetch or gc may be running. `spoilheap watch` shows who",
                Action::None,
            );
            self.push(it);
        }
        let pr = git::prunable(repo);
        if !pr.is_empty() {
            let mut it = Item::new(
                "Repos",
                "worktree-registration",
                format!(
                    "{rname}: {} stale worktree registration{}",
                    pr.len(),
                    plural(pr.len())
                ),
                repo,
                &Measure::default(),
            )
            .verdict(
                Verdict::Safe,
                "git still lists worktrees whose folders are gone (openai/codex#54891)",
                Action::WorktreePrune {
                    repo: repo.to_path_buf(),
                },
            );
            it.reclaim = 0;
            self.push(it);
        }
        self.worktree_container("Claude Code", &repo.join(".claude/worktrees"));
        for e in std::fs::read_dir(repo)
            .map(|r| r.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(".aider.tags.cache") && is_dir(&e.path()) {
                let m = walk::measure(&e.path());
                if m.bytes > 0 {
                    let it = Item::new(
                        "Aider",
                        "aider-cache",
                        format!("{rname}: Aider repo-map cache"),
                        &e.path(),
                        &m,
                    )
                    .verdict(
                        Verdict::Safe,
                        "Aider rebuilds its tag cache on the next run",
                        Action::Remove {
                            paths: vec![e.path()],
                            root: repo.to_path_buf(),
                        },
                    );
                    self.push(it);
                }
            }
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(90).collect()
}

fn find_build_dirs(root: &Path, depth: usize) -> Vec<(PathBuf, &'static str)> {
    let mut out = vec![];
    fn rec(p: &Path, d: usize, out: &mut Vec<(PathBuf, &'static str)>) {
        for c in subdirs(p) {
            let n = c
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if n == ".git" {
                continue;
            }
            if let Some((_, how)) = BUILD_DIRS.iter().find(|(b, _)| *b == n) {
                out.push((c, *how));
                continue;
            }
            if d > 1 && !n.starts_with('.') {
                rec(&c, d - 1, out);
            }
        }
    }
    rec(root, depth, &mut out);
    out
}

/// Repositories under the roots (depth-limited), never descending into build output.
pub fn find_repos(roots: &[PathBuf], depth: usize) -> Vec<PathBuf> {
    let mut out = vec![];
    fn rec(p: &Path, d: usize, out: &mut Vec<PathBuf>) {
        if git::is_main_repo(p) {
            out.push(p.to_path_buf());
            return;
        }
        if d == 0 {
            return;
        }
        for c in subdirs(p) {
            let n = c
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if n.starts_with('.') || BUILD_DIRS.iter().any(|(b, _)| *b == n) || n == "Library" {
                continue;
            }
            rec(&c, d - 1, out);
        }
    }
    for r in roots {
        rec(r, depth, &mut out);
    }
    out
}

pub fn default_roots(env: &Env) -> Vec<PathBuf> {
    let mut v = vec![];
    if let Ok(cwd) = std::env::current_dir() {
        if cwd != env.home {
            v.push(cwd);
        }
    }
    for d in [
        "code",
        "src",
        "projects",
        "dev",
        "work",
        "Developer",
        "repos",
        "git",
        "workspace",
        "Projects",
        "Code",
    ] {
        let p = env.home.join(d);
        if is_dir(&p) && !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

pub fn dig(env: &Env, opts: &Opts) -> Report {
    let mut cx = Ctx {
        env,
        opts,
        items: vec![],
        repos: vec![],
    };
    cx.codex();
    cx.crash_dumps();
    cx.claude();
    cx.opencode();
    cx.gemini();
    cx.cline();
    let mut repos: Vec<PathBuf> = vec![];
    for r in find_repos(&opts.roots, opts.repo_depth) {
        let r = crate::walk::canon(&r).unwrap_or(r);
        if !repos.contains(&r) {
            repos.push(r);
        }
    }
    for r in cx.repos.clone() {
        if !repos.contains(&r) {
            repos.push(r);
        }
    }
    let have_git = git::available();
    if have_git {
        for r in &repos {
            cx.repo(r);
        }
    }
    let mut rep = Report {
        roots: opts.roots.clone(),
        repos: repos.len(),
        ..Default::default()
    };
    if !have_git {
        rep.notes
            .push("git is not on PATH: worktrees and repos were not checked".into());
    }
    rep.items = order(cx.items);
    rep
}

pub const HARNESSES: &[&str] = &[
    "Codex",
    "Claude Code",
    "OpenCode",
    "Gemini CLI",
    "Cline",
    "Cursor",
    "Aider",
    "Repos",
];

/// Group by harness, biggest first, children right after their parent; number from 101.
fn order(items: Vec<Item>) -> Vec<Item> {
    let mut out: Vec<Item> = vec![];
    for h in HARNESSES {
        let mut tops: Vec<usize> = (0..items.len())
            .filter(|&i| items[i].harness == *h && items[i].parent.is_none())
            .collect();
        tops.sort_by(|&a, &b| {
            items[b]
                .bytes
                .cmp(&items[a].bytes)
                .then(items[a].path.cmp(&items[b].path))
        });
        for t in tops {
            let new_parent = out.len();
            let mut it = items[t].clone();
            it.parent = None;
            out.push(it);
            let mut kids: Vec<usize> = (0..items.len())
                .filter(|&i| items[i].parent == Some(t))
                .collect();
            kids.sort_by(|&a, &b| items[b].bytes.cmp(&items[a].bytes));
            for k in kids {
                let mut c = items[k].clone();
                c.parent = Some(new_parent);
                out.push(c);
            }
        }
    }
    for (i, it) in out.iter_mut().enumerate() {
        it.id = 101 + i as u32;
    }
    out
}
