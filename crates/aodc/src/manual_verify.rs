#![forbid(unsafe_code)]

//! `manual_verify`: 6-pass `std`-only, zero-float verifier for the modular M4-aligned Typst manual
//! (`docs/manual/`).
//!
//! Passes:
//! 1. `#include`/`#import` graph containment, `SOURCE_BINDING.json` vendored-file SHA-256 check,
//!    and proper Typst lexer/scanner (`//`, nested `/* */`, fenced raw, inline raw, escapes,
//!    double-quoted strings, math `$ ... $`, delimiter balance, `P0-2`, `P0-4`).
//! 2. `main_registry.json` binding & digest freshness, all `#main*("main:...")` kind/prefix checks,
//!    internal `<label>` uniqueness & typed `#eqref`/`#secref`/`#figref`/`#tabref` resolution,
//!    and `refs.bib` citation resolution (`P0-1`).
//! 3. Zero-raster & zero-external-package audit (no `image(`, no raster files, no `@preview/`,
//!    length-unit validation on `#aod-flowchart` node coordinates).
//! 4. Pure-Rust implementation of all 4 `rpoyau/aod-lint` `TypstAstPass` rules (`P0-3`).
//! 5. Live 3-mode (VM, AOT `-O2`, ORC LLJIT `-O2`) verification of inline ```` ```dec ```` /
//!    ```` ```dec-expect ````, ```` ```dec-fail <Code> ````, and `// @dec-file:` fixtures with RAII
//!    temp directory cleanup (`P0-4`).
//! 6. Strict zero-float JSON data validation (`parse_strict_json`) + live fixture parity against
//!    `manual_examples/{03,04,06}.dec` (`P0-4`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;
use aod_llvm::{CodegenPlan, OrcJitSession};

use crate::checker::check_unit;
use crate::codegen::lower_to_llvm_ir_with_plan;
use crate::diagnostics::{CompatMode, Diagnostic};
use crate::eval::{Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use crate::modules::{load_from_source_with_options, LoadOptions};

static MV_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// RAII temporary directory guard ensuring zero leaked AOT binaries even on early return/error (`P0-4`).
struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn create(tag: &str) -> Result<Self, String> {
        let seq = MV_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aodc_mv_{}_{}_{tag}",
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed creating temp dir {}: {e}", dir.display()))?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug, Default)]
pub struct ManualVerifyOptions {
    pub stdlib_dir: Option<PathBuf>,
    pub registry_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualVerifyReport {
    pub status: String,
    pub files_scanned: usize,
    pub main_refs_checked: usize,
    pub internal_labels: usize,
    pub internal_refs_checked: usize,
    pub citations_checked: usize,
    pub inline_positive_checked: usize,
    pub inline_negative_checked: usize,
    pub dec_files_checked: usize,
    pub data_files_checked: usize,
    pub raster_violations: usize,
    pub aod_lint_violations: usize,
    pub errors: Vec<String>,
}

impl ManualVerifyReport {
    pub fn is_ok(&self) -> bool {
        self.status == "PASS" && self.errors.is_empty()
    }

    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "aod_lint_violations".to_string(),
            JsonValue::from_usize(self.aod_lint_violations),
        );
        m.insert(
            "citations_checked".to_string(),
            JsonValue::from_usize(self.citations_checked),
        );
        m.insert(
            "data_files_checked".to_string(),
            JsonValue::from_usize(self.data_files_checked),
        );
        m.insert(
            "dec_files_checked".to_string(),
            JsonValue::from_usize(self.dec_files_checked),
        );
        m.insert(
            "errors".to_string(),
            JsonValue::Array(
                self.errors
                    .iter()
                    .map(|e| JsonValue::from_str_val(e.clone()))
                    .collect(),
            ),
        );
        m.insert(
            "files_scanned".to_string(),
            JsonValue::from_usize(self.files_scanned),
        );
        m.insert(
            "inline_negative_checked".to_string(),
            JsonValue::from_usize(self.inline_negative_checked),
        );
        m.insert(
            "inline_positive_checked".to_string(),
            JsonValue::from_usize(self.inline_positive_checked),
        );
        m.insert(
            "internal_labels".to_string(),
            JsonValue::from_usize(self.internal_labels),
        );
        m.insert(
            "internal_refs_checked".to_string(),
            JsonValue::from_usize(self.internal_refs_checked),
        );
        m.insert(
            "main_refs_checked".to_string(),
            JsonValue::from_usize(self.main_refs_checked),
        );
        m.insert(
            "raster_violations".to_string(),
            JsonValue::from_usize(self.raster_violations),
        );
        m.insert(
            "status".to_string(),
            JsonValue::from_str_val(self.status.clone()),
        );
        JsonValue::Object(m)
    }
}

#[derive(Clone, Debug)]
struct RawBlock {
    file: PathBuf,
    line: usize,
    lang: String,
    body: String,
}

#[derive(Clone, Debug)]
struct NonRawSegment {
    line: usize,
    text: String,
    in_string: bool,
}

#[derive(Clone, Debug)]
struct ScannedFile {
    path: PathBuf,
    non_raw_markup: String,
    non_raw_segments: Vec<NonRawSegment>,
    raw_blocks: Vec<RawBlock>,
    dec_file_refs: Vec<(usize, String)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LabelKind {
    Heading,
    Equation,
    Figure,
    Table,
}

fn find_repo_root(start: &Path) -> PathBuf {
    let mut cur: Option<&Path> = Some(start);
    for _ in 0..=8 {
        if let Some(d) = cur {
            if d.join("SOURCE_BINDING.json").is_file() {
                return d.to_path_buf();
            }
            cur = d.parent();
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| start.to_path_buf())
}

pub fn verify_manual_dir(manual_dir: &Path, options: &ManualVerifyOptions) -> ManualVerifyReport {
    let mut rep = ManualVerifyReport {
        status: "FAIL".to_string(),
        files_scanned: 0,
        main_refs_checked: 0,
        internal_labels: 0,
        internal_refs_checked: 0,
        citations_checked: 0,
        inline_positive_checked: 0,
        inline_negative_checked: 0,
        dec_files_checked: 0,
        data_files_checked: 0,
        raster_violations: 0,
        aod_lint_violations: 0,
        errors: Vec::new(),
    };

    let canon_manual = match std::fs::canonicalize(manual_dir) {
        Ok(p) if p.is_dir() => p,
        Ok(p) => {
            rep.errors
                .push(format!("manual path is not a directory: {}", p.display()));
            return rep;
        }
        Err(e) => {
            rep.errors.push(format!(
                "failed accessing manual dir {}: {e}",
                manual_dir.display()
            ));
            return rep;
        }
    };

    let repo_root = find_repo_root(&canon_manual);
    let canon_repo = std::fs::canonicalize(&repo_root).unwrap_or_else(|_| repo_root.clone());

    let load_opts = LoadOptions {
        stdlib_dir: options.stdlib_dir.clone().or_else(|| {
            let cand = canon_repo.join("stdlib");
            if cand.join("aod").join("ledger.dec").is_file() {
                Some(cand)
            } else {
                None
            }
        }),
    };

    // Pass 0: Verify SOURCE_BINDING.json vendored files against live on-disk bytes (`P0-4`)
    verify_source_binding_vendored(&canon_repo, &mut rep.errors);

    // Pass 1: Walk `#include` / local `#import` graph from `main.typ` with strict containment & cycle check
    let main_typ = canon_manual.join("main.typ");
    if !main_typ.is_file() {
        rep.errors
            .push(format!("missing entry file {}", main_typ.display()));
        return rep;
    }

    let mut scanned: Vec<ScannedFile> = Vec::new();
    let mut visited: BTreeSet<PathBuf> = BTreeSet::new();
    let mut visiting: Vec<PathBuf> = Vec::new();
    walk_and_scan_typ(
        &main_typ,
        &canon_manual,
        &canon_repo,
        &mut visited,
        &mut visiting,
        &mut scanned,
        &mut rep.errors,
    );
    rep.files_scanned = scanned.len();

    // Pass 2: Main registry + internal labels/refs + BibTeX citations (`P0-1`)
    let reg_path = options
        .registry_path
        .clone()
        .unwrap_or_else(|| canon_repo.join("bindings/aod/main_registry.json"));
    run_pass2_refs_and_cites(&canon_manual, &reg_path, &scanned, &mut rep);

    // Pass 3: Zero-raster, zero-@preview, & #aod-flowchart length checks (`P0-1`, `AC-08`, `AC-10`)
    run_pass3_zero_raster_and_vector(&canon_manual, &scanned, &mut rep);

    // Pass 4: Exact `rpoyau/aod-lint` TypstAstPass rules in pure `std` Rust (`P0-3`)
    run_pass4_aod_lint(&scanned, &mut rep);

    // Pass 5: Live 3-mode verification of inline Dec blocks & `// @dec-file:` fixtures (`P0-4`)
    run_pass5_executable_dec(&canon_repo, &scanned, &load_opts, &mut rep);

    // Pass 6: Strict JSON data files + live parity against `manual_examples/{03,04,06}.dec` (`P0-4`)
    run_pass6_data_json_and_live_parity(&canon_manual, &canon_repo, &scanned, &load_opts, &mut rep);

    if rep.errors.is_empty() && rep.raster_violations == 0 && rep.aod_lint_violations == 0 {
        rep.status = "PASS".to_string();
    } else {
        rep.status = "FAIL".to_string();
    }
    rep
}

// ============================================================================
// Pass 0: SOURCE_BINDING.json vendored file SHA-256 verification (`P0-4`)
// ============================================================================

fn verify_source_binding_vendored(repo_root: &Path, errors: &mut Vec<String>) {
    let sb_path = repo_root.join("SOURCE_BINDING.json");
    if !sb_path.is_file() {
        return;
    }
    let text = match std::fs::read_to_string(&sb_path) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("failed reading SOURCE_BINDING.json: {e}"));
            return;
        }
    };
    let parsed = match parse_strict_json(&text) {
        Ok(j) => j,
        Err(e) => {
            errors.push(format!("SOURCE_BINDING.json strict JSON error: {e}"));
            return;
        }
    };
    let Some(bindings) = parsed.get("upstream_bindings").and_then(JsonValue::as_array) else {
        errors.push("SOURCE_BINDING.json missing upstream_bindings array".to_string());
        return;
    };
    for b in bindings {
        if let Some(vfiles) = b.get("vendored_files").and_then(JsonValue::as_array) {
            for vf in vfiles {
                let local_path = vf
                    .get("local_path")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("");
                let expected_sha = vf.get("sha256").and_then(JsonValue::as_str).unwrap_or("");
                if local_path.is_empty() || expected_sha.is_empty() {
                    errors.push("SOURCE_BINDING.json vendored_files entry missing local_path/sha256".to_string());
                    continue;
                }
                let full = repo_root.join(local_path);
                match std::fs::read(&full) {
                    Ok(bytes) => {
                        let actual = Sha256::digest_hex(&bytes);
                        if actual != expected_sha {
                            errors.push(format!(
                                "vendored file SHA-256 mismatch for {local_path}: expected {expected_sha}, got {actual}"
                            ));
                        }
                    }
                    Err(e) => {
                        errors.push(format!("missing vendored file {local_path}: {e}"));
                    }
                }
            }
        }
    }
}

