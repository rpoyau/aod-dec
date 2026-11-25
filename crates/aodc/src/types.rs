use std::fmt;

use crate::ast::TypeExpr;
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Effect {
    PureNative,
    ReadOnlyObserver,
    ChamberMut,
    QExt,
    SysIo,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Type {
    Unit,
    Null,
    Int,
    Bool,
    Str,
    T1,
    B8,
    B64,
    Bytes,
    Q4,
    Ratio,
    Addr8,
    Support,
    Window,
    Range,
    Box3,
    Trace,
    Fusion,
    Coupling,
    Wavelet,
    ScopeMap,
    ShedWitness,
    RecipientOutcome,
    CadenceAssociation,
    ChamberView,
    SimResult,
    Struct { module: String, name: String },
    Enum { module: String, name: String },
    Array(Box<Type>, usize),
    List(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Tuple(Vec<Type>),
    ReadOnly(Box<Type>),
    QExt(Box<Type>),
    /// Internal wildcard for empty list/map literals unified by context annotations.
    Unknown,
}

impl Type {
    /// Balanced-ternary & exact rational arithmetic types (`t1`, `int`, `ratio`).
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::T1 | Type::Ratio)
    }

    /// Totally-ordered scalar types supporting `<`, `<=`, `>`, `>=`, `<=>`, `min`, `max`.
    pub fn is_ordered(&self) -> bool {
        matches!(
            self,
            Type::Int
                | Type::T1
                | Type::Ratio
                | Type::B8
                | Type::B64
                | Type::Q4
                | Type::Str
                | Type::Bytes
                | Type::Addr8
        )
    }

    /// Raw bit-container types supporting `&`, `|`, `^`, `~`, `<<`, `>>`.
    pub fn is_bitwise(&self) -> bool {
        matches!(self, Type::B8 | Type::B64 | Type::Q4)
    }

    pub fn is_qext(&self) -> bool {
        match self {
            Type::QExt(_) => true,
            Type::Array(inner, _) | Type::List(inner) | Type::ReadOnly(inner) => inner.is_qext(),
            Type::Map(k, v) => k.is_qext() || v.is_qext(),
            Type::Tuple(elems) => elems.iter().any(Type::is_qext),
            _ => false,
        }
    }

    /// Deterministic orderable key types supported in `map[K, V]`.
    pub fn is_map_key(&self) -> bool {
        match self {
            Type::Null
            | Type::Int
            | Type::Bool
            | Type::Str
            | Type::T1
            | Type::B8
            | Type::B64
            | Type::Bytes
            | Type::Q4
            | Type::Ratio
            | Type::Addr8
            | Type::Support
            | Type::Enum { .. } => true,
            Type::Array(inner, _) => inner.is_map_key(),
            Type::Tuple(elems) => elems.iter().all(Type::is_map_key),
            _ => false,
        }
    }

    pub fn is_read_only(&self) -> bool {
        matches!(self, Type::ReadOnly(_))
    }

    pub fn unwrap_read_only(&self) -> &Type {
        match self {
            Type::ReadOnly(inner) => inner.unwrap_read_only(),
            other => other,
        }
    }

    pub fn wrap_read_only(self) -> Type {
        match self {
            Type::ReadOnly(_) => self,
            other => Type::ReadOnly(Box::new(other)),
        }
    }

    pub fn resolve_with<F>(
        expr: &TypeExpr,
        line: usize,
        lookup_user_type: &F,
    ) -> DecResult<Type>
    where
        F: Fn(&str) -> Option<Type>,
    {
        let ty = match expr {
            TypeExpr::Unit => Type::Unit,
            TypeExpr::Named(name) => match name.as_str() {
                "null" => Type::Null,
                "int" => Type::Int,
                "bool" => Type::Bool,
                "str" => Type::Str,
                "t1" => Type::T1,
                "b8" => Type::B8,
                "b64" => Type::B64,
                "bytes" => Type::Bytes,
                "q4" => Type::Q4,
                "ratio" => Type::Ratio,
                "addr8" => Type::Addr8,
                "support" => Type::Support,
                "window" => Type::Window,
                "range" => Type::Range,
                "box" => Type::Box3,
                "trace" => Type::Trace,
                "fusion" => Type::Fusion,
                "coupling" => Type::Coupling,
                "wavelet" => Type::Wavelet,
                "scope_map" => Type::ScopeMap,
                "shed_witness" => Type::ShedWitness,
                "recipient_outcome" => Type::RecipientOutcome,
                "cadence_association" => Type::CadenceAssociation,
                "ChamberView" => Type::ChamberView,
                "SimResult" => Type::SimResult,
                "t32" => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "type 't32' is unified into 2-bit balanced-ternary 'int'; use 'int' (or 'b64' for raw 64-bit view)",
                    ));
                }
                "float" | "float32" | "float64" => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::FloatLiteralForbidden,
                        line,
                        format!("IEEE floating-point type '{name}' is forbidden in Dec; use exact 'ratio'"),
                    ));
                }
                "int8" | "int16" | "int32" | "int64" | "u8" | "u16" | "u32" | "u64" => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("binary integer width '{name}' is removed; use balanced-ternary 'int' or raw 'b8'/'b64'"),
                    ));
                }
                other => {
                    if let Some(user_ty) = lookup_user_type(other) {
                        user_ty
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("unknown type {other}"),
                        ));
                    }
                }
            },
            TypeExpr::Array(inner, len) => {
                if *len == 0 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "shaped array length [T; N] requires N >= 1",
                    ));
                }
                Type::Array(
                    Box::new(Self::resolve_with(inner, line, lookup_user_type)?),
                    *len,
                )
            }
            TypeExpr::List(inner) => {
                Type::List(Box::new(Self::resolve_with(inner, line, lookup_user_type)?))
            }
            TypeExpr::Map(k, v) => {
                let kt = Self::resolve_with(k, line, lookup_user_type)?;
                let vt = Self::resolve_with(v, line, lookup_user_type)?;
                Type::Map(Box::new(kt), Box::new(vt))
            }
            TypeExpr::Tuple(elems) => {
                let mut resolved = Vec::with_capacity(elems.len());
                for e in elems {
                    resolved.push(Self::resolve_with(e, line, lookup_user_type)?);
                }
                Type::Tuple(resolved)
            }
            TypeExpr::ReadOnly(inner) => {
                Type::ReadOnly(Box::new(Self::resolve_with(inner, line, lookup_user_type)?))
            }
            TypeExpr::QExt(inner) => {
                Type::QExt(Box::new(Self::resolve_with(inner, line, lookup_user_type)?))
            }
        };
        Ok(ty)
    }

    pub fn resolve(expr: &TypeExpr, line: usize) -> DecResult<Type> {
        Self::resolve_with(expr, line, &|_| None)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Unit => write!(f, "()"),
            Type::Null => write!(f, "null"),
            Type::Int => write!(f, "int"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::T1 => write!(f, "t1"),
            Type::B8 => write!(f, "b8"),
            Type::B64 => write!(f, "b64"),
            Type::Bytes => write!(f, "bytes"),
            Type::Q4 => write!(f, "q4"),
            Type::Ratio => write!(f, "ratio"),
            Type::Addr8 => write!(f, "addr8"),
            Type::Support => write!(f, "support"),
            Type::Window => write!(f, "window"),
            Type::Range => write!(f, "range"),
            Type::Box3 => write!(f, "box"),
            Type::Trace => write!(f, "trace"),
            Type::Fusion => write!(f, "fusion"),
            Type::Coupling => write!(f, "coupling"),
            Type::Wavelet => write!(f, "wavelet"),
            Type::ScopeMap => write!(f, "scope_map"),
            Type::ShedWitness => write!(f, "shed_witness"),
            Type::RecipientOutcome => write!(f, "recipient_outcome"),
            Type::CadenceAssociation => write!(f, "cadence_association"),
            Type::ChamberView => write!(f, "ChamberView"),
            Type::SimResult => write!(f, "SimResult"),
            Type::Struct { name, .. } => write!(f, "{name}"),
            Type::Enum { name, .. } => write!(f, "{name}"),
            Type::Array(t, n) => write!(f, "[{t}; {n}]"),
            Type::List(t) => write!(f, "[{t}]"),
            Type::Map(k, v) => write!(f, "map[{k}, {v}]"),
            Type::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{e}")?;
                }
                write!(f, ")")
            }
            Type::ReadOnly(t) => write!(f, "ReadOnly<{t}>"),
            Type::QExt(t) => write!(f, "QExt<{t}>"),
            Type::Unknown => write!(f, "_"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BuiltinSig {
    pub params: Vec<Type>,
    pub ret: Type,
    pub effect: Effect,
}

/// Builtin signature table covering Trit-Boxed `int`/`ratio`, `b8`/`b64`/`bytes`,
/// exact AOD/M3/support/ledger/witness primitives, quarantined `aod::qext`, and `aod::sys`.
pub fn lookup_builtin(canonical_name: &str) -> Vec<BuiltinSig> {
    use Effect::{PureNative, QExt, SysIo};
    let s = |params: Vec<Type>, ret: Type, effect: Effect| BuiltinSig {
        params,
        ret,
        effect,
    };

    match canonical_name {
        // --- Core constructors & exact conversions ---
        "t1" => vec![
            s(vec![Type::Int], Type::T1, PureNative),
            s(vec![Type::T1], Type::T1, PureNative),
        ],
        "q4" => vec![
            s(vec![Type::Int], Type::Q4, PureNative),
            s(vec![Type::Q4], Type::Q4, PureNative),
        ],
        "b8" => vec![
            s(vec![Type::Int], Type::B8, PureNative),
            s(vec![Type::B8], Type::B8, PureNative),
        ],
        "b64" => vec![
            s(vec![Type::Int], Type::B64, PureNative),
            s(vec![Type::B64], Type::B64, PureNative),
        ],
        "b64_bits" => vec![s(vec![Type::Int], Type::B64, PureNative)],
        "u64_int" => vec![s(vec![Type::B64], Type::Int, PureNative)],
        "int" => vec![
            s(vec![Type::Int], Type::Int, PureNative),
            s(vec![Type::T1], Type::Int, PureNative),
            s(vec![Type::Q4], Type::Int, PureNative),
            s(vec![Type::B8], Type::Int, PureNative),
            s(vec![Type::B64], Type::Int, PureNative),
            s(vec![Type::Ratio], Type::Int, PureNative),
        ],
        "ratio" => vec![
            s(vec![Type::Int, Type::Int], Type::Ratio, PureNative),
            s(vec![Type::Int], Type::Ratio, PureNative),
            s(vec![Type::T1], Type::Ratio, PureNative),
            s(vec![Type::Ratio], Type::Ratio, PureNative),
        ],
        "bytes" => vec![
            s(vec![Type::Bytes], Type::Bytes, PureNative),
            s(vec![Type::Str], Type::Bytes, PureNative),
            s(vec![Type::Addr8], Type::Bytes, PureNative),
            s(vec![Type::B64], Type::Bytes, PureNative),
            s(vec![Type::List(Box::new(Type::B8))], Type::Bytes, PureNative),
        ],
        "str_bytes" => vec![s(vec![Type::Bytes], Type::Str, PureNative)],
        "u16_le_bytes" | "u32_le_bytes" | "i32_le_bytes" | "i64_le_bytes" => {
            vec![s(vec![Type::Int], Type::Bytes, PureNative)]
        }
        "b64_le_bytes" => vec![s(vec![Type::B64], Type::Bytes, PureNative)],
        "push_byte" => vec![s(vec![Type::Bytes, Type::B8], Type::Bytes, PureNative)],
        "concat_bytes" => vec![s(vec![Type::Bytes, Type::Bytes], Type::Bytes, PureNative)],
        "slice_bytes" => vec![s(
            vec![Type::Bytes, Type::Int, Type::Int],
            Type::Bytes,
            PureNative,
        )],
        "sha256_bytes" => vec![s(vec![Type::Bytes], Type::Bytes, PureNative)],
        "hex_bytes" => vec![s(vec![Type::Bytes], Type::Str, PureNative)],
        "pack_ratio" => vec![s(vec![Type::Ratio], Type::B64, PureNative)],
        "unpack_ratio" => vec![s(vec![Type::B64], Type::Ratio, PureNative)],

        // --- Algebra, ternary, & bit queries ---
        "neg" => vec![
            s(vec![Type::Int], Type::Int, PureNative),
            s(vec![Type::T1], Type::T1, PureNative),
            s(vec![Type::Ratio], Type::Ratio, PureNative),
            s(
                vec![Type::QExt(Box::new(Type::Ratio))],
                Type::QExt(Box::new(Type::Ratio)),
                QExt,
            ),
        ],
        "abs" => vec![
            s(vec![Type::Int], Type::Int, PureNative),
            s(vec![Type::T1], Type::T1, PureNative),
            s(vec![Type::Ratio], Type::Ratio, PureNative),
            s(
                vec![Type::QExt(Box::new(Type::Ratio))],
                Type::QExt(Box::new(Type::Ratio)),
                QExt,
            ),
        ],
        "sgn" => vec![
            s(vec![Type::Int], Type::T1, PureNative),
            s(vec![Type::Ratio], Type::T1, PureNative),
            s(vec![Type::T1], Type::T1, PureNative),
        ],
        "min" | "max" => vec![
            s(vec![Type::Int, Type::Int], Type::Int, PureNative),
            s(vec![Type::T1, Type::T1], Type::T1, PureNative),
            s(vec![Type::Ratio, Type::Ratio], Type::Ratio, PureNative),
            s(vec![Type::B8, Type::B8], Type::B8, PureNative),
            s(vec![Type::B64, Type::B64], Type::B64, PureNative),
        ],
        "pow" => vec![
            s(vec![Type::Int, Type::Int], Type::Int, PureNative),
            s(vec![Type::Ratio, Type::Int], Type::Ratio, PureNative),
        ],
        "gcd" | "lcm" | "trit_shl" | "trit_shr" => {
            vec![s(vec![Type::Int, Type::Int], Type::Int, PureNative)]
        }
        "superpose_cancel" => vec![
            s(vec![Type::T1, Type::T1], Type::T1, PureNative),
            s(vec![Type::Int, Type::Int], Type::Int, PureNative),
        ],
        "popcnt" => vec![
            s(vec![Type::B64], Type::Int, PureNative),
            s(vec![Type::B8], Type::Int, PureNative),
            s(vec![Type::Q4], Type::Int, PureNative),
        ],
        "is_hamming1" => vec![s(vec![Type::Q4, Type::Q4], Type::Bool, PureNative)],
        "range_seq" => vec![s(
            vec![Type::Int, Type::Int],
            Type::List(Box::new(Type::Int)),
            PureNative,
        )],

        // --- Witness primitives ---
        "trace" => vec![s(
            vec![Type::Str, Type::Str, Type::Q4],
            Type::Trace,
            PureNative,
        )],
        "step" => vec![s(
            vec![Type::Trace, Type::Q4, Type::T1],
            Type::Trace,
            PureNative,
        )],
        "hinge" => vec![s(vec![Type::Trace], Type::Trace, PureNative)],
        "dwell" => vec![s(vec![Type::Trace], Type::Trace, PureNative)],
        "close" => vec![s(vec![Type::Trace], Type::Trace, PureNative)],
        "phases" => vec![s(vec![Type::Trace], Type::Int, PureNative)],
        "beats" => vec![s(vec![Type::Trace], Type::Int, PureNative)],
        "rate" => vec![s(vec![Type::Trace], Type::Ratio, PureNative)],
        "fold" => vec![s(
            vec![Type::Trace, Type::Trace, Type::Str, Type::Str],
            Type::Fusion,
            PureNative,
        )],
        "outer" => vec![s(vec![Type::Fusion, Type::Q4], Type::Trace, PureNative)],
        "fuse" => vec![s(vec![Type::Fusion, Type::Trace], Type::Trace, PureNative)],
        "coupling" => vec![s(
            vec![Type::Trace, Type::Trace, Type::Str],
            Type::Coupling,
            PureNative,
        )],
        "contact" => vec![s(
            vec![Type::Coupling, Type::Int, Type::Int],
            Type::Coupling,
            PureNative,
        )],
        "cadence" => vec![s(vec![Type::Coupling], Type::Ratio, PureNative)],
        "lag" => vec![s(vec![Type::Coupling], Type::Int, PureNative)],
        "drift" => vec![s(vec![Type::Coupling], Type::Int, PureNative)],
        "probability" => vec![s(
            vec![Type::Int, Type::Int, Type::Int, Type::T1],
            Type::Ratio,
            PureNative,
        )],

        // --- Exact AOD / support / ledger / M3 wavelet helpers ---
        "addr8" => vec![s(vec![Type::Str], Type::Addr8, PureNative)],
        "support" => vec![
            s(vec![Type::Str], Type::Support, PureNative),
            s(
                vec![Type::Int, Type::Int, Type::Int, Type::Bool],
                Type::Support,
                PureNative,
            ),
        ],
        "support_pi" => vec![s(vec![Type::Support], Type::Int, PureNative)],
        "support_rd" => vec![s(vec![Type::Support, Type::Int], Type::Int, PureNative)],
        "support_pressure" => vec![s(
            vec![Type::Support, Type::Int, Type::Int, Type::Int],
            Type::Int,
            PureNative,
        )],
        "support_overhang" => vec![s(
            vec![Type::Support, Type::Int, Type::Int, Type::Int],
            Type::Int,
            PureNative,
        )],
        "shedding_excess" => vec![s(vec![Type::Int, Type::Int], Type::Int, PureNative)],
        "sadar_promote" => vec![s(
            vec![Type::Int, Type::Int, Type::Ratio],
            Type::Ratio,
            PureNative,
        )],
        "boundary_of" => vec![
            s(vec![Type::Box3], Type::Str, PureNative),
            s(
                vec![Type::Window],
                Type::Tuple(vec![Type::Int, Type::Int]),
                PureNative,
            ),
            s(
                vec![Type::Range],
                Type::Tuple(vec![Type::Int, Type::Int]),
                PureNative,
            ),
            s(vec![Type::SimResult], Type::Str, PureNative),
        ],
        "len" => vec![
            s(vec![Type::Str], Type::Int, PureNative),
            s(vec![Type::Bytes], Type::Int, PureNative),
        ],
        "window_dedup_count" => vec![s(
            vec![
                Type::Str,
                Type::Str,
                Type::List(Box::new(Type::Int)),
                Type::List(Box::new(Type::Window)),
            ],
            Type::Int,
            PureNative,
        )],
        "native_cadence_ratio" => vec![s(
            vec![
                Type::Str,
                Type::Int,
                Type::Str,
                Type::Int,
                Type::Bool,
                Type::Int,
            ],
            Type::Ratio,
            PureNative,
        )],
        "single_contact_association" => vec![s(
            vec![Type::Str, Type::Str, Type::Str, Type::Window, Type::Int],
            Type::CadenceAssociation,
            PureNative,
        )],
        "recipient_eval_demo" => vec![s(
            vec![Type::Int, Type::Int, Type::Window],
            Type::RecipientOutcome,
            PureNative,
        )],

        // --- Strictly quarantined `aod::qext` builtins (NO unwrap to native!) ---
        "aod::qext::ext_decimal" => vec![s(
            vec![Type::Str],
            Type::QExt(Box::new(Type::Ratio)),
            QExt,
        )],
        "aod::qext::qext_from_ratio" => vec![s(
            vec![Type::Ratio],
            Type::QExt(Box::new(Type::Ratio)),
            QExt,
        )],
        "aod::qext::qext_add"
        | "aod::qext::qext_sub"
        | "aod::qext::qext_mul"
        | "aod::qext::qext_div" => vec![s(
            vec![
                Type::QExt(Box::new(Type::Ratio)),
                Type::QExt(Box::new(Type::Ratio)),
            ],
            Type::QExt(Box::new(Type::Ratio)),
            QExt,
        )],
        "aod::qext::qext_render" => vec![s(
            vec![Type::QExt(Box::new(Type::Ratio))],
            Type::QExt(Box::new(Type::Str)),
            QExt,
        )],
        "aod::qext::qext_round_half_even" => vec![s(
            vec![Type::QExt(Box::new(Type::Ratio)), Type::Int],
            Type::QExt(Box::new(Type::Ratio)),
            QExt,
        )],
        "aod::qext::qext_format_fixed" => vec![s(
            vec![Type::QExt(Box::new(Type::Ratio)), Type::Int],
            Type::QExt(Box::new(Type::Str)),
            QExt,
        )],

        // --- Explicit capability-gated `aod::sys` bootstrap I/O (`Effect::SysIo`) ---
        "aod::sys::cli_args" => vec![s(Vec::new(), Type::List(Box::new(Type::Bytes)), SysIo)],
        "aod::sys::read_file_bytes" => vec![s(vec![Type::Bytes], Type::Bytes, SysIo)],
        "aod::sys::write_executable_elf" => {
            vec![s(vec![Type::Bytes, Type::Bytes], Type::Int, SysIo)]
        }

        _ => Vec::new(),
    }
}

/// Core reserved builtin names that user `dec` functions may not shadow.
pub fn is_reserved_builtin_name(name: &str) -> bool {
    matches!(
        name,
        "t1" | "q4"
            | "b8"
            | "b64"
            | "int"
            | "ratio"
            | "bytes"
            | "mux"
            | "select"
            | "gate"
            | "walk"
            | "settle"
    )
}
