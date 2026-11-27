//! Safe Rust Plan 9 Runtime Namespace (`RtNamespace`) for `aod-rt` and `aodc`.
//!
//! Zero `unsafe` code (`#![deny(unsafe_code)]` inherited from crate root), zero floats,
//! 100% `std`-only. Routes all runtime file/capability operations (`aod::sys`) through
//! an explicit per-context Plan 9 `Namespace` (`empty` by default!) and provides the exact
//! mount specification projected into the Linux `unshare` + `pivot_root` container.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aod_core::ns::{
    BindMode, CleanPath, ContentHash, MountMode, MountPerm, Namespace, NsError, Provider,
};
use aod_core::ternary::T1;

use crate::ops::MAX_SYS_IO_BYTES;

/// Hard upper bound (64 KiB) on any single line or length-prefixed stream frame (`aod::sys`).
pub const MAX_NET_FRAME_BYTES: usize = 65_536;

const GUARDED_REPO_RELS: &[&str] = &[
    "tests/fixtures/v1_examples",
    "bindings/aod",
    "reference/aod-dec-dev",
    "SOURCE_BINDING.json",
];

#[derive(Clone, Debug)]
struct MemFile {
    bytes: Arc<Vec<u8>>,
    executable: bool,
}

/// Thread-safe in-memory synthetic filesystem (`ramfs`) bounded by `MAX_SYS_IO_BYTES` per file
/// and `max_files` total entries.
#[derive(Clone, Debug)]
pub struct MemFsProvider {
    files: Arc<Mutex<BTreeMap<String, MemFile>>>,
    max_files: usize,
}

impl Default for MemFsProvider {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl MemFsProvider {
    pub fn new(max_files: usize) -> Self {
        Self {
            files: Arc::new(Mutex::new(BTreeMap::new())),
            max_files: max_files.max(1),
        }
    }

    pub fn insert_ro_file(&self, subpath: &str, data: Vec<u8>) -> Result<(), NsError> {
        let clean = CleanPath::clean_subpath_str(subpath)?;
        let mut guard = self
            .files
            .lock()
            .map_err(|_| NsError::ProviderFault {
                path: clean.clone(),
                detail: "poisoned ramfs mutex".to_string(),
            })?;
        if !guard.contains_key(&clean) && guard.len() >= self.max_files {
            return Err(NsError::CapabilityDenied(
                "ramfs file count limit exceeded".to_string(),
            ));
        }
        guard.insert(
            clean,
            MemFile {
                bytes: Arc::new(data),
                executable: false,
            },
        );
        Ok(())
    }

    pub fn read_file(&self, subpath: &str) -> Result<Option<(Vec<u8>, bool)>, NsError> {
        let clean = CleanPath::clean_subpath_str(subpath)?;
        let guard = self
            .files
            .lock()
            .map_err(|_| NsError::ProviderFault {
                path: clean.clone(),
                detail: "poisoned ramfs mutex".to_string(),
            })?;
        Ok(guard
            .get(&clean)
            .map(|mf| ((*mf.bytes).clone(), mf.executable)))
    }

    fn write_file(&self, subpath: &str, data: &[u8], executable: bool) -> Result<(), NsError> {
        let clean = CleanPath::clean_subpath_str(subpath)?;
        if clean.is_empty() {
            return Err(NsError::ProviderFault {
                path: "/".to_string(),
                detail: "cannot write file bytes directly to mount root directory".to_string(),
            });
        }
        if data.len() > MAX_SYS_IO_BYTES {
            return Err(NsError::CapabilityDenied(format!(
                "file exceeds {MAX_SYS_IO_BYTES}-byte limit"
            )));
        }
        let mut guard = self
            .files
            .lock()
            .map_err(|_| NsError::ProviderFault {
                path: clean.clone(),
                detail: "poisoned ramfs mutex".to_string(),
            })?;
        if !guard.contains_key(&clean) && guard.len() >= self.max_files {
            return Err(NsError::CapabilityDenied(
                "ramfs file count limit exceeded".to_string(),
            ));
        }
        guard.insert(
            clean,
            MemFile {
                bytes: Arc::new(data.to_vec()),
                executable,
            },
        );
        Ok(())
    }
}

/// Explicitly mounted host directory chrooted to `active_root`, rejecting symlinks at every
/// path component and switching seamlessly to its in-container mount path after `pivot_root`.
#[derive(Clone, Debug)]
pub struct ScopedHostDirProvider {
    pub host_root: PathBuf,
    pub active_root: PathBuf,
}

impl ScopedHostDirProvider {
    pub fn new(host_dir: &Path, perm: MountPerm) -> Result<(ContentHash, Self), String> {
        let meta = std::fs::symlink_metadata(host_dir)
            .map_err(|e| format!("failed to stat mount source {}: {e}", host_dir.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "refusing to mount symlink directory {}",
                host_dir.display()
            ));
        }
        if !meta.is_dir() {
            return Err(format!(
                "mount source {} must be a directory",
                host_dir.display()
            ));
        }
        let canon = std::fs::canonicalize(host_dir)
            .map_err(|e| format!("failed to canonicalize {}: {e}", host_dir.display()))?;

        if perm == MountPerm::ReadWrite {
            reject_guarded_repo_overlap(&canon)?;
        }

        let perm_tag = match perm {
            MountPerm::ReadOnly => b"ro".as_slice(),
            MountPerm::ReadWrite => b"rw".as_slice(),
        };
        let id = ContentHash::digest_domain(
            b"aod.rt.host_dir.v1",
            &[canon.to_string_lossy().as_bytes(), perm_tag],
        );
        Ok((
            id,
            Self {
                host_root: canon.clone(),
                active_root: canon,
            },
        ))
    }

