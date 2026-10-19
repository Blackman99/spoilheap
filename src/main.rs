mod clean;
mod dig;
mod disk;
mod git;
mod guard;
mod mcp;
mod out;
mod procs;
mod render;
mod sqlite;
mod units;
mod walk;
mod watch;

use dig::{Env, Opts};
use render::Style;
use std::path::PathBuf;
use std::time::Duration;

const HELP: &str =
    "spoilheap — what your coding agents left on this disk, and what is safe to delete

USAGE
  spoilheap [--root DIR]... [--older 30d] [--all] [--json]
      List every leftover from Codex, Claude Code, OpenCode, Gemini CLI and Cline,
      with a SAFE / REVIEW / KEEP verdict and the reason. Changes nothing.
  spoilheap clean --safe [--yes]          clear every SAFE item (dry run without --yes)
  spoilheap clean 104 107 [--yes]         clear items by number, REVIEW ones included
  spoilheap watch [--every 10s] [--rate 1GB] [--floor 10GB]
      Sample agent folders and repo packs; name what grows faster than --rate per minute.
  spoilheap guard [--floor 10GB]          hook: exit 2 when free space is under the floor
  spoilheap hook claude|codex             print the hook config that runs `guard`
  spoilheap mcp                           read-only MCP server (disk_status, disk_leftovers)
  spoilheap paths                         where spoilheap looks

OPTIONS
  --root DIR     look for repositories here (repeatable; default: the current folder and
                 ~/code ~/src ~/projects ~/dev ~/work ~/Developer ~/repos if they exist)
  --depth N      how deep to look for repositories under each root (default 4)
  --older D      history older than this is REVIEW, newer is KEEP (default 30d)
  --all          also show items under 1 MB
  --json         machine-readable output
  --home DIR     treat DIR as the home folder (for testing)
  --no-color     plain output (NO_COLOR is respected too)
  -V, --version  print the version

Verdicts: SAFE = rebuildable or garbage (crash dumps, merged clean worktrees, git-ignored
build output, half-written git packs). REVIEW = yours to judge (old chats, worktrees with
commits that exist nowhere else, databases that need VACUUM). KEEP = never touched
(uncommitted work, recent history, live databases).

https://blackman99.github.io/spoilheap/";

struct Args {
    cmd: String,
    pos: Vec<String>,
    roots: Vec<PathBuf>,
    depth: usize,
    older: Duration,
    all: bool,
    json: bool,
    home: Option<PathBuf>,
    no_color: bool,
    safe: bool,
    yes: bool,
    every: Duration,
    rate: u64,
    floor: units::Amount,
    floor_raw: String,
    paths: Vec<PathBuf>,
    ticks: Option<u64>,
    verbose: bool,
}

const FLAGS: &[&str] = &[
    "--root",
    "--depth",
    "--older",
    "--all",
    "--json",
    "--home",
    "--no-color",
    "--safe",
    "--yes",
    "--every",
    "--rate",
    "--floor",
    "--path",
    "--ticks",
    "--verbose",
    "--help",
    "--version",
];

fn suggest(bad: &str) -> String {
    let b = bad.trim_start_matches('-');
    let best = FLAGS
        .iter()
        .min_by_key(|f| lev(b, f.trim_start_matches('-')))
        .unwrap();
    if lev(b, best.trim_start_matches('-')) <= 2 {
        format!(" (did you mean {best}?)")
    } else {
        String::new()
    }
}

