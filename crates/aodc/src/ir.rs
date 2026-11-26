use std::collections::BTreeMap;

use aod_core::bigint::BigInt;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::ns::{ContentHash, MountMode, MountPerm};
use aod_core::sha256::Sha256;

use crate::ast::Item;
use crate::checker::{check_unit, CheckedUnit};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{
    load_hermetic, load_hermetic_v4, BundledModule, BundledUnitRecord, NsMountRecord,
};

pub const IR_FORMAT: &str = "aod.dec.ir";
pub const IR_SCHEMA: &str = "aod.dec.ir.v3";
pub const IR_VERSION: i64 = 3;
pub const IR_SCHEMA_V4: &str = "aod.dec.ir.v4";
pub const IR_VERSION_V4: i64 = 4;
pub const IR_SYNTAX_DISCIPLINE: &str = "pure_wave_v2";
pub const MAX_IR_BYTES: usize = 8_000_000;

fn declaration_tags(checked: &CheckedUnit) -> Vec<JsonValue> {
    let mut decl_names = Vec::new();
    for item in &checked.unit.entry_program.items {
        let tag = match item {
            Item::Namespace(ns) => format!("namespace:{}", ns.name),
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
    decl_names
}

/// Encode a checked unit into canonical deterministic `DecIR`:
/// - Emits `aod.dec.ir.v3` (`version: 3`) on standard v3 programs without Plan 9 namespace extensions
///   (preserving byte-identical `DecIR v3` compatibility across all existing v3 suites).
/// - Emits `aod.dec.ir.v4` (`version: 4`) when Plan 9 `namespace` blocks, rooted `/path` imports,
///   isolated multi-version package bundles, or `mount dyn` capabilities are present.
pub fn encode_ir_json(checked: &CheckedUnit) -> JsonValue {
    if checked.unit.uses_plan9_ns {
        return encode_ir_v4_json(checked);
    }

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
    root.insert(
        "declarations".to_string(),
        JsonValue::Array(declaration_tags(checked)),
    );

    JsonValue::Object(root)
}

pub fn encode_ir_v4_json(checked: &CheckedUnit) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert("format".to_string(), JsonValue::from_str_val(IR_FORMAT));
    root.insert("schema".to_string(), JsonValue::from_str_val(IR_SCHEMA_V4));
    root.insert("version".to_string(), JsonValue::from_i64(IR_VERSION_V4));
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

    let mut units_map = BTreeMap::new();
    for (ukey, rec) in &checked.unit.bundled_units {
        let mut uobj = BTreeMap::new();
        uobj.insert(
            "provider_id".to_string(),
            JsonValue::from_str_val(rec.provider_id.to_hex()),
        );
        uobj.insert("sha256".to_string(), JsonValue::from_str_val(rec.sha256.clone()));
        uobj.insert("source".to_string(), JsonValue::from_str_val(rec.source.clone()));
        uobj.insert(
            "subpath".to_string(),
            JsonValue::from_str_val(rec.subpath.clone()),
        );
        units_map.insert(ukey.clone(), JsonValue::Object(uobj));
    }
    root.insert("units".to_string(), JsonValue::Object(units_map));

    let mut ns_tables_obj = BTreeMap::new();
    for (owner, rows) in &checked.unit.unit_ns_tables {
        let mut arr = Vec::with_capacity(rows.len());
        for r in rows {
            let mut m = BTreeMap::new();
            m.insert(
                "mode".to_string(),
                JsonValue::from_str_val(match r.mode {
                    MountMode::Static => "static",
                    MountMode::Dynamic => "dynamic",
                }),
            );
            m.insert(
                "perm".to_string(),
                JsonValue::from_str_val(match r.perm {
                    MountPerm::ReadOnly => "ro",
                    MountPerm::ReadWrite => "rw",
                }),
            );
            m.insert(
                "provider_id".to_string(),
                JsonValue::from_str_val(r.provider_id.to_hex()),
            );
            m.insert(
                "subpath_prefix".to_string(),
                JsonValue::from_str_val(r.subpath_prefix.clone()),
            );
            m.insert("target".to_string(), JsonValue::from_str_val(r.target.clone()));
            arr.push(JsonValue::Object(m));
        }
        ns_tables_obj.insert(owner.clone(), JsonValue::Array(arr));
    }
    root.insert(
        "namespace_tables".to_string(),
        JsonValue::Object(ns_tables_obj),
    );
    root.insert(
        "declarations".to_string(),
        JsonValue::Array(declaration_tags(checked)),
    );

    JsonValue::Object(root)
}

pub fn dumps_ir(checked: &CheckedUnit) -> String {
    encode_ir_json(checked).to_pretty_utf8_json()
}

pub fn dumps_ir_v4(checked: &CheckedUnit) -> String {
    encode_ir_v4_json(checked).to_pretty_utf8_json()
}

/// Strictly parse and hermetically rebuild-verify a `DecIR` (`v3` or `v4`) payload
/// with ZERO ambient filesystem or environment access.
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

    if *version_int == BigInt::from_i64(IR_VERSION_V4) {
        return loads_ir_v4(&parsed, obj);
    }

    if *version_int != BigInt::from_i64(IR_VERSION) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("unsupported Dec IR version {version_int}; expected {IR_VERSION} or {IR_VERSION_V4}"),
        ));
    }
    if obj.get("schema").and_then(JsonValue::as_str) != Some(IR_SCHEMA) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("DecIR v3 requires \"schema\": {IR_SCHEMA:?}"),
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

