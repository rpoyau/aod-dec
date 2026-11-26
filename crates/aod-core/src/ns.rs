//! Plan 9 & Inferno Context Namespace, `cleanname(2)` Path Normalizer, Content-Addressed
//! `Provider` Handles, and Copy-on-Write Persistent Ternary Mount Table (`Mhead`).
//!
//! 100% `std`-only, `#![forbid(unsafe_code)]` (inherited from crate root), zero floats.

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use crate::sha256::Sha256;

/// Errors returned by Plan 9 namespace path normalization, mount/bind configuration,
/// and provider lookups.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NsError {
    EmptyPath,
    MissingLeadingSlash(String),
    NulByteInPath,
    BackslashForbidden(String),
    ControlCharInPath,
    InvalidUtf8Path,
    SubpathEscape(String),
    InvalidHexDigest(String),
    NotMounted(String),
    ReadOnlyMount(String),
    CapabilityDenied(String),
    ProviderFault { path: String, detail: String },
}

impl fmt::Display for NsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NsError::EmptyPath => write!(f, "namespace path must not be empty"),
            NsError::MissingLeadingSlash(p) => {
                write!(f, "namespace path {p:?} must start with '/'")
            }
            NsError::NulByteInPath => write!(f, "namespace path must not contain NUL bytes"),
            NsError::BackslashForbidden(p) => {
                write!(f, "backslash is forbidden in Plan 9 namespace path {p:?}")
            }
            NsError::ControlCharInPath => {
                write!(f, "ASCII control characters are forbidden in namespace path")
            }
            NsError::InvalidUtf8Path => write!(f, "namespace path must be valid UTF-8"),
            NsError::SubpathEscape(p) => {
                write!(f, "relative subpath {p:?} escapes provider mount root")
            }
            NsError::InvalidHexDigest(h) => {
                write!(f, "invalid 64-character SHA-256 hex digest {h:?}")
            }
            NsError::NotMounted(p) => write!(f, "path {p:?} is not mounted in namespace"),
            NsError::ReadOnlyMount(p) => write!(f, "mount covering {p:?} is read-only"),
            NsError::CapabilityDenied(msg) => write!(f, "namespace capability denied: {msg}"),
            NsError::ProviderFault { path, detail } => {
                write!(f, "provider error at {path:?}: {detail}")
            }
        }
    }
}

impl std::error::Error for NsError {}

/// Canonical Plan 9 clean path (`cleanname(2)` — Pike, *"Lexical File Names in Plan 9
/// or Getting Dot-Dot Right"*).
///
/// Invariants:
/// - Always starts with `'/'`.
/// - Contains no `""` or `"."` or `".."` segments, no trailing `'/'` (except `"/"`),
///   no `'\\'`, no NUL bytes, and no ASCII control characters.
/// - Lexical `".."` at `"/"` clamps to `"/"`.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CleanPath(String);

impl CleanPath {
    pub fn root() -> Self {
        Self("/".to_string())
    }

    pub fn parse(raw: &[u8]) -> Result<Self, NsError> {
        if raw.is_empty() {
            return Err(NsError::EmptyPath);
        }
        if raw.contains(&0) {
            return Err(NsError::NulByteInPath);
        }
        let s = std::str::from_utf8(raw).map_err(|_| NsError::InvalidUtf8Path)?;
        Self::parse_str(s)
    }

