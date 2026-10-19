//! Terminal output: one line per item, a dim second line with path and reason.

use crate::dig::{Action, Env, Item, Report, Verdict};
use crate::disk::Space;
use crate::units;
use std::io::IsTerminal;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub struct Style {
    pub color: bool,
    pub width: usize,
}

impl Style {
    pub fn detect(stream_is_tty: bool) -> Style {
        let force = std::env::var_os("FORCE_COLOR").is_some_and(|v| v != "0") || std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0");
        let no = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) || std::env::var("TERM").is_ok_and(|t| t == "dumb");
        let color = force || (stream_is_tty && !no);
        Style { color, width: width() }
    }
    pub fn stdout() -> Style {
        Style::detect(std::io::stdout().is_terminal())
    }
    pub fn stderr() -> Style {
        Style::detect(std::io::stderr().is_terminal())
    }
    fn paint(&self, code: &str, s: &str) -> String {
        if self.color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }
    pub fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
    pub fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
    pub fn red(&self, s: &str) -> String {
        self.paint("1;31", s)
    }
    pub fn green(&self, s: &str) -> String {
        self.paint("32", s)
    }
    pub fn yellow(&self, s: &str) -> String {
        self.paint("33", s)
    }
    pub fn verdict(&self, v: Verdict) -> String {
        let w = format!("{:<6}", v.word());
        match v {
            Verdict::Safe => self.paint("1;32", &w),
            Verdict::Review => self.paint("1;33", &w),
            Verdict::Keep => self.paint("1;36", &w),
        }
    }
}

fn width() -> usize {
    if let Some(c) = std::env::var("COLUMNS").ok().and_then(|v| v.parse::<usize>().ok()) {
        return c.clamp(60, 200);
    }
    #[cfg(unix)]
    {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        for fd in [1, 2, 0] {
            if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0 {
                return (ws.ws_col as usize).clamp(60, 160);
            }
        }
    }
    100
}

/// Visible width (ANSI escapes do not count).
pub fn vis(s: &str) -> usize {
    let mut n = 0;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            if c == 'm' {
                esc = false;
            }
        } else if c == '\x1b' {
            esc = true;
        } else {
            n += 1;
        }
    }
    n
}

pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".into();
    }
    let keep = max - 1;
    let head = keep / 3;
    let tail = keep - head;
    let chars: Vec<char> = s.chars().collect();
    let a: String = chars[..head].iter().collect();
    let b: String = chars[chars.len() - tail..].iter().collect();
    format!("{a}…{b}")
}

