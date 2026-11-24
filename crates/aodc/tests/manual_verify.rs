use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::json::{parse_strict_json, JsonValue};
use aodc::{verify_manual_dir, ManualVerifyOptions};

static TEST_SEQ: AtomicU64 = AtomicU64::new(0);

fn aodc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aodc"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn temp_manual_dir(tag: &str) -> PathBuf {
    let seq = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "aodc_pr5_mv_{}_{}_{tag}",
        std::process::id(),
        seq
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_minimal_valid_manual(dir: &Path) {
    std::fs::create_dir_all(dir.join("chapters")).unwrap();
    std::fs::write(
        dir.join("refs.bib"),
        "@misc{aod-main-v41, author={Poyau, R.}, title={{AOD}}, year={2026}}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.typ"),
        r#"#import "/bindings/aod/aod_schema.typ": *
#show: aod-document.with(
  title: "Test Manual",
  bib-path: "/docs/manual/refs.bib",
  is-manual: true,
  main-registry-path: "/bindings/aod/main_registry.json",
  expected-main-digest: "a05a787d75633c95e09c3b15fa9900a270d4a4bb31418a2423c7e8121fac9289",
)
#include "chapters/ch1.typ"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("chapters/ch1.typ"),
        r#"#import "/bindings/aod/aod_schema.typ": *
= Intro <sec:intro>
We don't drop exact half-open windows $(0, 6]$ (#mainequation("main:eq:first-anchor"), #aod-cite("aod-main-v41")).
See #secref("sec:intro").

```dec
Dec main() -> int:
    emit 42
    return 0
```

```dec-expect
42
0
```
"#,
    )
    .unwrap();
}

fn default_opts() -> ManualVerifyOptions {
    let root = repo_root();
    ManualVerifyOptions {
        stdlib_dir: Some(root.join("stdlib")),
        registry_path: Some(root.join("bindings/aod/main_registry.json")),
    }
}

#[test]
fn ac04_ac05_live_docs_manual_verify_and_strict_json_cli() {
    let root = repo_root();
    let manual_dir = root.join("docs/manual");

    let rep = verify_manual_dir(&manual_dir, &default_opts());
    assert!(
        rep.is_ok(),
        "live docs/manual verification failed: {:?}",
        rep.errors
    );
    assert_eq!(rep.status, "PASS");
    assert!(rep.files_scanned >= 13);
    assert!(
        rep.main_refs_checked >= 30,
        "expected >= 30 Main refs, got {}",
        rep.main_refs_checked
    );
    assert!(rep.internal_labels >= 10);
    assert!(rep.internal_refs_checked >= 10);
    assert!(rep.citations_checked >= 8);
    assert!(
        rep.inline_positive_checked >= 5,
        "expected >= 5 inline positive blocks, got {}",
        rep.inline_positive_checked
    );
    assert!(
        rep.inline_negative_checked >= 4,
        "expected >= 4 inline negative blocks, got {}",
        rep.inline_negative_checked
    );
    assert_eq!(rep.dec_files_checked, 17);
    assert_eq!(rep.data_files_checked, 3);
    assert_eq!(rep.raster_violations, 0);
    assert_eq!(rep.aod_lint_violations, 0);

    // CLI `aodc manual-verify docs/manual --json` strict zero-float JSON check (`AC-05`)
    let cli = Command::new(aodc_bin())
        .args(["manual-verify", "--json"])
        .arg(&manual_dir)
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "CLI manual-verify --json failed: {}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let stdout = String::from_utf8_lossy(&cli.stdout);
    let parsed = parse_strict_json(&stdout).unwrap();
    assert_eq!(parsed.get("status").and_then(JsonValue::as_str), Some("PASS"));
    assert_eq!(
        parsed
            .get("raster_violations")
            .and_then(JsonValue::as_int)
            .and_then(|v| v.to_i64()),
        Some(0)
    );
    assert_eq!(
        parsed
            .get("aod_lint_violations")
            .and_then(JsonValue::as_int)
            .and_then(|v| v.to_i64()),
        Some(0)
    );
    assert_eq!(
        parsed
            .get("errors")
            .and_then(JsonValue::as_array)
            .map(<[JsonValue]>::len),
        Some(0)
    );
}