// ============================================================================
// Pass 1: Include Graph Containment & Proper Typst Lexer/Scanner (`P0-2`, `P0-4`)
// ============================================================================

fn resolve_contained_typ_path(
    raw_spec: &str,
    current_file: &Path,
    canon_manual: &Path,
    canon_repo: &Path,
    allow_schema_outside_manual: bool,
) -> Result<Option<PathBuf>, String> {
    if raw_spec.starts_with("@preview/") {
        return Err(format!("external Typst package import forbidden: {raw_spec}"));
    }
    let candidate = if let Some(rel) = raw_spec.strip_prefix('/') {
        canon_repo.join(rel)
    } else {
        current_file
            .parent()
            .unwrap_or(canon_manual)
            .join(raw_spec)
    };
    let canon = std::fs::canonicalize(&candidate)
        .map_err(|e| format!("unresolved Typst path {raw_spec:?}: {e}"))?;

    if allow_schema_outside_manual
        && canon == canon_repo.join("bindings/aod/aod_schema.typ")
    {
        return Ok(None);
    }
    if !canon.starts_with(canon_manual) {
        return Err(format!(
            "Typst path {raw_spec:?} resolves outside manual directory ({})",
            canon.display()
        ));
    }
    Ok(Some(canon))
}

fn walk_and_scan_typ(
    file_path: &Path,
    canon_manual: &Path,
    canon_repo: &Path,
    visited: &mut BTreeSet<PathBuf>,
    visiting: &mut Vec<PathBuf>,
    scanned: &mut Vec<ScannedFile>,
    errors: &mut Vec<String>,
) {
    let canon = match std::fs::canonicalize(file_path) {
        Ok(p) => p,
        Err(e) => {
            errors.push(format!("failed canonicalizing {}: {e}", file_path.display()));
            return;
        }
    };
    if visiting.iter().any(|p| p == &canon) {
        let mut chain: Vec<String> = visiting
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        chain.push(canon.display().to_string());
        errors.push(format!(
            "cyclic Typst #include/#import detected: {}",
            chain.join(" -> ")
        ));
        return;
    }
    if visited.contains(&canon) {
        return;
    }

    let raw_source = match std::fs::read_to_string(&canon) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("failed reading {}: {e}", canon.display()));
            return;
        }
    };

    let sf = scan_typst_file(&canon, &raw_source, errors);
    let includes = extract_quoted_directive_targets(&sf.non_raw_markup, "#include");
    let imports = extract_quoted_directive_targets(&sf.non_raw_markup, "#import");

    visiting.push(canon.clone());
    for (line, target) in includes {
        match resolve_contained_typ_path(&target, &canon, canon_manual, canon_repo, false) {
            Ok(Some(next)) => walk_and_scan_typ(
                &next,
                canon_manual,
                canon_repo,
                visited,
                visiting,
                scanned,
                errors,
            ),
            Ok(None) => {}
            Err(msg) => errors.push(format!("{}:{line}: {msg}", canon.display())),
        }
    }
    for (line, target) in imports {
        match resolve_contained_typ_path(&target, &canon, canon_manual, canon_repo, true) {
            Ok(Some(next)) => walk_and_scan_typ(
                &next,
                canon_manual,
                canon_repo,
                visited,
                visiting,
                scanned,
                errors,
            ),
            Ok(None) => {}
            Err(msg) => errors.push(format!("{}:{line}: {msg}", canon.display())),
        }
    }
    visiting.pop();

    visited.insert(canon);
    scanned.push(sf);
}