fn lev(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        cmd: "dig".into(),
        pos: vec![],
        roots: vec![],
        depth: 4,
        older: Duration::from_secs(30 * 86400),
        all: false,
        json: false,
        home: None,
        no_color: false,
        safe: false,
        yes: false,
        every: Duration::from_secs(10),
        rate: 1_000_000_000,
        floor: units::Amount::Bytes(10_000_000_000),
        floor_raw: "10GB".into(),
        paths: vec![],
        ticks: None,
        verbose: false,
    };
    let mut it = std::env::args().skip(1).peekable();
    let mut first = true;
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut val = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| it.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => a.cmd = "help".into(),
            "-V" | "--version" => a.cmd = "version".into(),
            "--root" => {
                let v = val("--root")?;
                let p = expand(&v);
                if !p.exists() {
                    return Err(format!("--root {v}: no such folder"));
                }
                if !p.is_dir() {
                    return Err(format!(
                        "--root {v} is a file; pass the folder that holds your repositories"
                    ));
                }
                a.roots.push(std::fs::canonicalize(&p).unwrap_or(p));
            }
            "--depth" => {
                a.depth = val("--depth")?
                    .parse()
                    .map_err(|_| "--depth needs a whole number, like 4".to_string())?
            }
            "--older" => a.older = units::parse_duration(&val("--older")?)?,
            "--all" => a.all = true,
            "--json" => a.json = true,
            "--home" => a.home = Some(expand(&val("--home")?)),
            "--no-color" => a.no_color = true,
            "--safe" => a.safe = true,
            "--yes" | "-y" => a.yes = true,
            "--verbose" | "-v" => a.verbose = true,
            "--every" => {
                a.every = units::parse_duration(&val("--every")?)?;
                if a.every < Duration::from_secs(1) {
                    return Err("--every must be at least 1s".into());
                }
            }
            "--rate" => {
                let raw = val("--rate")?;
                let raw = raw.trim_end_matches("/min").to_string();
                a.rate = match units::parse_amount(&raw)? {
                    units::Amount::Bytes(b) => b,
                    _ => return Err("--rate is a size per minute, like 1GB".into()),
                }
            }
            "--floor" => {
                a.floor_raw = val("--floor")?;
                a.floor = units::parse_amount(&a.floor_raw)?;
            }
            "--path" => a.paths.push(expand(&val("--path")?)),
            "--ticks" => {
                a.ticks = Some(
                    val("--ticks")?
                        .parse()
                        .map_err(|_| "--ticks needs a number".to_string())?,
                )
            }
            f if f.starts_with('-') && f.len() > 1 && f.parse::<i64>().is_err() => {
                return Err(format!("unknown option {f}{}", suggest(f)));
            }
            _ => {
                if first
                    && [
                        "dig", "clean", "watch", "guard", "hook", "mcp", "paths", "help", "version",
                    ]
                    .contains(&arg.as_str())
                {
                    a.cmd = arg.clone();
                } else if first && !arg.chars().all(|c| c.is_ascii_digit()) {
                    return Err(format!(
                        "unknown command '{arg}'. Commands: dig (default), clean, watch, guard, hook, mcp, paths"
                    ));
                } else {
                    a.pos.push(arg.clone());
                }
            }
        }
        first = false;
    }
    Ok(a)
}

fn expand(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        let home = if cfg!(windows) {
            std::env::var("USERPROFILE")
        } else {
            std::env::var("HOME")
        };
        if let Ok(h) = home {
            return PathBuf::from(h).join(rest);
        }
    }
    PathBuf::from(s)
}

fn style(a: &Args, stdout: bool) -> Style {
    let mut s = if stdout {
        Style::stdout()
    } else {
        Style::stderr()
    };
    if a.no_color {
        s.color = false;
    }
    s
}

fn disks(env: &Env, rep: &dig::Report) -> Vec<render::DiskLine> {
    let mut v: Vec<render::DiskLine> = vec![];
    let mut seen = vec![];
    let mut cands = vec![env.home.clone()];
    cands.extend(rep.items.iter().map(|i| i.path.clone()));
    for p in cands {
        let d = disk::device(&p);
        if seen.contains(&d) {
            continue;
        }
        seen.push(d);
        if let Ok(s) = disk::space(&p) {
            v.push(render::DiskLine {
                mount: mount_point(&disk::existing_ancestor(&p)),
                space: s,
            });
        }
    }
    v
}

/// The topmost folder still on the same filesystem (what df would call the mount).
fn mount_point(p: &std::path::Path) -> PathBuf {
    let dev = disk::device(p);
    let mut cur = p.to_path_buf();
    while let Some(parent) = cur.parent() {
        if disk::device(parent) != dev {
            break;
        }
        cur = parent.to_path_buf();
    }
    cur
}

