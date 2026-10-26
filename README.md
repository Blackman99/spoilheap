<p align="center"><img src="assets/logo-512.png" width="120" alt="spoilheap logo: a spoil heap of soil layers with a red-and-white ranging rod and a lime SAFE flag on top"></p>

<h1 align="center">spoilheap</h1>

<p align="center"><b>What did my coding agents leave on this disk, and what is safe to delete?</b><br>
Every worktree, session, crash dump, half-written git pack and bloated SQLite file that Codex, Claude Code, OpenCode, Gemini CLI and Cline left behind, each marked SAFE, REVIEW or KEEP with the reason. It clears only what is safe, and it can stop the agent before the disk hits zero.</p>

<p align="center">
<a href="https://github.com/Blackman99/spoilheap/actions/workflows/ci.yml"><img src="https://github.com/Blackman99/spoilheap/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
<a href="https://github.com/Blackman99/spoilheap/releases/latest"><img src="https://img.shields.io/github/v/release/Blackman99/spoilheap?color=9c5b3b" alt="release"></a>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-151413" alt="MIT"></a>
<a href="https://blackman99.github.io/spoilheap/">website</a> · <a href="https://blackman99.github.io/spoilheap/docs/">docs</a>
</p>

<p align="center"><img src="docs/demo.gif" width="860" alt="spoilheap on a demo home with 3.1 GB free: 117 GB of agent leftovers in 28 places, then clean --safe prints a dry-run plan for 56.5 GB, then guard exits 2 and tells the agent to stop"></p>

## The problem

Coding agents keep their own state on your disk, and none of them tells you the total.

