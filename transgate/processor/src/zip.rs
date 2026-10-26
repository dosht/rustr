//! A stored (uncompressed) two-entry ZIP, as the source's JSZip
//! `compression: 'STORE'` produces: sizes and CRCs in the local headers, no
//! data descriptors. The CRCs are computed first, so the entries can then be
//! streamed from disk with an exact `Content-Length`.

use std::path::Path;
use std::time::SystemTime;

use bytes::Bytes;
use tokio::io::AsyncReadExt;

pub async fn crc32_file(path: &Path) -> std::io::Result<(u32, u64)> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut crc = crc32fast::Hasher::new();
    let mut len = 0u64;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            return Ok((crc.finalize(), len));
        }
        crc.update(&buf[..n]);
        len += n as u64;
    }
}

/// One stored entry.
pub struct Entry {
    pub name: String,
    pub crc: u32,
    pub size: u64,
}

/// The archive as alternating header bytes and entry data: `[local header 1,
/// (data 1), local header 2, (data 2), central directory]`. `None` when it
/// would need ZIP64, which the source cannot produce either.
pub struct Layout {
    pub local_headers: Vec<Bytes>,
    pub central_directory: Bytes,
    pub total_len: u64,
}

fn dos_time(at: SystemTime) -> (u16, u16) {
    let (y, mo, d, h, mi, s, _) = crate::log::civil(at);
    let year = (y.clamp(1980, 2107) - 1980) as u16;
    let date = (year << 9) | ((mo as u16) << 5) | d as u16;
    let time = ((h as u16) << 11) | ((mi as u16) << 5) | (s as u16 / 2);
    (time, date)
}

pub fn layout(entries: &[Entry], at: SystemTime) -> Option<Layout> {
    let (time, date) = dos_time(at);
    let mut local_headers = Vec::new();
    let mut central = Vec::new();
    let mut offset: u64 = 0;
    for e in entries {
        let size = u32::try_from(e.size).ok()?;
        let local_offset = u32::try_from(offset).ok()?;
        let name = e.name.as_bytes();
        let flags: u16 = if e.name.is_ascii() { 0 } else { 1 << 11 };
        let mut h = Vec::with_capacity(30 + name.len());
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&10u16.to_le_bytes()); // version needed
        h.extend_from_slice(&flags.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes()); // stored
        h.extend_from_slice(&time.to_le_bytes());
        h.extend_from_slice(&date.to_le_bytes());
        h.extend_from_slice(&e.crc.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&(name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(name);
        offset += h.len() as u64 + e.size;
        local_headers.push(Bytes::from(h));

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by: DOS, 2.0
        central.extend_from_slice(&10u16.to_le_bytes());
        central.extend_from_slice(&flags.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&e.crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 2]); // extra
        central.extend_from_slice(&[0; 2]); // comment
        central.extend_from_slice(&[0; 2]); // disk
        central.extend_from_slice(&[0; 2]); // internal attributes
        central.extend_from_slice(&[0; 4]); // external attributes
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let cd_offset = u32::try_from(offset).ok()?;
    let cd_size = u32::try_from(central.len()).ok()?;
    let count = entries.len() as u16;
    central.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    central.extend_from_slice(&[0; 4]); // disk numbers
    central.extend_from_slice(&count.to_le_bytes());
    central.extend_from_slice(&count.to_le_bytes());
    central.extend_from_slice(&cd_size.to_le_bytes());
    central.extend_from_slice(&cd_offset.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    let total_len = offset + central.len() as u64;
    Some(Layout { local_headers, central_directory: Bytes::from(central), total_len })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_over_four_gigabytes_is_refused_rather_than_corrupted() {
        let big = [Entry { name: "a".into(), crc: 0, size: u64::from(u32::MAX) }, Entry { name: "b".into(), crc: 0, size: 1 }];
        assert!(layout(&big, SystemTime::UNIX_EPOCH).is_none());
    }

    /// `unzip -t` verifies the CRCs and layout of a real archive.
    #[tokio::test]
    async fn the_stored_layout_is_a_valid_zip() {
        let dir = std::env::temp_dir().join(format!("tg-zip-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::write(&a, b"first part").unwrap();
        std::fs::write(&b, vec![7u8; 70_000]).unwrap();
        let mut entries = Vec::new();
        for (path, name) in [(&a, "song_part1.mp3"), (&b, "song_part2.mp3")] {
            let (crc, size) = crc32_file(path).await.unwrap();
            entries.push(Entry { name: name.into(), crc, size });
        }
        let l = layout(&entries, SystemTime::now()).unwrap();
        let mut zip = Vec::new();
        for (header, path) in l.local_headers.iter().zip([&a, &b]) {
            zip.extend_from_slice(header);
            zip.extend_from_slice(&std::fs::read(path).unwrap());
        }
        zip.extend_from_slice(&l.central_directory);
        assert_eq!(zip.len() as u64, l.total_len);
        let archive = dir.join("t.zip");
        std::fs::write(&archive, &zip).unwrap();
        let out = std::process::Command::new("unzip").arg("-t").arg(&archive).output().expect("unzip installed");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{text}");
        assert!(text.contains("song_part1.mp3") && text.contains("song_part2.mp3") && text.contains("No errors"), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