    pub fn parse_str(raw: &str) -> Result<Self, NsError> {
        if raw.is_empty() {
            return Err(NsError::EmptyPath);
        }
        if raw.as_bytes().contains(&0) {
            return Err(NsError::NulByteInPath);
        }
        if raw.contains('\\') {
            return Err(NsError::BackslashForbidden(raw.to_string()));
        }
        if raw
            .bytes()
            .any(|b| b < 0x20 || b == 0x7f)
        {
            return Err(NsError::ControlCharInPath);
        }
        if !raw.starts_with('/') {
            return Err(NsError::MissingLeadingSlash(raw.to_string()));
        }

        let mut segs: Vec<&str> = Vec::new();
        for part in raw.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    // Plan 9 root clamping: `/..` resolves lexically to `/`.
                    let _ = segs.pop();
                }
                seg => segs.push(seg),
            }
        }

        if segs.is_empty() {
            Ok(Self::root())
        } else {
            let mut out = String::with_capacity(raw.len() + 1);
            for seg in segs {
                out.push('/');
                out.push_str(seg);
            }
            Ok(Self(out))
        }
    }

    /// Normalize a provider-relative subpath (`""` for mount root, `"a/b"` for nested items).
    /// Unlike `CleanPath::parse`, an unconsumed `".."` that attempts to climb above the
    /// provider's root is rejected with `NsError::SubpathEscape`.
    pub fn clean_subpath(raw: &[u8]) -> Result<String, NsError> {
        if raw.contains(&0) {
            return Err(NsError::NulByteInPath);
        }
        let s = std::str::from_utf8(raw).map_err(|_| NsError::InvalidUtf8Path)?;
        Self::clean_subpath_str(s)
    }

    pub fn clean_subpath_str(raw: &str) -> Result<String, NsError> {
        if raw.as_bytes().contains(&0) {
            return Err(NsError::NulByteInPath);
        }
        if raw.contains('\\') {
            return Err(NsError::BackslashForbidden(raw.to_string()));
        }
        if raw.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(NsError::ControlCharInPath);
        }
        if raw.starts_with('/') {
            return Err(NsError::SubpathEscape(raw.to_string()));
        }
        let mut segs: Vec<&str> = Vec::new();
        for part in raw.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if segs.pop().is_none() {
                        return Err(NsError::SubpathEscape(raw.to_string()));
                    }
                }
                seg => segs.push(seg),
            }
        }
        Ok(segs.join("/"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    pub fn segments(&self) -> Vec<&str> {
        if self.is_root() {
            Vec::new()
        } else {
            self.0[1..].split('/').collect()
        }
    }

    /// If `self` lies at or underneath `mount`, return the clean relative residual subpath.
    pub fn strip_mount_prefix<'a>(&'a self, mount: &CleanPath) -> Option<&'a str> {
        if mount.is_root() {
            return Some(if self.is_root() { "" } else { &self.0[1..] });
        }
        if self.0 == mount.0 {
            return Some("");
        }
        let m = mount.as_str();
        if self.0.len() > m.len()
            && self.0.starts_with(m)
            && self.0.as_bytes()[m.len()] == b'/'
        {
            Some(&self.0[m.len() + 1..])
        } else {
            None
        }
    }
}

impl fmt::Debug for CleanPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CleanPath({:?})", self.0)
    }
}

impl fmt::Display for CleanPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Compose two already-normalized provider subpaths (`""` or `"seg1/seg2"`).
pub fn compose_subpath(prefix: &str, residual: &str) -> String {
    match (prefix.is_empty(), residual.is_empty()) {
        (true, true) => String::new(),
        (false, true) => prefix.to_string(),
        (true, false) => residual.to_string(),
        (false, false) => format!("{prefix}/{residual}"),
    }
}

/// Immutable 32-byte FIPS 180-4 SHA-256 identity used for `Provider.id`, `UnitId`,
/// and Inferno-style `iface_sha256` signature hashes.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ContentHash(pub [u8; 32]);

impl ContentHash {
    pub const ZERO: Self = Self([0u8; 32]);

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn digest_bytes(data: &[u8]) -> Self {
        Self(Sha256::digest(data))
    }

    /// Length-prefixed domain-separated SHA-256 digest preventing tuple boundary collisions.
    pub fn digest_domain(domain: &[u8], parts: &[&[u8]]) -> Self {
        let mut h = Sha256::new();
        h.update(&(domain.len() as u64).to_be_bytes());
        h.update(domain);
        h.update(&(parts.len() as u64).to_be_bytes());
        for p in parts {
            h.update(&(p.len() as u64).to_be_bytes());
            h.update(p);
        }
        Self(h.finalize())
    }