fn loads_ir_v4(
    parsed: &JsonValue,
    obj: &BTreeMap<String, JsonValue>,
) -> DecResult<CheckedUnit> {
    if obj.get("schema").and_then(JsonValue::as_str) != Some(IR_SCHEMA_V4) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("DecIR v4 requires \"schema\": {IR_SCHEMA_V4:?}"),
        ));
    }
    if obj.get("syntax_discipline").and_then(JsonValue::as_str) != Some(IR_SYNTAX_DISCIPLINE) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "DecIR v4 requires \"syntax_discipline\": \"pure_wave_v2\"",
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
    if Sha256::digest_hex(source.as_bytes()) != claimed_sha {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR source_sha256 does not match embedded source",
        ));
    }

    let units_obj = obj
        .get("units")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "DecIR v4 requires a units object",
            )
        })?;
    let mut bundled_units = BTreeMap::new();
    for (ukey, uval) in units_obj {
        let uo = uval.as_object().ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                format!("unit {ukey:?} must be an object"),
            )
        })?;
        let pid_hex = uo
            .get("provider_id")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    format!("unit {ukey:?} missing provider_id"),
                )
            })?;
        let pid = ContentHash::from_hex(pid_hex).map_err(|e| {
            Diagnostic::new(DiagnosticCode::IrVerificationError, e.to_string())
        })?;
        let subpath = uo
            .get("subpath")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    format!("unit {ukey:?} missing subpath"),
                )
            })?;
        let u_src = uo
            .get("source")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    format!("unit {ukey:?} missing source"),
                )
            })?;
        let u_sha = uo
            .get("sha256")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    format!("unit {ukey:?} missing sha256"),
                )
            })?;
        bundled_units.insert(
            ukey.clone(),
            BundledUnitRecord {
                unit_key: ukey.clone(),
                provider_id: pid,
                subpath: subpath.to_string(),
                source: u_src.to_string(),
                sha256: u_sha.to_string(),
            },
        );
    }

    let tables_obj = obj
        .get("namespace_tables")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "DecIR v4 requires namespace_tables object",
            )
        })?;
    let mut ns_tables = BTreeMap::new();
    for (owner, arr_val) in tables_obj {
        let arr = arr_val.as_array().ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                format!("namespace_tables[{owner:?}] must be an array"),
            )
        })?;
        let mut rows = Vec::with_capacity(arr.len());
        for item in arr {
            let ro = item.as_object().ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    "namespace_table row must be an object",
                )
            })?;
            let mode = match ro.get("mode").and_then(JsonValue::as_str) {
                Some("static") => MountMode::Static,
                Some("dynamic") => MountMode::Dynamic,
                _ => {
                    return Err(Diagnostic::new(
                        DiagnosticCode::IrVerificationError,
                        "invalid mount mode in DecIR v4",
                    ));
                }
            };
            let perm = match ro.get("perm").and_then(JsonValue::as_str) {
                Some("ro") => MountPerm::ReadOnly,
                Some("rw") => MountPerm::ReadWrite,
                _ => {
                    return Err(Diagnostic::new(
                        DiagnosticCode::IrVerificationError,
                        "invalid mount perm in DecIR v4",
                    ));
                }
            };
            let pid_hex = ro
                .get("provider_id")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticCode::IrVerificationError,
                        "missing provider_id in namespace_table row",
                    )
                })?;
            let pid = ContentHash::from_hex(pid_hex).map_err(|e| {
                Diagnostic::new(DiagnosticCode::IrVerificationError, e.to_string())
            })?;
            let sub = ro
                .get("subpath_prefix")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticCode::IrVerificationError,
                        "missing subpath_prefix in namespace_table row",
                    )
                })?;
            let target = ro
                .get("target")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticCode::IrVerificationError,
                        "missing target in namespace_table row",
                    )
                })?;
            rows.push(NsMountRecord {
                target: target.to_string(),
                mode,
                perm,
                provider_id: pid,
                subpath_prefix: sub.to_string(),
            });
        }
        ns_tables.insert(owner.clone(), rows);
    }

    let loaded = load_hermetic_v4(source, &bundled_units, &ns_tables).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("IR v4 hermetic load failed: {e}"),
        )
    })?;
    let rebuilt = check_unit(loaded, true).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("IR v4 hermetic check failed: {e}"),
        )
    })?;

    let expected_canonical = dumps_ir_v4(&rebuilt);
    let parsed_canonical = parsed.to_pretty_utf8_json();
    if parsed_canonical != expected_canonical {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR v4 payload or metadata differs from hermetically validated source",
        ));
    }

    Ok(rebuilt)
}