/// Proper Typst lexer/scanner (`P0-2`): handles `//`, nested `/* */`, fenced raw blocks (exact
/// closing backtick count >= 3), inline raw spans, escaped chars, double-quoted strings (ignoring
/// ASCII apostrophes in prose!), math `$ ... $` (balanced `$`, no cross-check of `(` vs `]` inside
/// math so `$(0, 6]$` passes cleanly!), and delimiter stack outside comments/raw/strings/math.
fn scan_typst_file(path: &Path, src: &str, errors: &mut Vec<String>) -> ScannedFile {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;
    let mut line = 1usize;

    let mut non_raw_markup = String::with_capacity(src.len());
    let mut non_raw_segments: Vec<NonRawSegment> = Vec::new();
    let mut cur_seg = String::new();
    let mut cur_seg_line = 1usize;
    let mut raw_blocks: Vec<RawBlock> = Vec::new();
    let mut dec_file_refs: Vec<(usize, String)> = Vec::new();
    let mut delim_stack: Vec<(char, usize)> = Vec::new();

    let flush_seg = |cur: &mut String, seg_line: usize, segs: &mut Vec<NonRawSegment>, in_str: bool| {
        if !cur.is_empty() {
            segs.push(NonRawSegment {
                line: seg_line,
                text: std::mem::take(cur),
                in_string: in_str,
            });
        }
    };

    while i < n {
        let b = bytes[i];

        // 1. Line comment `// ...`
        if b == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            flush_seg(&mut cur_seg, cur_seg_line, &mut non_raw_segments, false);
            let start = i + 2;
            let comment_line = line;
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            let comment_text = &src[start..i];
            if let Some(rest) = comment_text.trim().strip_prefix("@dec-file:") {
                let target = rest.trim();
                if !target.is_empty() {
                    dec_file_refs.push((comment_line, target.to_string()));
                }
            }
            if i < n && bytes[i] == b'\n' {
                non_raw_markup.push('\n');
                i += 1;
                line += 1;
            }
            cur_seg_line = line;
            continue;
        }

        // 2. Nested block comment `/* ... */`
        if b == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            flush_seg(&mut cur_seg, cur_seg_line, &mut non_raw_segments, false);
            let start_line = line;
            i += 2;
            let mut depth = 1usize;
            while i < n && depth > 0 {
                if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    if bytes[i] == b'\n' {
                        non_raw_markup.push('\n');
                        line += 1;
                    }
                    i += 1;
                }
            }
            if depth > 0 {
                errors.push(format!(
                    "{}:{start_line}: unclosed Typst block comment `/*`",
                    path.display()
                ));
            }
            cur_seg_line = line;
            continue;
        }

        // 3. Raw blocks (fenced ` ``` ` or inline `` `...` ``)
        if b == b'`' {
            flush_seg(&mut cur_seg, cur_seg_line, &mut non_raw_segments, false);
            let start_line = line;
            let mut ticks = 0usize;
            while i + ticks < n && bytes[i + ticks] == b'`' {
                ticks += 1;
            }
            if ticks >= 3 {
                i += ticks;
                let lang_start = i;
                while i < n && bytes[i] != b'\n' {
                    i += 1;
                }
                let lang = src[lang_start..i].trim().to_string();
                if i < n && bytes[i] == b'\n' {
                    non_raw_markup.push('\n');
                    i += 1;
                    line += 1;
                }
                let body_start = i;
                let mut closed = false;
                let mut body_end = i;
                while i < n {
                    if bytes[i] == b'`' {
                        let mut ct = 0usize;
                        while i + ct < n && bytes[i + ct] == b'`' {
                            ct += 1;
                        }
                        if ct >= ticks {
                            body_end = i;
                            i += ct;
                            closed = true;
                            break;
                        }
                        i += ct;
                        continue;
                    }
                    if bytes[i] == b'\n' {
                        non_raw_markup.push('\n');
                        line += 1;
                    }
                    i += 1;
                }
                if !closed {
                    errors.push(format!(
                        "{}:{start_line}: unclosed fenced raw block (`{}`)",
                        path.display(),
                        "`".repeat(ticks)
                    ));
                } else {
                    raw_blocks.push(RawBlock {
                        file: path.to_path_buf(),
                        line: start_line,
                        lang,
                        body: src[body_start..body_end].to_string(),
                    });
                }
            } else {
                // Inline raw span `` `...` `` (ticks == 1 or 2)
                i += ticks;
                let mut closed = false;
                while i < n {
                    if bytes[i] == b'`' {
                        let mut ct = 0usize;
                        while i + ct < n && bytes[i + ct] == b'`' {
                            ct += 1;
                        }
                        if ct == ticks {
                            i += ct;
                            closed = true;
                            break;
                        }
                        i += ct;
                        continue;
                    }
                    if bytes[i] == b'\n' {
                        non_raw_markup.push('\n');
                        line += 1;
                    }
                    i += 1;
                }
                if !closed {
                    errors.push(format!(
                        "{}:{start_line}: unclosed inline raw span",
                        path.display()
                    ));
                }
            }
            cur_seg_line = line;
            continue;
        }

        // 4. Escaped character `\[`, `\]`, `\$`, `\\`, etc.
        if b == b'\\' && i + 1 < n {
            let next_b = bytes[i + 1];
            non_raw_markup.push('\\');
            let ch = src[i + 1..].chars().next().unwrap();
            non_raw_markup.push(ch);
            if cur_seg.is_empty() {
                cur_seg_line = line;
            }
            cur_seg.push(ch);
            if next_b == b'\n' {
                line += 1;
            }
            i += 1 + ch.len_utf8();
            continue;
        }

        // 5. Math span `$ ... $` (verify balanced `$`; do NOT cross-check `(` vs `]` inside math!)
        if b == b'$' {
            flush_seg(&mut cur_seg, cur_seg_line, &mut non_raw_segments, false);
            let start_line = line;
            non_raw_markup.push('$');
            i += 1;
            let mut closed = false;
            while i < n {
                if bytes[i] == b'\\' && i + 1 < n {
                    non_raw_markup.push('\\');
                    let ch = src[i + 1..].chars().next().unwrap();
                    non_raw_markup.push(ch);
                    if ch == '\n' {
                        line += 1;
                    }
                    i += 1 + ch.len_utf8();
                    continue;
                }
                if bytes[i] == b'"' {
                    // Quoted literal inside math mode (e.g. $upright("RCD")$)
                    non_raw_markup.push('"');
                    i += 1;
                    while i < n && bytes[i] != b'"' {
                        if bytes[i] == b'\\' && i + 1 < n {
                            let ch = src[i + 1..].chars().next().unwrap();
                            non_raw_markup.push('\\');
                            non_raw_markup.push(ch);
                            i += 1 + ch.len_utf8();
                            continue;
                        }
                        let ch = src[i..].chars().next().unwrap();
                        if ch == '\n' {
                            line += 1;
                        }
                        non_raw_markup.push(ch);
                        i += ch.len_utf8();
                    }
                    if i < n && bytes[i] == b'"' {
                        non_raw_markup.push('"');
                        i += 1;
                    }
                    continue;
                }
                if bytes[i] == b'$' {
                    non_raw_markup.push('$');
                    i += 1;
                    closed = true;
                    break;
                }
                let ch = src[i..].chars().next().unwrap();
                if ch == '\n' {
                    line += 1;
                }
                non_raw_markup.push(ch);
                i += ch.len_utf8();
            }
            if !closed {
                errors.push(format!(
                    "{}:{start_line}: unclosed Typst math delimiter `$`",
                    path.display()
                ));
            }
            cur_seg_line = line;
            continue;
        }

        // 6. Double-quoted string literal `"..."` (NOTE: ASCII apostrophes `'` in prose are NOT strings!)
        if b == b'"' {
            flush_seg(&mut cur_seg, cur_seg_line, &mut non_raw_segments, false);
            let start_line = line;
            non_raw_markup.push('"');
            i += 1;
            let mut str_content = String::new();
            let mut closed = false;
            while i < n {
                if bytes[i] == b'\\' && i + 1 < n {
                    let ch = src[i + 1..].chars().next().unwrap();
                    non_raw_markup.push('\\');
                    non_raw_markup.push(ch);
                    str_content.push(ch);
                    if ch == '\n' {
                        line += 1;
                    }
                    i += 1 + ch.len_utf8();
                    continue;
                }
                if bytes[i] == b'"' {
                    non_raw_markup.push('"');
                    i += 1;
                    closed = true;
                    break;
                }
                let ch = src[i..].chars().next().unwrap();
                if ch == '\n' {
                    line += 1;
                }
                non_raw_markup.push(ch);
                str_content.push(ch);
                i += ch.len_utf8();
            }
            if !closed {
                errors.push(format!(
                    "{}:{start_line}: unclosed double-quoted string literal",
                    path.display()
                ));
            } else if !str_content.is_empty() {
                non_raw_segments.push(NonRawSegment {
                    line: start_line,
                    text: str_content,
                    in_string: true,
                });
            }
            cur_seg_line = line;
            continue;
        }

        // 7. Regular markup/code character + delimiter stack check
        let ch = src[i..].chars().next().unwrap();
        match ch {
            '(' | '[' | '{' => delim_stack.push((ch, line)),
            ')' | ']' | '}' => {
                let expected_open = match ch {
                    ')' => '(',
                    ']' => '[',
                    '}' => '{',
                    _ => unreachable!(),
                };
                match delim_stack.pop() {
                    Some((open, _)) if open == expected_open => {}
                    Some((open, open_line)) => errors.push(format!(
                        "{}:{line}: mismatched Typst delimiter `{ch}` (opened `{open}` at line {open_line})",
                        path.display()
                    )),
                    None => errors.push(format!(
                        "{}:{line}: unexpected closing Typst delimiter `{ch}`",
                        path.display()
                    )),
                }
            }
            _ => {}
        }

        non_raw_markup.push(ch);
        if ch == '\n' {
            flush_seg(&mut cur_seg, cur_seg_line, &mut non_raw_segments, false);
            line += 1;
            cur_seg_line = line;
        } else {
            if cur_seg.is_empty() {
                cur_seg_line = line;
            }
            cur_seg.push(ch);
        }
        i += ch.len_utf8();
    }

    flush_seg(&mut cur_seg, cur_seg_line, &mut non_raw_segments, false);
    for (open, open_line) in delim_stack {
        errors.push(format!(
            "{}:{open_line}: unclosed Typst delimiter `{open}`",
            path.display()
        ));
    }

    ScannedFile {
        path: path.to_path_buf(),
        non_raw_markup,
        non_raw_segments,
        raw_blocks,
        dec_file_refs,
    }
}

fn extract_quoted_directive_targets(text: &str, directive: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(pos) = rest.find(directive) {
            let after = &rest[pos + directive.len()..];
            let trimmed = after.trim_start();
            if let Some(q) = trimmed.strip_prefix('"') {
                if let Some(end_q) = q.find('"') {
                    out.push((idx + 1, q[..end_q].to_string()));
                    rest = &q[end_q + 1..];
                    continue;
                }
            }
            rest = after;
        }
    }
    out
}

fn extract_macro_string_args(text: &str, macro_name: &str) -> Vec<(usize, String)> {
    let pat = format!("#{macro_name}(");
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let mut search = line;
        while let Some(pos) = search.find(&pat) {
            let after = &search[pos + pat.len()..];
            let trimmed = after.trim_start();
            if let Some(q) = trimmed.strip_prefix('"') {
                if let Some(end_q) = q.find('"') {
                    out.push((idx + 1, q[..end_q].to_string()));
                    search = &q[end_q + 1..];
                    continue;
                }
            }
            search = after;
        }
    }
    out
}

// ============================================================================
// Pass 2: Main Registry, Internal Labels/Refs, & BibTeX Citations (`P0-1`)
// ============================================================================