    pub fn from_hex(hex: &str) -> Result<Self, NsError> {
        if hex.len() != 64 {
            return Err(NsError::InvalidHexDigest(hex.to_string()));
        }
        let bytes = hex.as_bytes();
        let mut out = [0u8; 32];
        for i in 0..32 {
            let hi = decode_hex_nibble(bytes[2 * i])
                .ok_or_else(|| NsError::InvalidHexDigest(hex.to_string()))?;
            let lo = decode_hex_nibble(bytes[2 * i + 1])
                .ok_or_else(|| NsError::InvalidHexDigest(hex.to_string()))?;
            out[i] = (hi << 4) | lo;
        }
        Ok(Self(out))
    }

    pub fn to_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(64);
        for &b in &self.0 {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    }

    /// First 16 lowercase hex digits (64 bits) for compact LLVM symbol mangling and diagnostics.
    pub fn short_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(16);
        for &b in &self.0[..8] {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    }
}

fn decode_hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({})", self.short_hex())
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Cryptographic identity of a resolved compilation unit (`(Provider.id, clean_subpath)`),
/// decoupling nominal type and symbol identity from lexical mount strings (`/calc`).
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct UnitId(pub ContentHash);

impl UnitId {
    pub fn for_provider_unit(provider_id: ContentHash, clean_subpath: &str) -> Self {
        Self(ContentHash::digest_domain(
            b"aod.ns.unit.v1",
            &[provider_id.as_bytes(), clean_subpath.as_bytes()],
        ))
    }

    pub fn for_builtin(ns_path: &str) -> Self {
        Self(ContentHash::digest_domain(
            b"aod.ns.builtin.v1",
            &[ns_path.as_bytes()],
        ))
    }

    pub fn for_entry(entry_sha256: ContentHash) -> Self {
        Self(ContentHash::digest_domain(
            b"aod.ns.entry.v1",
            &[entry_sha256.as_bytes()],
        ))
    }

    pub fn short_hex(&self) -> String {
        self.0.short_hex()
    }

    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

impl fmt::Debug for UnitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UnitId({})", self.short_hex())
    }
}

impl fmt::Display for UnitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Plan 9 `bind` / `mount` union placement mode (`MREPL`, `MBEFORE`, `MAFTER`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum BindMode {
    Replace,
    Before,
    After,
}

/// Static compile-time fusion vs. Inferno `load`-style dynamic capability mount.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum MountMode {
    Static,
    Dynamic,
}

/// Read-only vs. read-write mount permission enforced by both `RtNamespace` and
/// the Linux `MS_RDONLY` container bind-mounter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum MountPerm {
    ReadOnly,
    ReadWrite,
}

/// Safe Rust vtable trait for module/unit providers (`Ok(None)` continues union search;
/// `Err(_)` aborts immediately without silent fallthrough on corrupted/tampered artifacts).
pub trait ProviderVTable<U>: Send + Sync {
    fn get_unit(&self, subpath: &[u8]) -> Result<Option<U>, NsError>;
}

/// Opaque mounted Provider entry inside a Plan 9 `Mhead` union stack.
#[derive(Clone)]
pub struct Provider<P> {
    pub id: ContentHash,
    pub mode: MountMode,
    pub perm: MountPerm,
    pub subpath_prefix: String,
    pub backend: P,
}

impl<P> Provider<P> {
    pub fn new(id: ContentHash, mode: MountMode, perm: MountPerm, backend: P) -> Self {
        Self {
            id,
            mode,
            perm,
            subpath_prefix: String::new(),
            backend,
        }
    }

    pub fn with_subpath_prefix(mut self, prefix: String) -> Self {
        self.subpath_prefix = prefix;
        self
    }

    fn same_mount_identity(&self, other: &Self) -> bool {
        self.id == other.id
            && self.mode == other.mode
            && self.perm == other.perm
            && self.subpath_prefix == other.subpath_prefix
    }
}

impl<P> fmt::Debug for Provider<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Provider")
            .field("id", &self.id)
            .field("mode", &self.mode)
            .field("perm", &self.perm)
            .field("subpath_prefix", &self.subpath_prefix)
            .finish()
    }
}

