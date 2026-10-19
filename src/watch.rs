//! `spoilheap watch`: sample agent locations and name what is growing fast.

use crate::dig::{self, Env, Opts};
use crate::disk;
use crate::procs;
use crate::render::Style;
use crate::units::{self, Amount};
use crate::walk::{self, Walker};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct WatchOpts {
    pub every: Duration,
    pub rate: u64, // bytes per minute that counts as runaway
    pub floor: Amount,
    pub ticks: Option<u64>,
}

pub fn targets(env: &Env, rep: &dig::Report) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    let mut add = |p: PathBuf| {
        if p.exists() && !v.iter().any(|x| p.starts_with(x)) {
            v.retain(|x| !x.starts_with(&p));
            v.push(p);
        }
    };
    add(env.codex());
    add(env.claude());
    add(env.xdg_data().join("opencode"));
    add(env.h(".gemini"));
    add(env.h(".cline"));
    for app in ["Codex", "ChatGPT", "Claude", "Cursor"] {
        add(env.app_support().join(app).join("Crashpad"));
    }
    for it in &rep.items {
        if it.kind == "tmp-pack" || it.kind == "worktree" {
            add(it.path.clone());
        }
    }
    v
}

fn newest_files(dir: &Path, n: usize) -> Vec<PathBuf> {
    let mut all: Vec<(std::time::SystemTime, PathBuf)> = vec![];
    Walker::new().each_file(dir, &mut |p, md| {
        if let Ok(t) = md.modified() {
            all.push((t, p.to_path_buf()));
        }
    });
    all.sort_by_key(|a| std::cmp::Reverse(a.0));
    all.into_iter().take(n).map(|(_, p)| p).collect()
}

pub fn run(env: &Env, opts: &Opts, w: &WatchOpts, st: &Style) -> i32 {
    let rep = dig::dig(env, opts);
    let mut tg = targets(env, &rep);
    // a pack directory is where codex#54955 happened: always watch the repos' packs
    for r in dig::find_repos(&opts.roots, opts.repo_depth) {
        let p = r.join(".git/objects/pack");
        if p.exists() && !tg.contains(&p) {
            tg.push(p);
        }
    }
    if tg.is_empty() {
        println!(
            "Nothing to watch: no agent folders found. Pass --root ~/code to watch your repos' git packs."
        );
        return 0;
    }
    println!(
        "{} {} location{} every {} · alarm at {}/min or under {} free · Ctrl-C to stop",
        st.bold("watching"),
        tg.len(),
        if tg.len() == 1 { "" } else { "s" },
        units::duration_label(w.every),
        units::bytes(w.rate),
        w.floor.label()
    );
    for t in &tg {
        println!("  {}", st.dim(&env.tilde(t)));
    }
    let mut last: Vec<u64> = tg.iter().map(|t| walk::measure(t).bytes).collect();
    let mut t0 = Instant::now();
    let mut tick = 0u64;
    let mut quiet_since = Instant::now();
    loop {
        std::thread::sleep(w.every);
        tick += 1;
        let dt = t0.elapsed().as_secs_f64().max(0.001);
        t0 = Instant::now();
        let clock = clock();
        let mut any = false;
        for (i, t) in tg.iter().enumerate() {
            let now = walk::measure(t).bytes;
            let delta = now as i64 - last[i] as i64;
            last[i] = now;
            if delta.unsigned_abs() < 1_000_000 {
                continue;
            }
            any = true;
            let per_min = (delta.unsigned_abs() as f64 * 60.0 / dt) as u64;
            let sign = if delta > 0 { "+" } else { "−" };
            let line = format!(
                "{clock}  {sign}{:>9}  {:>10}/min  {}",
                units::bytes(delta.unsigned_abs()),
                units::bytes(per_min),
                env.tilde(t)
            );
            if delta > 0 && per_min >= w.rate {
                println!("{}", st.red(&format!("{line}  RUNAWAY")));
                let files = newest_files(t, 12);
                if let Some(f) = files.first() {
                    println!("          newest file: {}", env.tilde(f));
                }
                let ps = procs::list();
                let pids = procs::writers(t, &files);
                if pids.is_empty() {
                    println!(
                        "          {}",
                        st.dim(
                            "no process has a file open there right now (it may write in bursts)"
                        )
                    );
                }
                for p in pids.iter().take(4) {
                    println!("          writer: {}", procs::chain(&ps, *p));
                }
            } else {
                println!("{line}");
            }
        }
        for c in crate::guard::checks(env, &[], w.floor) {
            if c.space.free < c.floor {
                println!(
                    "{}",
                    st.red(&format!(
                        "{clock}  only {} free on {} (floor {}): spoilheap clean --safe",
                        units::bytes(c.space.free),
                        env.tilde(&c.path),
                        units::bytes(c.floor)
                    ))
                );
                any = true;
            }
        }
        if any {
            quiet_since = Instant::now();
        } else if quiet_since.elapsed() >= Duration::from_secs(60) {
            let s = disk::space(&env.home).ok();
            println!(
                "{}",
                st.dim(&format!(
                    "{clock}  quiet · {} free",
                    s.map(|s| units::bytes(s.free)).unwrap_or_default()
                ))
            );
            quiet_since = Instant::now();
        }
        if w.ticks.is_some_and(|n| tick >= n) {
            return 0;
        }
    }
}

fn clock() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let off = local_offset();
    let t = (s as i64 + off).rem_euclid(86400);
    format!("{:02}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
}

fn local_offset() -> i64 {
    #[cfg(unix)]
    unsafe {
        let t: libc::time_t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&t, &mut tm).is_null() {
            return tm.tm_gmtoff;
        }
    }
    0
}