    fn resolve_checked_path(&self, clean_subpath: &str, create_parents: bool) -> Result<PathBuf, NsError> {
        if clean_subpath.is_empty() {
            return Err(NsError::ProviderFault {
                path: self.active_root.display().to_string(),
                detail: "expected file subpath beneath mounted directory, got mount root".to_string(),
            });
        }
        let parts: Vec<&str> = clean_subpath.split('/').collect();
        let mut cur = self.active_root.clone();
        for (idx, seg) in parts.iter().enumerate() {
            if seg.is_empty() || *seg == "." || *seg == ".." {
                return Err(NsError::SubpathEscape(clean_subpath.to_string()));
            }
            cur.push(seg);
            let is_last = idx + 1 == parts.len();
            match std::fs::symlink_metadata(&cur) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        return Err(NsError::CapabilityDenied(format!(
                            "symlink traversal forbidden inside mounted directory: {clean_subpath:?}"
                        )));
                    }
                    if !is_last && !meta.is_dir() {
                        return Err(NsError::ProviderFault {
                            path: clean_subpath.to_string(),
                            detail: "intermediate path component is not a directory".to_string(),
                        });
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if !is_last && create_parents {
                        std::fs::create_dir(&cur).map_err(|ce| NsError::ProviderFault {
                            path: clean_subpath.to_string(),
                            detail: format!("failed creating parent directory: {ce}"),
                        })?;
                    }
                }
                Err(e) => {
                    return Err(NsError::ProviderFault {
                        path: clean_subpath.to_string(),
                        detail: format!("stat error: {e}"),
                    });
                }
            }
        }
        Ok(cur)
    }

    fn read_bytes(&self, clean_subpath: &str) -> Result<Option<Vec<u8>>, NsError> {
        let full = self.resolve_checked_path(clean_subpath, false)?;
        let meta = match std::fs::symlink_metadata(&full) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(NsError::ProviderFault {
                    path: clean_subpath.to_string(),
                    detail: format!("failed to stat: {e}"),
                });
            }
        };
        if meta.file_type().is_symlink() {
            return Err(NsError::CapabilityDenied(format!(
                "refusing to read symlink {clean_subpath:?}"
            )));
        }
        if meta.is_dir() {
            return Err(NsError::ProviderFault {
                path: clean_subpath.to_string(),
                detail: "path refers to a directory, not a regular file".to_string(),
            });
        }
        if meta.len() > (MAX_SYS_IO_BYTES as u64) {
            return Err(NsError::CapabilityDenied(format!(
                "file {clean_subpath:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            )));
        }
        let data = std::fs::read(&full).map_err(|e| NsError::ProviderFault {
            path: clean_subpath.to_string(),
            detail: format!("failed to read: {e}"),
        })?;
        if data.len() > MAX_SYS_IO_BYTES {
            return Err(NsError::CapabilityDenied(format!(
                "file {clean_subpath:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            )));
        }
        Ok(Some(data))
    }

    fn write_bytes(&self, clean_subpath: &str, data: &[u8], executable: bool) -> Result<(), NsError> {
        if data.len() > MAX_SYS_IO_BYTES {
            return Err(NsError::CapabilityDenied(format!(
                "payload exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            )));
        }
        let full = self.resolve_checked_path(clean_subpath, true)?;
        std::fs::write(&full, data).map_err(|e| NsError::ProviderFault {
            path: clean_subpath.to_string(),
            detail: format!("failed to write: {e}"),
        })?;
        #[cfg(unix)]
        if executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&full, std::fs::Permissions::from_mode(0o755)).map_err(
                |e| NsError::ProviderFault {
                    path: clean_subpath.to_string(),
                    detail: format!("failed to chmod 0755: {e}"),
                },
            )?;
        }
        Ok(())
    }
}

fn reject_guarded_repo_overlap(canon_dir: &Path) -> Result<(), String> {
    let mut cur: Option<&Path> = Some(canon_dir);
    while let Some(dir) = cur {
        if dir.join("SOURCE_BINDING.json").exists() || dir.join("bindings/aod").exists() {
            for rel in GUARDED_REPO_RELS {
                let guarded = dir.join(rel);
                if let Ok(cg) = std::fs::canonicalize(&guarded) {
                    if canon_dir == cg || canon_dir.starts_with(&cg) || cg.starts_with(canon_dir) {
                        return Err(format!(
                            "refusing read-write mount {} overlapping frozen provenance path {rel}",
                            canon_dir.display()
                        ));
                    }
                }
            }
        }
        cur = dir.parent();
    }
    Ok(())
}

/// Ternary stream read classification (`+1t` admitted, `0t` EOF, `-1t` rejected/oversized).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamReadOutcome {
    Admitted(Vec<u8>),
    Eof,
    Rejected(Vec<u8>),
}

impl StreamReadOutcome {
    pub fn into_trit_tuple(self) -> (T1, Vec<u8>) {
        match self {
            StreamReadOutcome::Admitted(bytes) => (T1::POS, bytes),
            StreamReadOutcome::Eof => (T1::ZERO, Vec::new()),
            StreamReadOutcome::Rejected(reason) => (T1::NEG, reason),
        }
    }
}

fn is_peer_disconnect_err(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::BrokenPipe
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::NotConnected
            | ErrorKind::UnexpectedEof
    )
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<StreamReadOutcome> {
    let mut out = Vec::new();
    let mut overflowed = false;
    loop {
        let avail = reader.fill_buf()?;
        if avail.is_empty() {
            if out.is_empty() && !overflowed {
                return Ok(StreamReadOutcome::Eof);
            }
            break;
        }
        if let Some(nl_pos) = avail.iter().position(|&b| b == b'\n') {
            if !overflowed {
                let remaining = max_bytes.saturating_sub(out.len());
                if nl_pos <= remaining {
                    out.extend_from_slice(&avail[..nl_pos]);
                } else {
                    overflowed = true;
                    out.clear();
                }
            }
            reader.consume(nl_pos + 1);
            break;
        } else {
            let chunk_len = avail.len();
            if !overflowed {
                if out.len() + chunk_len <= max_bytes {
                    out.extend_from_slice(avail);
                } else {
                    overflowed = true;
                    out.clear();
                }
            }
            reader.consume(chunk_len);
        }
    }
    if overflowed {
        return Ok(StreamReadOutcome::Rejected(
            b"line exceeds max_bytes".to_vec(),
        ));
    }
    if out.last() == Some(&b'\r') {
        out.pop();
    }
    Ok(StreamReadOutcome::Admitted(out))
}

fn read_bounded_frame<R: Read>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<StreamReadOutcome> {
    let mut hdr = [0u8; 4];
    let mut first_read = 0usize;
    while first_read == 0 {
        match reader.read(&mut hdr[0..1]) {
            Ok(0) => return Ok(StreamReadOutcome::Eof),
            Ok(n) => first_read = n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    if let Err(e) = reader.read_exact(&mut hdr[first_read..4]) {
        if e.kind() == ErrorKind::UnexpectedEof {
            return Ok(StreamReadOutcome::Rejected(
                b"truncated frame header".to_vec(),
            ));
        }
        return Err(e);
    }
    let frame_len = u32::from_le_bytes(hdr) as usize;
    if frame_len > max_bytes {
        return Ok(StreamReadOutcome::Rejected(
            b"frame exceeds max_bytes".to_vec(),
        ));
    }
    let mut payload = vec![0u8; frame_len];
    if frame_len > 0 {
        if let Err(e) = reader.read_exact(&mut payload) {
            if e.kind() == ErrorKind::UnexpectedEof {
                return Ok(StreamReadOutcome::Rejected(
                    b"truncated frame payload".to_vec(),
                ));
            }
            return Err(e);
        }
    }
    Ok(StreamReadOutcome::Admitted(payload))
}

fn write_length_prefixed_frame<W: Write>(writer: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let hdr = (payload.len() as u32).to_le_bytes();
    writer.write_all(&hdr)?;
    if !payload.is_empty() {
        writer.write_all(payload)?;
    }
    writer.flush()
}

#[derive(Debug, Default)]
struct MemStreamInner {
    rx: VecDeque<u8>,
    tx: Vec<u8>,
    accepted_once: bool,
    eof_closed: bool,
}

/// Thread-safe in-memory duplex stream (`MemStreamProvider`) for deterministic stream/frame tests.
#[derive(Clone, Debug, Default)]
pub struct MemStreamProvider {
    inner: Arc<Mutex<MemStreamInner>>,
}

impl MemStreamProvider {
    pub fn from_input(input: Vec<u8>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(MemStreamInner {
                rx: VecDeque::from(input),
                tx: Vec::new(),
                accepted_once: false,
                eof_closed: false,
            })),
        }
    }

    pub fn push_input(&self, data: &[u8]) -> Result<(), String> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| "poisoned mem stream mutex".to_string())?;
        g.rx.extend(data.iter().copied());
        Ok(())
    }

    pub fn push_frame(&self, payload: &[u8]) -> Result<(), String> {
        if payload.len() > MAX_NET_FRAME_BYTES {
            return Err(format!(
                "frame exceeds {MAX_NET_FRAME_BYTES}-byte stream limit"
            ));
        }
        let mut g = self
            .inner
            .lock()
            .map_err(|_| "poisoned mem stream mutex".to_string())?;
        g.rx.extend((payload.len() as u32).to_le_bytes());
        g.rx.extend(payload.iter().copied());
        Ok(())
    }

    pub fn output_bytes(&self) -> Result<Vec<u8>, String> {
        let g = self
            .inner
            .lock()
            .map_err(|_| "poisoned mem stream mutex".to_string())?;
        Ok(g.tx.clone())
    }

    pub fn take_output(&self) -> Result<Vec<u8>, String> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| "poisoned mem stream mutex".to_string())?;
        Ok(std::mem::take(&mut g.tx))
    }

    fn accept_next(&self) -> Result<bool, NsError> {
        let mut g = self.inner.lock().map_err(|_| NsError::ProviderFault {
            path: "mem".to_string(),
            detail: "poisoned mem stream mutex".to_string(),
        })?;
        if !g.accepted_once && (!g.rx.is_empty() || !g.eof_closed) {
            g.accepted_once = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn read_line(&self, max_bytes: usize) -> Result<StreamReadOutcome, NsError> {
        let mut g = self.inner.lock().map_err(|_| NsError::ProviderFault {
            path: "mem".to_string(),
            detail: "poisoned mem stream mutex".to_string(),
        })?;
        if g.rx.is_empty() {
            g.eof_closed = true;
            return Ok(StreamReadOutcome::Eof);
        }
        let mut out = Vec::new();
        let mut overflowed = false;
        while let Some(b) = g.rx.pop_front() {
            if b == b'\n' {
                break;
            }
            if !overflowed {
                if out.len() < max_bytes {
                    out.push(b);
                } else {
                    overflowed = true;
                    out.clear();
                }
            }
        }
        if overflowed {
            return Ok(StreamReadOutcome::Rejected(
                b"line exceeds max_bytes".to_vec(),
            ));
        }
        if out.last() == Some(&b'\r') {
            out.pop();
        }
        Ok(StreamReadOutcome::Admitted(out))
    }

    fn read_frame(&self, max_bytes: usize) -> Result<StreamReadOutcome, NsError> {
        let mut g = self.inner.lock().map_err(|_| NsError::ProviderFault {
            path: "mem".to_string(),
            detail: "poisoned mem stream mutex".to_string(),
        })?;
        if g.rx.is_empty() {
            g.eof_closed = true;
            return Ok(StreamReadOutcome::Eof);
        }
        if g.rx.len() < 4 {
            g.rx.clear();
            g.eof_closed = true;
            return Ok(StreamReadOutcome::Rejected(
                b"truncated frame header".to_vec(),
            ));
        }
        let mut hdr = [0u8; 4];
        for slot in &mut hdr {
            *slot = g.rx.pop_front().unwrap_or(0);
        }
        let frame_len = u32::from_le_bytes(hdr) as usize;
        if frame_len > max_bytes {
            g.rx.clear();
            g.eof_closed = true;
            return Ok(StreamReadOutcome::Rejected(
                b"frame exceeds max_bytes".to_vec(),
            ));
        }
        if g.rx.len() < frame_len {
            g.rx.clear();
            g.eof_closed = true;
            return Ok(StreamReadOutcome::Rejected(
                b"truncated frame payload".to_vec(),
            ));
        }
        let mut payload = Vec::with_capacity(frame_len);
        for _ in 0..frame_len {
            payload.push(g.rx.pop_front().unwrap_or(0));
        }
        Ok(StreamReadOutcome::Admitted(payload))
    }

    fn write_raw(&self, data: &[u8]) -> Result<i64, NsError> {
        let mut g = self.inner.lock().map_err(|_| NsError::ProviderFault {
            path: "mem".to_string(),
            detail: "poisoned mem stream mutex".to_string(),
        })?;
        g.tx.extend_from_slice(data);
        Ok(data.len() as i64)
    }

    fn write_framed(&self, payload: &[u8]) -> Result<i64, NsError> {
        let mut g = self.inner.lock().map_err(|_| NsError::ProviderFault {
            path: "mem".to_string(),
            detail: "poisoned mem stream mutex".to_string(),
        })?;
        g.tx.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        g.tx.extend_from_slice(payload);
        Ok(payload.len() as i64)
    }
}

#[derive(Debug)]
enum StreamEndpoint {
    Stdio {
        accepted_once: bool,
        eof_seen: bool,
    },
    TcpClient {
        local_addr: Vec<u8>,
        stream: BufReader<TcpStream>,
        accepted_once: bool,
        closed: bool,
    },
    TcpServer {
        local_addr: Vec<u8>,
        listener: TcpListener,
        active: Option<BufReader<TcpStream>>,
    },
    #[cfg(unix)]
    UnixClient {
        local_addr: Vec<u8>,
        stream: BufReader<UnixStream>,
        accepted_once: bool,
        closed: bool,
    },
    #[cfg(unix)]
    UnixServer {
        local_addr: Vec<u8>,
        listener: UnixListener,
        active: Option<BufReader<UnixStream>>,
    },
    Mem(MemStreamProvider),
}

/// Thread-safe Plan 9 capability stream provider (`Stdio`, `TCP`, `Unix`, or `Mem`).
#[derive(Clone, Debug)]
pub struct StreamProvider {
    endpoint: Arc<Mutex<StreamEndpoint>>,
}

impl StreamProvider {
    pub fn new_stdio() -> (ContentHash, Self) {
        let id = ContentHash::digest_domain(b"aod.rt.stream.v1", &[b"stdio"]);
        (
            id,
            Self {
                endpoint: Arc::new(Mutex::new(StreamEndpoint::Stdio {
                    accepted_once: false,
                    eof_seen: false,
                })),
            },
        )
    }

    pub fn connect_tcp(addr: &str, perm: MountPerm) -> Result<(ContentHash, Self), String> {
        if addr.is_empty() || addr.as_bytes().contains(&0) {
            return Err("invalid empty or NUL-containing TCP address".to_string());
        }
        let stream = TcpStream::connect(addr)
            .map_err(|e| format!("failed to connect TCP stream {addr:?}: {e}"))?;
        let _ = stream.set_nodelay(true);
        let local_addr = stream
            .local_addr()
            .map(|a| a.to_string().into_bytes())
            .unwrap_or_else(|_| addr.as_bytes().to_vec());
        let perm_tag = match perm {
            MountPerm::ReadOnly => b"ro".as_slice(),
            MountPerm::ReadWrite => b"rw".as_slice(),
        };
        let id =
            ContentHash::digest_domain(b"aod.rt.stream.v1", &[b"tcp", addr.as_bytes(), perm_tag]);
        Ok((
            id,
            Self {
                endpoint: Arc::new(Mutex::new(StreamEndpoint::TcpClient {
                    local_addr,
                    stream: BufReader::new(stream),
                    accepted_once: false,
                    closed: false,
                })),
            },
        ))
    }

    pub fn bind_tcp(addr: &str, perm: MountPerm) -> Result<(ContentHash, Self), String> {
        if addr.is_empty() || addr.as_bytes().contains(&0) {
            return Err("invalid empty or NUL-containing TCP bind address".to_string());
        }
        let listener = TcpListener::bind(addr)
            .map_err(|e| format!("failed to bind TCP listener {addr:?}: {e}"))?;
        let local_addr = listener
            .local_addr()
            .map(|a| a.to_string().into_bytes())
            .unwrap_or_else(|_| addr.as_bytes().to_vec());
        let perm_tag = match perm {
            MountPerm::ReadOnly => b"ro".as_slice(),
            MountPerm::ReadWrite => b"rw".as_slice(),
        };
        let id = ContentHash::digest_domain(
            b"aod.rt.stream.v1",
            &[b"tcp-listen", &local_addr, perm_tag],
        );
        Ok((
            id,
            Self {
                endpoint: Arc::new(Mutex::new(StreamEndpoint::TcpServer {
                    local_addr,
                    listener,
                    active: None,
                })),
            },
        ))
    }

    #[cfg(unix)]
    pub fn connect_unix(sock_path: &Path, perm: MountPerm) -> Result<(ContentHash, Self), String> {
        let raw = sock_path.to_string_lossy();
        if raw.is_empty() || raw.as_bytes().contains(&0) {
            return Err("invalid empty or NUL-containing Unix socket path".to_string());
        }
        if let Ok(meta) = std::fs::symlink_metadata(sock_path) {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "refusing to connect symlink Unix socket {}",
                    sock_path.display()
                ));
            }
        }
        let stream = UnixStream::connect(sock_path).map_err(|e| {
            format!(
                "failed to connect Unix stream {:?}: {e}",
                sock_path.display()
            )
        })?;
        let local_addr = raw.as_bytes().to_vec();
        let perm_tag = match perm {
            MountPerm::ReadOnly => b"ro".as_slice(),
            MountPerm::ReadWrite => b"rw".as_slice(),
        };
        let id =
            ContentHash::digest_domain(b"aod.rt.stream.v1", &[b"unix", &local_addr, perm_tag]);
        Ok((
            id,
            Self {
                endpoint: Arc::new(Mutex::new(StreamEndpoint::UnixClient {
                    local_addr,
                    stream: BufReader::new(stream),
                    accepted_once: false,
                    closed: false,
                })),
            },
        ))
    }

    #[cfg(unix)]
    pub fn bind_unix(sock_path: &Path, perm: MountPerm) -> Result<(ContentHash, Self), String> {
        let raw = sock_path.to_string_lossy();
        if raw.is_empty() || raw.as_bytes().contains(&0) {
            return Err("invalid empty or NUL-containing Unix socket bind path".to_string());
        }
        reject_guarded_repo_file_target(sock_path)?;
        if let Some(parent) = sock_path.parent() {
            if !parent.as_os_str().is_empty() {
                let pmeta = std::fs::symlink_metadata(parent).map_err(|e| {
                    format!(
                        "failed to stat Unix socket parent directory {}: {e}",
                        parent.display()
                    )
                })?;
                if pmeta.file_type().is_symlink() {
                    return Err(format!(
                        "refusing to bind Unix socket inside symlink directory {}",
                        parent.display()
                    ));
                }
            }
        }
        if let Ok(meta) = std::fs::symlink_metadata(sock_path) {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "refusing to overwrite symlink {} for Unix listener",
                    sock_path.display()
                ));
            }
        }
        let listener = UnixListener::bind(sock_path).map_err(|e| {
            format!(
                "failed to bind Unix listener {:?}: {e}",
                sock_path.display()
            )
        })?;
        let local_addr = raw.as_bytes().to_vec();
        let perm_tag = match perm {
            MountPerm::ReadOnly => b"ro".as_slice(),
            MountPerm::ReadWrite => b"rw".as_slice(),
        };
        let id = ContentHash::digest_domain(
            b"aod.rt.stream.v1",
            &[b"unix-listen", &local_addr, perm_tag],
        );
        Ok((
            id,
            Self {
                endpoint: Arc::new(Mutex::new(StreamEndpoint::UnixServer {
                    local_addr,
                    listener,
                    active: None,
                })),
            },
        ))
    }

    pub fn from_mem(mem: MemStreamProvider, target: &CleanPath) -> (ContentHash, Self) {
        let id = ContentHash::digest_domain(b"aod.rt.stream.v1", &[b"mem", target.as_bytes()]);
        (
            id,
            Self {
                endpoint: Arc::new(Mutex::new(StreamEndpoint::Mem(mem))),
            },
        )
    }

    #[cfg(unix)]
    fn raw_fds(&self) -> Vec<i32> {
        let Ok(g) = self.endpoint.lock() else {
            return Vec::new();
        };
        match &*g {
            StreamEndpoint::Stdio { .. } | StreamEndpoint::Mem(_) => Vec::new(),
            StreamEndpoint::TcpClient { stream, closed, .. } => {
                if *closed {
                    Vec::new()
                } else {
                    vec![stream.get_ref().as_raw_fd()]
                }
            }
            StreamEndpoint::TcpServer {
                listener, active, ..
            } => {
                let mut fds = vec![listener.as_raw_fd()];
                if let Some(s) = active {
                    fds.push(s.get_ref().as_raw_fd());
                }
                fds
            }
            StreamEndpoint::UnixClient { stream, closed, .. } => {
                if *closed {
                    Vec::new()
                } else {
                    vec![stream.get_ref().as_raw_fd()]
                }
            }
            StreamEndpoint::UnixServer {
                listener, active, ..
            } => {
                let mut fds = vec![listener.as_raw_fd()];
                if let Some(s) = active {
                    fds.push(s.get_ref().as_raw_fd());
                }
                fds
            }
        }
    }

    fn is_listener(&self) -> bool {
        let Ok(g) = self.endpoint.lock() else {
            return false;
        };
        #[cfg(unix)]
        {
            matches!(
                &*g,
                StreamEndpoint::TcpServer { .. } | StreamEndpoint::UnixServer { .. }
            )
        }
        #[cfg(not(unix))]
        {
            matches!(&*g, StreamEndpoint::TcpServer { .. })
        }
    }

    fn local_addr_bytes(&self) -> Result<Vec<u8>, NsError> {
        let g = self.endpoint.lock().map_err(|_| NsError::ProviderFault {
            path: "stream".to_string(),
            detail: "poisoned stream mutex".to_string(),
        })?;
        match &*g {
            StreamEndpoint::Stdio { .. } => Ok(b"stdio".to_vec()),
            StreamEndpoint::Mem(_) => Ok(b"mem".to_vec()),
            StreamEndpoint::TcpClient { local_addr, .. }
            | StreamEndpoint::TcpServer { local_addr, .. } => Ok(local_addr.clone()),
            #[cfg(unix)]
            StreamEndpoint::UnixClient { local_addr, .. }
            | StreamEndpoint::UnixServer { local_addr, .. } => Ok(local_addr.clone()),
        }
    }

    fn accept_next(&self) -> Result<bool, NsError> {
        let mut g = self.endpoint.lock().map_err(|_| NsError::ProviderFault {
            path: "stream".to_string(),
            detail: "poisoned stream mutex".to_string(),
        })?;
        match &mut *g {
            StreamEndpoint::Stdio {
                accepted_once,
                eof_seen,
            } => {
                if !*accepted_once && !*eof_seen {
                    *accepted_once = true;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            StreamEndpoint::TcpClient {
                accepted_once,
                closed,
                ..
            } => {
                if !*accepted_once && !*closed {
                    *accepted_once = true;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            StreamEndpoint::TcpServer {
                listener, active, ..
            } => {
                let _ = active.take();
                let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                    path: "tcp-listen".to_string(),
                    detail: format!("accept failed: {e}"),
                })?;
                let _ = stream.set_nodelay(true);
                *active = Some(BufReader::new(stream));
                Ok(true)
            }
            #[cfg(unix)]
            StreamEndpoint::UnixClient {
                accepted_once,
                closed,
                ..
            } => {
                if !*accepted_once && !*closed {
                    *accepted_once = true;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            #[cfg(unix)]
            StreamEndpoint::UnixServer {
                listener, active, ..
            } => {
                let _ = active.take();
                let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                    path: "unix-listen".to_string(),
                    detail: format!("accept failed: {e}"),
                })?;
                *active = Some(BufReader::new(stream));
                Ok(true)
            }
            StreamEndpoint::Mem(m) => m.accept_next(),
        }
    }

    fn read_line(&self, max_bytes: usize) -> Result<StreamReadOutcome, NsError> {
        let mut g = self.endpoint.lock().map_err(|_| NsError::ProviderFault {
            path: "stream".to_string(),
            detail: "poisoned stream mutex".to_string(),
        })?;
        match &mut *g {
            StreamEndpoint::Stdio { eof_seen, .. } => {
                if *eof_seen {
                    return Ok(StreamReadOutcome::Eof);
                }
                let stdin = std::io::stdin();
                let mut lock = stdin.lock();
                let out =
                    read_bounded_line(&mut lock, max_bytes).map_err(|e| NsError::ProviderFault {
                        path: "stdio".to_string(),
                        detail: format!("stdin read error: {e}"),
                    })?;
                if out == StreamReadOutcome::Eof {
                    *eof_seen = true;
                }
                Ok(out)
            }
            StreamEndpoint::TcpClient { stream, closed, .. } => {
                if *closed {
                    return Ok(StreamReadOutcome::Eof);
                }
                match read_bounded_line(stream, max_bytes) {
                    Ok(out) => {
                        if out == StreamReadOutcome::Eof {
                            *closed = true;
                        }
                        Ok(out)
                    }
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *closed = true;
                        Ok(StreamReadOutcome::Eof)
                    }
                    Err(e) => Err(NsError::ProviderFault {
                        path: "tcp".to_string(),
                        detail: format!("tcp read error: {e}"),
                    }),
                }
            }
            StreamEndpoint::TcpServer {
                listener, active, ..
            } => {
                if active.is_none() {
                    let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                        path: "tcp-listen".to_string(),
                        detail: format!("accept failed: {e}"),
                    })?;
                    let _ = stream.set_nodelay(true);
                    *active = Some(BufReader::new(stream));
                }
                let reader = active.as_mut().expect("active tcp client");
                match read_bounded_line(reader, max_bytes) {
                    Ok(out) => {
                        if out == StreamReadOutcome::Eof {
                            *active = None;
                        }
                        Ok(out)
                    }
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *active = None;
                        Ok(StreamReadOutcome::Eof)
                    }
                    Err(e) => {
                        *active = None;
                        Err(NsError::ProviderFault {
                            path: "tcp-listen".to_string(),
                            detail: format!("tcp read error: {e}"),
                        })
                    }
                }
            }
            #[cfg(unix)]
            StreamEndpoint::UnixClient { stream, closed, .. } => {
                if *closed {
                    return Ok(StreamReadOutcome::Eof);
                }
                match read_bounded_line(stream, max_bytes) {
                    Ok(out) => {
                        if out == StreamReadOutcome::Eof {
                            *closed = true;
                        }
                        Ok(out)
                    }
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *closed = true;
                        Ok(StreamReadOutcome::Eof)
                    }
                    Err(e) => Err(NsError::ProviderFault {
                        path: "unix".to_string(),
                        detail: format!("unix read error: {e}"),
                    }),
                }
            }
            #[cfg(unix)]
            StreamEndpoint::UnixServer {
                listener, active, ..
            } => {
                if active.is_none() {
                    let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                        path: "unix-listen".to_string(),
                        detail: format!("accept failed: {e}"),
                    })?;
                    *active = Some(BufReader::new(stream));
                }
                let reader = active.as_mut().expect("active unix client");
                match read_bounded_line(reader, max_bytes) {
                    Ok(out) => {
                        if out == StreamReadOutcome::Eof {
                            *active = None;
                        }
                        Ok(out)
                    }
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *active = None;
                        Ok(StreamReadOutcome::Eof)
                    }
                    Err(e) => {
                        *active = None;
                        Err(NsError::ProviderFault {
                            path: "unix-listen".to_string(),
                            detail: format!("unix read error: {e}"),
                        })
                    }
                }
            }
            StreamEndpoint::Mem(m) => m.read_line(max_bytes),
        }
    }

    fn read_frame(&self, max_bytes: usize) -> Result<StreamReadOutcome, NsError> {
        let mut g = self.endpoint.lock().map_err(|_| NsError::ProviderFault {
            path: "stream".to_string(),
            detail: "poisoned stream mutex".to_string(),
        })?;
        match &mut *g {
            StreamEndpoint::Stdio { eof_seen, .. } => {
                if *eof_seen {
                    return Ok(StreamReadOutcome::Eof);
                }
                let stdin = std::io::stdin();
                let mut lock = stdin.lock();
                let out =
                    read_bounded_frame(&mut lock, max_bytes).map_err(|e| NsError::ProviderFault {
                        path: "stdio".to_string(),
                        detail: format!("stdin frame read error: {e}"),
                    })?;
                if !matches!(out, StreamReadOutcome::Admitted(_)) {
                    *eof_seen = true;
                }
                Ok(out)
            }
            StreamEndpoint::TcpClient { stream, closed, .. } => {
                if *closed {
                    return Ok(StreamReadOutcome::Eof);
                }
                match read_bounded_frame(stream, max_bytes) {
                    Ok(out) => {
                        if !matches!(out, StreamReadOutcome::Admitted(_)) {
                            *closed = true;
                        }
                        Ok(out)
                    }
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *closed = true;
                        Ok(StreamReadOutcome::Eof)
                    }
                    Err(e) => Err(NsError::ProviderFault {
                        path: "tcp".to_string(),
                        detail: format!("tcp frame read error: {e}"),
                    }),
                }
            }
            StreamEndpoint::TcpServer {
                listener, active, ..
            } => {
                if active.is_none() {
                    let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                        path: "tcp-listen".to_string(),
                        detail: format!("accept failed: {e}"),
                    })?;
                    let _ = stream.set_nodelay(true);
                    *active = Some(BufReader::new(stream));
                }
                let reader = active.as_mut().expect("active tcp client");
                match read_bounded_frame(reader, max_bytes) {
                    Ok(out) => {
                        if !matches!(out, StreamReadOutcome::Admitted(_)) {
                            *active = None;
                        }
                        Ok(out)
                    }
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *active = None;
                        Ok(StreamReadOutcome::Eof)
                    }
                    Err(e) => {
                        *active = None;
                        Err(NsError::ProviderFault {
                            path: "tcp-listen".to_string(),
                            detail: format!("tcp frame read error: {e}"),
                        })
                    }
                }
            }
            #[cfg(unix)]
            StreamEndpoint::UnixClient { stream, closed, .. } => {
                if *closed {
                    return Ok(StreamReadOutcome::Eof);
                }
                match read_bounded_frame(stream, max_bytes) {
                    Ok(out) => {
                        if !matches!(out, StreamReadOutcome::Admitted(_)) {
                            *closed = true;
                        }
                        Ok(out)
                    }
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *closed = true;
                        Ok(StreamReadOutcome::Eof)
                    }
                    Err(e) => Err(NsError::ProviderFault {
                        path: "unix".to_string(),
                        detail: format!("unix frame read error: {e}"),
                    }),
                }
            }
            #[cfg(unix)]
            StreamEndpoint::UnixServer {
                listener, active, ..
            } => {
                if active.is_none() {
                    let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                        path: "unix-listen".to_string(),
                        detail: format!("accept failed: {e}"),
                    })?;
                    *active = Some(BufReader::new(stream));
                }
                let reader = active.as_mut().expect("active unix client");
                match read_bounded_frame(reader, max_bytes) {
                    Ok(out) => {
                        if !matches!(out, StreamReadOutcome::Admitted(_)) {
                            *active = None;
                        }
                        Ok(out)
                    }
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *active = None;
                        Ok(StreamReadOutcome::Eof)
                    }
                    Err(e) => {
                        *active = None;
                        Err(NsError::ProviderFault {
                            path: "unix-listen".to_string(),
                            detail: format!("unix frame read error: {e}"),
                        })
                    }
                }
            }
            StreamEndpoint::Mem(m) => m.read_frame(max_bytes),
        }
    }

    fn write_raw(&self, data: &[u8]) -> Result<i64, NsError> {
        let mut g = self.endpoint.lock().map_err(|_| NsError::ProviderFault {
            path: "stream".to_string(),
            detail: "poisoned stream mutex".to_string(),
        })?;
        match &mut *g {
            StreamEndpoint::Stdio { .. } => {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                match lock.write_all(data).and_then(|()| lock.flush()) {
                    Ok(()) => Ok(data.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => Ok(-1),
                    Err(e) => Err(NsError::ProviderFault {
                        path: "stdio".to_string(),
                        detail: format!("stdout write error: {e}"),
                    }),
                }
            }
            StreamEndpoint::TcpClient { stream, closed, .. } => {
                if *closed {
                    return Ok(-1);
                }
                let w = stream.get_mut();
                match w.write_all(data).and_then(|()| w.flush()) {
                    Ok(()) => Ok(data.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *closed = true;
                        Ok(-1)
                    }
                    Err(e) => Err(NsError::ProviderFault {
                        path: "tcp".to_string(),
                        detail: format!("tcp write error: {e}"),
                    }),
                }
            }
            StreamEndpoint::TcpServer {
                listener, active, ..
            } => {
                if active.is_none() {
                    let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                        path: "tcp-listen".to_string(),
                        detail: format!("accept failed: {e}"),
                    })?;
                    let _ = stream.set_nodelay(true);
                    *active = Some(BufReader::new(stream));
                }
                let w = active.as_mut().expect("active tcp client").get_mut();
                match w.write_all(data).and_then(|()| w.flush()) {
                    Ok(()) => Ok(data.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *active = None;
                        Ok(-1)
                    }
                    Err(e) => {
                        *active = None;
                        Err(NsError::ProviderFault {
                            path: "tcp-listen".to_string(),
                            detail: format!("tcp write error: {e}"),
                        })
                    }
                }
            }
            #[cfg(unix)]
            StreamEndpoint::UnixClient { stream, closed, .. } => {
                if *closed {
                    return Ok(-1);
                }
                let w = stream.get_mut();
                match w.write_all(data).and_then(|()| w.flush()) {
                    Ok(()) => Ok(data.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *closed = true;
                        Ok(-1)
                    }
                    Err(e) => Err(NsError::ProviderFault {
                        path: "unix".to_string(),
                        detail: format!("unix write error: {e}"),
                    }),
                }
            }
            #[cfg(unix)]
            StreamEndpoint::UnixServer {
                listener, active, ..
            } => {
                if active.is_none() {
                    let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                        path: "unix-listen".to_string(),
                        detail: format!("accept failed: {e}"),
                    })?;
                    *active = Some(BufReader::new(stream));
                }
                let w = active.as_mut().expect("active unix client").get_mut();
                match w.write_all(data).and_then(|()| w.flush()) {
                    Ok(()) => Ok(data.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *active = None;
                        Ok(-1)
                    }
                    Err(e) => {
                        *active = None;
                        Err(NsError::ProviderFault {
                            path: "unix-listen".to_string(),
                            detail: format!("unix write error: {e}"),
                        })
                    }
                }
            }
            StreamEndpoint::Mem(m) => m.write_raw(data),
        }
    }

    fn write_framed(&self, payload: &[u8]) -> Result<i64, NsError> {
        let mut g = self.endpoint.lock().map_err(|_| NsError::ProviderFault {
            path: "stream".to_string(),
            detail: "poisoned stream mutex".to_string(),
        })?;
        match &mut *g {
            StreamEndpoint::Stdio { .. } => {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                match write_length_prefixed_frame(&mut lock, payload) {
                    Ok(()) => Ok(payload.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => Ok(-1),
                    Err(e) => Err(NsError::ProviderFault {
                        path: "stdio".to_string(),
                        detail: format!("stdout frame write error: {e}"),
                    }),
                }
            }
            StreamEndpoint::TcpClient { stream, closed, .. } => {
                if *closed {
                    return Ok(-1);
                }
                match write_length_prefixed_frame(stream.get_mut(), payload) {
                    Ok(()) => Ok(payload.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *closed = true;
                        Ok(-1)
                    }
                    Err(e) => Err(NsError::ProviderFault {
                        path: "tcp".to_string(),
                        detail: format!("tcp frame write error: {e}"),
                    }),
                }
            }
            StreamEndpoint::TcpServer {
                listener, active, ..
            } => {
                if active.is_none() {
                    let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                        path: "tcp-listen".to_string(),
                        detail: format!("accept failed: {e}"),
                    })?;
                    let _ = stream.set_nodelay(true);
                    *active = Some(BufReader::new(stream));
                }
                let w = active.as_mut().expect("active tcp client").get_mut();
                match write_length_prefixed_frame(w, payload) {
                    Ok(()) => Ok(payload.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *active = None;
                        Ok(-1)
                    }
                    Err(e) => {
                        *active = None;
                        Err(NsError::ProviderFault {
                            path: "tcp-listen".to_string(),
                            detail: format!("tcp frame write error: {e}"),
                        })
                    }
                }
            }
            #[cfg(unix)]
            StreamEndpoint::UnixClient { stream, closed, .. } => {
                if *closed {
                    return Ok(-1);
                }
                match write_length_prefixed_frame(stream.get_mut(), payload) {
                    Ok(()) => Ok(payload.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *closed = true;
                        Ok(-1)
                    }
                    Err(e) => Err(NsError::ProviderFault {
                        path: "unix".to_string(),
                        detail: format!("unix frame write error: {e}"),
                    }),
                }
            }
            #[cfg(unix)]
            StreamEndpoint::UnixServer {
                listener, active, ..
            } => {
                if active.is_none() {
                    let (stream, _) = listener.accept().map_err(|e| NsError::ProviderFault {
                        path: "unix-listen".to_string(),
                        detail: format!("accept failed: {e}"),
                    })?;
                    *active = Some(BufReader::new(stream));
                }
                let w = active.as_mut().expect("active unix client").get_mut();
                match write_length_prefixed_frame(w, payload) {
                    Ok(()) => Ok(payload.len() as i64),
                    Err(e) if is_peer_disconnect_err(e.kind()) => {
                        *active = None;
                        Ok(-1)
                    }
                    Err(e) => {
                        *active = None;
                        Err(NsError::ProviderFault {
                            path: "unix-listen".to_string(),
                            detail: format!("unix frame write error: {e}"),
                        })
                    }
                }
            }
            StreamEndpoint::Mem(m) => m.write_framed(payload),
        }
    }
}