/// Single candidate layer yielded by longest-prefix union resolution in `Namespace<P>`.
#[derive(Clone, Debug)]
pub struct ResolvedMount<'a, P> {
    pub mount_depth: usize,
    pub provider: &'a Provider<P>,
    pub subpath: String,
}

/// Persistent (`Arc` copy-on-write) Ternary Search Tree node keyed on path segments.
struct TstNode<P> {
    seg: Box<str>,
    left: Option<Arc<TstNode<P>>>,
    mid: Option<Arc<TstNode<P>>>,
    right: Option<Arc<TstNode<P>>>,
    mounts: Arc<[Provider<P>]>,
}

impl<P: Clone> Clone for TstNode<P> {
    fn clone(&self) -> Self {
        Self {
            seg: self.seg.clone(),
            left: self.left.clone(),
            mid: self.mid.clone(),
            right: self.right.clone(),
            mounts: Arc::clone(&self.mounts),
        }
    }
}

impl<P> fmt::Debug for TstNode<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TstNode")
            .field("seg", &self.seg)
            .field("mounts_len", &self.mounts.len())
            .finish()
    }
}

/// Per-context Plan 9 / Inferno Namespace mapping `CleanPath` prefixes to ordered
/// `Mhead` union stacks of `Provider<P>` handles via a persistent Copy-on-Write
/// Ternary Search Tree.
pub struct Namespace<P> {
    root_mounts: Arc<[Provider<P>]>,
    tree: Option<Arc<TstNode<P>>>,
}

impl<P> Clone for Namespace<P> {
    fn clone(&self) -> Self {
        Self {
            root_mounts: Arc::clone(&self.root_mounts),
            tree: self.tree.clone(),
        }
    }
}

impl<P> Default for Namespace<P> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<P> fmt::Debug for Namespace<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Namespace")
            .field("root_mounts", &self.root_mounts)
            .field("has_submounts", &self.tree.is_some())
            .finish()
    }
}

impl<P> Namespace<P> {
    /// Return a completely empty namespace (`clear` / Plan 9 `rfork(RFCNAMEG)`).
    pub fn empty() -> Self {
        Self {
            root_mounts: Arc::from([]),
            tree: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.root_mounts.is_empty() && !tst_has_any_mount(&self.tree)
    }

    /// Clear all mounts in place (`rfork(RFCNAMEG)` / `clear`).
    pub fn clear(&mut self) {
        self.root_mounts = Arc::from([]);
        self.tree = None;
    }
}

impl<P: Clone> Namespace<P> {

    /// Create a child namespace:
    /// - `isolate == false` (`rfork(RFNAMEG)`): `O(1)` Copy-on-Write `Arc` clone.
    /// - `isolate == true` (`rfork(RFCNAMEG)`): completely empty namespace.
    pub fn fork(&self, isolate: bool) -> Self {
        if isolate {
            Self::empty()
        } else {
            self.clone()
        }
    }

    /// Attach `provider` at `target` using `bind_mode` (`Replace`, `Before`, or `After`).
    pub fn mount(&mut self, provider: Provider<P>, target: &CleanPath, bind_mode: BindMode) {
        self.mount_many(vec![provider], target, bind_mode);
    }

    /// Pure functional wrapper around `mount`.
    pub fn with_mount(mut self, provider: Provider<P>, target: &CleanPath, bind_mode: BindMode) -> Self {
        self.mount(provider, target, bind_mode);
        self
    }

    fn mount_many(&mut self, incoming: Vec<Provider<P>>, target: &CleanPath, mode: BindMode) {
        let base_for_overlay: Vec<Provider<P>> = match mode {
            BindMode::Replace => Vec::new(),
            BindMode::Before | BindMode::After => {
                let exact = self.exact_mounts(target);
                if !exact.is_empty() {
                    exact.to_vec()
                } else {
                    // Inherit any ancestor mount covering `target` so `bind before/after`
                    // overlays cleanly onto subdirectories of an existing mount!
                    self.resolve(target)
                        .into_iter()
                        .map(|rm| Provider {
                            id: rm.provider.id,
                            mode: rm.provider.mode,
                            perm: rm.provider.perm,
                            subpath_prefix: rm.subpath,
                            backend: rm.provider.backend.clone(),
                        })
                        .collect()
                }
            }
        };

        let merged = splice_mounts(&base_for_overlay, incoming, mode);
        if target.is_root() {
            self.root_mounts = merged;
        } else {
            let segs = target.segments();
            self.tree = tst_set_mounts(&self.tree, &segs, merged);
        }
    }