#[test]
fn ac06_hostile_inline_tamper_unexpected_pass_and_unpaired_dec() {
    let opts = default_opts();

    // 0. Sanity: minimal valid manual with apostrophe + half-open math `$(0, 6]$` passes!
    let base = temp_manual_dir("base_ok");
    write_minimal_valid_manual(&base);
    let ok_rep = verify_manual_dir(&base, &opts);
    assert!(ok_rep.is_ok(), "minimal manual failed: {:?}", ok_rep.errors);
    let _ = std::fs::remove_dir_all(&base);

    // 1. Tampered dec-expect output -> FAIL
    let d1 = temp_manual_dir("tamper_expect");
    write_minimal_valid_manual(&d1);
    let ch1 = d1.join("chapters/ch1.typ");
    let raw = std::fs::read_to_string(&ch1).unwrap();
    std::fs::write(&ch1, raw.replace("42\n0", "99\n0")).unwrap();
    let r1 = verify_manual_dir(&d1, &opts);
    assert!(!r1.is_ok());
    assert!(r1.errors.iter().any(|e| e.contains("mismatch")));
    let _ = std::fs::remove_dir_all(&d1);

    // 2. Inline `dec-fail` unexpectedly passing -> FAIL
    let d2 = temp_manual_dir("neg_passes");
    write_minimal_valid_manual(&d2);
    let ch2 = d2.join("chapters/ch1.typ");
    let mut s2 = std::fs::read_to_string(&ch2).unwrap();
    s2.push_str("\n```dec-fail AssertionFailed\nDec main() -> int:\n    return 0\n```\n");
    std::fs::write(&ch2, s2).unwrap();
    let r2 = verify_manual_dir(&d2, &opts);
    assert!(!r2.is_ok());
    assert!(r2.errors.iter().any(|e| e.contains("succeeded")));
    let _ = std::fs::remove_dir_all(&d2);

    // 3. Unpaired ```dec without ```dec-expect -> FAIL
    let d3 = temp_manual_dir("unpaired");
    write_minimal_valid_manual(&d3);
    let ch3 = d3.join("chapters/ch1.typ");
    let mut s3 = std::fs::read_to_string(&ch3).unwrap();
    s3.push_str("\n```dec\nDec main() -> int:\n    return 0\n```\n");
    std::fs::write(&ch3, s3).unwrap();
    let r3 = verify_manual_dir(&d3, &opts);
    assert!(!r3.is_ok());
    assert!(r3.errors.iter().any(|e| e.contains("unpaired")));
    let _ = std::fs::remove_dir_all(&d3);
}

