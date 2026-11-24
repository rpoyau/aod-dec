#![forbid(unsafe_code)]

//! Pre-resolution runtime output path guard, ancestor-based repository root discovery,
//! atomic symlink-safe report writer, and bounded input readers (`P1-1`, `P1-2`).
//!
//! Strictly `std`-only, zero floats, zero external crates.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const ALLOWED_IN_REPO_OUTPUT_ROOTS: [&str; 4] = ["_build", "build", "dist", "release"];

static ATOMIC_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if can_pop {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            Component::Normal(seg) => out.push(seg),
        }
    }
    out
}

fn path_within_lexical(path: &Path, root: &Path) -> bool {
    let norm_path = lexical_normalize(path);
    let norm_root = lexical_normalize(root);
    norm_path.starts_with(&norm_root)
}

fn push_unique_root(roots: &mut Vec<PathBuf>, candidate: PathBuf) {
    let norm = lexical_normalize(&candidate);
    if !roots.contains(&norm) {
        roots.push(norm);
    }
    if let Ok(canon) = fs::canonicalize(&candidate) {
        if !roots.contains(&canon) {
            roots.push(canon);
        }
    }
}

fn is_repo_root_marker(dir: &Path) -> bool {
    fs::symlink_metadata(dir.join("SOURCE_BINDING.json")).is_ok()
        || fs::symlink_metadata(dir.join(".git")).is_ok()
}

/// Walk every lexical ancestor of `start`, collecting any ancestor containing `SOURCE_BINDING.json`
/// or `.git` so repo roots are discovered even when invoked from outside the repository (`P1-1`).
pub fn discover_ancestor_repo_roots(start: &Path, out: &mut Vec<PathBuf>) {
    let mut cur = lexical_normalize(start);
    loop {
        if is_repo_root_marker(&cur) {
            push_unique_root(out, cur.clone());
        }
        if !cur.pop() {
            break;
        }
    }
}

/// Collect all guarded repository roots from `cwd`, `current_exe`, explicit input paths
/// (`--binding`, `--vectors`, `--fixture`), and the lexical ancestors of `report_path` itself.
pub fn collect_guarded_roots(report_path: &Path, explicit_paths: &[&Path]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let cwd = env::current_dir().ok();
    if let Some(ref c) = cwd {
        discover_ancestor_repo_roots(c, &mut roots);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            discover_ancestor_repo_roots(parent, &mut roots);
        }
    }
    for &p in explicit_paths {
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else if let Some(ref c) = cwd {
            c.join(p)
        } else {
            p.to_path_buf()
        };
        discover_ancestor_repo_roots(&abs, &mut roots);
    }
    let abs_report = if report_path.is_absolute() {
        report_path.to_path_buf()
    } else if let Some(ref c) = cwd {
        c.join(report_path)
    } else {
        report_path.to_path_buf()
    };
    discover_ancestor_repo_roots(&abs_report, &mut roots);
    roots
}