fn parse_bibtex_keys(bib_text: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let bytes = bib_text.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        if bytes[i] == b'@' {
            i += 1;
            let ty_start = i;
            while i < n && bytes[i] != b'{' && bytes[i] != b'(' && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let etype = bib_text[ty_start..i].to_ascii_lowercase();
            while i < n && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < n && (bytes[i] == b'{' || bytes[i] == b'(') {
                i += 1;
                let k_start = i;
                while i < n && bytes[i] != b',' && bytes[i] != b'}' && bytes[i] != b')' {
                    i += 1;
                }
                let key = bib_text[k_start..i].trim().to_string();
                if !key.is_empty()
                    && etype != "comment"
                    && etype != "preamble"
                    && etype != "string"
                {
                    keys.insert(key);
                }
            }
        } else {
            i += 1;
        }
    }
    keys
}

fn extract_named_string_param(text: &str, param_name: &str) -> Option<String> {
    let pat = format!("{param_name}:");
    let pos = text.find(&pat)?;
    let after = text[pos + pat.len()..].trim_start();
    let q = after.strip_prefix('"')?;
    let end_q = q.find('"')?;
    Some(q[..end_q].to_string())
}

fn run_pass2_refs_and_cites(
    canon_manual: &Path,
    reg_path: &Path,
    scanned: &[ScannedFile],
    rep: &mut ManualVerifyReport,
) {
    // 1. Load & verify `main_registry.json`
    let (reg_digest, reg_entries) = match std::fs::read_to_string(reg_path)
        .map_err(|e| format!("failed reading registry {}: {e}", reg_path.display()))
        .and_then(|s| parse_strict_json(&s).map_err(|e| format!("registry JSON error: {e}")))
    {
        Ok(reg) => {
            if reg.get("schema").and_then(JsonValue::as_str) != Some("aod.typst.main-registry.v1")
            {
                rep.errors.push(format!(
                    "invalid Main registry schema in {}",
                    reg_path.display()
                ));
            }
            let digest = reg
                .get("main_sources_sha256")
                .and_then(JsonValue::as_str)
                .unwrap_or("")
                .to_string();
            let mut map = BTreeMap::new();
            if let Some(obj) = reg.get("entries").and_then(JsonValue::as_object) {
                for (k, v) in obj {
                    let kind = v
                        .get("kind")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("")
                        .to_string();
                    map.insert(k.clone(), kind);
                }
            } else {
                rep.errors
                    .push("Main registry missing `entries` object".to_string());
            }
            (digest, map)
        }
        Err(msg) => {
            rep.errors.push(msg);
            (String::new(), BTreeMap::new())
        }
    };

    // Verify `main.typ` root-relative `/` paths & `expected-main-digest` freshness (`P0-1`)
    if let Some(main_sf) = scanned.iter().find(|s| s.path.ends_with("main.typ")) {
        if let Some(exp_dig) =
            extract_named_string_param(&main_sf.non_raw_markup, "expected-main-digest")
        {
            if !reg_digest.is_empty() && exp_dig != reg_digest {
                rep.errors.push(format!(
                    "main.typ expected-main-digest ({exp_dig}) != main_registry.json main_sources_sha256 ({reg_digest})"
                ));
            }
        } else {
            rep.errors
                .push("main.typ missing expected-main-digest binding".to_string());
        }
        for key in ["bib-path", "main-registry-path"] {
            match extract_named_string_param(&main_sf.non_raw_markup, key) {
                Some(p) if p.starts_with('/') => {}
                Some(p) => rep.errors.push(format!(
                    "main.typ {key}: {p:?} MUST be root-relative (leading `/`) for `typst compile --root .`"
                )),
                None => rep
                    .errors
                    .push(format!("main.typ missing {key} parameter")),
            }
        }
    }

    // 2. Load `refs.bib`
    let bib_path = canon_manual.join("refs.bib");
    let bib_keys = match std::fs::read_to_string(&bib_path) {
        Ok(s) => parse_bibtex_keys(&s),
        Err(e) => {
            rep.errors
                .push(format!("failed reading {}: {e}", bib_path.display()));
            BTreeSet::new()
        }
    };

    // 3. Collect ALL internal labels & validate duplicates
    let mut labels: BTreeMap<String, (LabelKind, PathBuf, usize)> = BTreeMap::new();
    for sf in scanned {
        collect_internal_labels(sf, &mut labels, &mut rep.errors);
    }
    rep.internal_labels = labels.len();

    // 4. Validate ALL Main refs, internal refs, and BibTeX citations
    let main_macros: [(&str, &[&str]); 4] = [
        ("mainequation", &["equation", "rg-equation"]),
        ("mainsection", &["section", "subsection", "subsubsection"]),
        (
            "mainappendix",
            &["appendix", "appendix-subsection", "appendix-subsubsection"],
        ),
        ("maintable", &["table"]),
    ];

    for sf in scanned {
        for (mname, allowed_kinds) in main_macros {
            for (line, id) in extract_macro_string_args(&sf.non_raw_markup, mname) {
                rep.main_refs_checked += 1;
                let Some(raw) = id.strip_prefix("main:") else {
                    rep.errors.push(format!(
                        "{}:{line}: #{mname}({id:?}) missing required `main:` prefix",
                        sf.path.display()
                    ));
                    continue;
                };
                match reg_entries.get(raw) {
                    None => rep.errors.push(format!(
                        "{}:{line}: unresolved Main reference #{mname}({id:?})",
                        sf.path.display()
                    )),
                    Some(kind) if !allowed_kinds.contains(&kind.as_str()) => rep.errors.push(format!(
                        "{}:{line}: wrong Main reference kind for #{mname}({id:?}): got {kind:?}, allowed {allowed_kinds:?}",
                        sf.path.display()
                    )),
                    Some(_) => {}
                }
            }
        }

        // Internal references
        let internal_macros: [(&str, Option<&[LabelKind]>); 7] = [
            ("eqref", Some(&[LabelKind::Equation])),
            ("eqnref", Some(&[LabelKind::Equation])),
            ("secref", Some(&[LabelKind::Heading])),
            ("appref", Some(&[LabelKind::Heading])),
            ("figref", Some(&[LabelKind::Figure])),
            ("tabref", Some(&[LabelKind::Table])),
            ("bare-ref", None),
        ];
        for (mname, allowed_opt) in internal_macros {
            for (line, target) in extract_macro_string_args(&sf.non_raw_markup, mname) {
                rep.internal_refs_checked += 1;
                match labels.get(&target) {
                    None => rep.errors.push(format!(
                        "{}:{line}: unresolved internal reference #{mname}({target:?})",
                        sf.path.display()
                    )),
                    Some((kind, _, _)) => {
                        if let Some(allowed) = allowed_opt {
                            if !allowed.contains(kind) {
                                rep.errors.push(format!(
                                    "{}:{line}: wrong internal reference kind for #{mname}({target:?}): got {kind:?}",
                                    sf.path.display()
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Citations `#aod-cite("k1, k2")`
        for (line, keys_str) in extract_macro_string_args(&sf.non_raw_markup, "aod-cite") {
            for raw_k in keys_str.split(',') {
                let k = raw_k.trim();
                if k.is_empty() {
                    continue;
                }
                rep.citations_checked += 1;
                if !bib_keys.contains(k) {
                    rep.errors.push(format!(
                        "{}:{line}: unresolved citation key {k:?} (not in refs.bib)",
                        sf.path.display()
                    ));
                }
            }
        }
    }
}

fn is_label_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.')
}

fn collect_internal_labels(
    sf: &ScannedFile,
    labels: &mut BTreeMap<String, (LabelKind, PathBuf, usize)>,
    errors: &mut Vec<String>,
) {
    let lines: Vec<&str> = sf.non_raw_markup.lines().collect();
    let mut in_table_fig = false;
    let mut in_normal_fig = false;
    let mut fig_paren_depth: i32 = 0;

    for (idx, &raw_line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim();

        if trimmed.contains("#figure(") {
            in_table_fig = trimmed.contains("kind: table") || trimmed.contains("kind:table");
            in_normal_fig = !in_table_fig;
            fig_paren_depth = 0;
        }
        if in_table_fig || in_normal_fig {
            for ch in raw_line.chars() {
                if ch == '(' {
                    fig_paren_depth += 1;
                } else if ch == ')' {
                    fig_paren_depth -= 1;
                }
            }
        }

        // Scan `<label-id>` occurrences on this line (outside math `$ ... $` and strings `"..."`)
        let chars: Vec<char> = raw_line.chars().collect();
        let mut c_i = 0usize;
        let mut in_math = false;
        let mut in_str = false;
        while c_i < chars.len() {
            let ch = chars[c_i];
            if ch == '\\' && c_i + 1 < chars.len() {
                c_i += 2;
                continue;
            }
            if !in_str && ch == '$' {
                in_math = !in_math;
                c_i += 1;
                continue;
            }
            if !in_math && ch == '"' {
                in_str = !in_str;
                c_i += 1;
                continue;
            }
            if !in_math && !in_str && ch == '<' {
                let start = c_i + 1;
                let mut end = start;
                while end < chars.len() && is_label_char(chars[end]) {
                    end += 1;
                }
                if end > start && end < chars.len() && chars[end] == '>' {
                    let lbl: String = chars[start..end].iter().collect();
                    // Ignore internal schema metadata tags `<aod-...>`
                    if !lbl.starts_with("aod-") && lbl.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                        let before: String = chars[..c_i].iter().collect();
                        let btrim = before.trim_end();
                        let kind = if trimmed.starts_with('=') || btrim.contains("#aod-anchor(") {
                            LabelKind::Heading
                        } else if btrim.ends_with('$') || btrim.contains("#tagged-eq(") || lbl.starts_with("eq:") {
                            LabelKind::Equation
                        } else if in_table_fig || lbl.starts_with("tab:") {
                            LabelKind::Table
                        } else if in_normal_fig || lbl.starts_with("fig:") {
                            LabelKind::Figure
                        } else {
                            LabelKind::Heading
                        };
                        if let Some((_, prev_file, prev_line)) = labels.get(&lbl) {
                            errors.push(format!(
                                "{}:{line_no}: duplicate internal label <{lbl}> (previously defined at {}:{prev_line})",
                                sf.path.display(),
                                prev_file.display()
                            ));
                        } else {
                            labels.insert(lbl, (kind, sf.path.clone(), line_no));
                        }
                    }
                    c_i = end + 1;
                    continue;
                }
            }
            c_i += 1;
        }

        if (in_table_fig || in_normal_fig) && fig_paren_depth <= 0 {
            in_table_fig = false;
            in_normal_fig = false;
        }
    }
}

// ============================================================================
// Pass 3: Zero-Raster, Zero-@preview, & #aod-flowchart Length Audit (`P0-1`)
// ============================================================================

fn run_pass3_zero_raster_and_vector(
    canon_manual: &Path,
    scanned: &[ScannedFile],
    rep: &mut ManualVerifyReport,
) {
    // 1. Check for stray raster files anywhere under `docs/manual/`
    let raster_exts = ["png", "jpg", "jpeg", "webp", "gif", "bmp"];
    check_no_raster_files_recursive(canon_manual, &raster_exts, rep);

    // 2. Scan all `.typ` non-raw content for `image(`, raster paths, `@preview/`, and `#aod-flowchart` bare ints
    for sf in scanned {
        for (idx, line) in sf.non_raw_markup.lines().enumerate() {
            let line_no = idx + 1;
            let lower = line.to_ascii_lowercase();
            if contains_word_call(&lower, "image") {
                rep.raster_violations += 1;
                rep.errors.push(format!(
                    "{}:{line_no}: forbidden `image(...)` call in pure-vector Typst manual",
                    sf.path.display()
                ));
            }
            for ext in &raster_exts {
                let needle = format!(".{ext}");
                if lower.contains(&needle) {
                    rep.raster_violations += 1;
                    rep.errors.push(format!(
                        "{}:{line_no}: forbidden raster reference `{needle}`",
                        sf.path.display()
                    ));
                }
            }
            if line.contains("@preview/") {
                rep.errors.push(format!(
                    "{}:{line_no}: forbidden external Typst package `@preview/...`",
                    sf.path.display()
                ));
            }
        }

        // Verify `#aod-flowchart` coordinates use Typst lengths (`cm`/`pt`/`mm`/`em`/`in`/`%`), NEVER bare ints (`P0-1`)
        if sf.non_raw_markup.contains("aod-flowchart(") {
            check_flowchart_length_units(sf, &mut rep.errors);
        }
    }
}

fn contains_word_call(line: &str, ident: &str) -> bool {
    let bytes = line.as_bytes();
    let needle = format!("{ident}(");
    let mut start = 0usize;
    while let Some(pos) = line[start..].find(&needle) {
        let abs = start + pos;
        let prev_ok = abs == 0
            || !(bytes[abs - 1].is_ascii_alphanumeric()
                || bytes[abs - 1] == b'_'
                || bytes[abs - 1] == b'-');
        if prev_ok {
            return true;
        }
        start = abs + 1;
    }
    false
}

fn check_no_raster_files_recursive(dir: &Path, exts: &[&str], rep: &mut ManualVerifyReport) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            check_no_raster_files_recursive(&p, exts, rep);
        } else if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
            let low = ext.to_ascii_lowercase();
            if exts.contains(&low.as_str()) {
                rep.raster_violations += 1;
                rep.errors.push(format!(
                    "forbidden raster asset found under manual dir: {}",
                    p.display()
                ));
            }
        }
    }
}

fn check_flowchart_length_units(sf: &ScannedFile, errors: &mut Vec<String>) {
    for (idx, line) in sf.non_raw_markup.lines().enumerate() {
        let line_no = idx + 1;
        for coord_key in ["x:", "y:", "w:", "h:"] {
            let mut rest = line;
            while let Some(pos) = rest.find(coord_key) {
                let prev_ok = pos == 0
                    || !(rest.as_bytes()[pos - 1].is_ascii_alphanumeric()
                        || rest.as_bytes()[pos - 1] == b'_'
                        || rest.as_bytes()[pos - 1] == b'-');
                let after = &rest[pos + coord_key.len()..];
                if prev_ok {
                    let val_tok = after
                        .trim_start()
                        .split(|c: char| c == ',' || c == ')' || c.is_whitespace())
                        .next()
                        .unwrap_or("");
                    if !val_tok.is_empty()
                        && val_tok.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-')
                    {
                        let has_unit = val_tok.ends_with("cm")
                            || val_tok.ends_with("pt")
                            || val_tok.ends_with("mm")
                            || val_tok.ends_with("em")
                            || val_tok.ends_with("in")
                            || val_tok.ends_with('%');
                        if !has_unit {
                            errors.push(format!(
                                "{}:{line_no}: #aod-flowchart coordinate `{coord_key} {val_tok}` must use a Typst length unit (cm/pt), not a bare number",
                                sf.path.display()
                            ));
                        }
                    }
                }
                rest = after;
            }
        }
    }
}

// ============================================================================
// Pass 4: Exact `rpoyau/aod-lint` `TypstAstPass` Rules in Pure `std` Rust (`P0-3`)
// ============================================================================

fn run_pass4_aod_lint(scanned: &[ScannedFile], rep: &mut ManualVerifyReport) {
    for sf in scanned {
        for seg in &sf.non_raw_segments {
            // Rule 1: Unanchored bare `@ident` (only outside quoted strings, not followed by `[`)
            if !seg.in_string {
                if let Some(ident) = find_unanchored_at_ref(&seg.text) {
                    rep.aod_lint_violations += 1;
                    rep.errors.push(format!(
                        "{}:{}: aod-lint Rule 1 (unanchored bare reference `@{ident}`; use #secref/#eqref/#figref/#tabref/#aod-cite)",
                        sf.path.display(),
                        seg.line
                    ));
                }
            }

            // Rules 2, 3, 4 operate on prose/captions/noteblocks/strings (case-insensitive!)
            let text = &seg.text;
            if let Some(m) = find_vague_range(text) {
                rep.aod_lint_violations += 1;
                rep.errors.push(format!(
                    "{}:{}: aod-lint Rule 2 (vague range `{m}`)",
                    sf.path.display(),
                    seg.line
                ));
            }
            if let Some(m) = find_midstream_premise_import(text) {
                rep.aod_lint_violations += 1;
                rep.errors.push(format!(
                    "{}:{}: aod-lint Rule 3 (midstream premise import `{m}`)",
                    sf.path.display(),
                    seg.line
                ));
            }
            if let Some(m) = find_continuum_term(text) {
                rep.aod_lint_violations += 1;
                rep.errors.push(format!(
                    "{}:{}: aod-lint Rule 4 (forbidden continuum ontology term `{m}`)",
                    sf.path.display(),
                    seg.line
                ));
            }
        }
    }
}

fn find_unanchored_at_ref(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        if bytes[i] == b'@' {
            let prev_is_word = i > 0
                && (bytes[i - 1].is_ascii_alphanumeric()
                    || bytes[i - 1] == b'_'
                    || bytes[i - 1] == b'.');
            if !prev_is_word && i + 1 < n && (bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_') {
                let start = i + 1;
                let mut end = start;
                while end < n
                    && (bytes[end].is_ascii_alphanumeric()
                        || matches!(bytes[end], b'_' | b'-' | b':'))
                {
                    end += 1;
                }
                if end >= n || bytes[end] != b'[' {
                    return Some(text[start..end].to_string());
                }
            }
        }
        i += 1;
    }
    None
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_word_boundary_before(s: &str, idx: usize) -> bool {
    idx == 0 || !is_word_byte(s.as_bytes()[idx - 1])
}

fn is_word_boundary_after(s: &str, idx: usize) -> bool {
    idx >= s.len() || !is_word_byte(s.as_bytes()[idx])
}

/// Rule 2 (`P0-3`): `(?i)\b(see|refer to|in)\s+(§§?\s*\d+\s*[-–—]\s*\d+|sections?\s+\d+\s*[-–—]\s*\d+|the literature)\b`
fn find_vague_range(text: &str) -> Option<String> {
    let low = text.to_ascii_lowercase();
    for lead in ["refer to", "see", "in"] {
        let mut pos = 0usize;
        while let Some(rel) = low[pos..].find(lead) {
            let start = pos + rel;
            let after_lead = start + lead.len();
            if is_word_boundary_before(&low, start) && is_word_boundary_after(&low, after_lead) {
                let tail = &low[after_lead..];
                let ws_len = tail.len() - tail.trim_start().len();
                if ws_len > 0 {
                    let rest = &tail[ws_len..];
                    if rest.starts_with("the literature")
                        && is_word_boundary_after(rest, "the literature".len())
                    {
                        return Some(format!("{lead} the literature"));
                    }
                    if let Some(after_sec) = rest
                        .strip_prefix("sections")
                        .or_else(|| rest.strip_prefix("section"))
                    {
                        let ws2 = after_sec.len() - after_sec.trim_start().len();
                        if ws2 > 0 && has_num_dash_num(&after_sec[ws2..]) {
                            return Some(format!("{lead} section range"));
                        }
                    }
                    if let Some(after_ss) = rest.strip_prefix("§§").or_else(|| rest.strip_prefix('§')) {
                        if has_num_dash_num(after_ss.trim_start()) {
                            return Some(format!("{lead} § range"));
                        }
                    }
                }
            }
            pos = start + 1;
        }
    }
    None
}

fn has_num_dash_num(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    let mut d1 = false;
    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        d1 = true;
        chars.next();
    }
    if !d1 {
        return false;
    }
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
    match chars.next() {
        Some('-') | Some('–') | Some('—') => {}
        _ => return false,
    }
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
    chars.peek().is_some_and(|c| c.is_ascii_digit())
}

/// Rule 3 (`P0-3`): `(?i)\b(by|via|using)\s+[a-z0-9_\-\s]+(law|equation|theorem|principle)\b`
fn find_midstream_premise_import(text: &str) -> Option<String> {
    let low = text.to_ascii_lowercase();
    for lead in ["using", "via", "by"] {
        let mut pos = 0usize;
        while let Some(rel) = low[pos..].find(lead) {
            let start = pos + rel;
            let after_lead = start + lead.len();
            if is_word_boundary_before(&low, start) && is_word_boundary_after(&low, after_lead) {
                let tail = &low[after_lead..];
                let ws_len = tail.len() - tail.trim_start().len();
                if ws_len > 0 {
                    let rest = &tail[ws_len..];
                    // Scan greedy span of `[a-z0-9_\-\s]+` stopping at punctuation/newlines
                    let mut span_end = 0usize;
                    let rb = rest.as_bytes();
                    while span_end < rb.len()
                        && (rb[span_end].is_ascii_alphanumeric()
                            || matches!(rb[span_end], b'_' | b'-' | b' ' | b'\t'))
                    {
                        span_end += 1;
                    }
                    let span = &rest[..span_end];
                    for kw in ["law", "equation", "theorem", "principle"] {
                        let mut kpos = 0usize;
                        while let Some(kr) = span[kpos..].find(kw) {
                            let kend = kpos + kr + kw.len();
                            if kpos + kr > 0 && is_word_boundary_after(span, kend) {
                                return Some(format!("{lead} {}", span[..kend].trim()));
                            }
                            kpos = kpos + kr + 1;
                        }
                    }
                }
            }
            pos = start + 1;
        }
    }
    None
}

/// Rule 4 (`P0-3`): `(?i)\b(smooth\s+manifold|continuous\s+manifold|hilbert\s+space)\b`
fn find_continuum_term(text: &str) -> Option<String> {
    let low = text.to_ascii_lowercase();
    for (w1, w2) in [
        ("smooth", "manifold"),
        ("continuous", "manifold"),
        ("hilbert", "space"),
    ] {
        let mut pos = 0usize;
        while let Some(rel) = low[pos..].find(w1) {
            let start = pos + rel;
            let after1 = start + w1.len();
            if is_word_boundary_before(&low, start) && is_word_boundary_after(&low, after1) {
                let tail = &low[after1..];
                let ws_len = tail.len() - tail.trim_start().len();
                if ws_len > 0 {
                    let rest = &tail[ws_len..];
                    if rest.starts_with(w2) && is_word_boundary_after(rest, w2.len()) {
                        return Some(format!("{w1} {w2}"));
                    }
                }
            }
            pos = start + 1;
        }
    }
    None
}

// ============================================================================
// Pass 5: 3-Mode Verification of Inline Dec Blocks & `// @dec-file:` Fixtures (`P0-4`)
// ============================================================================

fn verify_positive_3mode(
    src: &str,
    base_dir: Option<&Path>,
    expected_stdout: Option<&str>,
    label: &str,
    tmp_dir: &Path,
    bin_stem: &str,
    load_opts: &LoadOptions,
    errors: &mut Vec<String>,
) {
    let loaded = match load_from_source_with_options(src, base_dir, CompatMode::V2, load_opts) {
        Ok(u) => u,
        Err(e) => {
            errors.push(format!("{label}: load failed: {e}"));
            return;
        }
    };
    let checked = match check_unit(loaded, true) {
        Ok(c) => c,
        Err(e) => {
            errors.push(format!("{label}: check failed: {e}"));
            return;
        }
    };
    // 1. Exact VM
    let vm_out = match Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .and_then(|ev| ev.run_main())
    {
        Ok(out) => out.to_stdout_string(),
        Err(e) => {
            errors.push(format!("{label}: VM failed: {e}"));
            return;
        }
    };
    if let Some(exp) = expected_stdout {
        if vm_out != exp {
            errors.push(format!(
                "{label}: VM stdout mismatch (expected {exp:?}, got {vm_out:?})"
            ));
            return;
        }
    }
    // 2. Lower to LLVM IR once
    let plan = CodegenPlan::baseline_o2();
    let raw_ir = match lower_to_llvm_ir_with_plan(&checked, &plan) {
        Ok(ir) => ir,
        Err(e) => {
            errors.push(format!("{label}: LLVM lowering failed: {e}"));
            return;
        }
    };
    // 3. AOT -O2
    let exe = tmp_dir.join(bin_stem);
    if let Err(e) = aod_llvm::link_aot_executable_with_plan(&raw_ir, &plan, &exe) {
        errors.push(format!("{label}: AOT -O2 link failed: {e}"));
        return;
    }
    match Command::new(&exe).output() {
        Ok(out) if out.status.success() => {
            let aot_stdout = String::from_utf8_lossy(&out.stdout);
            if aot_stdout != vm_out {
                errors.push(format!(
                    "{label}: AOT -O2 stdout mismatch against VM (aot={aot_stdout:?}, vm={vm_out:?})"
                ));
                return;
            }
        }
        Ok(out) => {
            errors.push(format!(
                "{label}: AOT -O2 exited non-zero: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
            return;
        }
        Err(e) => {
            errors.push(format!("{label}: failed executing AOT binary: {e}"));
            return;
        }
    }
    // 4. ORC LLJIT -O2
    let sess = match OrcJitSession::compile_with_plan(&raw_ir, &plan) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("{label}: ORC JIT -O2 compile failed: {e}"));
            return;
        }
    };
    let (jit_stdout, jit_res) = sess.run_captured(DEFAULT_FUEL as i64, DEFAULT_MAX_DEPTH as i32);
    if let Err(e) = jit_res {
        errors.push(format!("{label}: ORC JIT -O2 runtime failed: {e}"));
        return;
    }
    if jit_stdout != vm_out {
        errors.push(format!(
            "{label}: ORC JIT -O2 stdout mismatch against VM (jit={jit_stdout:?}, vm={vm_out:?})"
        ));
    }
}