#[test]
fn ac07_hostile_refs_digest_labels_cites_includes_and_containment() {
    let opts = default_opts();

    // 1. Missing / prefixless / wrong-kind Main refs + broken internal ref + missing cite + dup label
    let d = temp_manual_dir("bad_refs");
    write_minimal_valid_manual(&d);
    let ch = d.join("chapters/ch1.typ");
    let mut s = std::fs::read_to_string(&ch).unwrap();
    s.push_str(
        r#"
= Dup <sec:intro>
#mainequation("main:eq:does-not-exist")
#mainequation("eq:first-anchor")
#mainsection("main:eq:first-anchor")
#secref("sec:missing")
#aod-cite("no-such-bib-key")
"#,
    );
    std::fs::write(&ch, s).unwrap();
    let rep = verify_manual_dir(&d, &opts);
    assert!(!rep.is_ok());
    assert!(rep.errors.iter().any(|e| e.contains("duplicate internal label")));
    assert!(rep.errors.iter().any(|e| e.contains("unresolved Main reference")));
    assert!(rep.errors.iter().any(|e| e.contains("missing required `main:` prefix")));
    assert!(rep.errors.iter().any(|e| e.contains("wrong Main reference kind")));
    assert!(rep.errors.iter().any(|e| e.contains("unresolved internal reference")));
    assert!(rep.errors.iter().any(|e| e.contains("unresolved citation key")));
    let _ = std::fs::remove_dir_all(&d);

    // 2. Stale `expected-main-digest`
    let d_dig = temp_manual_dir("stale_digest");
    write_minimal_valid_manual(&d_dig);
    let main_p = d_dig.join("main.typ");
    let mraw = std::fs::read_to_string(&main_p).unwrap();
    std::fs::write(
        &main_p,
        mraw.replace(
            "a05a787d75633c95e09c3b15fa9900a270d4a4bb31418a2423c7e8121fac9289",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
    )
    .unwrap();
    let r_dig = verify_manual_dir(&d_dig, &opts);
    assert!(!r_dig.is_ok());
    assert!(r_dig.errors.iter().any(|e| e.contains("expected-main-digest")));
    let _ = std::fs::remove_dir_all(&d_dig);

    // 3. Cyclic `#include` + escaping `../` `#include` + escaping `@dec-file`
    let d_cyc = temp_manual_dir("cycle_escape");
    write_minimal_valid_manual(&d_cyc);
    std::fs::write(
        d_cyc.join("chapters/ch1.typ"),
        "#include \"../main.typ\"\n// @dec-file: ../../../../etc/hosts\n",
    )
    .unwrap();
    let r_cyc = verify_manual_dir(&d_cyc, &opts);
    assert!(!r_cyc.is_ok());
    assert!(r_cyc.errors.iter().any(|e| e.contains("cyclic")));
    assert!(r_cyc.errors.iter().any(|e| e.contains("escapes repository root")));
    let _ = std::fs::remove_dir_all(&d_cyc);
}

#[test]
fn ac08_ac09_ac10_hostile_rasters_aod_lint_flowchart_units_and_data_drift() {
    let opts = default_opts();

    // 1. All 4 aod-lint rules + image() + .png + @preview + bare flowchart int + unclosed $
    let d = temp_manual_dir("lint_and_raster");
    write_minimal_valid_manual(&d);
    let ch = d.join("chapters/ch1.typ");
    let mut s = std::fs::read_to_string(&ch).unwrap();
    s.push_str(
        r#"
#import "@preview/cetz:0.1.0"
#image("foo.png")
#aod-flowchart(((id: "a", x: 5, y: 2cm, w: 3cm, h: 1cm, body: [x]),), ())
Unanchored @sec:intro reference.
Please see sections 1-3 for context.
This follows by standard conservation law in prose.
A smooth manifold is forbidden even here.
"#,
    );
    std::fs::write(&ch, s).unwrap();
    let rep = verify_manual_dir(&d, &opts);
    assert!(!rep.is_ok());
    assert!(rep.raster_violations >= 2);
    assert!(
        rep.aod_lint_violations >= 4,
        "expected all 4 aod-lint rules to trigger, got {}: {:?}",
        rep.aod_lint_violations,
        rep.errors
    );
    assert!(rep.errors.iter().any(|e| e.contains("@preview/")));
    assert!(rep.errors.iter().any(|e| e.contains("Typst length unit")));
    let _ = std::fs::remove_dir_all(&d);

    // 2. Hostile JSON data float, duplicate key, and live-fixture drift (`AC-09`)
    let d_json = temp_manual_dir("bad_data_json");
    write_minimal_valid_manual(&d_json);
    std::fs::create_dir_all(d_json.join("data")).unwrap();
    std::fs::write(d_json.join("data/float_bad.json"), "{\"x\": 1.5}\n").unwrap();
    std::fs::write(d_json.join("data/dup_bad.json"), "{\"a\": 1, \"a\": 2}\n").unwrap();
    let real_ds = std::fs::read_to_string(repo_root().join("docs/manual/data/double_slit_delta3.json")).unwrap();
    std::fs::write(
        d_json.join("data/double_slit_delta3.json"),
        real_ds.replacen("\"sum_tx\": 448", "\"sum_tx\": 999", 1),
    )
    .unwrap();
    let r_json = verify_manual_dir(&d_json, &opts);
    assert!(!r_json.is_ok());
    assert!(r_json.errors.iter().any(|e| e.contains("float_bad.json")));
    assert!(r_json.errors.iter().any(|e| e.contains("dup_bad.json")));
    assert!(r_json.errors.iter().any(|e| e.contains("999")));
    let _ = std::fs::remove_dir_all(&d_json);
}