    /// Plan 9 `bind(existing, target, mode)`: eagerly resolves `existing` in the current
    /// namespace to its active `(Provider, composed_subpath)` handle(s) and splices them
    /// onto `target`. Eager handle binding guarantees bind cycles can never occur.
    pub fn bind(
        &mut self,
        existing: &CleanPath,
        target: &CleanPath,
        mode: BindMode,
    ) -> Result<(), NsError> {
        let resolved = self.resolve(existing);
        if resolved.is_empty() {
            return Err(NsError::NotMounted(existing.as_str().to_string()));
        }
        let incoming: Vec<Provider<P>> = resolved
            .into_iter()
            .map(|rm| Provider {
                id: rm.provider.id,
                mode: rm.provider.mode,
                perm: rm.provider.perm,
                subpath_prefix: rm.subpath,
                backend: rm.provider.backend.clone(),
            })
            .collect();
        self.mount_many(incoming, target, mode);
        Ok(())
    }

    /// Pure functional wrapper around `bind`.
    pub fn with_bind(
        mut self,
        existing: &CleanPath,
        target: &CleanPath,
        mode: BindMode,
    ) -> Result<Self, NsError> {
        self.bind(existing, target, mode)?;
        Ok(self)
    }

    /// Unmount either a specific `provider_id` (`Some(id)`) or the entire union stack (`None`)
    /// at exact mount point `target`. Returns `true` if any mount was removed.
    pub fn unmount(&mut self, target: &CleanPath, provider_id: Option<ContentHash>) -> bool {
        let cur = self.exact_mounts(target);
        if cur.is_empty() {
            return false;
        }
        let next: Vec<Provider<P>> = match provider_id {
            None => Vec::new(),
            Some(pid) => cur.iter().filter(|p| p.id != pid).cloned().collect(),
        };
        if next.len() == cur.len() {
            return false;
        }
        let next_arc: Arc<[Provider<P>]> = Arc::from(next);
        if target.is_root() {
            self.root_mounts = next_arc;
        } else {
            let segs = target.segments();
            self.tree = tst_set_mounts(&self.tree, &segs, next_arc);
        }
        true
    }

    /// Return the exact `Mhead` slice attached at `target` (empty if `target` is not itself a mount point).
    pub fn exact_mounts(&self, target: &CleanPath) -> &[Provider<P>] {
        if target.is_root() {
            return &self.root_mounts;
        }
        let segs = target.segments();
        tst_find_node(&self.tree, &segs)
            .map(|n| n.mounts.as_ref())
            .unwrap_or(&[])
    }

    /// Longest-prefix mount lookup returning the active `Mhead` union stack in resolution order.
    pub fn resolve<'a>(&'a self, path: &CleanPath) -> Vec<ResolvedMount<'a, P>> {
        let segs = path.segments();
        let mut best: Option<(usize, &'a [Provider<P>])> = if self.root_mounts.is_empty() {
            None
        } else {
            Some((0, &self.root_mounts))
        };

        let mut cur = &self.tree;
        for (idx, seg) in segs.iter().enumerate() {
            match tst_step(cur, seg) {
                Some(node) => {
                    if !node.mounts.is_empty() {
                        best = Some((idx + 1, &node.mounts));
                    }
                    cur = &node.mid;
                }
                None => break,
            }
        }