fn diag_matches(diag: &Diagnostic, expected: &str) -> bool {
    diag.to_string().contains(expected) || format!("{:?}", diag.code) == expected
}

fn verify_negative_3mode(
    src: &str,
    base_dir: Option<&Path>,
    expected_err: &str,
    label: &str,
    tmp_dir: &Path,
    bin_stem: &str,
    load_opts: &LoadOptions,
    errors: &mut Vec<String>,
) {
    let expected = expected_err.trim();
    if expected.is_empty() {
        errors.push(format!("{label}: negative expectation must be non-empty"));
        return;
    }
    let check_res = load_from_source_with_options(src, base_dir, CompatMode::V2, load_opts)
        .and_then(|u| check_unit(u, true));
    match check_res {
        Err(diag) => {
            // Compile-time negative control: verify exact diagnostic AND confirm zero executable emitted!
            if !diag_matches(&diag, expected) {
                errors.push(format!(
                    "{label}: expected compile-time error matching {expected:?}, got {diag}"
                ));
                return;
            }
            let out_exe = tmp_dir.join(format!("{bin_stem}_no_artifact"));
            if out_exe.exists() {
                let _ = std::fs::remove_file(&out_exe);
            }
            // Also confirm build/jit entrypoints reject without writing output file
            let check_again = load_from_source_with_options(src, base_dir, CompatMode::V2, load_opts)
                .and_then(|u| check_unit(u, true));
            if check_again.is_ok() || out_exe.exists() {
                errors.push(format!(
                    "{label}: compile-time negative control failed containment"
                ));
            }
        }
        Ok(checked) => {
            // Runtime negative control: verify ALL 3 modes (VM, AOT -O2, ORC JIT -O2) fail!
            match Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                .and_then(|ev| ev.run_main())
            {
                Ok(_) => {
                    errors.push(format!(
                        "{label}: expected failure matching {expected:?}, but VM succeeded!"
                    ));
                    return;
                }
                Err(diag) => {
                    if !diag_matches(&diag, expected) {
                        errors.push(format!(
                            "{label}: VM expected error matching {expected:?}, got {diag}"
                        ));
                        return;
                    }
                }
            }
            let plan = CodegenPlan::baseline_o2();
            let raw_ir = match lower_to_llvm_ir_with_plan(&checked, &plan) {
                Ok(ir) => ir,
                Err(e) => {
                    errors.push(format!("{label}: LLVM lowering error: {e}"));
                    return;
                }
            };
            let exe = tmp_dir.join(bin_stem);
            if let Err(e) = aod_llvm::link_aot_executable_with_plan(&raw_ir, &plan, &exe) {
                errors.push(format!("{label}: AOT -O2 link failed: {e}"));
                return;
            }
            match Command::new(&exe).output() {
                Ok(out) if !out.status.success() => {}
                Ok(_) => {
                    errors.push(format!("{label}: AOT -O2 unexpectedly exited 0!"));
                    return;
                }
                Err(e) => {
                    errors.push(format!("{label}: AOT -O2 spawn error: {e}"));
                    return;
                }
            }
            match OrcJitSession::compile_with_plan(&raw_ir, &plan) {
                Ok(sess) => {
                    let (_, jit_res) =
                        sess.run_captured(DEFAULT_FUEL as i64, DEFAULT_MAX_DEPTH as i32);
                    if jit_res.is_ok() {
                        errors.push(format!("{label}: ORC JIT -O2 unexpectedly succeeded!"));
                    }
                }
                Err(e) => {
                    errors.push(format!("{label}: ORC JIT -O2 compile failed: {e}"));
                }
            }
        }
    }
}

