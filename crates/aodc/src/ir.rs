use std::collections::BTreeMap;

use aod_core::bigint::BigInt;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;

use crate::ast::Item;
use crate::checker::{check_unit, CheckedUnit};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{load_hermetic, BundledModule};

pub const IR_FORMAT: &str = "aod.dec.ir";
pub const IR_SCHEMA: &str = "aod.dec.ir.v3";
pub const IR_VERSION: i64 = 3;
pub const IR_SYNTAX_DISCIPLINE: &str = "pure_wave_v2";
pub const MAX_IR_BYTES: usize = 8_000_000;

/// Encode a checked unit into canonical deterministic `DecIR` (`version: 3`),
/// bundling all resolved filesystem/stdlib module sources + SHA-256 digests.
pub fn encode_ir_json(checked: &CheckedUnit) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert("format".to_string(), JsonValue::from_str_val(IR_FORMAT));
    root.insert("schema".to_string(), JsonValue::from_str_val(IR_SCHEMA));
    root.insert("version".to_string(), JsonValue::from_i64(IR_VERSION));
    root.insert(
        "syntax_discipline".to_string(),
        JsonValue::from_str_val(IR_SYNTAX_DISCIPLINE),
    );
    root.insert(
        "source".to_string(),
        JsonValue::from_str_val(checked.unit.entry_source.clone()),
    );
    root.insert(
        "source_sha256".to_string(),
        JsonValue::from_str_val(checked.unit.entry_sha256.clone()),
    );

    let mut mods_map = BTreeMap::new();
    for (k, m) in &checked.unit.bundled_modules {
        let mut mobj = BTreeMap::new();
        mobj.insert("sha256".to_string(), JsonValue::from_str_val(m.sha256.clone()));
        mobj.insert("source".to_string(), JsonValue::from_str_val(m.source.clone()));
        mods_map.insert(k.clone(), JsonValue::Object(mobj));
    }
    root.insert("modules".to_string(), JsonValue::Object(mods_map));

    let mut decl_names = Vec::new();
    for item in &checked.unit.entry_program.items {
        let tag = match item {
            Item::Use(_) => "use".to_string(),
            Item::Const(c) => format!("const:{}", c.name),
            Item::Struct(s) => format!("struct:{}", s.name),
            Item::Enum(e) => format!("enum:{}", e.name),
            Item::DecFn(f) => format!("dec:{}", f.name),
            Item::Chamber(c) => format!("chamber:{}", c.name),
            Item::Prepare(p) => format!("prepare:{}", p.name),
            Item::Observe(o) => format!("observe:{}", o.name),
            Item::QuarantineMod(q) => format!("quarantine:{}", q.name),
        };
        decl_names.push(JsonValue::from_str_val(tag));
    }
    root.insert("declarations".to_string(), JsonValue::Array(decl_names));

    JsonValue::Object(root)
}

pub fn dumps_ir(checked: &CheckedUnit) -> String {
    encode_ir_json(checked).to_pretty_utf8_json()
}

/// Strictly parse and hermetically rebuild-verify a `DecIR` payload.
pub fn loads_ir(text: &str) -> DecResult<CheckedUnit> {
    if text.len() > MAX_IR_BYTES {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR exceeds the 8 MB core limit",
        ));
    }
    let parsed = parse_strict_json(text).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("invalid IR JSON: {e}"),
        )
    })?;
    let obj = parsed.as_object().ok_or_else(|| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR root must be a JSON object",
        )
    })?;

    if obj.get("format").and_then(JsonValue::as_str) != Some(IR_FORMAT) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "unsupported Dec IR format",
        ));
    }
    if obj.get("schema").and_then(JsonValue::as_str) != Some(IR_SCHEMA) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("DecIR v3 requires \"schema\": {IR_SCHEMA:?}"),
        ));
    }
    let version_int = obj
        .get("version")
        .and_then(JsonValue::as_int)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "IR version must be an exact integer",
            )
        })?;
    if *version_int != BigInt::from_i64(IR_VERSION) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("unsupported Dec IR version {version_int}; expected {IR_VERSION}"),
        ));
    }
    if obj.get("syntax_discipline").and_then(JsonValue::as_str) != Some(IR_SYNTAX_DISCIPLINE) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "DecIR v3 requires \"syntax_discipline\": \"pure_wave_v2\"",
        ));
    }

    let source = obj
        .get("source")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "IR source must be a string",
            )
        })?;
    let claimed_sha = obj
        .get("source_sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "IR source_sha256 must be a string",
            )
        })?;
    let actual_sha = Sha256::digest_hex(source.as_bytes());
    if claimed_sha != actual_sha {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR source_sha256 does not match embedded source",
        ));
    }

    let mut bundled = BTreeMap::new();
    let mods = obj
        .get("modules")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "DecIR requires a modules object",
            )
        })?;
    for (mod_name, mod_val) in mods {
        let mobj = mod_val.as_object().ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                format!("module entry {mod_name:?} must be a JSON object"),
            )
        })?;
        let m_src = mobj
            .get("source")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    format!("module {mod_name:?} missing source"),
                )
            })?;
        let m_sha = mobj
            .get("sha256")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    format!("module {mod_name:?} missing sha256"),
                )
            })?;
        if Sha256::digest_hex(m_src.as_bytes()) != m_sha {
            return Err(Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                format!("module {mod_name:?} SHA-256 mismatch"),
            ));
        }
        bundled.insert(
            mod_name.clone(),
            BundledModule {
                source: m_src.to_string(),
                sha256: m_sha.to_string(),
            },
        );
    }

    // Strict hermetic rebuild with ZERO ambient filesystem access!
    let loaded = load_hermetic(source, &bundled).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("IR hermetic load failed: {e}"),
        )
    })?;
    let rebuilt = check_unit(loaded, true).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("IR hermetic check failed: {e}"),
        )
    })?;

    // Canonical JSON equality prevents bool/int tampering, extra fields, or modified metadata.
    let expected_canonical = dumps_ir(&rebuilt);
    let parsed_canonical = parsed.to_pretty_utf8_json();
    if parsed_canonical != expected_canonical {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR payload or metadata differs from hermetically validated source",
        ));
    }

    Ok(rebuilt)
}
