//! Free and total space of the filesystem that holds a path.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    pub total: u64,
    /// Space an unprivileged user can still write (statvfs f_bavail).
    pub free: u64,
}

impl Space {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }
    pub fn free_pct(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.free as f64 * 100.0 / self.total as f64 }
    }
}

/// Test hook: SPOILHEAP_FAKE_FREE=<bytes>[/<total>] pretends the disk has that much free space.
fn fake() -> Option<Space> {
    let v = std::env::var("SPOILHEAP_FAKE_FREE").ok()?;
    let mut it = v.split('/');
    let free = crate::units::parse_amount(it.next()?).ok()?;
    let total = it.next().and_then(|t| crate::units::parse_amount(t).ok());
    let total = match total {
        Some(crate::units::Amount::Bytes(b)) => b,
        _ => 500_000_000_000,
    };
    Some(Space { free: free.resolve(total), total })
}

pub fn space(path: &Path) -> std::io::Result<Space> {
    if let Some(s) = fake() {
        return Ok(s);
    }
    let p = existing_ancestor(path);
    imp::space(&p)
}

/// statvfs needs a path that exists; walk up until one does.
pub fn existing_ancestor(path: &Path) -> std::path::PathBuf {
    let mut p = path.to_path_buf();
    while !p.exists() {
        if !p.pop() {
            return std::path::PathBuf::from("/");
        }
    }
    p
}

/// A stable id for the filesystem a path lives on (st_dev on Unix).
pub fn device(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(existing_ancestor(path)).ok().map(|m| m.dev())
    }
    #[cfg(not(unix))]
    {
        let p = existing_ancestor(path);
        let s = p.to_string_lossy().to_ascii_uppercase();
        s.chars().next().map(|c| c as u64)
    }
}

#[cfg(unix)]
mod imp {
    use super::Space;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub fn space(p: &Path) -> std::io::Result<Space> {
        let c = CString::new(p.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has a NUL byte"))?;
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let frsize = if st.f_frsize > 0 { st.f_frsize as u64 } else { st.f_bsize as u64 };
        Ok(Space { total: st.f_blocks as u64 * frsize, free: st.f_bavail as u64 * frsize })
    }
}

#[cfg(windows)]
mod imp {
    use super::Space;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    pub fn space(p: &Path) -> std::io::Result<Space> {
        let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Space { total, free: avail })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn real_disk_has_space() {
        let s = super::space(&std::env::temp_dir()).unwrap();
        assert!(s.total > 0 && s.free <= s.total);
    }
}