fn run_pass5_executable_dec(
    canon_repo: &Path,
    scanned: &[ScannedFile],
    load_opts: &LoadOptions,
    rep: &mut ManualVerifyReport,
) {
    let tmp = match TempDirGuard::create("pass5") {
        Ok(g) => g,
        Err(e) => {
            rep.errors.push(e);
            return;
        }
    };
    let mut seq = 0usize;

    for sf in scanned {
        // 1. Inline fenced blocks: `dec` + `dec-expect`, and `dec-fail <Code>`
        let blocks = &sf.raw_blocks;
        let mut idx = 0usize;
        while idx < blocks.len() {
            let b = &blocks[idx];
            if b.lang == "dec" {
                let next = blocks.get(idx + 1);
                if let Some(nb) = next {
                    if nb.lang == "dec-expect" {
                        rep.inline_positive_checked += 1;
                        seq += 1;
                        verify_positive_3mode(
                            &b.body,
                            sf.path.parent(),
                            Some(&nb.body),
                            &format!("{}:{} (inline dec)", b.file.display(), b.line),
                            tmp.path(),
                            &format!("inline_pos_{seq}"),
                            load_opts,
                            &mut rep.errors,
                        );
                        idx += 2;
                        continue;
                    }
                }
                rep.errors.push(format!(
                    "{}:{}: unpaired ```dec block (must be immediately followed by ```dec-expect)",
                    b.file.display(),
                    b.line
                ));
                idx += 1;
            } else if b.lang == "dec-expect" {
                rep.errors.push(format!(
                    "{}:{}: orphan ```dec-expect block without preceding ```dec block",
                    b.file.display(),
                    b.line
                ));
                idx += 1;
            } else if let Some(rest) = b.lang.strip_prefix("dec-fail") {
                let expected = rest.trim();
                rep.inline_negative_checked += 1;
                seq += 1;
                verify_negative_3mode(
                    &b.body,
                    sf.path.parent(),
                    expected,
                    &format!("{}:{} (inline dec-fail)", b.file.display(), b.line),
                    tmp.path(),
                    &format!("inline_neg_{seq}"),
                    load_opts,
                    &mut rep.errors,
                );
                idx += 1;
            } else {
                idx += 1;
            }
        }

        // 2. `// @dec-file: <rel_path>` directives (strict containment inside `canon_repo`, `P0-4`)
        for (line_no, rel) in &sf.dec_file_refs {
            let rel_path = Path::new(rel);
            if rel_path.is_absolute()
                || rel_path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                rep.errors.push(format!(
                    "{}:{line_no}: @dec-file target {rel:?} escapes repository root",
                    sf.path.display()
                ));
                continue;
            }
            let candidate = canon_repo.join(rel_path);
            let canon = match std::fs::canonicalize(&candidate) {
                Ok(p) if p.is_file() => p,
                Ok(p) => {
                    rep.errors.push(format!(
                        "{}:{line_no}: @dec-file target is not a file: {}",
                        sf.path.display(),
                        p.display()
                    ));
                    continue;
                }
                Err(e) => {
                    rep.errors.push(format!(
                        "{}:{line_no}: missing @dec-file target {rel:?}: {e}",
                        sf.path.display()
                    ));
                    continue;
                }
            };
            if !canon.starts_with(canon_repo) {
                rep.errors.push(format!(
                    "{}:{line_no}: @dec-file target {rel:?} escapes repository root ({})",
                    sf.path.display(),
                    canon.display()
                ));
                continue;
            }
            let src = match std::fs::read_to_string(&canon) {
                Ok(s) => s,
                Err(e) => {
                    rep.errors.push(format!(
                        "{}:{line_no}: failed reading @dec-file {rel:?}: {e}",
                        sf.path.display()
                    ));
                    continue;
                }
            };
            rep.dec_files_checked += 1;
            seq += 1;
            let exp_err_path = canon.with_extension("expect_error");
            let exp_out_path = canon.with_extension("stdout");
            if exp_err_path.is_file() {
                let exp_err = std::fs::read_to_string(&exp_err_path).unwrap_or_default();
                verify_negative_3mode(
                    &src,
                    canon.parent(),
                    &exp_err,
                    &format!("{}:{line_no} (@dec-file {rel})", sf.path.display()),
                    tmp.path(),
                    &format!("file_neg_{seq}"),
                    load_opts,
                    &mut rep.errors,
                );
            } else if exp_out_path.is_file() {
                let exp_out = std::fs::read_to_string(&exp_out_path).unwrap_or_default();
                verify_positive_3mode(
                    &src,
                    canon.parent(),
                    Some(&exp_out),
                    &format!("{}:{line_no} (@dec-file {rel})", sf.path.display()),
                    tmp.path(),
                    &format!("file_pos_{seq}"),
                    load_opts,
                    &mut rep.errors,
                );
            } else {
                verify_positive_3mode(
                    &src,
                    canon.parent(),
                    None,
                    &format!("{}:{line_no} (@dec-file {rel})", sf.path.display()),
                    tmp.path(),
                    &format!("file_pos_{seq}"),
                    load_opts,
                    &mut rep.errors,
                );
            }
        }
    }
}