/// Runtime VFS backend attached to a Plan 9 `Provider<RtFsBackend>`.
#[derive(Clone, Debug)]
pub enum RtFsBackend {
    RamFs(MemFsProvider),
    HostDir(ScopedHostDirProvider),
    Stream(StreamProvider),
}

impl RtFsBackend {
    fn read_bytes(&self, clean_subpath: &str) -> Result<Option<Vec<u8>>, NsError> {
        match self {
            RtFsBackend::RamFs(m) => Ok(m.read_file(clean_subpath)?.map(|(b, _)| b)),
            RtFsBackend::HostDir(h) => h.read_bytes(clean_subpath),
            RtFsBackend::Stream(_) => Err(NsError::ProviderFault {
                path: clean_subpath.to_string(),
                detail: "path refers to a stream endpoint; use read_stream_line or read_stream_frame"
                    .to_string(),
            }),
        }
    }

    fn write_bytes(&self, clean_subpath: &str, data: &[u8], executable: bool) -> Result<(), NsError> {
        match self {
            RtFsBackend::RamFs(m) => m.write_file(clean_subpath, data, executable),
            RtFsBackend::HostDir(h) => h.write_bytes(clean_subpath, data, executable),
            RtFsBackend::Stream(_) => Err(NsError::ProviderFault {
                path: clean_subpath.to_string(),
                detail:
                    "path refers to a stream endpoint; use write_stream_bytes or write_stream_frame"
                        .to_string(),
            }),
        }
    }
}

