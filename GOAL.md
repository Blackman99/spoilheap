# Goal

Show every gigabyte your coding agents left on this machine, say per item whether it is safe to delete and why, clear only what is safe, and stop the agent before the disk hits zero.

## Who it is for
People who run Claude Code, Codex (CLI and desktop), OpenCode, Gemini CLI or Cline on a laptop or a cloud dev box, and who have had a "disk full" surprise or are heading for one.

## Rules that do not change
1. **Looking never changes anything.** `spoilheap`, `paths`, `watch`, `guard` and `mcp` only read.
2. **Deleting is opt-in, item by item.** `clean` is a dry run without `--yes`, it re-checks every item right before acting, and KEEP is never touched.
3. **Only agent-owned paths.** spoilheap only works in the folders the agents themselves define (taken from their source or docs), plus `.git/objects/pack`, stale worktree registrations and git-ignored build output inside agent worktrees.
4. **git decides about worktrees.** Uncommitted changes or a process inside means KEEP. Commits that exist nowhere else mean REVIEW, with the count. Removal goes through `git worktree remove`, never `rm -rf` and never `--force`.
5. **Databases are never edited.** spoilheap measures free pages from the header and prints the `VACUUM` command for you to run.
6. **No network, no telemetry.**

## Out of scope
- General system cleaning (browser caches, Xcode, Docker images). Mole and similar tools do that well.
- Reading or summarising what is *in* transcripts. For secrets pasted into chats, use [spillscan](https://github.com/Blackman99/spillscan).
- Deleting anything automatically, on a schedule or from an MCP call.