fn main() {
    // behave like other CLI tools when piped into `head`: exit quietly on a closed pipe
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let a = match parse() {
        Ok(a) => a,
        Err(e) => {
            let st = Style::stderr();
            eprintln!(
                "{} {e}\n{}",
                st.red("spoilheap:"),
                st.dim("run `spoilheap --help` for usage")
            );
            std::process::exit(64);
        }
    };
    let env = Env::from_process(a.home.clone());
    let roots = if !a.roots.is_empty() {
        a.roots.clone()
    } else {
        dig::default_roots(&env)
    };
    let opts = Opts {
        older: a.older,
        roots,
        now: std::time::SystemTime::now(),
        repo_depth: a.depth,
    };
    let code = match a.cmd.as_str() {
        "help" => {
            println!("{HELP}");
            0
        }
        "version" => {
            println!("spoilheap {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "dig" => {
            let st = style(&a, true);
            if !a.pos.is_empty() {
                eprintln!(
                    "{} `spoilheap {}` lists, it does not clear. To clear those items: spoilheap clean {}",
                    st.yellow("note"),
                    a.pos.join(" "),
                    a.pos.join(" ")
                );
            }
            let rep = dig::dig(&env, &opts);
            let ds = disks(&env, &rep);
            out::save_ids(&env, &rep);
            if a.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&out::report(&rep, &ds)).unwrap()
                );
            } else {
                let min = if a.all { 0 } else { 1_000_000 };
                let hidden = rep
                    .items
                    .iter()
                    .filter(|i| {
                        i.bytes < min && i.kind != "worktree-registration" && i.kind != "worktree"
                    })
                    .count();
                print!("{}", render::report(&env, &rep, &ds, &st, min, hidden));
            }
            0
        }
        "clean" => {
            let st = style(&a, true);
            let mut ids = vec![];
            for p in &a.pos {
                match p
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<u32>()
                {
                    Ok(n) => ids.push(n),
                    Err(_) => {
                        eprintln!(
                            "{} '{p}' is not an item number. Use the numbers in brackets from `spoilheap`, e.g. spoilheap clean 104",
                            st.red("spoilheap:")
                        );
                        std::process::exit(64);
                    }
                }
            }
            if !a.safe && ids.is_empty() {
                eprintln!(
                    "{} say what to clear: `spoilheap clean --safe` for every SAFE item, or item numbers from `spoilheap` (e.g. spoilheap clean 104 107). Nothing is deleted without --yes.",
                    st.red("spoilheap:")
                );
                std::process::exit(64);
            }
            let rep = dig::dig(&env, &opts);
            if let Some((_, age)) = out::load_ids(&env) {
                if !ids.is_empty() && age > 86400 {
                    eprintln!(
                        "{} those numbers come from a dig {} ago; run `spoilheap` again if unsure",
                        st.yellow("note"),
                        units::age(age)
                    );
                }
            }
            let c = clean::run(
                &env,
                &rep,
                &clean::Choice {
                    safe: a.safe,
                    ids,
                    yes: a.yes,
                },
                &st,
            );
            if !a.yes {
                // keep the id map from the dig the user saw, not this re-scan
            }
            c
        }
        "watch" => {
            let st = style(&a, true);
            watch::run(
                &env,
                &opts,
                &watch::WatchOpts {
                    every: a.every,
                    rate: a.rate,
                    floor: a.floor,
                    ticks: a.ticks,
                },
                &st,
            )
        }
        "guard" => guard::run(
            &env,
            a.floor,
            a.paths.clone(),
            !a.verbose && !render::is_tty(),
        ),
        "hook" => match a.pos.first() {
            None => {
                eprintln!(
                    "spoilheap: which agent? `spoilheap hook claude` or `spoilheap hook codex`"
                );
                64
            }
            Some(t) => match guard::hook_snippet(t, &a.floor_raw) {
                Ok(s) => {
                    print!("{s}");
                    0
                }
                Err(e) => {
                    eprintln!("spoilheap: {e}");
                    64
                }
            },
        },
        "mcp" => mcp::serve(&env),
        "paths" => {
            let st = style(&a, true);
            let mut list: Vec<(String, PathBuf)> = vec![
                ("Codex".into(), env.codex()),
                ("Claude Code".into(), env.claude()),
                ("OpenCode".into(), env.xdg_data().join("opencode")),
                ("Gemini CLI".into(), env.h(".gemini/tmp")),
                ("Cline".into(), env.h(".cline/data")),
            ];
            for ed in ["Code", "Cursor", "Windsurf", "VSCodium"] {
                list.push((
                    format!("Cline in {ed}"),
                    env.app_support()
                        .join(ed)
                        .join("User/globalStorage/saoudrizwan.claude-dev"),
                ));
            }
            for app in ["Codex", "ChatGPT", "Claude", "Cursor"] {
                list.push((
                    format!("{app} crash dumps"),
                    env.app_support().join(app).join("Crashpad"),
                ));
            }
            list.push(("bun cache".into(), env.h(".bun/install/cache/@openai")));
            for (n, p) in list {
                let mark = if p.exists() {
                    st.green("found  ")
                } else {
                    st.dim("absent ")
                };
                println!("{mark} {:<20} {}", n, env.tilde(&p));
            }
            for r in &opts.roots {
                println!(
                    "{} {:<20} {}",
                    st.green("root   "),
                    "repositories",
                    env.tilde(r)
                );
            }
            0
        }
        _ => 64,
    };
    std::process::exit(code);
}
