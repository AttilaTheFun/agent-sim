//! Reader for x86Sim's frame ring: the shared-memory file simfb publishes every presented frame into.
//!
//! Layout (little endian): "SFBR" magic; u32 slot count at 8 and slot size at 12; u64 sequence at 32 and u32 latest
//! slot at 40; 32-byte slot descriptors from 64 (u64 seq, u64 publish time ns, u32 width, height, stride); pixels
//! (BGRA) from 4096, one slot after another. simfb wakes futex waiters on the low word of the sequence.

use memmap2::Mmap;
use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

const HEADER: usize = 4096;
const MAGIC: &[u8; 4] = b"SFBR";

/// One frame copied out of the ring, rows packed to `width * 4` bytes.
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub seq: u64,
    pub published_ns: u64,
    pub bgra: Vec<u8>,
}

pub struct Ring {
    path: PathBuf,
    map: Option<Mmap>,
    id: (u64, u64, u64),
}

impl Ring {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            map: None,
            id: (0, 0, 0),
        }
    }

    /// Maps the ring, again when a reboot replaced it (another inode or size).
    fn open(&mut self) -> bool {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            self.map = None;
            return false;
        };
        let id = (meta.dev(), meta.ino(), meta.len());
        if self.map.is_some() && id == self.id {
            return true;
        }
        if (meta.len() as usize) < HEADER {
            return false;
        }
        let Ok(file) = File::open(&self.path) else {
            return false;
        };
        // SAFETY: the file is only ever read through this map; simfb writes into it concurrently, and every value
        // read from it is checked (magic, bounds, sequence before and after the copy) before it is used.
        #[allow(unsafe_code)]
        let map = unsafe { Mmap::map(&file) };
        let Ok(map) = map else { return false };
        if &map[..4] != MAGIC {
            return false;
        }
        self.map = Some(map);
        self.id = id;
        true
    }

    fn u32_at(map: &Mmap, off: usize) -> u32 {
        u32::from_le_bytes(map[off..off + 4].try_into().unwrap())
    }

    fn seq_atomic(map: &Mmap) -> &AtomicU64 {
        // SAFETY: offset 32 of a page-aligned map is 8-byte aligned and inside the header.
        #[allow(unsafe_code)]
        unsafe {
            &*(map.as_ptr().add(32) as *const AtomicU64)
        }
    }

    fn slot_seq(map: &Mmap, off: usize) -> u64 {
        // SAFETY: slot descriptors start at 64 + 32 * i: 8-byte aligned, and bounds were checked by the caller.
        #[allow(unsafe_code)]
        unsafe {
            (*(map.as_ptr().add(off) as *const AtomicU64)).load(Ordering::Acquire)
        }
    }

    /// Frames published so far (0 when there is no ring yet).
    pub fn seq(&mut self) -> u64 {
        if !self.open() {
            return 0;
        }
        Self::seq_atomic(self.map.as_ref().unwrap()).load(Ordering::Acquire)
    }

    /// Blocks until the sequence differs from `seen` or `timeout` passes; returns the current sequence.
    pub fn wait(&mut self, seen: u64, timeout: Duration) -> u64 {
        if !self.open() {
            std::thread::sleep(timeout.min(Duration::from_millis(100)));
            return self.seq();
        }
        let map = self.map.as_ref().unwrap();
        let cur = Self::seq_atomic(map).load(Ordering::Acquire);
        if cur != seen {
            return cur;
        }
        let word = map.as_ptr().wrapping_add(32) as *const AtomicU32;
        let ts = libc::timespec {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as libc::c_long,
        };
        #[cfg(target_os = "linux")]
        // SAFETY: FUTEX_WAIT only reads the word, which lies inside the live map; it returns at once if the word no
        // longer equals the low 32 bits of `seen`.
        #[allow(unsafe_code)]
        unsafe {
            libc::syscall(
                libc::SYS_futex,
                word,
                libc::FUTEX_WAIT,
                seen as u32,
                &ts as *const libc::timespec,
                std::ptr::null::<u32>(),
                0,
            );
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (word, ts);
            std::thread::sleep(Duration::from_millis(4));
        }
        self.seq()
    }

    /// The newest frame, or None (no ring yet, or it changed under every attempt).
    pub fn latest(&mut self) -> Option<Frame> {
        if !self.open() {
            return None;
        }
        let map = self.map.as_ref().unwrap();
        let slots = Self::u32_at(map, 8) as usize;
        let slot_bytes = Self::u32_at(map, 12) as usize;
        for _ in 0..4 {
            let seq = Self::seq_atomic(map).load(Ordering::Acquire);
            let latest = Self::u32_at(map, 40) as usize;
            if seq == 0 || latest >= slots {
                return None;
            }
            let d = 64 + 32 * latest;
            let sseq = Self::slot_seq(map, d);
            let published_ns = u64::from_le_bytes(map[d + 8..d + 16].try_into().unwrap());
            let width = Self::u32_at(map, d + 16) as usize;
            let height = Self::u32_at(map, d + 20) as usize;
            let stride = Self::u32_at(map, d + 24) as usize;
            let base = HEADER + latest * slot_bytes;
            if sseq == 0
                || width == 0
                || stride < width * 4
                || stride * height > slot_bytes
                || base + stride * height > map.len()
            {
                continue;
            }
            let mut bgra = vec![0u8; width * 4 * height];
            for y in 0..height {
                let src = base + y * stride;
                bgra[y * width * 4..(y + 1) * width * 4]
                    .copy_from_slice(&map[src..src + width * 4]);
            }
            if Self::slot_seq(map, d) == sseq {
                return Some(Frame {
                    width,
                    height,
                    seq: sseq,
                    published_ns,
                    bgra,
                });
            }
        }
        None
    }
}