/// Parsed CLI / bootstrap runtime mount directive (`--mount <spec>`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RtMountSpec {
    HostDir {
        perm: MountPerm,
        bind_mode: BindMode,
        host_dir: PathBuf,
        target: CleanPath,
    },
    RamFs {
        bind_mode: BindMode,
        target: CleanPath,
    },
    Stdio {
        perm: MountPerm,
        bind_mode: BindMode,
        target: CleanPath,
    },
    TcpConnect {
        perm: MountPerm,
        bind_mode: BindMode,
        addr: String,
        target: CleanPath,
    },
    TcpListen {
        perm: MountPerm,
        bind_mode: BindMode,
        addr: String,
        target: CleanPath,
    },
    UnixConnect {
        perm: MountPerm,
        bind_mode: BindMode,
        sock_path: PathBuf,
        target: CleanPath,
    },
    UnixListen {
        perm: MountPerm,
        bind_mode: BindMode,
        sock_path: PathBuf,
        target: CleanPath,
    },
}

impl RtMountSpec {
    /// Parse `--mount` specifications:
    /// - `ro:<host_dir>:/<ns_path>` / `rw:<host_dir>:/<ns_path>` / `ramfs:/<ns_path>`
    /// - `[ro-]stdio:/<ns_path>`
    /// - `[ro-]tcp:<host>:<port>:/<ns_path>` / `[ro-]tcp-listen:<host>:<port>:/<ns_path>`
    /// - `[ro-]unix:<sock_path>:/<ns_path>` / `[ro-]unix-listen:<sock_path>:/<ns_path>`
    pub fn parse_cli(spec: &str) -> Result<Self, String> {
        let (mode_tok, rest) = spec.split_once(':').ok_or_else(|| {
            format!(
                "invalid --mount spec {spec:?}; expected ro:<dir>:/path, rw:<dir>:/path, ramfs:/path, stdio:/path, or tcp:<addr>:/path"
            )
        })?;

        let (base_mode, bind_mode) = if let Some(m) = mode_tok.strip_suffix("+before") {
            (m, BindMode::Before)
        } else if let Some(m) = mode_tok.strip_suffix("+after") {
            (m, BindMode::After)
        } else {
            (mode_tok, BindMode::Replace)
        };

        match base_mode {
            "ramfs" => {
                let target = CleanPath::parse_str(rest)
                    .map_err(|e| format!("invalid target path in --mount {spec:?}: {e}"))?;
                return Ok(RtMountSpec::RamFs { bind_mode, target });
            }
            "stdio" | "ro-stdio" => {
                let perm = if base_mode == "ro-stdio" {
                    MountPerm::ReadOnly
                } else {
                    MountPerm::ReadWrite
                };
                let target = CleanPath::parse_str(rest)
                    .map_err(|e| format!("invalid target path in --mount {spec:?}: {e}"))?;
                return Ok(RtMountSpec::Stdio {
                    perm,
                    bind_mode,
                    target,
                });
            }
            "ro" | "rw" | "tcp" | "ro-tcp" | "tcp-listen" | "ro-tcp-listen" | "unix"
            | "ro-unix" | "unix-listen" | "ro-unix-listen" => {}
            other => {
                return Err(format!(
                    "unknown --mount mode {other:?} in {spec:?}; expected ro, rw, ramfs, stdio, tcp, tcp-listen, unix, or unix-listen"
                ));
            }
        }

        // Split `<endpoint>:/ns/path` at the last `:` preceding a `/` so `host:port` and host paths remain unambiguous.
        let (endpoint_str, ns_str) = rest.rsplit_once(":/").ok_or_else(|| {
            format!("invalid --mount spec {spec:?}; expected {mode_tok}:<source>:/<ns_path>")
        })?;
        if endpoint_str.is_empty() {
            return Err(format!("empty mount source in --mount {spec:?}"));
        }
        let target_raw = format!("/{ns_str}");
        let target = CleanPath::parse_str(&target_raw).map_err(|e| {
            format!("invalid namespace target {target_raw:?} in --mount {spec:?}: {e}")
        })?;

        match base_mode {
            "ro" | "rw" => {
                let perm = if base_mode == "rw" {
                    MountPerm::ReadWrite
                } else {
                    MountPerm::ReadOnly
                };
                Ok(RtMountSpec::HostDir {
                    perm,
                    bind_mode,
                    host_dir: PathBuf::from(endpoint_str),
                    target,
                })
            }
            "tcp" | "ro-tcp" => Ok(RtMountSpec::TcpConnect {
                perm: if base_mode == "ro-tcp" {
                    MountPerm::ReadOnly
                } else {
                    MountPerm::ReadWrite
                },
                bind_mode,
                addr: endpoint_str.to_string(),
                target,
            }),
            "tcp-listen" | "ro-tcp-listen" => Ok(RtMountSpec::TcpListen {
                perm: if base_mode == "ro-tcp-listen" {
                    MountPerm::ReadOnly
                } else {
                    MountPerm::ReadWrite
                },
                bind_mode,
                addr: endpoint_str.to_string(),
                target,
            }),
            "unix" | "ro-unix" => Ok(RtMountSpec::UnixConnect {
                perm: if base_mode == "ro-unix" {
                    MountPerm::ReadOnly
                } else {
                    MountPerm::ReadWrite
                },
                bind_mode,
                sock_path: PathBuf::from(endpoint_str),
                target,
            }),
            "unix-listen" | "ro-unix-listen" => Ok(RtMountSpec::UnixListen {
                perm: if base_mode == "ro-unix-listen" {
                    MountPerm::ReadOnly
                } else {
                    MountPerm::ReadWrite
                },
                bind_mode,
                sock_path: PathBuf::from(endpoint_str),
                target,
            }),
            _ => unreachable!(),
        }
    }
}