- "**~/.codex alone 59 GB**": [a post](https://x.com/Voxyz_ai/status/2111472304643612774) with more than 100k views and close to 2k bookmarks, from people checking their own machines.
- One Codex run left **263.7 GB of `tmp_pack_*`** from a git that died mid-write ([openai/codex#54955](https://github.com/openai/codex/issues/54955)). The Codex desktop app can crash-loop and write a Crashpad dump about every second ([openai/codex#25921](https://github.com/openai/codex/issues/25921), [HN](https://news.ycombinator.com/item?id=50133231)).
- An OpenCode database grew to **12.85 GB** in about two weeks, and hourly pruning did not shrink it, because SQLite does not give space back on `DELETE` without a `VACUUM` ([anomalyco/opencode#56827](https://github.com/anomalyco/opencode/issues/56827)). The same user's disk then filled up, ENOSPC corrupted the database mid-transaction, and OpenCode quarantined every session ([#56829](https://github.com/anomalyco/opencode/issues/56829)).
- Codex worktrees are never reclaimed ([openai/codex#54891](https://github.com/openai/codex/issues/54891)). One cloud box got so full that its cleanup command could not start ([#55038](https://github.com/openai/codex/issues/55038)). Agents also keep working past low-disk warnings ([anthropics/claude-code#103088](https://github.com/anthropics/claude-code/issues/103088)).

The hard part is not finding big folders; `du` does that. The hard part is knowing **which of 40 worktrees still holds work nobody pushed**, whether a 12 GB database is rows or free pages, and whether a `tmp_pack` file belongs to a git that is still running. spoilheap answers that per item, and says why.

## Try it in 10 seconds

```sh
curl -fsSL https://blackman99.github.io/spoilheap/install.sh | sh
spoilheap          # look. This deletes nothing.
```

This is the top of the output on the demo home in this repo (`scripts/demohome.sh /tmp/h big`, with real git worktrees, a real SQLite file and sparse big files; 3.1 GB free is simulated):

```text
spoilheap  what your coding agents left on this disk
disk   /  ████████████████████████  99% used · 3.1 GB free of 494.0 GB  LOW
agents 117.1 GB in 28 places · SAFE 56.5 GB · REVIEW 54.2 GB · KEEP 6.2 GB

CODEX  83.5 GB
  [101] REVIEW sessions older than 30 d                              32.7 GB  12 files · 2 mo ago
               ~/.codex/sessions — chat rollouts; `codex resume` can reopen them, nothing else reads
               them
  [102] SAFE   Codex crash dumps                                     28.5 GB  100 files · 1 min ago
               ~/Library/Application Support/Codex/Crashpad — crash loop: 60 dumps in the last hour
               (openai/codex#25921). Quit Codex first or the pile grows straight back
  [106] KEEP   worktree codex/ledger                                    4 KB  5 files · 1 min ago
               ~/.codex/worktrees/c903/payments — 2 uncommitted changes: real work
  [107] SAFE   ↳ build output target/                                 5.0 GB  3 h ago
               ~/.codex/worktrees/c903/payments/target — git-ignored and rebuildable (cargo build);
               the worktree itself is untouched
  [108] REVIEW worktree codex/webhook-sig                               4 KB  4 files · 9 d ago
               ~/.codex/worktrees/b77c/payments — clean, but 2 commits exist only here; push or
               merge first
  [109] SAFE   worktree payments (detached)                             4 KB  3 files · 9 d ago
               ~/.codex/worktrees/a41f/payments — clean, and every commit is also on another branch
               or remote
  …
OPENCODE  10.0 GB
  [122] REVIEW opencode.db: 506.2 MB of 675.9 MB is free pages              675.9 MB  just now
               ~/.local/share/opencode/opencode.db — SQLite never shrinks on DELETE; VACUUM gives the
               free pages back. It needs ~169.7 MB free while it runs
  …
REPOS  12.0 GB
  [127] SAFE   payments: 6 half-written git packs                    12.0 GB  6 files · 2 d ago
               ~/code/payments/.git/objects/pack — tmp_pack_* from a git that died mid-write
               (openai/codex#54955); untouched for 1 h+

next
  spoilheap clean --safe      frees 56.5 GB from 12 SAFE items (prints the plan; add --yes to do it)
  spoilheap clean 101 104     also clears REVIEW items once you have looked at them
  spoilheap clean 122         databases: prints the VACUUM command, never runs it
  spoilheap hook claude       stop the agent before the disk is full (also: hook codex)
```

## What it will and will not delete

- **`spoilheap` on its own only reads.** So do `paths`, `watch`, `procs`, `guard` and `mcp`. The only file they write is the list of item numbers, in your cache folder. `procs` never sends a signal: it prints the `kill -TERM` line for you to run.
- **`clean` is a dry run unless you add `--yes`.** It prints every path or git command it would run, and the total.
- **It works only inside the agents' own folders** (listed [below](#coverage)) and, in the repositories it finds (under the current folder, `~/code`, `~/src` and similar, or `--root`), only inside `.git/objects/pack`, stale worktree registrations and git-ignored build output in agent worktrees. It never follows a symlink, never leaves the folder an item came from, and never crosses into another filesystem.
- **Worktrees are removed with `git worktree remove`, never `rm -rf`,** and only after a fresh re-check: no uncommitted changes, every commit also on another branch, tag or remote, and no process working inside it. If git refuses, that item fails with git's reason and the rest go on.
- **KEEP is never touched, even if you name it.** That covers uncommitted work, a worktree a process is sitting in, and recent history.
- **Databases are never edited.** spoilheap prints the `VACUUM` command and how much free space it needs, for you to run once the app is closed.

## Commands

| Command | What it does |
|---|---|
| `spoilheap` | Inventory grouped by agent, with a number, size, age, verdict and reason per item, then the exact next commands. `--json` for scripts. |
| `spoilheap clean --safe [--yes]` | Plan, then (with `--yes`) clear every SAFE item. Afterwards it measures free space again and tells you if the disk got back less than planned, for example because a process still holds deleted files open. |
| `spoilheap clean 104 107 [--yes]` | Clear items by number, REVIEW ones included. Numbers come from your last run, so they point at what you just read. |
| `spoilheap watch [--rate 1GB] [--floor 10GB]` | Samples the agent folders and repo packs every 10 s. For anything that grows faster than `--rate` a minute, it prints `RUNAWAY` with the newest file and the process chain writing it (Linux `/proc/*/fd`, macOS `lsof`). |
| `spoilheap guard [--floor 10GB\|5%]` | For a PreToolUse hook. Exits 2 under the floor, with a message to the agent naming the three biggest SAFE items. Claude Code and Codex both block the tool call on exit 2. |
| `spoilheap hook claude\|codex` | Prints the settings block that runs `guard`. |
| `spoilheap mcp` | A read-only MCP server with `disk_status` and `disk_leftovers`. There is no delete tool. |
| `spoilheap paths` | Every location it reads, with `found` / `absent` on this machine. |
| `spoilheap procs [--all]` | Agent processes grouped by the agent running them, with memory and CPU. Orphans (parent at PID 1 or gone) get SAFE / REVIEW, the rest KEEP, and SAFE ones get a `kill -TERM` line to run yourself. Linux and macOS; not in v0.1.0 yet. |

How each verdict is decided, flag by flag, is in the [docs](https://blackman99.github.io/spoilheap/docs/#verdicts).

### Leftover processes

Agents leave processes behind too: Claude Code snapshot shells orphaned at PPID 1 and still burning a core ([anthropics/claude-code#103313](https://github.com/anthropics/claude-code/issues/103313)), MCP servers whose session ended, Codex helpers doubled by restarts ([openai/codex#55141](https://github.com/openai/codex/issues/55141)). `spoilheap procs` lists them without touching them:

```text
spoilheap procs  agent processes on this machine · read-only, nothing is signalled
2 agents running · 13 processes · 1.3 GB memory · 3 orphans · SAFE 2 · REVIEW 1 · KEEP 9

CLAUDE CODE  9 processes · 991.3 MB
     4242 KEEP   claude                                                 412.0 MB  3.2% CPU · 1 h
                 ~/code/shop — Claude Code itself, running with 4 helper processes
     4411 KEEP   ↳ node vitest                                          180.0 MB  88% CPU · just now
     4302 KEEP   ↳ MCP server mcp-server-playwright                      98.0 MB  0.4% CPU · 1 h
     4300 KEEP   ↳ MCP server mcp-server-filesystem                      61.0 MB  0.1% CPU · 1 h
     4410 KEEP   ↳ snapshot shell (zsh): npm test                         6.0 MB  0% CPU · just now
     6100 REVIEW orphan · node vite                                     140.0 MB  0.3% CPU · 2 h
                 ~/code/shop — PPID 1, but its working folder is in the live worktree ~/code/shop:
                 it may still be working there
     8810 SAFE   orphan · MCP server mcp-server-postgres                 88.0 MB  0% CPU · 34 d
                 ~/old/scratch (deleted) — parent 8800 is gone: the Claude Code session that started
                 it has ended; nothing of it is in a live worktree, its folder was deleted
     7731 SAFE   orphan · snapshot shell (zsh): yes > /dev/null           6.3 MB  99% CPU · 3 d
                 /tmp — PPID 1: Claude Code exited and left this shell behind; nothing of it is in a
                 live worktree; 1 child process below it

CODEX  4 processes · 308.0 MB
     5100 KEEP   codex                                                   48.0 MB  0.2% CPU · 1 h
                 ~/.codex/worktrees/7f3e/shop — Codex itself, running with 3 helper processes
     5101 KEEP   ↳ codex                                                120.0 MB  1.1% CPU · 1 h
     5110 KEEP   ↳ MCP server mcp-server-github ×2                      140.0 MB  0% CPU · 59 min

note ×N: that many helpers run the same command under one agent. Codex restarts have doubled them
     (openai/codex#55141); restarting the agent clears them

next
  kill -TERM 8810 7731        stops 2 SAFE orphans (SIGTERM lets them exit cleanly)
  kill -TERM 6100             REVIEW: only once you know it is not yours
  spoilheap only lists; run these yourself, then `spoilheap procs` again to check
```

Matching is narrow on purpose: agents by executable or package name, helpers only below a live agent, orphans only when the command line or an agent marker variable ties them to an agent. A plain `node` is never listed. An orphan with its working folder or an open file inside a live worktree is REVIEW, since it may still be doing your work. The full rules are in the [docs](https://blackman99.github.io/spoilheap/docs/#procs).

## Coverage

Every path below comes from the agent's own source or docs, not from guesswork.

| Agent | What spoilheap measures | Verdicts |
|---|---|---|
| **Codex** (CLI + desktop) | `$CODEX_HOME/worktrees` (per worktree, via git), `sessions/`, `archived_sessions/`, `log/`, `.tmp/`, `logs_*.sqlite`; Crashpad dumps of the Codex / ChatGPT apps; old bun-installed builds | worktrees SAFE / REVIEW / KEEP by git state; history REVIEW after `--older` (30 d); crash dumps and old logs SAFE |
| **Claude Code** | `$CLAUDE_CONFIG_DIR/projects` (transcripts), `file-history/`, `shell-snapshots/`, `debug/`; `<repo>/.claude/worktrees`; Claude desktop Crashpad | transcripts and file history REVIEW after 30 d; debug logs and shell snapshots SAFE after 1 d |
| **OpenCode** | `opencode*.db` (+ WAL) or `$OPENCODE_DB`, with free pages read from the SQLite header; `snapshot/<project>` undo repos; `storage/`; `log/` | database REVIEW when it is worth a VACUUM (never edited); snapshots REVIEW after 30 d |
| **Gemini CLI** | `~/.gemini/tmp/<project hash>` (chats, checkpoints, logs) | REVIEW after 30 d |
| **Cline** | `tasks/` and `checkpoints/` under VS Code, Cursor, Windsurf and VSCodium `globalStorage/saoudrizwan.claude-dev`, and `~/.cline/data` | REVIEW after 30 d |
| **Your repos** (`--root`) | `.git/objects/pack/tmp_pack_*` and `tmp_idx_*`, stale worktree registrations, `.aider.tags.cache.v4`, git-ignored `target/` / `node_modules/` / `.venv/` inside agent worktrees | SAFE once untouched for 1 h; `git worktree prune` for registrations |

Sizes are allocated blocks, not file lengths, and hard links are counted once. Linux, macOS and Windows are supported. On Windows, process attribution in `watch` comes later ([ROADMAP](ROADMAP.md)).

**How it relates to Mole.** [Mole](https://github.com/tw93/Mole) (about 71k stars) is a very good general Mac and Windows cleaner, and it already clears rebuildable AI caches. By design it stops there. It shows each agent worktree folder as one total for you to review, and its maintainer keeps history, sessions and agent worktrees out of its cleanup (see its `lib/clean/user.sh` and `SYNC.md`). It has no live growth watch and no hook an agent can call. spoilheap covers exactly that gap, so the two tools work well side by side.

## Install

```sh
curl -fsSL https://blackman99.github.io/spoilheap/install.sh | sh     # Linux, macOS; checks SHA-256
cargo install --git https://github.com/Blackman99/spoilheap --tag v0.1.0   # from source, Rust 1.85+
```

On Windows, download `spoilheap-v0.1.0-x86_64-pc-windows-msvc.zip` (or `aarch64-pc-windows-msvc`) from [Releases](https://github.com/Blackman99/spoilheap/releases). Every release ships static Linux builds (x86_64 and aarch64, musl), macOS builds (arm64 and x86_64) and Windows builds (x86_64 and arm64), all listed in `checksums.txt`. The installer puts the binary in `~/.local/bin`; set `SPOILHEAP_BIN` to change that or `SPOILHEAP_VERSION` to pin a version.

To stop Claude Code before the disk fills, run `spoilheap hook claude` and paste the block it prints into `~/.claude/settings.json`. For Codex, use `spoilheap hook codex` and `~/.codex/config.toml`. To give an agent read-only access through MCP: `claude mcp add spoilheap -- spoilheap mcp`.

## More

- [GOAL.md](GOAL.md): what this tool is for, and what it will never do
- [ROADMAP.md](ROADMAP.md): stopping SAFE orphans, Windows process attribution, retention rules
- [CONTRIBUTING.md](CONTRIBUTING.md): adding a path requires a link to the agent's source or docs
- No network access and no telemetry. MIT licensed.

*Spoil heap: in archaeology, the pile of earth dug out of a trench.*