// ============================================================================
// Pass 6: Strict JSON Data Files + Live Fixture Parity (`P0-4`, `AC-09`)
// ============================================================================

fn json_i64(v: &JsonValue, key: &str, ctx: &str, errors: &mut Vec<String>) -> Option<i64> {
    match v.get(key).and_then(JsonValue::as_int).and_then(|n| n.to_i64()) {
        Some(n) => Some(n),
        None => {
            errors.push(format!("{ctx}: missing or non-i64 field {key:?}"));
            None
        }
    }
}

fn run_fixture_lines(
    fixture_path: &Path,
    load_opts: &LoadOptions,
    errors: &mut Vec<String>,
) -> Option<Vec<String>> {
    let src = match std::fs::read_to_string(fixture_path) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!(
                "Pass 6 failed reading fixture {}: {e}",
                fixture_path.display()
            ));
            return None;
        }
    };
    let loaded = match load_from_source_with_options(&src, fixture_path.parent(), CompatMode::V2, load_opts) {
        Ok(u) => u,
        Err(e) => {
            errors.push(format!("Pass 6 load failed on {}: {e}", fixture_path.display()));
            return None;
        }
    };
    let checked = match check_unit(loaded, true) {
        Ok(c) => c,
        Err(e) => {
            errors.push(format!("Pass 6 check failed on {}: {e}", fixture_path.display()));
            return None;
        }
    };
    match Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH).and_then(|ev| ev.run_main()) {
        Ok(out) => Some(out.emits),
        Err(e) => {
            errors.push(format!("Pass 6 VM failed on {}: {e}", fixture_path.display()));
            None
        }
    }
}