fn wrap(text: &str, width: usize, indent: usize) -> Vec<String> {
    let mut lines = vec![];
    let mut cur = String::new();
    for w in text.split(' ') {
        if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > width {
            lines.push(cur);
            cur = String::new();
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    let pad = " ".repeat(indent);
    lines.into_iter().enumerate().map(|(i, l)| if i == 0 { l } else { format!("{pad}{l}") }).collect()
}

pub fn bar(st: &Style, used_frac: f64, n: usize) -> String {
    let filled = ((used_frac.clamp(0.0, 1.0)) * n as f64).round() as usize;
    let a = "█".repeat(filled);
    let b = "░".repeat(n - filled.min(n));
    let a = if used_frac >= 0.9 { st.red(&a) } else { st.bold(&a) };
    format!("{a}{}", st.dim(&b))
}

pub struct DiskLine {
    pub mount: PathBuf,
    pub space: Space,
}

pub fn totals(items: &[Item]) -> (u64, u64, u64, u64) {
    let mut all = 0;
    let (mut s, mut r, mut k) = (0, 0, 0);
    for it in items {
        all += it.bytes;
        match it.verdict {
            Verdict::Safe => s += it.reclaim,
            Verdict::Review => r += if it.reclaim > 0 { it.reclaim } else { 0 },
            Verdict::Keep => k += it.bytes,
        }
    }
    (all, s, r, k)
}

pub fn report(env: &Env, rep: &Report, disks: &[DiskLine], st: &Style, shown_min: u64, hidden: usize) -> String {
    let mut o = String::new();
    let w = st.width;
    o.push_str(&format!("{}  {}\n", st.bold("spoilheap"), st.dim("what your coding agents left on this disk")));
    for d in disks {
        let s = d.space;
        let frac = if s.total == 0 { 0.0 } else { s.used() as f64 / s.total as f64 };
        let label = format!(
            "{} free of {} ({:.1}%)",
            units::bytes(s.free),
            units::bytes(s.total),
            s.free_pct()
        );
        let free = if s.free_pct() < 10.0 { st.red(&label) } else { label };
        o.push_str(&format!("disk   {}  {}  {}\n", st.bold(&env.tilde(&d.mount)), bar(st, frac, 24), free));
    }
    let items: Vec<&Item> = rep.items.iter().collect();
    let (all, safe, review, keep) = totals(&rep.items);
    if items.is_empty() {
        o.push_str("\nNothing found. No Codex, Claude Code, OpenCode, Gemini CLI or Cline data on this machine");
        if rep.repos == 0 {
            o.push_str(", and no repositories under the roots");
        }
        o.push_str(".\n");
        o.push_str(&st.dim(&format!("Looked in: {}\n", looked_in(env).join(", "))));
        return o;
    }
    o.push_str(&format!(
        "agents {} in {} places · {} {} · {} {} · {} {}\n",
        st.bold(&units::bytes(all)),
        items.len(),
        st.verdict(Verdict::Safe).trim_end(),
        st.bold(&units::bytes(safe)),
        st.verdict(Verdict::Review).trim_end(),
        units::bytes(review),
        st.verdict(Verdict::Keep).trim_end(),
        units::bytes(keep),
    ));
    let size_w = 9;
    let id_w = 6;
    for h in crate::dig::HARNESSES {
        let group: Vec<&&Item> = items.iter().filter(|i| i.harness == *h && (i.bytes >= shown_min || i.kind == "worktree-registration" || i.kind == "worktree")).collect();
        if group.is_empty() {
            continue;
        }
        let gsum: u64 = group.iter().map(|i| i.bytes).sum();
        o.push('\n');
        o.push_str(&format!("{}  {}\n", st.bold(&h.to_uppercase()), st.dim(&units::bytes(gsum))));
        for it in group {
            let child = it.parent.is_some();
            let lead = if child { "  ↳ " } else { "" };
            let meta = meta(it);
            let fixed = 2 + id_w + 1 + 7 + vis(lead) + 1 + size_w + 2 + meta.chars().count();
            let label_w = w.saturating_sub(fixed).max(16);
            let label = clip(&it.label, label_w);
            let pad = label_w.saturating_sub(label.chars().count());
            o.push_str(&format!(
                "  {} {} {}{}{} {:>size_w$}  {}\n",
                st.dim(&format!("[{}]", it.id)),
                st.verdict(it.verdict),
                lead,
                label,
                " ".repeat(pad),
                units::bytes(it.bytes),
                st.dim(&meta),
            ));
            let ind = 2 + id_w + 1 + 7;
            let path = env.tilde(&it.path);
            let text = format!("{} — {}", clip(&path, (w.saturating_sub(ind)).max(20) * 2 / 3), it.reason);
            for l in wrap(&text, w.saturating_sub(ind).max(30), ind) {
                o.push_str(&format!("{}{}\n", " ".repeat(ind), st.dim(l.trim_start())));
            }
        }
    }
    if hidden > 0 {
        o.push_str(&st.dim(&format!("\n{hidden} item{} under {} hidden (--all shows them)\n", if hidden == 1 { "" } else { "s" }, units::bytes(shown_min))));
    }
    if !rep.notes.is_empty() {
        o.push('\n');
        for n in &rep.notes {
            o.push_str(&format!("{} {n}\n", st.yellow("note")));
        }
    }
    o.push_str(&format!("\n{}\n", st.bold("next")));
    let n_safe = rep.items.iter().filter(|i| i.verdict == Verdict::Safe).count();
    if n_safe > 0 {
        o.push_str(&format!("  {}   frees {} from {} SAFE item{} (prints the plan; add --yes to do it)\n", st.green("spoilheap clean --safe"), units::bytes(safe), n_safe, if n_safe == 1 { "" } else { "s" }));
    }
    let reviews: Vec<String> = rep.items.iter().filter(|i| i.verdict == Verdict::Review).take(2).map(|i| i.id.to_string()).collect();
    if !reviews.is_empty() {
        let cmd = format!("spoilheap clean {}", reviews.join(" "));
        o.push_str(&format!("  {}{}also clears REVIEW items you have looked at\n", st.yellow(&cmd), " ".repeat(25usize.saturating_sub(cmd.len()))));
    }
    let manual: Vec<&Item> = rep.items.iter().filter(|i| matches!(i.action, Action::Manual(_))).collect();
    if !manual.is_empty() {
        o.push_str(&format!("  {}  databases: spoilheap prints the VACUUM command, never runs it\n", "spoilheap clean <id>   "));
    }
    o.push_str(&format!("  {}   stop the agent before the disk is full (also: hook codex)\n", "spoilheap hook claude  "));
    if rep.repos == 0 && rep.roots.is_empty() {
        o.push_str(&st.dim("  no repositories scanned: pass --root ~/code to check tmp_pack garbage and .claude/worktrees\n"));
    }
    o
}

pub fn meta(it: &Item) -> String {
    let now = std::time::SystemTime::now();
    let mut parts = vec![];
    if it.files > 1 {
        parts.push(format!("{} files", units::thousands(it.files)));
    }
    if let Some(t) = it.newest {
        parts.push(format!("newest {}", units::age(units::age_secs(t, now))));
    }
    parts.join(" · ")
}

pub fn looked_in(env: &Env) -> Vec<String> {
    vec![
        env.tilde(&env.codex()),
        env.tilde(&env.claude()),
        env.tilde(&env.xdg_data().join("opencode")),
        env.tilde(&env.h(".gemini")),
        env.tilde(&env.h(".cline")),
    ]
}

pub fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}
