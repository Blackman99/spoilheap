//! Clear chosen items: a dry run unless --yes, KEEP is never touched, databases are
//! never edited, worktrees go through `git worktree remove` (which refuses dirty ones).

use crate::dig::{Action, Env, Item, Report, Verdict};
use crate::disk;
use crate::git;
use crate::render::Style;
use crate::units;
use std::path::{Path, PathBuf};

pub struct Choice {
    pub safe: bool,
    pub ids: Vec<u32>,
    pub yes: bool,
}

pub fn select<'a>(env: &Env, rep: &'a Report, ch: &Choice, err: &mut Vec<String>) -> Vec<&'a Item> {
    let mut out: Vec<&Item> = vec![];
    if ch.safe {
        out.extend(rep.items.iter().filter(|i| i.verdict == Verdict::Safe));
    }
    if !ch.ids.is_empty() {
        let map = crate::out::load_ids(env);
        for id in &ch.ids {
            let found = match &map {
                Some((m, _)) => m.iter().find(|(i, _)| i == id).and_then(|(_, key)| rep.items.iter().find(|it| &it.key() == key)),
                None => rep.items.iter().find(|it| it.id == *id),
            };
            match found {
                Some(it) => {
                    if !out.iter().any(|o| o.key() == it.key()) {
                        out.push(it)
                    }
                }
                None => err.push(format!("[{id}] is not in the last dig any more (it may be gone already). Run `spoilheap` to see current numbers.")),
            }
        }
    }
    out
}

fn remove_paths(paths: &[PathBuf], root: &Path) -> (u64, Vec<String>) {
    let mut errs = vec![];
    let mut n = 0;
    for p in paths {
        // only ever inside the item's root, never through a symlink
        if !p.starts_with(root) {
            errs.push(format!("refused {} (outside {})", p.display(), root.display()));
            continue;
        }
        let Ok(md) = std::fs::symlink_metadata(p) else { continue };
        let r = if md.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
        match r {
            Ok(()) => n += 1,
            Err(e) => errs.push(format!("{}: {e}", p.display())),
        }
        // remove folders the deletion left empty, up to the root
        let mut d = p.parent();
        while let Some(dir) = d {
            if dir == root || !dir.starts_with(root) {
                break;
            }
            if std::fs::remove_dir(dir).is_err() {
                break;
            }
            d = dir.parent();
        }
    }
    (n, errs)
}

pub fn describe(env: &Env, it: &Item) -> String {
    match &it.action {
        Action::Remove { paths, .. } => {
            if paths.len() == 1 && paths[0] == it.path {
                format!("delete {}", env.tilde(&it.path))
            } else {
                format!("delete {} {} under {}", units::thousands(paths.len() as u64), if paths.len() == 1 { "entry" } else { "entries" }, env.tilde(&it.path))
            }
        }
        Action::WorktreeRemove { repo, worktree, .. } => format!("git -C {} worktree remove {}", env.tilde(repo), env.tilde(worktree)),
        Action::WorktreePrune { repo } => format!("git -C {} worktree prune", env.tilde(repo)),
        Action::Manual(c) => format!("by hand: {c}"),
        Action::None => "nothing to do".into(),
    }
}

pub fn run(env: &Env, rep: &Report, ch: &Choice, st: &Style) -> i32 {
    let mut errs = vec![];
    let chosen = select(env, rep, ch, &mut errs);
    for e in &errs {
        eprintln!("{} {e}", st.yellow("skip"));
    }
    if chosen.is_empty() {
        if ch.safe {
            println!("Nothing is SAFE to clear right now. `spoilheap` lists REVIEW items you can pick by number.");
            return 0;
        }
        return if errs.is_empty() { 0 } else { 1 };
    }
    let probe = chosen.first().map(|i| i.path.clone()).unwrap_or_else(|| env.home.clone());
    let before = disk::space(&probe).ok();
    let mut planned = 0u64;
    let mut code = 0;
    println!("{}", st.bold(if ch.yes { "clearing" } else { "plan (dry run)" }));
    for it in &chosen {
        let tag = format!("[{}]", it.id);
        if it.verdict == Verdict::Keep {
            println!("  {} {} {} {}", st.dim(&tag), st.verdict(it.verdict), it.label, st.dim(&format!("— KEEP is never touched ({})", it.reason)));
            continue;
        }
        if let Action::Manual(c) = &it.action {
            println!("  {} {} {}  {}", st.dim(&tag), st.verdict(it.verdict), it.label, st.dim("— spoilheap never edits a database; run:"));
            println!("        {c}");
            continue;
        }
        let what = describe(env, it);
        println!("  {} {} {:<44} {:>9}  {}", st.dim(&tag), st.verdict(it.verdict), crate::render::clip(&it.label, 44), if it.reclaim == 0 { "—".to_string() } else { units::bytes(it.reclaim) }, st.dim(&what));
        planned += it.reclaim;
        if !ch.yes {
            continue;
        }
        let res: Result<(), String> = match &it.action {
            Action::Remove { paths, root } => {
                let (_, e) = remove_paths(paths, root);
                if e.is_empty() { Ok(()) } else { Err(e.join("; ")) }
            }
            Action::WorktreeRemove { repo, worktree, container } => {
                // re-check right before acting: an agent may have written since the dig
                let now = git::analyze(worktree);
                if now.dirty > 0 {
                    Err(format!("now has {} uncommitted change(s); left alone", now.dirty))
                } else {
                    match git::git(repo, &["worktree", "remove", &worktree.display().to_string()]) {
                        Ok(_) => {
                            if let Some(c) = container {
                                let mut d = worktree.parent();
                                while let Some(dir) = d {
                                    if dir == c.as_path() || std::fs::remove_dir(dir).is_err() {
                                        break;
                                    }
                                    d = dir.parent();
                                }
                            }
                            Ok(())
                        }
                        Err(e) => Err(format!("git refused: {}", e.lines().next().unwrap_or(""))),
                    }
                }
            }
            Action::WorktreePrune { repo } => git::git(repo, &["worktree", "prune"]).map(|_| ()),
            _ => Ok(()),
        };
        match res {
            Ok(()) => println!("        {}", st.green("done")),
            Err(e) => {
                println!("        {} {e}", st.red("failed"));
                code = 1;
            }
        }
    }
    if !ch.yes {
        println!("\n{} would free {}. Nothing was deleted. Run the same command with --yes to do it.", st.bold("dry run:"), units::bytes(planned));
        return code;
    }
    if let (Some(b), Ok(a)) = (before, disk::space(&probe)) {
        let gained = a.free.saturating_sub(b.free);
        println!("\nfree space {} → {} (+{})", units::bytes(b.free), units::bytes(a.free), units::bytes(gained));
        if planned > 2_000_000_000 && gained * 10 < planned * 7 {
            let running = crate::procs::running_agents(&crate::procs::list());
            let who = if running.is_empty() { "a running process".to_string() } else { running.join(", ") };
            println!(
                "{} the disk got back less than planned. Deleted files that {who} still has open are only freed when it exits.",
                st.yellow("note")
            );
        }
    }
    code
}
