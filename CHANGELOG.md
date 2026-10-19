# Changelog

## 0.1.0 — 2026-10-19

First release.

- `spoilheap` (dig): finds what Codex, Claude Code, OpenCode, Gemini CLI, Cline and Aider left on disk (worktrees, build output inside them, session history, crash dumps, half-written git packs, SQLite free pages, snapshots, logs) and gives each item a verdict: SAFE, REVIEW or KEEP, with the reason.
- Worktree verdicts come from git itself: uncommitted files and running processes mean KEEP; commits that exist on no branch, tag or remote mean REVIEW; clean and merged means SAFE.
- `spoilheap clean --safe` / `clean 104 107`: dry run by default, `--yes` to act. Worktrees go through `git worktree remove`; databases are never edited (it prints the `VACUUM` command instead).
- `spoilheap watch`: names the folder that is growing, the file being written and the process chain writing it.
- `spoilheap guard` + `spoilheap hook claude|codex`: a PreToolUse hook that blocks the agent's next tool call when free space drops under a floor.
- `spoilheap mcp`: read-only MCP server (`disk_status`, `disk_leftovers`).
- Prebuilt binaries for Linux (x86_64, aarch64, static musl), macOS (arm64, x86_64) and Windows (x86_64).
