//! Read-only memory map wrapper for immutable pack files.
//!
//! Why unsafe here: the workspace denies `unsafe_code`, so the parent module
//! carries the only scoped exception in this crate. Raw positioned reads do
//! not meet the measured lookup-latency target, while mappings let immutable
//! pack generations share pages across bounded snapshots.
//!
//! Safety invariants:
//! - Mapped files are immutable for the lifetime of the mapping. Index runs
//!   are published with an atomic rename and never mutated in place; the
//!   append pack is only extended, and the pack mapping is replaced after
//!   every append and after open-time tail truncation, before any read.
//! - Nothing ever writes through a mapping; all access is via shared
//!   slices. `Send`/`Sync` are therefore sound: concurrent readers only
//!   read stable bytes.
//! - External truncation or mutation of a live mapped file is outside the
//!   process fault model and can cause SIGBUS, like any mmap-backed engine.
//!   The pack writer never mutates mapped prefixes or immutable run files;
//!   startup validates committed bytes before publishing a view.
//! - A zero-length map stores a dangling-but-never-dereferenced pointer
//!   and yields an empty slice.

use anyhow::{Context, Result};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::OnceLock;

/// Read-only mapping of one immutable file (or file prefix).
#[derive(Debug)]
pub(crate) struct Mmap {
    ptr: *mut libc::c_void,
    len: usize,
}

#[allow(unsafe_code)]
impl Mmap {
    /// Maps the first `len` bytes of `file`; `len == 0` yields an empty map.
    pub(crate) fn map(file: &File, len: u64, what: &Path) -> Result<Self> {
        let len = usize::try_from(len).context("mmap length does not fit usize")?;
        if len == 0 {
            return Ok(Self {
                ptr: std::ptr::null_mut(),
                len: 0,
            });
        }
        // SAFETY: read-only mapping of a valid file descriptor; the kernel
        // keeps the mapping alive independently of the descriptor. Invariants
        // above guarantee the file is not mutated while mapped.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            anyhow::bail!(
                "mmap {} bytes of {} failed: {}",
                len,
                what.display(),
                std::io::Error::last_os_error()
            );
        }
        Ok(Self { ptr, len })
    }

    /// Maps the first `len` bytes and requires random-access readahead advice.
    ///
    /// Callers use a separate mapping for sparse lookups so this hint cannot
    /// degrade sequential scrubs, validation, or compaction.
    pub(crate) fn map_random(file: &File, len: u64, what: &Path) -> Result<Self> {
        let mapping = Self::map(file, len, what)?;
        if mapping.len == 0 {
            return Ok(mapping);
        }
        // SAFETY: `ptr`/`len` describe the live mapping created above.
        // MADV_RANDOM changes only the kernel's readahead policy; it does not
        // mutate mapped bytes or relax any lifetime invariant.
        let result = unsafe { libc::madvise(mapping.ptr, mapping.len, libc::MADV_RANDOM) };
        if result != 0 {
            anyhow::bail!(
                "madvise(MADV_RANDOM) for {} bytes of {} failed: {}",
                mapping.len,
                what.display(),
                std::io::Error::last_os_error()
            );
        }
        Ok(mapping)
    }

    /// Creates a dedicated sequential mapping for streaming validation or
    /// compaction. It is separate from the lookup mapping so readahead advice
    /// and page reclamation cannot perturb point readers.
    pub(crate) fn map_sequential(file: &File, len: u64, what: &Path) -> Result<Self> {
        let mapping = Self::map(file, len, what)?;
        if mapping.len == 0 {
            return Ok(mapping);
        }
        // SAFETY: `ptr`/`len` describe the live read-only mapping above.
        let result = unsafe { libc::madvise(mapping.ptr, mapping.len, libc::MADV_SEQUENTIAL) };
        if result != 0 {
            anyhow::bail!(
                "madvise(MADV_SEQUENTIAL) for {} bytes of {} failed: {}",
                mapping.len,
                what.display(),
                std::io::Error::last_os_error()
            );
        }
        Ok(mapping)
    }

    /// Releases complete pages in an already-consumed byte range of a
    /// dedicated sequential mapping. The mapping and file bytes remain valid;
    /// a later second pass faults them back in deterministically.
    pub(crate) fn advise_dontneed(&self, start: usize, end: usize) -> Result<usize> {
        if self.len == 0 || start >= end {
            return Ok(start);
        }
        anyhow::ensure!(end <= self.len, "madvise range exceeds mapping");
        let page = *PAGE_SIZE.get_or_init(|| {
            // SAFETY: sysconf has no memory-safety preconditions.
            let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            usize::try_from(value)
                .ok()
                .filter(|value| *value > 0)
                .unwrap_or(4096)
        });
        let aligned_start = start.div_ceil(page) * page;
        let aligned_end = end / page * page;
        if aligned_start >= aligned_end {
            return Ok(start);
        }
        // SAFETY: the aligned subrange lies inside this live mapping.
        let address = unsafe { self.ptr.cast::<u8>().add(aligned_start).cast() };
        let result =
            unsafe { libc::madvise(address, aligned_end - aligned_start, libc::MADV_DONTNEED) };
        if result != 0 {
            anyhow::bail!(
                "madvise(MADV_DONTNEED) for mapped range {aligned_start}..{aligned_end} failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(aligned_end)
    }

    /// The mapped bytes as a shared slice.
    pub(crate) fn as_slice(&self) -> &[u8] {
        if self.ptr.is_null() {
            return &[];
        }
        // SAFETY: `ptr` is a live read-only mapping of `len` bytes for the
        // lifetime of `self`; bytes are immutable per the module invariants.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
    }
}

static PAGE_SIZE: OnceLock<usize> = OnceLock::new();

#[allow(unsafe_code)]
impl Drop for Mmap {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: `ptr`/`len` came from a successful `mmap` and are
            // unmapped exactly once here.
            let _ = unsafe { libc::munmap(self.ptr, self.len) };
        }
    }
}

// SAFETY: mappings are read-only and the underlying files are immutable
// while mapped, so shared references across threads cannot race.
#[allow(unsafe_code)]
unsafe impl Send for Mmap {}
#[allow(unsafe_code)]
unsafe impl Sync for Mmap {}
