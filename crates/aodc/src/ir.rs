use std::collections::BTreeMap;

use aod_core::bigint::BigInt;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;

use crate::ast::{DecFnKind, Item};
use crate::checker::{check_unit, CheckedUnit};
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{load_hermetic, BundledModule};

pub const IR_FORMAT: &str = "aod.dec.ir";
pub const MAX_IR_BYTES: usize = 8_000_000;

/// Encode a checked unit into canonical deterministic `DecIR` (`version: 1` for `--compat v1`,
/// `version: 2` for Dec v2 bundling all resolved filesystem module sources + SHA-256 digests).
pub fn encode_ir_json(checked: &CheckedUnit) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert("format".to_string(), JsonValue::from_str_val(IR_FORMAT));
    root.insert(
        "source".to_string(),
        JsonValue::from_str_val(checked.unit.entry_source.clone()),
    );
    root.insert(
        "source_sha256".to_string(),
        JsonValue::from_str_val(checked.unit.entry_sha256.clone()),
    );

    match checked.unit.compat {
        CompatMode::V1 => {
            root.insert("version".to_string(), JsonValue::from_i64(1));
            let mut funcs = Vec::new();
            for item in &checked.unit.entry_program.items {
                if let Item::DecFn(f) = item {
                    let mut fobj = BTreeMap::new();
                    fobj.insert("line".to_string(), JsonValue::from_usize(f.line));
                    fobj.insert("name".to_string(), JsonValue::from_str_val(f.name.clone()));
                    if let DecFnKind::MultiArm {
                        header_params,
                        output_name,
                        arms,
                        ..
                    } = &f.kind
                    {
                        fobj.insert(
                            "output".to_string(),
                            JsonValue::from_str_val(output_name.clone().unwrap_or_default()),
                        );
                        fobj.insert(
                            "params".to_string(),
                            JsonValue::Array(
                                header_params
                                    .iter()
                                    .map(|p| JsonValue::from_str_val(p.clone()))
                                    .collect(),
                            ),
                        );
                        let branches = arms
                            .iter()
                            .map(|arm| {
                                let mut bobj = BTreeMap::new();
                                bobj.insert("line".to_string(), JsonValue::from_usize(arm.line));
                                bobj.insert(
                                    "output".to_string(),
                                    JsonValue::from_str_val(
                                        arm.output_name.clone().unwrap_or_default(),
                                    ),
                                );
                                bobj.insert(
                                    "params".to_string(),
                                    JsonValue::Array(
                                        arm.params
                                            .iter()
                                            .map(|p| {
                                                let mut pobj = BTreeMap::new();
                                                match p {
                                                    crate::ast::DecArmParam::TypedVar { ty, name } => {
                                                        pobj.insert(
                                                            "name".to_string(),
                                                            JsonValue::from_str_val(name.clone()),
                                                        );
                                                        pobj.insert(
                                                            "type".to_string(),
                                                            JsonValue::from_str_val(format!("{ty:?}")),
                                                        );
                                                    }
                                                    other => {
                                                        pobj.insert(
                                                            "pattern".to_string(),
                                                            JsonValue::from_str_val(format!("{other:?}")),
                                                        );
                                                    }
                                                }
                                                JsonValue::Object(pobj)
                                            })
                                            .collect(),
                                    ),
                                );
                                bobj.insert(
                                    "return_type".to_string(),
                                    JsonValue::from_str_val(format!("{:?}", arm.return_type)),
                                );
                                bobj.insert(
                                    "stmt_count".to_string(),
                                    JsonValue::from_usize(arm.body.len()),
                                );
                                JsonValue::Object(bobj)
                            })
                            .collect();
                        fobj.insert("branches".to_string(), JsonValue::Array(branches));
                    }
                    funcs.push(JsonValue::Object(fobj));
                }
            }
            root.insert("functions".to_string(), JsonValue::Array(funcs));
        }
        CompatMode::V2 => {
            root.insert("version".to_string(), JsonValue::from_i64(2));
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
        }
    }

    JsonValue::Object(root)
}

pub fn dumps_ir(checked: &CheckedUnit) -> String {
    encode_ir_json(checked).to_pretty_utf8_json()
}

/// Strictly parse and hermetically rebuild-verify a `DecIR` payload (`P1-2`).
///
/// Rejects:
/// - payloads exceeding 8 MB
/// - duplicate JSON keys or floating-point numbers
/// - unsupported format or version
/// - source or bundled module SHA-256 mismatch
/// - ANY structural/field diff against canonical hermetically rebuilt IR (zero ambient FS reads!).
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
    let version_int = obj
        .get("version")
        .and_then(JsonValue::as_int)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "IR version must be an exact integer",
            )
        })?;
    let compat = if *version_int == BigInt::from_i64(1) {
        CompatMode::V1
    } else if *version_int == BigInt::from_i64(2) {
        CompatMode::V2
    } else {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("unsupported Dec IR version {version_int}"),
        ));
    };

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
    if compat == CompatMode::V2 {
        let mods = obj
            .get("modules")
            .and_then(JsonValue::as_object)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    "DecIR v2 requires a modules object",
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
    }

    // Strict hermetic rebuild with ZERO ambient filesystem access!
    let loaded = load_hermetic(source, &bundled, compat)?;
    let rebuilt = check_unit(loaded, true)?;

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
