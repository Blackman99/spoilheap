# Roadmap

Each item comes from an open issue in an agent's own tracker. Dates are targets, not promises.

## v0.2: orphaned agent processes
Leftover processes count too. Codex restarts have doubled its helper and MCP servers (111 → 198 app processes, 78 → 156 Node helpers in 27 min: [openai/codex#55141](https://github.com/openai/codex/issues/55141)). Claude Code can leave its snapshot shell orphaned at PPID 1, burning a core ([anthropics/claude-code#103313](https://github.com/anthropics/claude-code/issues/103313)).
- [x] `spoilheap procs`: agent helper processes grouped by parent agent, with orphans (PPID 1, or a parent that no longer exists) marked, plus memory and CPU, and SAFE / REVIEW / KEEP verdicts. It only lists, and prints the `kill -TERM` line for SAFE orphans.
- [ ] Stopping from spoilheap itself: SAFE orphans only, re-checked right before, SIGTERM first, item by item and opt-in like `clean --yes`.

## v0.3: Windows parity
- Process attribution in `watch` on Windows (Restart Manager API).
- `spoilheap procs` on Windows (it says "not supported yet" today).
- Codex desktop paths on Windows (`%LOCALAPPDATA%`), checked against the app's source.

## v0.4: retention rules
- An optional `~/.config/spoilheap/keep.toml` (for example `sessions = "30d"`, `worktrees = "merged"`), run from a SessionStart hook. It only ever applies to SAFE items, and it prints what it did.

## Not planned
- Automatic deletion of REVIEW items, a delete tool over MCP, or editing agent databases.
