#!/usr/bin/env bash
# Build a fake home that looks like a heavy agent user's machine, with REAL git
# worktrees, a REAL SQLite database full of free pages, and real files.
# usage: scripts/demohome.sh DIR [big]
#   big: allocate realistic sizes with fallocate (Linux; needs ~40 GB free)
set -euo pipefail
H=${1:?usage: demohome.sh DIR [big]}
BIG=${2:-}
rm -rf "$H"; mkdir -p "$H"
H=$(cd "$H" && pwd)
export GIT_AUTHOR_NAME=demo GIT_AUTHOR_EMAIL=demo@example.com GIT_COMMITTER_NAME=demo GIT_COMMITTER_EMAIL=demo@example.com
git_q() { git -c init.defaultBranch=main -c advice.detachedHead=false "$@" >/dev/null 2>&1; }

# blob PATH SIZE [AGE]: a file of SIZE bytes (k/M/G suffix), mtime AGE ago (e.g. "40 days")
blob() {
  local p=$1 sz=$2 age=${3:-}
  mkdir -p "$(dirname "$p")"
  if [ -n "$BIG" ] && command -v fallocate >/dev/null; then
    fallocate -l "$sz" "$p"
  else
    # small mode: 1/1000 of the size so the demo runs anywhere
    local n; n=$(numfmt --from=iec "$sz" 2>/dev/null || echo 4096)
    head -c $(( n / 1000 + 1 )) /dev/urandom > "$p"
  fi
  if [ -n "$age" ]; then touch -d "$age ago" "$p"; fi
}

# --- Codex -------------------------------------------------------------
C=$H/.codex
for d in 2026/05/02 2026/06/11 2026/07/19 2026/08/23; do
  for i in 1 2 3; do blob "$C/sessions/$d/rollout-$d-$i.jsonl" 2600M "$(( (RANDOM % 60) + 60 )) days"; done
done
for i in 1 2 3 4; do blob "$C/sessions/2026/10/1$i/rollout-recent-$i.jsonl" 900M "$i days"; done
blob "$C/archived_sessions/rollout-archived-1.jsonl" 3100M "75 days"
blob "$C/log/codex-tui.log.1" 1400M "6 days"
blob "$C/log/codex-tui.log" 40M ""

# a real repo with real worktrees
R=$H/code/payments
mkdir -p "$R"; cd "$R"
git_q init; printf 'target/\nnode_modules/\n' > .gitignore; echo "# payments" > README.md
git_q add -A; git_q commit -m init
git_q checkout -b feat/retry-backoff; echo retry > retry.rs; git_q add -A; git_q commit -m "retry"; git_q checkout main; git_q merge --no-ff feat/retry-backoff -m merge
mkdir -p "$C/worktrees/a41f/payments" "$C/worktrees/b77c/payments" "$C/worktrees/c903/payments" "$C/worktrees/d215/payments"
git_q worktree add --detach "$C/worktrees/a41f/payments" feat/retry-backoff    # merged, clean → SAFE
git_q worktree add -b codex/webhook-sig "$C/worktrees/b77c/payments" main       # unique commits → REVIEW
( cd "$C/worktrees/b77c/payments"; echo sig > sig.rs; git_q add -A; git_q commit -m sig; echo v2 >> sig.rs; git_q commit -am sig2 )
git_q worktree add -b codex/ledger "$C/worktrees/c903/payments" main            # dirty → KEEP
( cd "$C/worktrees/c903/payments"; echo wip > ledger.rs; echo wip2 > ledger_test.rs )
git_q worktree add --detach "$C/worktrees/d215/payments" main                  # clean, merged → SAFE
blob "$C/worktrees/a41f/payments/target/debug/deps/libpayments.rlib" 6200M "9 days"
blob "$C/worktrees/c903/payments/target/debug/payments" 4800M "3 hours"
blob "$C/worktrees/d215/payments/node_modules/.pnpm/pkg.tgz" 2100M "12 days"
# every file in the worktrees gets an old mtime so they are not "touched just now"
find "$C/worktrees/a41f" "$C/worktrees/d215" "$C/worktrees/b77c" -path '*/.git' -prune -o -type f -exec touch -d "9 days ago" {} +
# a worktree whose folder was deleted by hand → prunable registration
git_q worktree add --detach "$H/tmp-wt" main; rm -rf "$H/tmp-wt"
# a git that died mid-write: half-written packs (openai/codex#54955)
for i in 1 2 3 4 5 6; do blob "$R/.git/objects/pack/tmp_pack_$(head -c4 /dev/urandom | od -An -tx1 | tr -d ' \n')" 1900M "2 days"; done
cd "$H"

# --- crash dumps (macOS layout) ---------------------------------------
CP="$H/Library/Application Support/Codex/Crashpad"
for i in $(seq 1 60); do blob "$CP/pending/dump-$i.dmp" 280M "$(( i % 50 )) minutes"; done
for i in $(seq 1 40); do blob "$CP/completed/old-$i.dmp" 260M "3 days"; done
printf 'x' > "$CP/settings.dat"

# --- Claude Code -------------------------------------------------------
CL=$H/.claude
for p in -Users-demo-code-payments -Users-demo-code-site -Users-demo-notes; do
  for i in 1 2 3; do blob "$CL/projects/$p/session-old-$i.jsonl" 700M "$(( 40 + i * 9 )) days"; done
  blob "$CL/projects/$p/session-new.jsonl" 300M "2 days"
done
blob "$CL/file-history/s1/edit-1" 900M "50 days"
blob "$CL/file-history/s2/edit-1" 200M "1 days"
for i in 1 2 3; do blob "$CL/shell-snapshots/snapshot-zsh-$i.sh" 2M "4 days"; done
blob "$CL/debug/latest.txt" 300M "8 days"

# --- OpenCode: a real SQLite DB where DELETE never shrank the file -------
O=$H/.local/share/opencode
mkdir -p "$O"
python3 - "$O/opencode.db" "$BIG" <<'PY'
import sqlite3, sys, os
db, big = sys.argv[1], sys.argv[2]
rows = 40000 if big else 4000
con = sqlite3.connect(db)
con.execute("PRAGMA page_size=4096")
con.execute("CREATE TABLE event (id INTEGER PRIMARY KEY, body BLOB)")
con.execute("CREATE TABLE session (id INTEGER PRIMARY KEY, title TEXT)")
blob = os.urandom(16384)
con.executemany("INSERT INTO event(body) VALUES (?)", ((blob,) for _ in range(rows)))
con.executemany("INSERT INTO session(title) VALUES (?)", (("s%d" % i,) for i in range(200)))
con.commit()
con.execute("DELETE FROM event WHERE id % 4 != 0")   # prune 75% of events
con.commit(); con.close()
PY
for p in 6f1d2a0c 9be4410a; do blob "$O/snapshot/$p/objects/pack/pack-1.pack" 3600M "45 days"; done
blob "$O/snapshot/0a77c3e1/objects/pack/pack-1.pack" 1200M "1 days"
blob "$O/log/2026-10-10.log" 500M "9 days"

# --- Gemini CLI, Cline ------------------------------------------------
blob "$H/.gemini/tmp/3f2a9c1e8b7d/chats/session-1.json" 400M "70 days"
blob "$H/.cline/data/tasks/1759000000000/ui_messages.json" 600M "60 days"
blob "$H/.cline/data/checkpoints/9921/objects/pack/p.pack" 1500M "60 days"

echo "$H"
