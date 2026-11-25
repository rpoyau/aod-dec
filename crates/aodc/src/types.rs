use std::fmt;

use crate::ast::TypeExpr;
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode, SyntaxDiscipline};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Effect {
    PureNative,
    ReadOnlyObserver,
    ChamberMut,
    QExt,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Type {
    Unit,
    Null,
    Int,
    U8,
    B64,
    Bool,
    Str,
    Bytes,
    T1,
    T9,
    T32,
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
    List(Box<Type>),
    Array(Box<Type>, usize),
    Map(Box<Type>, Box<Type>),
    Tuple(Vec<Type>),
    ReadOnly(Box<Type>),
    QExt(Box<Type>),
    /// Internal wildcard for empty list/map literals unified by context annotations.
    Unknown,
}

impl Type {
    pub fn is_v1_type(&self) -> bool {
        matches!(
            self,
            Type::Int
                | Type::Bool
                | Type::Str
                | Type::T1
                | Type::T32
                | Type::Q4
                | Type::Ratio
                | Type::Trace
                | Type::Fusion
                | Type::Coupling
        )
    }

    pub fn is_numeric_v1(&self) -> bool {
        matches!(self, Type::Int | Type::T1 | Type::T32 | Type::Ratio)
    }

    pub fn is_qext(&self) -> bool {
        match self {
            Type::QExt(_) => true,
            Type::List(inner) | Type::Array(inner, _) | Type::ReadOnly(inner) => inner.is_qext(),
            Type::Map(k, v) => k.is_qext() || v.is_qext(),
            Type::Tuple(elems) => elems.iter().any(Type::is_qext),
            _ => false,
        }
    }

    /// True if `str` appears anywhere inside this type (`PureWaveV2` string ban).
    pub fn contains_str(&self) -> bool {
        match self {
            Type::Str => true,
            Type::List(inner)
            | Type::Array(inner, _)
            | Type::ReadOnly(inner)
            | Type::QExt(inner) => inner.contains_str(),
            Type::Map(k, v) => k.contains_str() || v.contains_str(),
            Type::Tuple(elems) => elems.iter().any(Type::contains_str),
            _ => false,
        }
    }

    /// Deterministic orderable key types supported in `map[K, V]`.
    pub fn is_map_key(&self) -> bool {
        match self {
            Type::Null
            | Type::Int
            | Type::U8
            | Type::B64
            | Type::Bool
            | Type::Str
            | Type::Bytes
            | Type::T1
            | Type::T9
            | Type::T32
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

    /// Composite/container types that propagate `ReadOnly<...>` when accessed through a read-only view.
    pub fn needs_readonly_propagation(&self) -> bool {
        matches!(
            self,
            Type::List(_)
                | Type::Array(_, _)
                | Type::Map(_, _)
                | Type::Struct { .. }
                | Type::Enum { .. }
                | Type::Tuple(_)
                | Type::ChamberView
                | Type::SimResult
        )
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

    pub fn resolve_with_discipline<F>(
        expr: &TypeExpr,
        compat: CompatMode,
        discipline: SyntaxDiscipline,
        line: usize,
        lookup_user_type: &F,
    ) -> DecResult<Type>
    where
        F: Fn(&str) -> Option<Type>,
    {
        let ty = Self::resolve_with(expr, compat, line, lookup_user_type)?;
        if discipline == SyntaxDiscipline::PureWaveV2 && ty.contains_str() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("UTF-8 `str` type is forbidden in --strict-wave (got {ty}); use `bytes`"),
            ));
        }
        Ok(ty)
    }

    pub fn resolve_with<F>(
        expr: &TypeExpr,
        compat: CompatMode,
        line: usize,
        lookup_user_type: &F,
    ) -> DecResult<Type>
    where
        F: Fn(&str) -> Option<Type>,
    {
        let ty = match expr {
            TypeExpr::Unit => Type::Unit,
            TypeExpr::Named(name) => match name.as_str() {
                "Null" => Type::Null,
                "int" => Type::Int,
                "u8" => Type::U8,
                "b64" => Type::B64,
                "bool" => Type::Bool,
                "str" => Type::Str,
                "bytes" => Type::Bytes,
                "t1" => Type::T1,
                "t9" => Type::T9,
                "t32" => Type::T32,
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
            TypeExpr::List(inner) => Type::List(Box::new(Self::resolve_with(
                inner,
                compat,
                line,
                lookup_user_type,
            )?)),
            TypeExpr::Array(inner, n) => {
                if !(1..=65_536).contains(n) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("array[T, N] length N must satisfy 1 <= N <= 65536, got {n}"),
                    ));
                }
                let elem_ty = Self::resolve_with(inner, compat, line, lookup_user_type)?;
                if elem_ty == Type::Unit {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "array element type cannot be unit ()",
                    ));
                }
                Type::Array(Box::new(elem_ty), *n)
            }
            TypeExpr::Map(k, v) => {
                let kt = Self::resolve_with(k, compat, line, lookup_user_type)?;
                let vt = Self::resolve_with(v, compat, line, lookup_user_type)?;
                Type::Map(Box::new(kt), Box::new(vt))
            }
            TypeExpr::Tuple(elems) => {
                let mut resolved = Vec::with_capacity(elems.len());
                for e in elems {
                    resolved.push(Self::resolve_with(e, compat, line, lookup_user_type)?);
                }
                Type::Tuple(resolved)
            }
            TypeExpr::ReadOnly(inner) => Type::ReadOnly(Box::new(Self::resolve_with(
                inner,
                compat,
                line,
                lookup_user_type,
            )?)),
            TypeExpr::QExt(inner) => Type::QExt(Box::new(Self::resolve_with(
                inner,
                compat,
                line,
                lookup_user_type,
            )?)),
        };

        if compat == CompatMode::V1 && !ty.is_v1_type() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("unknown type {ty} in --compat v1"),
            ));
        }
        Ok(ty)
    }

    pub fn resolve(expr: &TypeExpr, compat: CompatMode, line: usize) -> DecResult<Type> {
        Self::resolve_with(expr, compat, line, &|_| None)
    }

    pub fn v1_name(&self) -> Option<&'static str> {
        match self {
            Type::Int => Some("int"),
            Type::Bool => Some("bool"),
            Type::Str => Some("str"),
            Type::T1 => Some("t1"),
            Type::T32 => Some("t32"),
            Type::Q4 => Some("q4"),
            Type::Ratio => Some("ratio"),
            Type::Trace => Some("trace"),
            Type::Fusion => Some("fusion"),
            Type::Coupling => Some("coupling"),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Unit => write!(f, "()"),
            Type::Null => write!(f, "Null"),
            Type::Int => write!(f, "int"),
            Type::U8 => write!(f, "u8"),
            Type::B64 => write!(f, "b64"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::Bytes => write!(f, "bytes"),
            Type::T1 => write!(f, "t1"),
            Type::T9 => write!(f, "t9"),
            Type::T32 => write!(f, "t32"),
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
            Type::List(t) => write!(f, "[{t}]"),
            Type::Array(t, n) => write!(f, "array[{t}, {n}]"),
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
    pub v1_allowed: bool,
}

/// Builtin signature table covering all 22 v1 builtins, v2 exact AOD/M3/support/ledger builtins,
/// and strictly quarantined `aod::qext` builtins (`P0-1`).
pub fn lookup_builtin(canonical_name: &str) -> Vec<BuiltinSig> {
    use Effect::{PureNative, QExt};
    let s = |params: Vec<Type>, ret: Type, effect: Effect, v1_allowed: bool| BuiltinSig {
        params,
        ret,
        effect,
        v1_allowed,
    };

    match canonical_name {
        // --- v1 & v2 core constructors / conversions ---
        "t1" => vec![
            s(vec![Type::Int], Type::T1, PureNative, true),
            s(vec![Type::T1], Type::T1, PureNative, false),
        ],
        "t9" => vec![
            s(vec![Type::Int], Type::T9, PureNative, false),
            s(vec![Type::T1], Type::T9, PureNative, false),
            s(vec![Type::U8], Type::T9, PureNative, false),
            s(vec![Type::T9], Type::T9, PureNative, false),
            s(vec![Type::T32], Type::T9, PureNative, false),
        ],
        "t32" => vec![
            s(vec![Type::Int], Type::T32, PureNative, true),
            s(vec![Type::T1], Type::T32, PureNative, false),
            s(vec![Type::U8], Type::T32, PureNative, false),
            s(vec![Type::T9], Type::T32, PureNative, false),
            s(vec![Type::T32], Type::T32, PureNative, false),
            s(vec![Type::B64], Type::T32, PureNative, false),
        ],
        "q4" => vec![
            s(vec![Type::Int], Type::Q4, PureNative, true),
            s(vec![Type::U8], Type::Q4, PureNative, false),
            s(vec![Type::Q4], Type::Q4, PureNative, false),
        ],
        "u8" => vec![
            s(vec![Type::Int], Type::U8, PureNative, false),
            s(vec![Type::U8], Type::U8, PureNative, false),
            s(vec![Type::Bool], Type::U8, PureNative, false),
            s(vec![Type::Q4], Type::U8, PureNative, false),
            s(vec![Type::T9], Type::U8, PureNative, false),
            s(vec![Type::T32], Type::U8, PureNative, false),
            s(vec![Type::B64], Type::U8, PureNative, false),
        ],
        "b64" => vec![
            s(vec![Type::Int], Type::B64, PureNative, false),
            s(vec![Type::U8], Type::B64, PureNative, false),
            s(vec![Type::Q4], Type::B64, PureNative, false),
            s(vec![Type::B64], Type::B64, PureNative, false),
            s(vec![Type::T32], Type::B64, PureNative, false),
            s(vec![Type::T9], Type::B64, PureNative, false),
        ],
        "int" => vec![
            s(vec![Type::Int], Type::Int, PureNative, true),
            s(vec![Type::T1], Type::Int, PureNative, true),
            s(vec![Type::T32], Type::Int, PureNative, true),
            s(vec![Type::Q4], Type::Int, PureNative, true),
            s(vec![Type::Ratio], Type::Int, PureNative, true),
            s(vec![Type::U8], Type::Int, PureNative, false),
            s(vec![Type::B64], Type::Int, PureNative, false),
            s(vec![Type::T9], Type::Int, PureNative, false),
        ],
        "ratio" => vec![
            s(vec![Type::Int, Type::Int], Type::Ratio, PureNative, true),
            s(vec![Type::Int], Type::Ratio, PureNative, true),
        ],
        "pack" => vec![
            s(vec![Type::T32], Type::Int, PureNative, true),
            s(vec![Type::T9], Type::Int, PureNative, false),
        ],
        "unpack" => vec![s(vec![Type::Int], Type::T32, PureNative, true)],
        "unpack_t9" => vec![
            s(vec![Type::Int], Type::T9, PureNative, false),
            s(vec![Type::B64], Type::T9, PureNative, false),
        ],
        "neg" => vec![
            s(vec![Type::Int], Type::Int, PureNative, true),
            s(vec![Type::T1], Type::T1, PureNative, true),
            s(vec![Type::T32], Type::T32, PureNative, true),
            s(vec![Type::Ratio], Type::Ratio, PureNative, true),
            s(vec![Type::T9], Type::T9, PureNative, false),
        ],
        "bitnot" => vec![s(vec![Type::B64], Type::B64, PureNative, false)],
        "popcnt" => vec![s(vec![Type::B64], Type::Int, PureNative, false)],
        "is_hamming1" => vec![s(
            vec![Type::Q4, Type::Q4],
            Type::Bool,
            PureNative,
            false,
        )],
        "popcnt_xor" => vec![s(
            vec![Type::Q4, Type::Q4],
            Type::Int,
            PureNative,
            false,
        )],
        "superpose_cancel" => vec![
            s(vec![Type::T1, Type::T1], Type::T1, PureNative, false),
            s(vec![Type::T32, Type::T32], Type::T32, PureNative, false),
        ],
        "bytes" => vec![
            s(
                vec![Type::List(Box::new(Type::U8))],
                Type::Bytes,
                PureNative,
                false,
            ),
            s(vec![Type::Str], Type::Bytes, PureNative, false),
            s(vec![Type::Bytes], Type::Bytes, PureNative, false),
            s(vec![Type::B64], Type::Bytes, PureNative, false),
            s(vec![Type::Addr8], Type::Bytes, PureNative, false),
        ],
        "slice_bytes" => vec![s(
            vec![Type::Bytes, Type::Int, Type::Int],
            Type::Bytes,
            PureNative,
            false,
        )],
        "concat_bytes" => vec![s(
            vec![Type::Bytes, Type::Bytes],
            Type::Bytes,
            PureNative,
            false,
        )],
        "sha256_bytes" => vec![s(vec![Type::Bytes], Type::Bytes, PureNative, false)],
        "hex_bytes" => vec![s(vec![Type::Bytes], Type::Bytes, PureNative, false)],
        "verify_sheddic_route" | "audit_sheddic_route" => vec![s(
            vec![Type::Int, Type::U8],
            Type::Bool,
            PureNative,
            false,
        )],
        "verify_frg_cross_return" => vec![s(
            vec![Type::Int, Type::Int, Type::Int],
            Type::Bool,
            PureNative,
            false,
        )],

        // --- v1 & v2 witness primitives (including Phase D2 `bytes` overloads) ---
        "trace" => vec![
            s(
                vec![Type::Str, Type::Str, Type::Q4],
                Type::Trace,
                PureNative,
                true,
            ),
            s(
                vec![Type::Bytes, Type::Bytes, Type::Q4],
                Type::Trace,
                PureNative,
                false,
            ),
        ],
        "step" => vec![s(
            vec![Type::Trace, Type::Q4, Type::T1],
            Type::Trace,
            PureNative,
            true,
        )],
        "hinge" => vec![s(vec![Type::Trace], Type::Trace, PureNative, true)],
        "dwell" => vec![s(vec![Type::Trace], Type::Trace, PureNative, true)],
        "close" => vec![s(vec![Type::Trace], Type::Trace, PureNative, true)],
        "phases" => vec![s(vec![Type::Trace], Type::Int, PureNative, true)],
        "beats" => vec![s(vec![Type::Trace], Type::Int, PureNative, true)],
        "rate" => vec![s(vec![Type::Trace], Type::Ratio, PureNative, true)],
        "fold" => vec![
            s(
                vec![Type::Trace, Type::Trace, Type::Str, Type::Str],
                Type::Fusion,
                PureNative,
                true,
            ),
            s(
                vec![Type::Trace, Type::Trace, Type::Bytes, Type::Bytes],
                Type::Fusion,
                PureNative,
                false,
            ),
        ],
        "outer" => vec![s(
            vec![Type::Fusion, Type::Q4],
            Type::Trace,
            PureNative,
            true,
        )],
        "fuse" => vec![s(
            vec![Type::Fusion, Type::Trace],
            Type::Trace,
            PureNative,
            true,
        )],
        "coupling" => vec![
            s(
                vec![Type::Trace, Type::Trace, Type::Str],
                Type::Coupling,
                PureNative,
                true,
            ),
            s(
                vec![Type::Trace, Type::Trace, Type::Bytes],
                Type::Coupling,
                PureNative,
                false,
            ),
        ],
        "contact" => vec![s(
            vec![Type::Coupling, Type::Int, Type::Int],
            Type::Coupling,
            PureNative,
            true,
        )],
        "cadence" => vec![s(vec![Type::Coupling], Type::Ratio, PureNative, true)],
        "lag" => vec![s(vec![Type::Coupling], Type::Int, PureNative, true)],
        "drift" => vec![s(vec![Type::Coupling], Type::Int, PureNative, true)],
        "probability" => vec![s(
            vec![Type::Int, Type::Int, Type::Int, Type::T1],
            Type::Ratio,
            PureNative,
            true,
        )],

        // --- v2 exact AOD / support / ledger / M3 wavelet helpers ---
        "addr8" => vec![
            s(vec![Type::Str], Type::Addr8, PureNative, false),
            s(vec![Type::Bytes], Type::Addr8, PureNative, false),
        ],
        "support" => vec![
            s(vec![Type::Str], Type::Support, PureNative, false),
            s(vec![Type::Bytes], Type::Support, PureNative, false),
            s(
                vec![Type::Int, Type::Int, Type::Int, Type::Bool],
                Type::Support,
                PureNative,
                false,
            ),
        ],
        "support_pi" => vec![s(vec![Type::Support], Type::Int, PureNative, false)],
        "support_rd" => vec![s(
            vec![Type::Support, Type::Int],
            Type::Int,
            PureNative,
            false,
        )],
        "support_pressure" => vec![s(
            vec![Type::Support, Type::Int, Type::Int, Type::Int],
            Type::Int,
            PureNative,
            false,
        )],
        "support_overhang" => vec![s(
            vec![Type::Support, Type::Int, Type::Int, Type::Int],
            Type::Int,
            PureNative,
            false,
        )],
        "shedding_excess" => vec![s(
            vec![Type::Int, Type::Int],
            Type::Int,
            PureNative,
            false,
        )],
        "sadar_promote" => vec![s(
            vec![Type::Int, Type::Int, Type::Ratio],
            Type::Ratio,
            PureNative,
            false,
        )],
        "boundary_of" => vec![
            s(vec![Type::Box3], Type::Str, PureNative, false),
            s(
                vec![Type::Window],
                Type::Tuple(vec![Type::Int, Type::Int]),
                PureNative,
                false,
            ),
            s(
                vec![Type::Range],
                Type::Tuple(vec![Type::Int, Type::Int]),
                PureNative,
                false,
            ),
            s(vec![Type::SimResult], Type::Str, PureNative, false),
        ],
        "len" => vec![s(vec![Type::Str], Type::Int, PureNative, false)],
        "window_dedup_count" => vec![
            s(
                vec![
                    Type::Str,
                    Type::Str,
                    Type::List(Box::new(Type::Int)),
                    Type::List(Box::new(Type::Window)),
                ],
                Type::Int,
                PureNative,
                false,
            ),
            s(
                vec![
                    Type::Bytes,
                    Type::Bytes,
                    Type::List(Box::new(Type::Int)),
                    Type::List(Box::new(Type::Window)),
                ],
                Type::Int,
                PureNative,
                false,
            ),
        ],
        "native_cadence_ratio" => vec![
            s(
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
                false,
            ),
            s(
                vec![
                    Type::Bytes,
                    Type::Int,
                    Type::Bytes,
                    Type::Int,
                    Type::Bool,
                    Type::Int,
                ],
                Type::Ratio,
                PureNative,
                false,
            ),
        ],
        "single_contact_association" => vec![
            s(
                vec![Type::Str, Type::Str, Type::Str, Type::Window, Type::Int],
                Type::CadenceAssociation,
                PureNative,
                false,
            ),
            s(
                vec![
                    Type::Bytes,
                    Type::Bytes,
                    Type::Bytes,
                    Type::Window,
                    Type::Int,
                ],
                Type::CadenceAssociation,
                PureNative,
                false,
            ),
        ],
        "recipient_eval_demo" => vec![s(
            vec![Type::Int, Type::Int, Type::Window],
            Type::RecipientOutcome,
            PureNative,
            false,
        )],

        // --- Phase D2 exact scalar & rational intrinsics ---
        "num" | "den" => vec![s(vec![Type::Ratio], Type::Int, PureNative, false)],
        "abs" => vec![
            s(vec![Type::Int], Type::Int, PureNative, false),
            s(vec![Type::T1], Type::T1, PureNative, false),
            s(vec![Type::T9], Type::T9, PureNative, false),
            s(vec![Type::T32], Type::T32, PureNative, false),
            s(vec![Type::Ratio], Type::Ratio, PureNative, false),
            s(
                vec![Type::QExt(Box::new(Type::Ratio))],
                Type::QExt(Box::new(Type::Ratio)),
                QExt,
                false,
            ),
        ],
        "signum" => vec![
            s(vec![Type::Int], Type::T1, PureNative, false),
            s(vec![Type::T1], Type::T1, PureNative, false),
            s(vec![Type::T9], Type::T1, PureNative, false),
            s(vec![Type::T32], Type::T1, PureNative, false),
            s(vec![Type::Ratio], Type::T1, PureNative, false),
        ],
        "min" | "max" => vec![
            s(vec![Type::Int, Type::Int], Type::Int, PureNative, false),
            s(vec![Type::U8, Type::U8], Type::U8, PureNative, false),
            s(vec![Type::B64, Type::B64], Type::B64, PureNative, false),
            s(vec![Type::T1, Type::T1], Type::T1, PureNative, false),
            s(vec![Type::T9, Type::T9], Type::T9, PureNative, false),
            s(vec![Type::T32, Type::T32], Type::T32, PureNative, false),
            s(vec![Type::Ratio, Type::Ratio], Type::Ratio, PureNative, false),
            s(
                vec![
                    Type::QExt(Box::new(Type::Ratio)),
                    Type::QExt(Box::new(Type::Ratio)),
                ],
                Type::QExt(Box::new(Type::Ratio)),
                QExt,
                false,
            ),
        ],
        "clamp" => vec![
            s(
                vec![Type::Int, Type::Int, Type::Int],
                Type::Int,
                PureNative,
                false,
            ),
            s(
                vec![Type::U8, Type::U8, Type::U8],
                Type::U8,
                PureNative,
                false,
            ),
            s(
                vec![Type::B64, Type::B64, Type::B64],
                Type::B64,
                PureNative,
                false,
            ),
            s(
                vec![Type::T1, Type::T1, Type::T1],
                Type::T1,
                PureNative,
                false,
            ),
            s(
                vec![Type::T9, Type::T9, Type::T9],
                Type::T9,
                PureNative,
                false,
            ),
            s(
                vec![Type::T32, Type::T32, Type::T32],
                Type::T32,
                PureNative,
                false,
            ),
            s(
                vec![Type::Ratio, Type::Ratio, Type::Ratio],
                Type::Ratio,
                PureNative,
                false,
            ),
        ],
        "gcd" | "lcm" => vec![
            s(vec![Type::Int, Type::Int], Type::Int, PureNative, false),
            s(vec![Type::T9, Type::T9], Type::T9, PureNative, false),
            s(vec![Type::T32, Type::T32], Type::T32, PureNative, false),
        ],
        "pow" => vec![
            s(vec![Type::Int, Type::Int], Type::Int, PureNative, false),
            s(vec![Type::T9, Type::Int], Type::T9, PureNative, false),
            s(vec![Type::T9, Type::T9], Type::T9, PureNative, false),
            s(vec![Type::T32, Type::Int], Type::T32, PureNative, false),
            s(vec![Type::T32, Type::T32], Type::T32, PureNative, false),
            s(vec![Type::Ratio, Type::Int], Type::Ratio, PureNative, false),
        ],

        // --- Strictly quarantined `aod::qext` builtins (`P0-1`, NO unwrap to native!) ---
        "aod::qext::ext_decimal" => vec![
            s(
                vec![Type::Str],
                Type::QExt(Box::new(Type::Ratio)),
                QExt,
                false,
            ),
            s(
                vec![Type::Bytes],
                Type::QExt(Box::new(Type::Ratio)),
                QExt,
                false,
            ),
        ],
        "aod::qext::qext_from_ratio" => vec![s(
            vec![Type::Ratio],
            Type::QExt(Box::new(Type::Ratio)),
            QExt,
            false,
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
            false,
        )],
        "aod::qext::qext_render" => vec![s(
            vec![Type::QExt(Box::new(Type::Ratio))],
            Type::QExt(Box::new(Type::Str)),
            QExt,
            false,
        )],
        "aod::qext::qext_round_half_even" => vec![s(
            vec![Type::QExt(Box::new(Type::Ratio)), Type::Int],
            Type::QExt(Box::new(Type::Ratio)),
            QExt,
            false,
        )],
        "aod::qext::qext_format_fixed" => vec![s(
            vec![Type::QExt(Box::new(Type::Ratio)), Type::Int],
            Type::QExt(Box::new(Type::Str)),
            QExt,
            false,
        )],
        "aod::qext::qext_to_bytes_exact" | "aod::qext::qext_render_bytes" => vec![s(
            vec![Type::QExt(Box::new(Type::Ratio))],
            Type::QExt(Box::new(Type::Bytes)),
            QExt,
            false,
        )],
        "aod::qext::qext_format_bytes_fixed" | "aod::qext::qext_format_fixed_bytes" => vec![s(
            vec![Type::QExt(Box::new(Type::Ratio)), Type::Int],
            Type::QExt(Box::new(Type::Bytes)),
            QExt,
            false,
        )],
        _ => Vec::new(),
    }
}

pub fn is_v1_builtin_name(name: &str) -> bool {
    lookup_builtin(name).iter().any(|sig| sig.v1_allowed)
}
