use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CompatMode {
    V1,
    V2,
}

/// Syntax discipline selector (`CompatMode::V1 -> LegacyV1`, `CompatMode::V2 -> PureWaveV2`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum SyntaxDiscipline {
    LegacyV1,
    #[default]
    PureWaveV2,
}

impl SyntaxDiscipline {
    pub fn for_compat(compat: CompatMode) -> Self {
        match compat {
            CompatMode::V1 => Self::LegacyV1,
            CompatMode::V2 => Self::PureWaveV2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegacyV1 => "legacy_v1",
            Self::PureWaveV2 => "pure_wave_v2",
        }
    }

    pub fn from_str_tag(s: &str) -> Option<Self> {
        match s {
            "legacy_v1" => Some(Self::LegacyV1),
            "pure_wave_v2" => Some(Self::PureWaveV2),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DiagnosticCode {
    LexError,
    ParseError,
    FloatLiteralForbidden,
    InvalidOctalAnchor,
    InvalidTritLiteral,
    InvalidSupportLiteral,
    OddSupportRequiresYro,
    EvenSupportForbidsYro,
    ZeroDenominator,
    TypeError,
    ImplicitCoercionForbidden,
    ObserverMutationForbidden,
    QExtLeakageForbidden,
    ModuleNotFound,
    ModuleCycleError,
    IrVerificationError,
    RuntimeError,
    AssertionFailed,
    BudgetExhausted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub message: String,
}

pub type DecResult<T> = Result<T, Diagnostic>;

impl Diagnostic {
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            line: None,
            col: None,
            message: message.into(),
        }
    }

    pub fn at_line(code: DiagnosticCode, line: usize, message: impl Into<String>) -> Self {
        Self {
            code,
            line: Some(line),
            col: None,
            message: message.into(),
        }
    }

    pub fn at(code: DiagnosticCode, line: usize, col: usize, message: impl Into<String>) -> Self {
        Self {
            code,
            line: Some(line),
            col: Some(col),
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.col) {
            (Some(l), Some(c)) => write!(f, "line {l}:{c}: {:?}: {}", self.code, self.message),
            (Some(l), None) => write!(f, "line {l}: {:?}: {}", self.code, self.message),
            (None, _) => write!(f, "{:?}: {}", self.code, self.message),
        }
    }
}

impl std::error::Error for Diagnostic {}
