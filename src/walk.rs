//! Measure a directory tree the way the disk sees it: allocated blocks, hard links
//! counted once, symlinks never followed, never crossing into another filesystem.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Default)]
pub struct Measure {
    pub bytes: u64,
    pub files: u64,
    pub newest: Option<SystemTime>,
    pub oldest: Option<SystemTime>,
}

impl Measure {
    pub fn add_file(&mut self, bytes: u64, mtime: Option<SystemTime>) {
        self.bytes += bytes;
        self.files += 1;
        if let Some(t) = mtime {
            if self.newest.is_none_or(|n| t > n) {
                self.newest = Some(t);
            }
            if self.oldest.is_none_or(|o| t < o) {
                self.oldest = Some(t);
            }
        }
    }
    pub fn merge(&mut self, o: &Measure) {
        self.bytes += o.bytes;
        self.files += o.files;
        for t in [o.newest, o.oldest].into_iter().flatten() {
            if self.newest.is_none_or(|n| t > n) {
                self.newest = Some(t);
            }
            if self.oldest.is_none_or(|x| t < x) {
                self.oldest = Some(t);
            }
        }
    }
}

/// Bytes the file occupies on disk.
pub fn allocated(md: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        md.blocks() * 512
    }
    #[cfg(not(unix))]
    {
        md.len()
    }
}

#[derive(Default)]
pub struct Walker {
    seen: HashSet<(u64, u64)>,
    /// Directories (by absolute path) to skip; their bytes belong to another item.
    pub skip: HashSet<PathBuf>,
}

impl Walker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true the first time a (dev, inode) pair is seen.
    fn first_sight(&mut self, md: &fs::Metadata) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if md.nlink() > 1 {
                return self.seen.insert((md.dev(), md.ino()));
            }
            true
        }
        #[cfg(not(unix))]
        {
            let _ = md;
            true
        }
    }

    pub fn measure(&mut self, root: &Path) -> Measure {
        let mut m = Measure::default();
        self.visit(root, None, &mut m, &mut |_, _| {});
        m
    }

    /// Walk `root`, calling `f(path, metadata)` for each regular file.
    pub fn each_file(&mut self, root: &Path, f: &mut dyn FnMut(&Path, &fs::Metadata)) -> Measure {
        let mut m = Measure::default();
        self.visit(root, None, &mut m, f);
        m
    }

    fn visit(&mut self, p: &Path, dev: Option<u64>, m: &mut Measure, f: &mut dyn FnMut(&Path, &fs::Metadata)) {
        let Ok(md) = fs::symlink_metadata(p) else { return };
        let ft = md.file_type();
        if ft.is_symlink() || self.skip.contains(p) {
            return;
        }
        #[cfg(unix)]
        let this_dev = {
            use std::os::unix::fs::MetadataExt;
            Some(md.dev())
        };
        #[cfg(not(unix))]
        let this_dev: Option<u64> = None;
        if let (Some(a), Some(b)) = (dev, this_dev) {
            if a != b {
                return; // another filesystem mounted inside: not ours to count
            }
        }
        if ft.is_dir() {
            m.bytes += allocated(&md);
            let Ok(rd) = fs::read_dir(p) else { return };
            for e in rd.flatten() {
                self.visit(&e.path(), this_dev.or(dev), m, f);
            }
        } else if ft.is_file() {
            if self.first_sight(&md) {
                m.add_file(allocated(&md), md.modified().ok());
                f(p, &md);
            }
        }
    }
}

/// canonicalize without Windows' \\?\ prefix (git and most tools reject it).
pub fn canon<P: AsRef<Path>>(p: P) -> std::io::Result<PathBuf> {
    let c = fs::canonicalize(p)?;
    #[cfg(windows)]
    {
        let s = c.to_string_lossy();
        if let Some(r) = s.strip_prefix(r"\\?\") {
            if !r.starts_with("UNC") {
                return Ok(PathBuf::from(r));
            }
        }
    }
    Ok(c)
}

pub fn measure(root: &Path) -> Measure {
    Walker::new().measure(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_files_and_dedupes_hardlinks() {
        let d = std::env::temp_dir().join(format!("spoilheap-walk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("a/b")).unwrap();
        fs::write(d.join("a/one"), vec![1u8; 50_000]).unwrap();
        fs::write(d.join("a/b/two"), vec![1u8; 70_000]).unwrap();
        #[cfg(unix)]
        {
            fs::hard_link(d.join("a/one"), d.join("a/b/one-again")).unwrap();
            std::os::unix::fs::symlink(d.join("a"), d.join("loop")).unwrap();
        }
        let m = measure(&d);
        assert_eq!(m.files, 2, "hard link counted once, symlink not followed");
        assert!(m.bytes >= 120_000, "allocated at least the data: {}", m.bytes);
        assert!(m.newest.is_some());
        fs::remove_dir_all(&d).unwrap();
    }
}
