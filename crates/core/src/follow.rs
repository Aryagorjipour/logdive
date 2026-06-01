//! File tailing for follow mode (`--follow`).
//!
//! This module owns the low-level mechanics of watching a growing file:
//! tracking the read offset, buffering partial lines, and detecting the
//! two conditions that require re-opening the file:
//!
//! - **Rotation**: the file at the watched path has been replaced by a new
//!   file. Detected by comparing `(dev, ino)` before and after each read
//!   via [`std::os::unix::fs::MetadataExt`]. When the identity changes,
//!   the old handle is closed, the path is re-opened, and the offset resets
//!   to 0.
//!
//! - **Truncation**: the on-disk file size is smaller than the tracked
//!   offset — a `>` redirect or `truncate(1)` was used in-place. On
//!   detection, the offset resets to 0 and reading continues from the
//!   beginning of the (now-shorter) file.
//!
//! Both conditions are checked on every call to [`FileTailer::read_new_lines`].
//!
//! # Tail-from-EOF semantics
//!
//! [`FileTailer::open`] seeks to the current end-of-file immediately. The
//! first call to [`read_new_lines`] therefore returns only bytes appended
//! *after* `open` was called, matching `tail -f` behaviour.
//!
//! # Partial line buffering
//!
//! Bytes that do not yet end with a newline are kept in `leftover` until
//! subsequent reads complete the line. Both `\n` and `\r\n` are stripped.
//!
//! # Thread safety
//!
//! `FileTailer` is single-threaded by design. The CLI watch loop owns the
//! instance exclusively. No `Mutex` is needed.
//!
//! # Unix-only
//!
//! `(dev, ino)` rotation detection requires `std::os::unix::fs::MetadataExt`.
//! The module is gated with `#![cfg(unix)]`; Windows support is deferred to
//! v0.3.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::error::{LogdiveError, Result};

/// Read buffer for each [`FileTailer::read_new_lines`] call.
///
/// 8 KiB is a common sweet-spot: large enough to amortise syscall overhead
/// for typical log line rates, small enough to keep stack pressure low.
const READ_BUFFER_SIZE: usize = 8 * 1024;

/// Tracks a growing file, yielding newly appended complete lines on each
/// call to [`read_new_lines`].
///
/// See the module-level documentation for the full semantics.
#[derive(Debug)]
pub struct FileTailer {
    /// Watched path. Re-opened on rotation.
    path: PathBuf,
    /// Open handle to the file at `path` at construction or last rotation.
    file: File,
    /// Byte offset of the next read inside the current file handle.
    offset: u64,
    /// inode number of the open file handle, used for rotation detection.
    inode: u64,
    /// Device number of the open file handle, used for rotation detection.
    dev: u64,
    /// Bytes of an incomplete trailing line carried over between reads.
    leftover: Vec<u8>,
}

impl FileTailer {
    /// Open the file at `path` and seek to its current end.
    ///
    /// Subsequent calls to [`read_new_lines`] return only data appended
    /// after this point — identical to `tail -f` startup behaviour.
    ///
    /// Returns `Err` if the file does not exist or cannot be opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path).map_err(|e| LogdiveError::io_at(&path, e))?;

        let meta = file.metadata().map_err(|e| LogdiveError::io_at(&path, e))?;
        let inode = meta.ino();
        let dev = meta.dev();
        let offset = meta.len();

