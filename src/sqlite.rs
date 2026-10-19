//! Read a SQLite file's 100-byte header without opening the database.
//! Offsets from https://www.sqlite.org/fileformat2.html#the_database_header

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub page_size: u64,
    pub page_count: u64,
    pub free_pages: u64,
}

impl Header {
    pub fn free_bytes(&self) -> u64 {
        self.free_pages * self.page_size
    }
}

pub fn header(path: &Path) -> Option<Header> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut b = [0u8; 100];
    f.read_exact(&mut b).ok()?;
    parse(&b, f.metadata().ok()?.len())
}

pub fn parse(b: &[u8; 100], file_len: u64) -> Option<Header> {
    if &b[0..16] != b"SQLite format 3\0" {
        return None;
    }
    let ps = u16::from_be_bytes([b[16], b[17]]) as u64;
    let page_size = if ps == 1 { 65536 } else { ps };
    if page_size < 512 {
        return None;
    }
    let be32 = |o: usize| u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as u64;
    let change = be32(24);
    let valid_for = be32(92);
    let mut page_count = be32(28);
    // The in-header size is only trusted when "version-valid-for" matches the change counter.
    if page_count == 0 || valid_for != change {
        page_count = file_len / page_size;
    }
    let free_pages = be32(36).min(page_count);
    Some(Header {
        page_size,
        page_count,
        free_pages,
    })
}

/// Largest tables by bytes, via the sqlite3 CLI in read-only mode (needs the dbstat
/// virtual table, which most builds include). Never writes; gives up after 4 s.
pub fn top_tables(path: &Path) -> Option<Vec<(String, u64)>> {
    let mut child = Command::new("sqlite3")
        .arg("-readonly")
        .arg(path)
        .arg("SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY 2 DESC LIMIT 3;")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if start.elapsed() > Duration::from_secs(4) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
    let mut out = String::new();
    child.stdout?.read_to_string(&mut out).ok()?;
    let v: Vec<(String, u64)> = out
        .lines()
        .filter_map(|l| {
            let (n, s) = l.rsplit_once('|')?;
            Some((n.to_string(), s.trim().parse().ok()?))
        })
        .collect();
    if v.is_empty() { None } else { Some(v) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hdr(ps: u16, pages: u32, free: u32, valid: bool) -> [u8; 100] {
        let mut b = [0u8; 100];
        b[..16].copy_from_slice(b"SQLite format 3\0");
        b[16..18].copy_from_slice(&ps.to_be_bytes());
        b[24..28].copy_from_slice(&7u32.to_be_bytes());
        b[28..32].copy_from_slice(&pages.to_be_bytes());
        b[36..40].copy_from_slice(&free.to_be_bytes());
        b[92..96].copy_from_slice(&(if valid { 7u32 } else { 6 }).to_be_bytes());
        b
    }
    #[test]
    fn reads_free_pages() {
        let h = parse(&hdr(4096, 1000, 750, true), 4096 * 1000).unwrap();
        assert_eq!(h.free_bytes(), 750 * 4096);
        let h = parse(&hdr(1, 10, 2, true), 65536 * 10).unwrap();
        assert_eq!(h.page_size, 65536);
        // stale in-header size: fall back to the file length
        let h = parse(&hdr(4096, 1, 0, false), 4096 * 300).unwrap();
        assert_eq!(h.page_count, 300);
        let mut bad = hdr(4096, 1, 0, true);
        bad[0] = b'X';
        assert!(parse(&bad, 1).is_none());
    }
}