/// Exact kernel bind-mount instruction for `pivot_root` container initialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PivotBindMount {
    pub host_dir: PathBuf,
    pub container_path: String,
    pub perm: MountPerm,
}

#[derive(Clone, Debug)]
enum RecordedRtOp {
    Mount {
        target: CleanPath,
        prov: Provider<RtFsBackend>,
        bind_mode: BindMode,
    },
    Bind {
        existing: CleanPath,
        target: CleanPath,
        mode: BindMode,
    },
}

/// Per-task Plan 9 Runtime Namespace (`empty` by default: zero ambient host disk or network access).
#[derive(Clone, Debug)]
pub struct RtNamespace {
    ns: Namespace<RtFsBackend>,
    unconfined: bool,
    recorded_ops: Vec<RecordedRtOp>,
}

impl Default for RtNamespace {
    fn default() -> Self {
        Self::empty()
    }
}

impl RtNamespace {
    /// Completely empty capability namespace (`clear` / `rfork(RFCNAMEG)`): default for all tasks!
    pub fn empty() -> Self {
        Self {
            ns: Namespace::empty(),
            unconfined: false,
            recorded_ops: Vec::new(),
        }
    }

    /// Explicit opt-in unconfined mode (`--unconfined` CLI flag ONLY).
    pub fn unconfined_host() -> Self {
        Self {
            ns: Namespace::empty(),
            unconfined: true,
            recorded_ops: Vec::new(),
        }
    }

