use std::fmt;

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
    ShapeMismatch,
    ImplicitCoercionForbidden,
    ObserverMutationForbidden,
    QExtLeakageForbidden,
    SysIoForbidden,
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
