use std::fmt;

/// Unified error type for all exact mathematical, witness, wavelet, and ledger operations in `aod-core`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    ZeroDivision,
    Overflow(String),
    DomainError(String),
    InvalidTritEncoding(u8),
    InvalidOctalAnchor(String),
    InvalidSupportForm(String),
    OddSupportRequiresYro(String),
    EvenSupportForbidsYro(String),
    InvalidDecimalLiteral(String),
    InvalidJson(String),
    DuplicateJsonKey(String),
    FloatForbidden(String),
    ValidationError(String),
    CollisionError(String),
    AliasingError(String),
    ScopeMapRequired(String),
    ScopeMapForbidden(String),
    CoordinateRelationRequired(String),
    CoordinateRelationForbidden(String),
    ChronologyError(String),
    PromotionForbidden(String),
    HammingViolation(String),
}

pub type CoreResult<T> = Result<T, CoreError>;

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::ZeroDivision => write!(f, "zero divisor or zero denominator"),
            CoreError::Overflow(msg) => write!(f, "overflow: {msg}"),
            CoreError::DomainError(msg) => write!(f, "domain error: {msg}"),
            CoreError::InvalidTritEncoding(code) => {
                write!(
                    f,
                    "invalid 2-bit trit encoding {code:#04b}: 11 is invalid trit encoding; it is not Null"
                )
            }
            CoreError::InvalidOctalAnchor(msg) => write!(f, "invalid octal anchor: {msg}"),
            CoreError::InvalidSupportForm(msg) => write!(f, "invalid support form: {msg}"),
            CoreError::OddSupportRequiresYro(msg) => {
                write!(f, "odd support length requires explicit _yro suffix: {msg}")
            }
            CoreError::EvenSupportForbidsYro(msg) => {
                write!(f, "even support length forbids _yro suffix: {msg}")
            }
            CoreError::InvalidDecimalLiteral(msg) => {
                write!(f, "invalid quarantined decimal literal: {msg}")
            }
            CoreError::InvalidJson(msg) => write!(f, "invalid JSON: {msg}"),
            CoreError::DuplicateJsonKey(key) => write!(f, "duplicate JSON key: {key}"),
            CoreError::FloatForbidden(msg) => {
                write!(f, "floating-point values are forbidden in exact AOD: {msg}")
            }
            CoreError::ValidationError(msg) => write!(f, "validation error: {msg}"),
            CoreError::CollisionError(msg) => write!(f, "return_id collision: {msg}"),
            CoreError::AliasingError(msg) => write!(f, "occurrence aliasing: {msg}"),
            CoreError::ScopeMapRequired(msg) => write!(f, "explicit validated scope_map required: {msg}"),
            CoreError::ScopeMapForbidden(msg) => write!(f, "cross-scope map forbidden on same scope: {msg}"),
            CoreError::CoordinateRelationRequired(msg) => {
                write!(f, "explicit declared_cut_coordinate_relation required: {msg}")
            }
            CoreError::CoordinateRelationForbidden(msg) => {
                write!(f, "cross-coordinate relation forbidden on shared coordinate: {msg}")
            }
            CoreError::ChronologyError(msg) => write!(f, "chronology violation: {msg}"),
            CoreError::PromotionForbidden(msg) => write!(f, "wave promotion forbidden: {msg}"),
            CoreError::HammingViolation(msg) => write!(f, "Hamming-1 adjacency violation: {msg}"),
        }
    }
}

impl std::error::Error for CoreError {}
