//! JSON output and the id map that lets `spoilheap clean 104` find item [104] again.

use crate::dig::{Action, Env, Item, Report};
use crate::render::DiskLine;
use crate::units;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

fn secs(t: Option<SystemTime>, now: SystemTime) -> Value {
    t.map(|t| json!(units::age_secs(t, now))).unwrap_or(Value::Null)
}

pub fn action(a: &Action) -> Value {
    match a {
        Action::Remove { paths, .. } => json!({"type": "remove", "paths": paths.len()}),
        Action::WorktreeRemove { repo, worktree, .. } => json!({"type": "git-worktree-remove", "command": format!("git -C '{}' worktree remove '{}'", repo.display(), worktree.display())}),
        Action::WorktreePrune { repo } => json!({"type": "git-worktree-prune", "command": format!("git -C '{}' worktree prune", repo.display())}),
        Action::Manual(c) => json!({"type": "manual", "command": c}),
        Action::None => json!({"type": "none"}),
    }
}

pub fn item(it: &Item, now: SystemTime) -> Value {
    json!({
        "id": it.id,
        "harness": it.harness,
        "kind": it.kind,
        "label": it.label,
        "path": it.path.display().to_string(),
        "bytes": it.bytes,
        "reclaimable_bytes": it.reclaim,
        "files": it.files,
        "newest_age_s": secs(it.newest, now),
        "oldest_age_s": secs(it.oldest, now),
        "verdict": it.verdict.word(),
        "reason": it.reason,
        "action": action(&it.action),
        "parent": it.parent.map(|p| p as u32 + 101),
    })
}

pub fn report(rep: &Report, disks: &[DiskLine]) -> Value {
    let now = SystemTime::now();
    let (all, safe, review, keep) = crate::render::totals(&rep.items);
    json!({
        "tool": "spoilheap",
        "version": env!("CARGO_PKG_VERSION"),
        "generated_at": now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        "disks": disks.iter().map(|d| json!({"path": d.mount.display().to_string(), "total_bytes": d.space.total, "free_bytes": d.space.free})).collect::<Vec<_>>(),
        "totals": {"bytes": all, "safe_reclaimable_bytes": safe, "review_reclaimable_bytes": review, "keep_bytes": keep},
        "repos_scanned": rep.repos,
        "notes": rep.notes,
        "items": rep.items.iter().map(|i| item(i, now)).collect::<Vec<_>>(),
    })
}

pub fn save_ids(env: &Env, rep: &Report) {
    let dir = env.cache_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let v = json!({
        "generated_at": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        "items": rep.items.iter().map(|i| json!({"id": i.id, "key": i.key()})).collect::<Vec<_>>(),
    });
    let tmp = dir.join("last.json.tmp");
    if std::fs::write(&tmp, v.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join("last.json"));
    }
}

/// (id → key, age of the map in seconds)
pub fn load_ids(env: &Env) -> Option<(Vec<(u32, String)>, u64)> {
    let s = std::fs::read_to_string(env.cache_dir().join("last.json")).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    let at = v["generated_at"].as_u64().unwrap_or(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let items = v["items"].as_array()?.iter().filter_map(|i| Some((i["id"].as_u64()? as u32, i["key"].as_str()?.to_string()))).collect();
    Some((items, now.saturating_sub(at)))
}