        let Some((depth, mounts)) = best else {
            return Vec::new();
        };
        let residual = segs[depth..].join("/");
        mounts
            .iter()
            .map(|prov| ResolvedMount {
                mount_depth: depth,
                provider: prov,
                subpath: compose_subpath(&prov.subpath_prefix, &residual),
            })
            .collect()
    }

    /// Enumerate all active mount points and their `Mhead` union stacks in deterministic
    /// lexicographic `CleanPath` order.
    pub fn entries(&self) -> Vec<(CleanPath, &[Provider<P>])> {
        let mut out = Vec::new();
        if !self.root_mounts.is_empty() {
            out.push((CleanPath::root(), self.root_mounts.as_ref()));
        }
        let mut prefix = String::new();
        tst_collect_entries(&self.tree, &mut prefix, &mut out);
        out
    }

    /// Transform every provider backend in place (cloning COW nodes as needed).
    /// Used by the Linux `pivot_root` containerizer to switch host-rooted mounts to their
    /// in-container pivoted mount paths once `/.old` is detached.
    pub fn map_backends<F>(&self, mut f: F) -> Self
    where
        F: FnMut(&CleanPath, &Provider<P>) -> P,
    {
        let root_path = CleanPath::root();
        let new_root: Vec<Provider<P>> = self
            .root_mounts
            .iter()
            .map(|p| {
                let nb = f(&root_path, p);
                Provider {
                    id: p.id,
                    mode: p.mode,
                    perm: p.perm,
                    subpath_prefix: p.subpath_prefix.clone(),
                    backend: nb,
                }
            })
            .collect();
        let mut prefix = String::new();
        let new_tree = tst_map_backends(&self.tree, &mut prefix, &mut f);
        Self {
            root_mounts: Arc::from(new_root),
            tree: new_tree,
        }
    }
}

impl<U> Namespace<Arc<dyn ProviderVTable<U>>> {
    /// Walk the longest-prefix union stack at `path`, returning the first `(UnitId, MountMode, U)`
    /// produced by a provider (`Ok(None)` advances to the next union layer; `Err` aborts immediately).
    pub fn lookup_unit(&self, path: &CleanPath) -> Result<Option<(UnitId, MountMode, U)>, NsError> {
        let candidates = self.resolve(path);
        if candidates.is_empty() {
            return Err(NsError::NotMounted(path.as_str().to_string()));
        }
        for cand in candidates {
            if let Some(unit) = cand.provider.backend.get_unit(cand.subpath.as_bytes())? {
                let uid = UnitId::for_provider_unit(cand.provider.id, &cand.subpath);
                return Ok(Some((uid, cand.provider.mode, unit)));
            }
        }
        Ok(None)
    }
}

fn splice_mounts<P: Clone>(
    existing: &[Provider<P>],
    incoming: Vec<Provider<P>>,
    mode: BindMode,
) -> Arc<[Provider<P>]> {
    let mut out: Vec<Provider<P>> = Vec::new();
    let push_dedup = |dst: &mut Vec<Provider<P>>, cand: Provider<P>| {
        if !dst.iter().any(|x| x.same_mount_identity(&cand)) {
            dst.push(cand);
        }
    };
    match mode {
        BindMode::Replace => {
            for p in incoming {
                push_dedup(&mut out, p);
            }
        }
        BindMode::Before => {
            for p in incoming {
                push_dedup(&mut out, p);
            }
            for p in existing {
                push_dedup(&mut out, p.clone());
            }
        }
        BindMode::After => {
            for p in existing {
                push_dedup(&mut out, p.clone());
            }
            for p in incoming {
                push_dedup(&mut out, p);
            }
        }
    }
    Arc::from(out)
}

fn tst_has_any_mount<P>(node: &Option<Arc<TstNode<P>>>) -> bool {
    let Some(n) = node else {
        return false;
    };
    !n.mounts.is_empty()
        || tst_has_any_mount(&n.left)
        || tst_has_any_mount(&n.mid)
        || tst_has_any_mount(&n.right)
}

fn tst_step<'a, P>(mut cur: &'a Option<Arc<TstNode<P>>>, seg: &str) -> Option<&'a TstNode<P>> {
    while let Some(n) = cur {
        match seg.cmp(&n.seg) {
            Ordering::Less => cur = &n.left,
            Ordering::Greater => cur = &n.right,
            Ordering::Equal => return Some(n),
        }
    }
    None
}