fn reject_pre_resolution_symlinks(
    raw_path: &Path,
    guarded_roots: &[PathBuf],
    cwd: &Path,
) -> Result<(), String> {
    let lexical_abs = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        cwd.join(raw_path)
    };
    let norm_abs = lexical_normalize(&lexical_abs);

    // 1. Unconditional target symlink rejection (catches existing & dangling symlinks everywhere)
    for probe in [&lexical_abs, &norm_abs] {
        if let Ok(meta) = fs::symlink_metadata(probe) {
            if meta.file_type().is_symlink() {
                return Err("--report target must not be a symlink".to_string());
            }
        }
    }

    // 2. Pre-resolution lexical prefix walk scoped to `guarded_roots` (catches `_build/escape -> /tmp`
    //    before canonicalization leaves the repository, while permitting OS `/tmp -> /private/tmp`).
    let mut cur = PathBuf::new();
    for comp in lexical_abs.components() {
        match comp {
            Component::Prefix(p) => cur.push(p.as_os_str()),
            Component::RootDir => cur.push(comp.as_os_str()),
            Component::CurDir => continue,
            Component::ParentDir => {
                cur.pop();
                continue;
            }
            Component::Normal(seg) => {
                cur.push(seg);
                if let Ok(meta) = fs::symlink_metadata(&cur) {
                    if meta.file_type().is_symlink() {
                        let cur_in_resolved_parent = cur
                            .parent()
                            .and_then(|p| fs::canonicalize(p).ok())
                            .map(|rp| rp.join(seg));
                        let inside_guarded = guarded_roots.iter().any(|r| {
                            path_within_lexical(&cur, r)
                                || cur_in_resolved_parent
                                    .as_ref()
                                    .is_some_and(|cp| path_within_lexical(cp, r))
                        });
                        if inside_guarded {
                            return Err(format!(
                                "--report must not traverse symlink {:?} inside repository",
                                seg.to_string_lossy()
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Resolve a path that may contain non-existent trailing components without failing on missing leaves.
fn resolve_with_missing_tail(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => resolved.push(p.as_os_str()),
            Component::RootDir => resolved.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(
                    resolved.components().next_back(),
                    Some(Component::Normal(_))
                );
                if can_pop {
                    resolved.pop();
                }
            }
            Component::Normal(seg) => {
                resolved.push(seg);
                if fs::symlink_metadata(&resolved).is_ok() {
                    if let Ok(canon) = fs::canonicalize(&resolved) {
                        resolved = canon;
                    }
                }
            }
        }
    }
    resolved
}

/// Validate a `--report` runtime output path BEFORE reading inputs or creating directories.
///
/// Rejects:
/// - empty paths, `.`, `..`, trailing `/` or `\` or `/.` or `/..`
/// - target symlinks (including dangling symlinks) anywhere
/// - parent symlinks inside any guarded repository root
/// - targets inside any guarded repository root unless placed at `<root>/{_build,build,dist,release}/<subpath>` (`>= 2` components)
/// - existing directories or non-directory parent files.
pub fn validate_runtime_output_path(
    raw_path: &Path,
    guarded_roots: &[PathBuf],
) -> Result<PathBuf, String> {
    if raw_path.as_os_str().is_empty() {
        return Err("--report path must not be empty".to_string());
    }
    let raw_str = raw_path.to_string_lossy();
    if raw_str == "."
        || raw_str == ".."
        || raw_str.ends_with('/')
        || raw_str.ends_with('\\')
        || raw_str.ends_with("/.")
        || raw_str.ends_with("\\.")
        || raw_str.ends_with("/..")
        || raw_str.ends_with("\\..")
        || matches!(
            raw_path.components().next_back(),
            None | Some(Component::CurDir)
                | Some(Component::ParentDir)
                | Some(Component::RootDir)
                | Some(Component::Prefix(_))
        )
    {
        return Err(
            "--report path must name a file, not a directory or trailing dot reference".to_string(),
        );
    }

    let cwd = env::current_dir().map_err(|e| format!("failed resolving current directory: {e}"))?;
    let abs_input = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        cwd.join(raw_path)
    };

    // Always include lexical ancestor repo roots of `raw_path` itself (`P1-1`).
    let mut all_roots: Vec<PathBuf> = Vec::new();
    for r in guarded_roots {
        push_unique_root(&mut all_roots, r.clone());
    }
    discover_ancestor_repo_roots(&abs_input, &mut all_roots);

    reject_pre_resolution_symlinks(raw_path, &all_roots, &cwd)?;

    let candidate = resolve_with_missing_tail(&abs_input);
    // Also discover repo roots along the resolved candidate ancestors.
    discover_ancestor_repo_roots(&candidate, &mut all_roots);

    for guarded in &all_roots {
        if let Ok(rel) = candidate.strip_prefix(guarded) {
            let parts: Vec<String> = rel
                .components()
                .filter_map(|c| match c {
                    Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect();
            if parts.len() < 2 || !ALLOWED_IN_REPO_OUTPUT_ROOTS.contains(&parts[0].as_str()) {
                return Err(format!(
                    "--report target {:?} is inside a distributable repository directory; write outside the repository or under _build/",
                    parts.join("/")
                ));
            }
            let mut walk = guarded.clone();
            for part in &parts {
                walk.push(part);
                if let Ok(meta) = fs::symlink_metadata(&walk) {
                    if meta.file_type().is_symlink() {
                        return Err(
                            "--report must not traverse symlinks inside repository".to_string(),
                        );
                    }
                }
            }
        }
    }

    if let Ok(meta) = fs::symlink_metadata(&candidate) {
        if meta.file_type().is_symlink() {
            return Err("--report target must not be a symlink".to_string());
        }
        if meta.is_dir() {
            return Err("--report target already exists as a directory".to_string());
        }
    }

    let mut anc = candidate.parent();
    while let Some(p) = anc {
        if let Ok(meta) = fs::symlink_metadata(p) {
            if !meta.is_dir() {
                return Err("--report parent path exists as a non-directory file".to_string());
            }
            break;
        }
        anc = p.parent();
    }

    Ok(candidate)
}

/// Atomically write `content` to a previously validated `--report` path via an exclusive
/// `create_new(true)` sibling temp file, `sync_all`, and `rename`, cleaning up on any error.
pub fn write_report_atomic(validated_path: &Path, content: &[u8]) -> Result<(), String> {
    if let Ok(meta) = fs::symlink_metadata(validated_path) {
        if meta.file_type().is_symlink() {
            return Err("--report target must not be a symlink".to_string());
        }
        if meta.is_dir() {
            return Err("--report target already exists as a directory".to_string());
        }
    }

    let parent = validated_path
        .parent()
        .ok_or_else(|| "--report path has no parent directory".to_string())?;
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed creating --report parent directory ({:?})", e.kind()))?;
    }

    let fname = validated_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("report");
    let pid = std::process::id();

    let mut tmp_path = None;
    let mut tmp_file = None;
    for _ in 0..64 {
        let seq = ATOMIC_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let cand = parent.join(format!(".{fname}.tmp.{pid}.{seq}"));
        match OpenOptions::new().write(true).create_new(true).open(&cand) {
            Ok(f) => {
                tmp_path = Some(cand);
                tmp_file = Some(f);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(format!(
                    "failed creating atomic temp file ({:?})",
                    e.kind()
                ));
            }
        }
    }

    let tmp_path = tmp_path
        .ok_or_else(|| "failed allocating unique sibling temp file for --report".to_string())?;
    let mut tmp_file = tmp_file.expect("tmp_file created alongside tmp_path");

    let write_res = (|| -> io::Result<()> {
        tmp_file.write_all(content)?;
        tmp_file.sync_all()?;
        Ok(())
    })();
    drop(tmp_file);

    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("failed writing --report temp file ({:?})", e.kind()));
    }

    if let Err(e) = fs::rename(&tmp_path, validated_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!(
            "failed atomically renaming --report temp file ({:?})",
            e.kind()
        ));
    }

    Ok(())
}

/// Bounded regular-file reader (`!is_symlink() && is_file()`, `<= max_bytes`) with zero host paths in errors.
pub fn read_bounded_regular_file(
    path: &Path,
    label: &str,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("{label}: failed inspecting file ({:?})", e.kind()))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        return Err(format!("{label}: symlink rejected"));
    }
    if !ft.is_file() {
        return Err(format!("{label}: not a regular file"));
    }
    if meta.len() > max_bytes as u64 {
        return Err(format!(
            "{label}: file exceeds maximum size limit of {max_bytes} bytes"
        ));
    }
    let file =
        fs::File::open(path).map_err(|e| format!("{label}: failed opening file ({:?})", e.kind()))?;
    let open_meta = file
        .metadata()
        .map_err(|e| format!("{label}: failed reading file metadata ({:?})", e.kind()))?;
    if !open_meta.is_file() {
        return Err(format!("{label}: not a regular file"));
    }
    let mut buf = Vec::new();
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{label}: failed reading file ({:?})", e.kind()))?;
    if buf.len() > max_bytes {
        return Err(format!(
            "{label}: file exceeds maximum size limit of {max_bytes} bytes"
        ));
    }
    Ok(buf)
}

/// Bounded stdin reader (`<= max_bytes`).
pub fn read_bounded_stdin(max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    io::stdin()
        .take((max_bytes + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("stdin: failed reading input ({:?})", e.kind()))?;
    if buf.len() > max_bytes {
        return Err(format!(
            "stdin: input exceeds maximum size limit of {max_bytes} bytes"
        ));
    }
    Ok(buf)
}