    pub fn is_unconfined(&self) -> bool {
        self.unconfined
    }

    pub fn is_empty(&self) -> bool {
        !self.unconfined && self.ns.is_empty()
    }

    pub fn fork(&self, isolate: bool) -> Self {
        if isolate {
            Self::empty()
        } else {
            self.clone()
        }
    }

    pub fn mount_ramfs(
        &mut self,
        ramfs: MemFsProvider,
        target: &CleanPath,
        bind_mode: BindMode,
    ) {
        let id = ContentHash::digest_domain(b"aod.rt.ramfs.v1", &[target.as_bytes()]);
        let prov = Provider::new(
            id,
            MountMode::Static,
            MountPerm::ReadWrite,
            RtFsBackend::RamFs(ramfs),
        );
        self.ns.mount(prov.clone(), target, bind_mode);
        self.recorded_ops.push(RecordedRtOp::Mount {
            target: target.clone(),
            prov,
            bind_mode,
        });
    }

    pub fn mount_stream(
        &mut self,
        id: ContentHash,
        stream: StreamProvider,
        target: &CleanPath,
        perm: MountPerm,
        bind_mode: BindMode,
    ) {
        let prov = Provider::new(id, MountMode::Static, perm, RtFsBackend::Stream(stream));
        self.ns.mount(prov.clone(), target, bind_mode);
        self.recorded_ops.push(RecordedRtOp::Mount {
            target: target.clone(),
            prov,
            bind_mode,
        });
    }

