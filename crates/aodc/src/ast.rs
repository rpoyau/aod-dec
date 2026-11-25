use aod_core::bigint::BigInt;
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::{Addr8, T1};
pub use aod_rt::sim::{AxisDir, ProbeKind, WallPolicy as WallPolicyAst};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TritLiteral {
    NegOne,
    Zero,
    PosOne,
    Hinge0H,
    Dwell0D,
}

impl TritLiteral {
    pub const fn to_t1(self) -> T1 {
        match self {
            TritLiteral::NegOne => T1::NEG,
            TritLiteral::Zero | TritLiteral::Hinge0H | TritLiteral::Dwell0D => T1::ZERO,
            TritLiteral::PosOne => T1::POS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Item {
    Use(UseDecl),
    Const(ConstDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    DecFn(DecFnDecl),
    Chamber(ChamberDecl),
    Prepare(PrepareDecl),
    Observe(ObserveDecl),
    QuarantineMod(QuarantineModDecl),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UseDecl {
    pub path: Vec<String>,
    pub items: Option<Vec<String>>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstDecl {
    pub name: String,
    pub ty: Option<TypeExpr>,
    pub value: Expr,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<(String, TypeExpr)>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumVariantDecl {
    pub name: String,
    pub payloads: Vec<TypeExpr>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<EnumVariantDecl>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeExpr {
    Unit,
    Named(String),
    /// Fixed compile-time shaped array `[T; N]`.
    Array(Box<TypeExpr>, usize),
    /// Dynamic stream / slice `[T]`.
    List(Box<TypeExpr>),
    Map(Box<TypeExpr>, Box<TypeExpr>),
    Tuple(Vec<TypeExpr>),
    ReadOnly(Box<TypeExpr>),
    QExt(Box<TypeExpr>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Pattern {
    Wildcard,
    Binding(String),
    NullLit,
    IntLit(BigInt),
    BoolLit(bool),
    StrLit(String),
    BytesLit(Vec<u8>),
    TritLit(T1),
    RatioLit(Ratio),
    Addr8Lit(Addr8),
    SupportLit(SupportForm),
    EnumVariant {
        path: Vec<String>,
        bindings: Vec<Pattern>,
    },
    Tuple(Vec<Pattern>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecArmParam {
    /// `(x: int)`
    TypedVar { name: String, ty: TypeExpr },
    /// `(-1t: t1)` or `(0: int)`
    TypedPattern { pattern: Pattern, ty: TypeExpr },
    /// Literal or enum pattern whose type is inferred from sibling arms or pattern literal (`-1t`, `0`, `Role::Pos`).
    Pattern(Pattern),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecArm {
    pub params: Vec<DecArmParam>,
    pub guard: Option<Expr>,
    pub return_type: TypeExpr,
    pub body: Vec<Stmt>,
    pub line: usize,
}

/// Uniform clean-break `dec <name>:` signature-combinatorics block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecFnDecl {
    pub name: String,
    pub arms: Vec<DecArm>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChamberDecl {
    pub name: String,
    pub space: Expr,
    pub walls: WallPolicyAst,
    pub engine: Option<String>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClockDecl {
    pub name: String,
    pub period: Expr,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InjectDecl {
    pub support: Expr,
    pub pos: Expr,
    pub dir: AxisDir,
    pub orient: Expr,
    pub shells: Expr,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareDecl {
    pub name: String,
    pub chamber: String,
    pub lets: Vec<Stmt>,
    pub clocks: Vec<ClockDecl>,
    pub injections: Vec<InjectDecl>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeDecl {
    Declarative {
        kind: ProbeKind,
        name: String,
        region: Expr,
        capture: Vec<String>,
        line: usize,
    },
    Procedural {
        kind: ProbeKind,
        name: String,
        param_name: String,
        return_type: TypeExpr,
        body: Vec<Stmt>,
        line: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObserveDecl {
    pub name: String,
    pub probes: Vec<ProbeDecl>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantineModDecl {
    pub name: String,
    pub items: Vec<Item>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LetPattern {
    Single(String),
    Tuple(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: usize,
}

/// Pure side-effect-free local statement set (zero `if`, `for`, `while`, `mut`, or reassignment!).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StmtKind {
    Let {
        pattern: LetPattern,
        ty: Option<TypeExpr>,
        value: Expr,
    },
    Assert(Expr),
    Emit(Expr),
    Return(Option<Expr>),
    Pass,
    QuarantineBlock {
        body: Vec<Stmt>,
    },
    ExprStmt(Expr),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum UnaryOp {
    Pos,
    Neg,
    Not,
    BitNot,
    Boundary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AdverbOp {
    FoldAdd,
    FoldMul,
    FoldMin,
    FoldMax,
    FoldAnd,
    FoldOr,
    FoldSuperpose,
    ScanAdd,
    ScanMul,
    ScanMin,
    ScanMax,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Min,
    Max,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Cmp3,
    And,
    Or,
    Sieve,
    OuterAdd,
    OuterMul,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchExprArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExprKind {
    NullLit,
    IntLit(BigInt),
    BoolLit(bool),
    StrLit(String),
    BytesLit(Vec<u8>),
    RatioLit(Ratio),
    Addr8Lit(Addr8),
    SupportLit(SupportForm),
    TritLit(TritLiteral),
    Ident(String),
    Path(Vec<String>),
    /// `[e0, e1, ...]` sequence literal (`[T; N]` when non-empty, `[T]` when empty or slice-contextual).
    SeqLit(Vec<Expr>),
    /// `map[(k1, v1), ...]` or `map[]`.
    MapLit(Vec<(Expr, Expr)>),
    Tuple(Vec<Expr>),
    WindowLit {
        start: Box<Expr>,
        end: Box<Expr>,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Adverb {
        op: AdverbOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// Stream pipeline `lhs |> stage` (already desugared or preserved for pull-iterator loop fusion!).
    Pipe {
        input: Box<Expr>,
        stage: Box<PipeStage>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchExprArm>,
    },
    Field {
        base: Box<Expr>,
        name: String,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    BoxExpr(Vec<Expr>),
    Simulate {
        prepare: String,
        ticks: Box<Expr>,
        observers: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PipeStage {
    Adverb(AdverbOp),
    Boundary,
    /// Operator section `(+ k)`, `(* k)`, `(== -1t)`, etc. (`input op rhs`).
    Section {
        op: BinaryOp,
        rhs: Expr,
    },
    /// Function/combinator stage `f` or `f(arg1, arg2, ...)` -> `f(input, arg1, arg2, ...)`.
    Call {
        callee: Expr,
        extra_args: Vec<Expr>,
    },
}