        // Seek to EOF so we only return bytes appended after open().
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| LogdiveError::io_at(&path, e))?;

        Ok(Self {
            path,
            file,
            offset,
            inode,
            dev,
            leftover: Vec::new(),
        })
    }

    /// Read any newly appended bytes, split into complete lines, and return
    /// them. A partial trailing line is buffered until it is terminated.
    ///
    /// Both `\n` and `\r\n` line endings are stripped. Invalid UTF-8 bytes
    /// are replaced with U+FFFD via [`String::from_utf8_lossy`].
    ///
    /// Rotation and truncation are checked before every read. If the path
    /// briefly disappears during a rotation (the window between the old
    /// file being renamed away and the new one being created), this method
    /// returns `Ok(vec![])` and retries on the next call.
    pub fn read_new_lines(&mut self) -> Result<Vec<String>> {
        // --- Rotation / truncation check -----------------------------------
        match std::fs::metadata(&self.path) {
            Ok(meta) => {
                let current_ino = meta.ino();
                let current_dev = meta.dev();
                let current_size = meta.len();

                if current_ino != self.inode || current_dev != self.dev {
                    // The file at the path is a different inode: rotation.
                    self.handle_rotation()?;
                } else if current_size < self.offset {
                    // Same inode but size shrank: truncation.
                    self.offset = 0;
                    self.leftover.clear();
                    self.file
                        .seek(SeekFrom::Start(0))
                        .map_err(|e| LogdiveError::io_at(&self.path, e))?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Path is momentarily absent (mid-rotation gap). Return
                // empty and let the caller retry on the next event.
                return Ok(vec![]);
            }
            Err(e) => return Err(LogdiveError::io_at(&self.path, e)),
        }

        // --- Read new bytes -------------------------------------------------
        let mut buf = [0u8; READ_BUFFER_SIZE];
        let mut raw_bytes: Vec<u8> = Vec::new();

        loop {
            match self.file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    raw_bytes.extend_from_slice(&buf[..n]);
                    self.offset += n as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(LogdiveError::io_at(&self.path, e)),
            }
        }

        if raw_bytes.is_empty() && self.leftover.is_empty() {
            return Ok(vec![]);
        }

        // Prepend any leftover bytes from the previous call.
        let mut combined = std::mem::take(&mut self.leftover);
        combined.extend_from_slice(&raw_bytes);

        // --- Split into lines -----------------------------------------------
        let mut lines: Vec<String> = Vec::new();
        let mut start = 0usize;

        while start < combined.len() {
            // Find the next newline.
            match combined[start..].iter().position(|&b| b == b'\n') {
                Some(rel) => {
                    let end = start + rel;
                    // Slice the line bytes, stripping \r if present.
                    let line_bytes = if end > start && combined[end - 1] == b'\r' {
                        &combined[start..end - 1]
                    } else {
                        &combined[start..end]
                    };
                    let line = String::from_utf8_lossy(line_bytes).into_owned();
                    lines.push(line);
                    start = end + 1; // skip the \n
                }
                None => {
                    // Remainder is a partial line — buffer it.
                    self.leftover = combined[start..].to_vec();
                    return Ok(lines);
                }
            }
        }

        Ok(lines)
    }

    /// Close the current file handle and re-open the path from the beginning.
    ///
    /// Called when rotation is detected (inode or device changed). If the
    /// new file at `path` does not yet exist (brief rotation gap), this
    /// returns `Ok(())` with the internal state left stale; the next call
    /// to [`read_new_lines`] will retry the rotation check.
    fn handle_rotation(&mut self) -> Result<()> {
        match File::open(&self.path) {
            Ok(new_file) => {
                let meta = new_file
                    .metadata()
                    .map_err(|e| LogdiveError::io_at(&self.path, e))?;
                self.file = new_file;
                self.offset = 0;
                self.inode = meta.ino();
                self.dev = meta.dev();
                self.leftover.clear();
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // New file not yet present — caller will retry.
                Ok(())
            }
            Err(e) => Err(LogdiveError::io_at(&self.path, e)),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    // Helper: append bytes to a NamedTempFile and flush.
    fn append(f: &mut NamedTempFile, data: &[u8]) {
        f.write_all(data).expect("write");
        f.flush().expect("flush");
    }

    // Helper: append bytes to a plain File and flush.
    fn append_file(f: &mut File, data: &[u8]) {
        f.write_all(data).expect("write");
        f.flush().expect("flush");
    }

    // -----------------------------------------------------------------------
    // Test 1
    // -----------------------------------------------------------------------
    /// Opening a file with existing content, the first `read_new_lines`
    /// returns no lines — we start at EOF, not BOF.
    #[test]
    fn open_at_eof_returns_no_initial_lines() {
        let mut f = NamedTempFile::new().unwrap();
        append(&mut f, b"existing line\n");

        let mut tailer = FileTailer::open(f.path()).unwrap();
        let lines = tailer.read_new_lines().unwrap();
        assert!(
            lines.is_empty(),
            "expected no lines on first read, got {lines:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Test 2
    // -----------------------------------------------------------------------
    /// After opening, appending a single complete line makes it available.
    #[test]
    fn single_append_returns_appended_lines() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        append(&mut f, b"foo\n");
        let lines = tailer.read_new_lines().unwrap();
        assert_eq!(lines, vec!["foo"]);
    }

    // -----------------------------------------------------------------------
    // Test 3
    // -----------------------------------------------------------------------
    /// Multiple append–read cycles preserve ordering and completeness.
    #[test]
    fn multiple_appends_across_calls() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        append(&mut f, b"alpha\n");
        let first = tailer.read_new_lines().unwrap();
        assert_eq!(first, vec!["alpha"]);

        append(&mut f, b"beta\ngamma\n");
        let second = tailer.read_new_lines().unwrap();
        assert_eq!(second, vec!["beta", "gamma"]);
    }

    // -----------------------------------------------------------------------
    // Test 4
    // -----------------------------------------------------------------------
    /// A second consecutive read with no new data returns an empty vec.
    #[test]
    fn read_after_no_new_data_returns_empty() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        append(&mut f, b"line\n");
        tailer.read_new_lines().unwrap(); // consume
        let second = tailer.read_new_lines().unwrap();
        assert!(second.is_empty(), "expected empty, got {second:?}");
    }

    // -----------------------------------------------------------------------
    // Test 5
    // -----------------------------------------------------------------------
    /// Opening an empty file and appending nothing returns no lines.
    #[test]
    fn empty_file_returns_no_lines() {
        let f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();
        let lines = tailer.read_new_lines().unwrap();
        assert!(lines.is_empty());
    }

    // -----------------------------------------------------------------------
    // Test 6
    // -----------------------------------------------------------------------
    /// An incomplete line is buffered until the newline arrives.
    #[test]
    fn partial_line_buffered_until_newline() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        // Write partial — no newline yet.
        append(&mut f, b"par");
        let first = tailer.read_new_lines().unwrap();
        assert!(
            first.is_empty(),
            "partial should be buffered, got {first:?}"
        );

        // Complete the line.
        append(&mut f, b"tial\n");
        let second = tailer.read_new_lines().unwrap();
        assert_eq!(second, vec!["partial"]);
    }

    // -----------------------------------------------------------------------
    // Test 7
    // -----------------------------------------------------------------------
    /// Multiple complete lines plus a trailing partial: complete lines are
    /// returned immediately; the partial is held back.
    #[test]
    fn multiple_lines_with_partial_at_end() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        append(&mut f, b"a\nb\nc");
        let lines = tailer.read_new_lines().unwrap();
        assert_eq!(lines, vec!["a", "b"], "got {lines:?}");
        // "c" is still buffered; no newline yet.
        assert!(!tailer.leftover.is_empty(), "leftover should hold 'c'");
    }

    // -----------------------------------------------------------------------
    // Test 8
    // -----------------------------------------------------------------------
    /// A line that exceeds the 8 KiB read buffer is reassembled correctly
    /// across multiple read() calls.
    #[test]
    fn very_long_line_buffered_correctly() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        // 20 KiB of 'x' followed by a newline — spans three 8 KiB buffers.
        let long_line: Vec<u8> = std::iter::repeat_n(b'x', 20 * 1024).collect();
        let mut data = long_line.clone();
        data.push(b'\n');
        append(&mut f, &data);

        let lines = tailer.read_new_lines().unwrap();
        assert_eq!(lines.len(), 1, "expected one line, got {}", lines.len());
        let expected: String = "x".repeat(20 * 1024);
        assert_eq!(lines[0], expected);
    }

    // -----------------------------------------------------------------------
    // Test 9
    // -----------------------------------------------------------------------
    /// Unicode content is preserved exactly.
    #[test]
    fn unicode_lines_preserved() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        append(&mut f, "héllo wörld 日本語\n".as_bytes());
        let lines = tailer.read_new_lines().unwrap();
        assert_eq!(lines, vec!["héllo wörld 日本語"]);
    }

    // -----------------------------------------------------------------------
    // Test 10
    // -----------------------------------------------------------------------
    /// CRLF line endings have the `\r` stripped; the returned string has no
    /// trailing carriage return.
    #[test]
    fn crlf_line_endings_stripped() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        append(&mut f, b"line1\r\nline2\r\n");
        let lines = tailer.read_new_lines().unwrap();
        assert_eq!(lines, vec!["line1", "line2"]);
    }

    // -----------------------------------------------------------------------
    // Test 11
    // -----------------------------------------------------------------------
    /// Blank lines (just `\n`) are returned as empty strings, not silently
    /// dropped.
    #[test]
    fn blank_lines_are_returned() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        append(&mut f, b"\n\n");
        let lines = tailer.read_new_lines().unwrap();
        assert_eq!(lines, vec!["", ""], "got {lines:?}");
    }

    // -----------------------------------------------------------------------
    // Test 12
    // -----------------------------------------------------------------------
    /// Truncation (same inode, size shrinks below offset) resets the offset
    /// and reads from the start of the truncated file.
    #[test]
    fn truncation_resets_offset() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        // Write and consume some data so the offset is non-zero.
        append(&mut f, b"old data\n");
        let first = tailer.read_new_lines().unwrap();
        assert_eq!(first, vec!["old data"]);

        // Truncate the file to zero.
        f.as_file().set_len(0).unwrap();
        f.as_file().seek(SeekFrom::Start(0)).unwrap();

        // Write fresh content to the now-empty file.
        append(&mut f, b"fresh\n");

        let second = tailer.read_new_lines().unwrap();
        assert_eq!(second, vec!["fresh"], "got {second:?}");
    }

    // -----------------------------------------------------------------------
    // Test 13
    // -----------------------------------------------------------------------
    /// Rotation via rename: the watched path is renamed away, a new file is
    /// created at the same path, and new content is appended. The tailer
    /// should re-open and return the new content.
    #[test]
    fn rotation_via_rename_reopens_file() {
        let dir = TempDir::new().unwrap();
        let watched = dir.path().join("app.log");

        // Create the initial file.
        std::fs::write(&watched, b"initial\n").unwrap();
        let mut tailer = FileTailer::open(&watched).unwrap();

        // Rotate: rename the current file away, create a new one.
        let rotated = dir.path().join("app.log.1");
        std::fs::rename(&watched, &rotated).unwrap();
        // Create new file at the watched path.
        let mut new_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&watched)
            .unwrap();
        append_file(&mut new_file, b"new\n");

        let lines = tailer.read_new_lines().unwrap();
        assert_eq!(lines, vec!["new"], "got {lines:?}");
    }

    // -----------------------------------------------------------------------
    // Test 14
    // -----------------------------------------------------------------------
    /// After a rotation, subsequent appends to the new file continue to
    /// arrive in correct order across multiple reads.
    #[test]
    fn rotation_then_more_appends() {
        let dir = TempDir::new().unwrap();
        let watched = dir.path().join("app.log");

        std::fs::write(&watched, b"before\n").unwrap();
        let mut tailer = FileTailer::open(&watched).unwrap();

        // Rotate.
        let rotated = dir.path().join("app.log.1");
        std::fs::rename(&watched, &rotated).unwrap();
        let mut new_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&watched)
            .unwrap();

        // First batch after rotation.
        append_file(&mut new_file, b"first\n");
        let batch1 = tailer.read_new_lines().unwrap();
        assert_eq!(batch1, vec!["first"], "batch1: {batch1:?}");

        // Second batch — more appends to the same new file.
        append_file(&mut new_file, b"second\nthird\n");
        let batch2 = tailer.read_new_lines().unwrap();
        assert_eq!(batch2, vec!["second", "third"], "batch2: {batch2:?}");
    }

    // -----------------------------------------------------------------------
    // Test 15
    // -----------------------------------------------------------------------
    /// `FileTailer::open` on a non-existent path returns `Err`.
    #[test]
    fn missing_file_errors_on_open() {
        let result = FileTailer::open("/nonexistent/path/that/does/not/exist.log");
        assert!(result.is_err(), "expected Err on missing file");
        assert!(
            matches!(result.unwrap_err(), LogdiveError::Io { .. }),
            "expected LogdiveError::Io"
        );
    }

    // -----------------------------------------------------------------------
    // Test 16
    // -----------------------------------------------------------------------
    /// If the watched file is deleted after `open`, `read_new_lines` returns
    /// `Ok(vec![])` — the NotFound branch in the metadata check. The caller
    /// can retry on the next poll cycle.
    #[test]
    fn file_deleted_after_open_returns_ok_empty() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_path_buf();

        let mut tailer = FileTailer::open(&path).unwrap();

        // Delete the file while the tailer holds it open.
        drop(f);
        assert!(!path.exists(), "file must be gone before read");

        let result = tailer.read_new_lines();
        assert!(result.is_ok(), "deleted file must not return Err");
        assert!(
            result.unwrap().is_empty(),
            "deleted file must return empty line list"
        );
    }

    // -----------------------------------------------------------------------
    // Test 17
    // -----------------------------------------------------------------------
    /// After several empty-read cycles, a burst of lines appended all at once
    /// must be returned in full on the first read that finds data.
    #[test]
    fn burst_of_lines_after_idle_reads_returns_all() {
        let mut f = NamedTempFile::new().unwrap();
        let mut tailer = FileTailer::open(f.path()).unwrap();

        // Multiple empty reads to confirm the idle state.
        for _ in 0..3 {
            assert!(tailer.read_new_lines().unwrap().is_empty());
        }

        // Burst: append 50 lines at once.
        let burst: String = (0..50).map(|i| format!("line-{i}\n")).collect();
        append(&mut f, burst.as_bytes());

        let lines = tailer.read_new_lines().unwrap();
        assert_eq!(
            lines.len(),
            50,
            "all 50 burst lines must arrive in one read"
        );
        assert_eq!(lines[0], "line-0");
        assert_eq!(lines[49], "line-49");
    }
}