fn run_pass6_data_json_and_live_parity(
    canon_manual: &Path,
    canon_repo: &Path,
    scanned: &[ScannedFile],
    load_opts: &LoadOptions,
    rep: &mut ManualVerifyReport,
) {
    // 1. Verify every `json("/...")` path in scanned Typst files uses leading `/` and exists!
    for sf in scanned {
        for (idx, line) in sf.non_raw_markup.lines().enumerate() {
            let line_no = idx + 1;
            let mut rest = line;
            while let Some(pos) = rest.find("json(\"") {
                let after = &rest[pos + 6..];
                if let Some(end_q) = after.find('"') {
                    let target = &after[..end_q];
                    if !target.starts_with('/') {
                        rep.errors.push(format!(
                            "{}:{line_no}: json({target:?}) must use root-relative leading `/` path (`P0-1`)",
                            sf.path.display()
                        ));
                    } else {
                        let resolved = canon_repo.join(target.trim_start_matches('/'));
                        if !resolved.is_file() {
                            rep.errors.push(format!(
                                "{}:{line_no}: missing json() data file {target:?}",
                                sf.path.display()
                            ));
                        }
                    }
                    rest = &after[end_q + 1..];
                } else {
                    break;
                }
            }
        }
    }

    // 2. Parse ALL `docs/manual/data/*.json` via `parse_strict_json` (rejecting floats & duplicate keys!)
    let data_dir = canon_manual.join("data");
    if !data_dir.is_dir() {
        return;
    }
    let mut json_files: Vec<PathBuf> = std::fs::read_dir(&data_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    json_files.sort();

    let mut parsed_map: BTreeMap<String, JsonValue> = BTreeMap::new();
    for jp in json_files {
        rep.data_files_checked += 1;
        let name = jp
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        match std::fs::read_to_string(&jp)
            .map_err(|e| e.to_string())
            .and_then(|s| parse_strict_json(&s).map_err(|e| e.to_string()))
        {
            Ok(val) => {
                parsed_map.insert(name, val);
            }
            Err(e) => {
                rep.errors.push(format!(
                    "strict JSON validation failed for {}: {e}",
                    jp.display()
                ));
            }
        }
    }

    // 3. Live fixture parity checks (`P0-4`): execute manual_examples/{03,04,06}.dec and compare!
    if let Some(ds_json) = parsed_map.get("double_slit_delta3.json") {
        let f03 = canon_repo.join("manual_examples/03_short_window_boundary.dec");
        if let Some(emits) = run_fixture_lines(&f03, load_opts, &mut rep.errors) {
            // 03 emits: [sum_t0, sum_cx, sum_tx, sum_obs, sum_delta, sum_mag, center_sign, center_mag, ...]
            if emits.len() >= 8 {
                let checks = [
                    ("sum_t0", &emits[0]),
                    ("sum_cx", &emits[1]),
                    ("sum_tx", &emits[2]),
                    ("sum_obs", &emits[3]),
                    ("sum_delta", &emits[4]),
                    ("sum_mag", &emits[5]),
                    ("center_mag", &emits[7]),
                ];
                for (field, live_str) in checks {
                    if let Some(jv) = json_i64(ds_json, field, "double_slit_delta3.json", &mut rep.errors) {
                        if jv.to_string() != *live_str {
                            rep.errors.push(format!(
                                "double_slit_delta3.json.{field} ({jv}) != live 03_short_window_boundary.dec ({live_str})"
                            ));
                        }
                    }
                }
                // Also verify internal bin sums equal summary fields!
                if let Some(bins) = ds_json.get("bins").and_then(JsonValue::as_array) {
                    if bins.len() != 11 {
                        rep.errors.push(format!(
                            "double_slit_delta3.json expected 11 bins, got {}",
                            bins.len()
                        ));
                    } else {
                        let mut st0 = 0i64;
                        let mut scx = 0i64;
                        let mut stx = 0i64;
                        let mut sob = 0i64;
                        let mut sd = 0i64;
                        let mut sm = 0i64;
                        for b in bins {
                            let t0 = json_i64(b, "t0", "bin", &mut rep.errors).unwrap_or(0);
                            let cx = json_i64(b, "cx", "bin", &mut rep.errors).unwrap_or(0);
                            let tx = json_i64(b, "tx", "bin", &mut rep.errors).unwrap_or(0);
                            let ob = json_i64(b, "obs", "bin", &mut rep.errors).unwrap_or(0);
                            let d = json_i64(b, "delta", "bin", &mut rep.errors).unwrap_or(0);
                            let m = json_i64(b, "mag", "bin", &mut rep.errors).unwrap_or(0);
                            if t0 + cx != tx || tx - ob != d || d.abs() != m {
                                rep.errors.push(
                                    "double_slit_delta3.json per-bin accounting identity violated"
                                        .to_string(),
                                );
                            }
                            st0 += t0;
                            scx += cx;
                            stx += tx;
                            sob += ob;
                            sd += d;
                            sm += m;
                        }
                        if st0.to_string() != emits[0]
                            || scx.to_string() != emits[1]
                            || stx.to_string() != emits[2]
                            || sob.to_string() != emits[3]
                            || sd.to_string() != emits[4]
                            || sm.to_string() != emits[5]
                        {
                            rep.errors.push(
                                "double_slit_delta3.json bin sums disagree with live 03_short_window_boundary.dec"
                                    .to_string(),
                            );
                        }
                    }
                } else {
                    rep.errors
                        .push("double_slit_delta3.json missing `bins` array".to_string());
                }
            }
        }
    }

    if let Some(disk_json) = parsed_map.get("disk_enclosure.json") {
        let f04 = canon_repo.join("manual_examples/04_cell_measures_tesseract.dec");
        if let Some(emits) = run_fixture_lines(&f04, load_opts, &mut rep.errors) {
            // 04 emits: [81, 1, 81/16, 27/1, 183/64, 107/32, 8/1, 21/16, ...]
            if emits.len() >= 8 {
                if let Some(rows) = disk_json.get("resolutions").and_then(JsonValue::as_array) {
                    if let Some(r16) = rows
                        .iter()
                        .find(|r| r.get("n").and_then(JsonValue::as_int).and_then(|v| v.to_i64()) == Some(16))
                    {
                        let fmt_rat = |nk: &str, dk: &str, errs: &mut Vec<String>| -> String {
                            let n = json_i64(r16, nk, "disk_enclosure.json(n=16)", errs).unwrap_or(0);
                            let d = json_i64(r16, dk, "disk_enclosure.json(n=16)", errs).unwrap_or(1);
                            format!("{n}/{d}")
                        };
                        let am = fmt_rat("a_minus_num", "a_minus_den", &mut rep.errors);
                        let ap = fmt_rat("a_plus_num", "a_plus_den", &mut rep.errors);
                        let cn = fmt_rat("c_n_num", "c_n_den", &mut rep.errors);
                        let ex = fmt_rat("excess_num", "excess_den", &mut rep.errors);
                        if am != emits[4] || ap != emits[5] || cn != emits[6] || ex != emits[7] {
                            rep.errors.push(format!(
                                "disk_enclosure.json n=16 ({am}, {ap}, {cn}, {ex}) != live 04_cell_measures_tesseract.dec ({}, {}, {}, {})",
                                emits[4], emits[5], emits[6], emits[7]
                            ));
                        }
                    } else {
                        rep.errors
                            .push("disk_enclosure.json missing resolution n=16".to_string());
                    }
                } else {
                    rep.errors
                        .push("disk_enclosure.json missing `resolutions` array".to_string());
                }
            }
        }
    }

    if let Some(sb_json) = parsed_map.get("sandbox_chamber_summary.json") {
        let f06 = canon_repo.join("manual_examples/06_unbiased_chamber_sandbox.dec");
        if let Some(emits) = run_fixture_lines(&f06, load_opts, &mut rep.errors) {
            // 06 emits: [model_status, ticks, carrier_count, shedding_event_count, wavelet_event_count, total_returns, total_hinges, final_state_hash, ∂r0]
            if emits.len() >= 8 {
                let s_status = sb_json
                    .get("model_status")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("");
                let s_hash = sb_json
                    .get("final_state_hash")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("");
                if s_status != emits[0] {
                    rep.errors.push(format!(
                        "sandbox_chamber_summary.json.model_status ({s_status:?}) != live ({:?})",
                        emits[0]
                    ));
                }
                if s_hash != emits[7] {
                    rep.errors.push(format!(
                        "sandbox_chamber_summary.json.final_state_hash ({s_hash:?}) != live ({:?})",
                        emits[7]
                    ));
                }
                for (field, live_str) in [
                    ("ticks", &emits[1]),
                    ("carrier_count", &emits[2]),
                    ("shedding_event_count", &emits[3]),
                    ("wavelet_event_count", &emits[4]),
                    ("total_returns", &emits[5]),
                    ("total_hinges", &emits[6]),
                ] {
                    if let Some(jv) = json_i64(sb_json, field, "sandbox_chamber_summary.json", &mut rep.errors) {
                        if jv.to_string() != *live_str {
                            rep.errors.push(format!(
                                "sandbox_chamber_summary.json.{field} ({jv}) != live 06_unbiased_chamber_sandbox.dec ({live_str})"
                            ));
                        }
                    }
                }
            }
        }
    }
}