fn tst_find_node<'a, P>(
    mut cur: &'a Option<Arc<TstNode<P>>>,
    segs: &[&str],
) -> Option<&'a TstNode<P>> {
    let (&first, rest) = segs.split_first()?;
    let mut matched = tst_step(cur, first)?;
    for &seg in rest {
        cur = &matched.mid;
        matched = tst_step(cur, seg)?;
    }
    Some(matched)
}

fn tst_set_mounts<P: Clone>(
    cur: &Option<Arc<TstNode<P>>>,
    segs: &[&str],
    mounts: Arc<[Provider<P>]>,
) -> Option<Arc<TstNode<P>>> {
    let (&first, rest) = segs.split_first()?;
    match cur {
        None => {
            if rest.is_empty() {
                if mounts.is_empty() {
                    None
                } else {
                    Some(Arc::new(TstNode {
                        seg: first.into(),
                        left: None,
                        mid: None,
                        right: None,
                        mounts,
                    }))
                }
            } else {
                let mid = tst_set_mounts(&None, rest, mounts);
                mid.map(|m| {
                    Arc::new(TstNode {
                        seg: first.into(),
                        left: None,
                        mid: Some(m),
                        right: None,
                        mounts: Arc::from([]),
                    })
                })
            }
        }
        Some(n) => {
            let mut cloned = (**n).clone();
            match first.cmp(&cloned.seg) {
                Ordering::Less => {
                    cloned.left = tst_set_mounts(&cloned.left, segs, mounts);
                }
                Ordering::Greater => {
                    cloned.right = tst_set_mounts(&cloned.right, segs, mounts);
                }
                Ordering::Equal => {
                    if rest.is_empty() {
                        cloned.mounts = mounts;
                    } else {
                        cloned.mid = tst_set_mounts(&cloned.mid, rest, mounts);
                    }
                }
            }
            Some(Arc::new(cloned))
        }
    }
}

fn tst_collect_entries<'a, P>(
    cur: &'a Option<Arc<TstNode<P>>>,
    prefix: &mut String,
    out: &mut Vec<(CleanPath, &'a [Provider<P>])>,
) {
    let Some(n) = cur else {
        return;
    };
    tst_collect_entries(&n.left, prefix, out);

    let prev_len = prefix.len();
    prefix.push('/');
    prefix.push_str(&n.seg);
    if !n.mounts.is_empty() {
        out.push((CleanPath(prefix.clone()), n.mounts.as_ref()));
    }
    tst_collect_entries(&n.mid, prefix, out);
    prefix.truncate(prev_len);

    tst_collect_entries(&n.right, prefix, out);
}

fn tst_map_backends<P: Clone, F>(
    cur: &Option<Arc<TstNode<P>>>,
    prefix: &mut String,
    f: &mut F,
) -> Option<Arc<TstNode<P>>>
where
    F: FnMut(&CleanPath, &Provider<P>) -> P,
{
    let Some(n) = cur else {
        return None;
    };
    let left = tst_map_backends(&n.left, prefix, f);

    let prev_len = prefix.len();
    prefix.push('/');
    prefix.push_str(&n.seg);
    let here_path = CleanPath(prefix.clone());
    let mounts: Arc<[Provider<P>]> = if n.mounts.is_empty() {
        Arc::from([])
    } else {
        let mapped: Vec<Provider<P>> = n
            .mounts
            .iter()
            .map(|p| Provider {
                id: p.id,
                mode: p.mode,
                perm: p.perm,
                subpath_prefix: p.subpath_prefix.clone(),
                backend: f(&here_path, p),
            })
            .collect();
        Arc::from(mapped)
    };
    let mid = tst_map_backends(&n.mid, prefix, f);
    prefix.truncate(prev_len);

    let right = tst_map_backends(&n.right, prefix, f);

    Some(Arc::new(TstNode {
        seg: n.seg.clone(),
        left,
        mid,
        right,
        mounts,
    }))
}