    pub fn mount_mem_stream(
        &mut self,
        mem: MemStreamProvider,
        target: &CleanPath,
        perm: MountPerm,
        bind_mode: BindMode,
    ) {
        let (id, sp) = StreamProvider::from_mem(mem, target);
        self.mount_stream(id, sp, target, perm, bind_mode);
    }

    pub fn mount_host_dir(
        &mut self,
        host_dir: &Path,
        target: &CleanPath,
        perm: MountPerm,
        bind_mode: BindMode,
    ) -> Result<(), String> {
        let (id, scoped) = ScopedHostDirProvider::new(host_dir, perm)?;
        let prov = Provider::new(
            id,
            MountMode::Static,
            perm,
            RtFsBackend::HostDir(scoped),
        );
        self.ns.mount(prov.clone(), target, bind_mode);
        self.recorded_ops.push(RecordedRtOp::Mount {
            target: target.clone(),
            prov,
            bind_mode,
        });
        Ok(())
    }

    pub fn bind(
        &mut self,
        existing: &CleanPath,
        target: &CleanPath,
        mode: BindMode,
    ) -> Result<(), String> {
        self.ns.bind(existing, target, mode).map_err(|e| e.to_string())?;
        self.recorded_ops.push(RecordedRtOp::Bind {
            existing: existing.clone(),
            target: target.clone(),
            mode,
        });
        Ok(())
    }

    pub fn apply_spec(&mut self, spec: &RtMountSpec) -> Result<(), String> {
        match spec {
            RtMountSpec::RamFs { bind_mode, target } => {
                self.mount_ramfs(MemFsProvider::default(), target, *bind_mode);
                Ok(())
            }
            RtMountSpec::HostDir {
                perm,
                bind_mode,
                host_dir,
                target,
            } => self.mount_host_dir(host_dir, target, *perm, *bind_mode),
            RtMountSpec::Stdio {
                perm,
                bind_mode,
                target,
            } => {
                let (id, sp) = StreamProvider::new_stdio();
                self.mount_stream(id, sp, target, *perm, *bind_mode);
                Ok(())
            }
            RtMountSpec::TcpConnect {
                perm,
                bind_mode,
                addr,
                target,
            } => {
                let (id, sp) = StreamProvider::connect_tcp(addr, *perm)?;
                self.mount_stream(id, sp, target, *perm, *bind_mode);
                Ok(())
            }
            RtMountSpec::TcpListen {
                perm,
                bind_mode,
                addr,
                target,
            } => {
                let (id, sp) = StreamProvider::bind_tcp(addr, *perm)?;
                self.mount_stream(id, sp, target, *perm, *bind_mode);
                Ok(())
            }
            RtMountSpec::UnixConnect {
                perm,
                bind_mode,
                sock_path,
                target,
            } => {
                #[cfg(unix)]
                {
                    let (id, sp) = StreamProvider::connect_unix(sock_path, *perm)?;
                    self.mount_stream(id, sp, target, *perm, *bind_mode);
                    Ok(())
                }
                #[cfg(not(unix))]
                {
                    let _ = (perm, bind_mode, sock_path, target);
                    Err("Unix domain stream mounts require a Unix host".to_string())
                }
            }
            RtMountSpec::UnixListen {
                perm,
                bind_mode,
                sock_path,
                target,
            } => {
                #[cfg(unix)]
                {
                    let (id, sp) = StreamProvider::bind_unix(sock_path, *perm)?;
                    self.mount_stream(id, sp, target, *perm, *bind_mode);
                    Ok(())
                }
                #[cfg(not(unix))]
                {
                    let _ = (perm, bind_mode, sock_path, target);
                    Err("Unix domain listener mounts require a Unix host".to_string())
                }
            }
        }
    }

    /// Seed `base_specs` (declared in a `namespace <name>:` block) first, then replay any
    /// caller/CLI mounts & binds from `self` on top (preserving `BindMode::Before`/`After`/`Replace`).
    pub fn overlay_onto_base_specs(&self, base_specs: &[RtMountSpec]) -> Result<Self, String> {
        if self.unconfined || base_specs.is_empty() {
            return Ok(self.clone());
        }
        let mut merged = Self::empty();
        for spec in base_specs {
            merged.apply_spec(spec)?;
        }
        for op in &self.recorded_ops {
            match op {
                RecordedRtOp::Mount {
                    target,
                    prov,
                    bind_mode,
                } => {
                    merged.ns.mount(prov.clone(), target, *bind_mode);
                    merged.recorded_ops.push(op.clone());
                }
                RecordedRtOp::Bind {
                    existing,
                    target,
                    mode,
                } => {
                    merged.bind(existing, target, *mode)?;
                }
            }
        }
        Ok(merged)
    }

    /// Sorted raw file descriptors (`>= 3`) owned by pre-opened capability streams in `self.ns`
    /// that must survive `close_range` when spawning a sandboxed `fork()` worker.
    #[cfg(unix)]
    pub fn preserved_raw_fds(&self) -> Vec<i32> {
        let mut fds = BTreeSet::new();
        for (_, stack) in self.ns.entries() {
            for prov in stack {
                if let RtFsBackend::Stream(ref s) = prov.backend {
                    for fd in s.raw_fds() {
                        if fd >= 3 {
                            fds.insert(fd);
                        }
                    }
                }
            }
        }
        fds.into_iter().collect()
    }

