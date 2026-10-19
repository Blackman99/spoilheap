//! Ask git (never guess) whether a worktree still holds work.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Default)]
pub struct Worktree {
    /// The repository's main working tree (where `git worktree remove` is run).
    pub main_repo: Option<PathBuf>,
    pub branch: Option<String>,
    pub dirty: usize,
    /// Commits reachable from HEAD that no other branch, tag or remote has.
    pub unique: usize,
    /// Git no longer knows this folder (its gitdir is gone).
    pub orphaned: bool,
    pub error: Option<String>,
}

pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0") // never take index.lock just to look
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// `<dir>/.git` is a file (`gitdir: …`) for a linked worktree.
pub fn is_linked_worktree(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git"))
        .map(|m| m.is_file())
        .unwrap_or(false)
}

pub fn is_main_repo(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git"))
        .map(|m| m.is_dir())
        .unwrap_or(false)
}

fn gitdir_of(wt: &Path) -> Option<PathBuf> {
    let s = std::fs::read_to_string(wt.join(".git")).ok()?;
    let p = s.trim().strip_prefix("gitdir:")?.trim();
    let p = PathBuf::from(p);
    Some(if p.is_absolute() { p } else { wt.join(p) })
}

pub fn analyze(wt: &Path) -> Worktree {
    let mut w = Worktree::default();
    let Some(gd) = gitdir_of(wt) else {
        w.orphaned = true;
        return w;
    };
    if !gd.exists() {
        w.orphaned = true;
        return w;
    }
    // The repo's record must point back at this folder; a copied or moved worktree
    // still has a .git link, but git no longer treats it as a working tree.
    if let Ok(back) = std::fs::read_to_string(gd.join("gitdir")) {
        let back = PathBuf::from(back.trim());
        let here = crate::walk::canon(wt.join(".git")).unwrap_or_else(|_| wt.join(".git"));
        let there = crate::walk::canon(&back).unwrap_or(back);
        if here != there {
            w.orphaned = true;
            return w;
        }
    }
    // <common>/worktrees/<name>/commondir → "../.." ; main repo = parent of <common> when it is ".git"
    if let Ok(cd) = std::fs::read_to_string(gd.join("commondir")) {
        let common = gd.join(cd.trim());
        let common = crate::walk::canon(&common).unwrap_or(common);
        if common.file_name().is_some_and(|n| n == ".git") {
            w.main_repo = common.parent().map(Path::to_path_buf);
        } else {
            w.main_repo = Some(common);
        }
    }
    match git(
        wt,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    ) {
        Ok(s) => w.dirty = s.lines().filter(|l| !l.trim().is_empty()).count(),
        Err(e) => {
            w.error = Some(e);
            return w;
        }
    }
    w.branch = git(wt, &["symbolic-ref", "-q", "--short", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let mut args: Vec<String> = vec![
        "rev-list".into(),
        "--count".into(),
        "HEAD".into(),
        "--not".into(),
    ];
    if let Some(b) = &w.branch {
        args.push(format!("--exclude={b}")); // --branches patterns are relative to refs/heads
    }
    args.extend(["--branches".into(), "--tags".into(), "--remotes".into()]);
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match git(wt, &a) {
        Ok(s) => w.unique = s.trim().parse().unwrap_or(0),
        Err(e) => {
            // An unborn HEAD or a broken repo: do not claim it is safe.
            w.error = Some(e);
        }
    }
    w
}

/// Is `rel` (relative to the worktree) ignored by git? Only ignored folders are
/// ever offered as rebuildable build output.
pub fn is_ignored(wt: &Path, rel: &str) -> bool {
    // Nothing inside may be tracked, and git must call the folder ignored.
    match git(wt, &["ls-files", "--", rel]) {
        Ok(s) if s.trim().is_empty() => {}
        _ => return false,
    }
    Command::new("git")
        .arg("-C")
        .arg(wt)
        .args(["check-ignore", "-q", rel])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Registrations whose folder is gone (`git worktree list --porcelain` marks them "prunable").
pub fn prunable(repo: &Path) -> Vec<PathBuf> {
    let Ok(s) = git(repo, &["worktree", "list", "--porcelain"]) else {
        return vec![];
    };
    let mut out = vec![];
    let mut cur: Option<PathBuf> = None;
    for l in s.lines() {
        if let Some(p) = l.strip_prefix("worktree ") {
            cur = Some(PathBuf::from(p));
        } else if l.starts_with("prunable") {
            if let Some(c) = cur.take() {
                out.push(c);
            }
        }
    }
    out
}

pub fn available() -> bool {
    Command::new("git")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