    /// Returns `true` if any mounted stream provider is a pre-bound `TcpListener` or `UnixListener`
    /// requiring `accept`/`accept4` inside the sandbox.
    pub fn has_stream_listener(&self) -> bool {
        for (_, stack) in self.ns.entries() {
            for prov in stack {
                if let RtFsBackend::Stream(ref s) = prov.backend {
                    if s.is_listener() {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Build the exact kernel bind-mount list and corresponding post-`pivot_root` `RtNamespace`
    /// where every `ScopedHostDirProvider` points to its in-container mount path (`/src`, `/out`,
    /// or `/.ns_layers/<idx>` for multi-layer union stacks).
    pub fn prepare_pivot_root_plan(&self) -> (Vec<PivotBindMount>, Self) {
        let mut mounts = Vec::new();
        let mut layer_seq = 0usize;

        // Pre-scan which mount points are single-layer non-root HostDir mounts without subpath prefixes.
        let mut direct_mounts = BTreeSet::new();
        for (target, stack) in self.ns.entries() {
            if !target.is_root()
                && stack.len() == 1
                && stack[0].subpath_prefix.is_empty()
                && matches!(stack[0].backend, RtFsBackend::HostDir(_))
            {
                direct_mounts.insert(target);
            }
        }
        // Also guard against nested mount points colliding with a direct parent's internal `.ns_layers`
        let mapped_ns = self.ns.map_backends(|mount_point, prov| match &prov.backend {
            RtFsBackend::RamFs(m) => RtFsBackend::RamFs(m.clone()),
            RtFsBackend::Stream(s) => RtFsBackend::Stream(s.clone()),
            RtFsBackend::HostDir(h) => {
                let container_path = if direct_mounts.contains(mount_point) {
                    mount_point.as_str().to_string()
                } else {
                    let p = format!("/.ns_layers/{layer_seq}");
                    layer_seq += 1;
                    p
                };
                mounts.push(PivotBindMount {
                    host_dir: h.host_root.clone(),
                    container_path: container_path.clone(),
                    perm: prov.perm,
                });
                RtFsBackend::HostDir(ScopedHostDirProvider {
                    host_root: h.host_root.clone(),
                    active_root: PathBuf::from(container_path),
                })
            }
        });

        (
            mounts,
            Self {
                ns: mapped_ns,
                unconfined: self.unconfined,
                recorded_ops: Vec::new(),
            },
        )
    }

    /// Return all host directories and their required permissions for Landlock LSM fallback rules.
    pub fn all_host_bind_dirs(&self) -> Vec<(PathBuf, MountPerm)> {
        let mut by_path: BTreeMap<PathBuf, MountPerm> = BTreeMap::new();
        for (_, stack) in self.ns.entries() {
            for prov in stack {
                if let RtFsBackend::HostDir(ref h) = prov.backend {
                    let entry = by_path
                        .entry(h.host_root.clone())
                        .or_insert(MountPerm::ReadOnly);
                    if prov.perm == MountPerm::ReadWrite {
                        *entry = MountPerm::ReadWrite;
                    }
                }
            }
        }
        by_path.into_iter().collect()
    }

    fn resolve_stream(
        &self,
        raw_path: &[u8],
        require_write: bool,
    ) -> Result<StreamProvider, String> {
        if raw_path.is_empty() || raw_path.contains(&0) {
            return Err("invalid empty or NUL-containing stream path".to_string());
        }
        let clean_path = CleanPath::parse(raw_path)
            .map_err(|e| format!("sandbox namespace error: {e}"))?;
        let candidates = self.ns.resolve(&clean_path);
        if candidates.is_empty() {
            return Err(format!(
                "sandbox namespace error: stream path {:?} is not mounted",
                clean_path.as_str()
            ));
        }
        let mut saw_ro_stream = false;
        let mut saw_non_stream = false;
        for cand in candidates {
            match &cand.provider.backend {
                RtFsBackend::Stream(sp) => {
                    if !cand.subpath.is_empty() {
                        return Err(format!(
                            "sandbox namespace error: stream mount does not accept subpath {:?}",
                            cand.subpath
                        ));
                    }
                    if require_write && cand.provider.perm != MountPerm::ReadWrite {
                        saw_ro_stream = true;
                        continue;
                    }
                    return Ok(sp.clone());
                }
                RtFsBackend::RamFs(_) | RtFsBackend::HostDir(_) => {
                    saw_non_stream = true;
                }
            }
        }
        if saw_ro_stream {
            return Err(format!(
                "sandbox namespace error: stream mount covering {:?} is read-only",
                clean_path.as_str()
            ));
        }
        if saw_non_stream {
            return Err(format!(
                "sandbox namespace error: path {:?} is a filesystem mount, not a stream endpoint",
                clean_path.as_str()
            ));
        }
        Err(format!(
            "sandbox namespace error: stream path {:?} is not mounted",
            clean_path.as_str()
        ))
    }

    pub fn accept_stream(&self, raw_path: &[u8]) -> Result<bool, String> {
        let sp = self.resolve_stream(raw_path, false)?;
        sp.accept_next()
            .map_err(|e| format!("sandbox namespace error: {e}"))
    }

    pub fn stream_local_addr(&self, raw_path: &[u8]) -> Result<Vec<u8>, String> {
        let sp = self.resolve_stream(raw_path, false)?;
        sp.local_addr_bytes()
            .map_err(|e| format!("sandbox namespace error: {e}"))
    }

    pub fn read_stream_line(
        &self,
        raw_path: &[u8],
        max_bytes: usize,
    ) -> Result<(T1, Vec<u8>), String> {
        if max_bytes == 0 || max_bytes > MAX_NET_FRAME_BYTES {
            return Err(format!(
                "read_stream_line max_bytes must be in 1..={MAX_NET_FRAME_BYTES}, got {max_bytes}"
            ));
        }
        let sp = self.resolve_stream(raw_path, false)?;
        let out = sp
            .read_line(max_bytes)
            .map_err(|e| format!("sandbox namespace error: {e}"))?;
        Ok(out.into_trit_tuple())
    }

    pub fn read_stream_frame(
        &self,
        raw_path: &[u8],
        max_bytes: usize,
    ) -> Result<(T1, Vec<u8>), String> {
        if max_bytes > MAX_NET_FRAME_BYTES {
            return Err(format!(
                "read_stream_frame max_bytes must be in 0..={MAX_NET_FRAME_BYTES}, got {max_bytes}"
            ));
        }
        let sp = self.resolve_stream(raw_path, false)?;
        let out = sp
            .read_frame(max_bytes)
            .map_err(|e| format!("sandbox namespace error: {e}"))?;
        Ok(out.into_trit_tuple())
    }

    pub fn write_stream_bytes(&self, raw_path: &[u8], payload: &[u8]) -> Result<i64, String> {
        if payload.len() > MAX_NET_FRAME_BYTES {
            return Err(format!(
                "write_stream_bytes payload ({} bytes) exceeds {MAX_NET_FRAME_BYTES}-byte limit",
                payload.len()
            ));
        }
        let sp = self.resolve_stream(raw_path, true)?;
        sp.write_raw(payload)
            .map_err(|e| format!("sandbox namespace error: {e}"))
    }

    pub fn write_stream_frame(&self, raw_path: &[u8], payload: &[u8]) -> Result<i64, String> {
        if payload.len() > MAX_NET_FRAME_BYTES {
            return Err(format!(
                "write_stream_frame payload ({} bytes) exceeds {MAX_NET_FRAME_BYTES}-byte limit",
                payload.len()
            ));
        }
        let sp = self.resolve_stream(raw_path, true)?;
        sp.write_framed(payload)
            .map_err(|e| format!("sandbox namespace error: {e}"))
    }

    /// Read a file through the task's Plan 9 namespace (enforcing `MAX_SYS_IO_BYTES` and
    /// refusing ambient host paths unless `unconfined` was explicitly set).
    pub fn read_file_bytes(&self, raw_path: &[u8]) -> Result<Vec<u8>, String> {
        if raw_path.is_empty() || raw_path.contains(&0) {
            return Err("invalid empty or NUL-containing file path".to_string());
        }
        if self.unconfined {
            let path_str = std::str::from_utf8(raw_path)
                .map_err(|_| "file path must be valid UTF-8".to_string())?;
            let meta = std::fs::symlink_metadata(path_str)
                .map_err(|e| format!("failed to stat {path_str:?}: {e}"))?;
            if meta.file_type().is_symlink() {
                return Err(format!("refusing to read symlink {path_str:?}"));
            }
            if meta.len() > (MAX_SYS_IO_BYTES as u64) {
                return Err(format!(
                    "file {path_str:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
                ));
            }
            let data = std::fs::read(path_str)
                .map_err(|e| format!("failed to read {path_str:?}: {e}"))?;
            if data.len() > MAX_SYS_IO_BYTES {
                return Err(format!(
                    "file {path_str:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
                ));
            }
            return Ok(data);
        }

        let clean_path = CleanPath::parse(raw_path)
            .map_err(|e| format!("sandbox namespace error: {e}"))?;
        let candidates = self.ns.resolve(&clean_path);
        if candidates.is_empty() {
            return Err(format!(
                "sandbox namespace error: path {:?} is not mounted",
                clean_path.as_str()
            ));
        }

        for cand in candidates {
            if let Some(bytes) = cand
                .provider
                .backend
                .read_bytes(&cand.subpath)
                .map_err(|e| format!("sandbox namespace error: {e}"))?
            {
                return Ok(bytes);
            }
        }
        Err(format!(
            "file {:?} not found in mounted namespace",
            clean_path.as_str()
        ))
    }

    /// Write an executable ELF64 image through the task's Plan 9 namespace (selecting the first
    /// `ReadWrite` layer in the union stack at `raw_path`, or failing with `ReadOnlyMount` / `NotMounted`).
    pub fn write_executable_elf(&self, raw_path: &[u8], img: &[u8]) -> Result<usize, String> {
        if raw_path.is_empty() || raw_path.contains(&0) {
            return Err("invalid empty or NUL-containing output ELF path".to_string());
        }
        if img.len() < 64 || &img[0..4] != b"\x7FELF" {
            return Err("write_executable_elf requires a valid ELF64 header (\\x7FELF)".to_string());
        }
        if img.len() > MAX_SYS_IO_BYTES {
            return Err(format!(
                "ELF image exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            ));
        }

        if self.unconfined {
            let path_str = std::str::from_utf8(raw_path)
                .map_err(|_| "output ELF path must be valid UTF-8".to_string())?;
            let target = Path::new(path_str);
            if let Ok(meta) = std::fs::symlink_metadata(target) {
                if meta.file_type().is_symlink() {
                    return Err(format!("refusing to overwrite symlink {path_str:?}"));
                }
            }
            reject_guarded_repo_file_target(target)?;
            std::fs::write(target, img)
                .map_err(|e| format!("failed to write ELF {path_str:?}: {e}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| format!("failed to chmod +x {path_str:?}: {e}"))?;
            }
            return Ok(img.len());
        }

        let clean_path = CleanPath::parse(raw_path)
            .map_err(|e| format!("sandbox namespace error: {e}"))?;
        let candidates = self.ns.resolve(&clean_path);
        if candidates.is_empty() {
            return Err(format!(
                "sandbox namespace error: path {:?} is not mounted",
                clean_path.as_str()
            ));
        }

        for cand in &candidates {
            if cand.provider.perm == MountPerm::ReadWrite {
                cand.provider
                    .backend
                    .write_bytes(&cand.subpath, img, true)
                    .map_err(|e| format!("sandbox namespace error: {e}"))?;
                return Ok(img.len());
            }
        }
        Err(format!(
            "sandbox namespace error: mount covering {:?} is read-only",
            clean_path.as_str()
        ))
    }
}

fn reject_guarded_repo_file_target(target: &Path) -> Result<(), String> {
    let abs = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cwd error: {e}"))?
            .join(target)
    };
    let Some(parent) = abs.parent() else {
        return Ok(());
    };
    if let Ok(canon_parent) = std::fs::canonicalize(parent) {
        let full = canon_parent.join(abs.file_name().unwrap_or_default());
        let mut cur: Option<&Path> = Some(&canon_parent);
        while let Some(dir) = cur {
            if dir.join("SOURCE_BINDING.json").exists() || dir.join("bindings/aod").exists() {
                for rel in GUARDED_REPO_RELS {
                    let guarded = dir.join(rel);
                    if let Ok(cg) = std::fs::canonicalize(&guarded) {
                        if full == cg || full.starts_with(&cg) {
                            return Err(format!(
                                "refusing to overwrite frozen provenance artifact under {rel}"
                            ));
                        }
                    }
                }
            }
            cur = dir.parent();
        }
    }
    Ok(())
}
