use aod_core::bigint::BigInt;
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::{Addr8, T1};
pub use aod_rt::sim::{AxisDir, ProbeKind, WallPolicy as WallPolicyAst};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TritLiteral {
    Zero,
    One,
    Hinge0H,
    Dwell0D,
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
    List(Box<TypeExpr>),
    Map(Box<TypeExpr>, Box<TypeExpr>),
    Tuple(Vec<TypeExpr>),
    ReadOnly(Box<TypeExpr>),
    QExt(Box<TypeExpr>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedParam {
    pub name: String,
    pub ty: TypeExpr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Pattern {
    Wildcard,
    Binding(String),
    IntLit(BigInt),
    BoolLit(bool),
    StrLit(String),
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
    TypedVar { ty: TypeExpr, name: String },
    TypedPattern { ty: TypeExpr, pattern: Pattern },
    Pattern(Pattern),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecArm {
    pub params: Vec<DecArmParam>,
    pub guard: Option<Expr>,
    pub return_type: TypeExpr,
    pub output_name: Option<String>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecFnKind {
    MultiArm {
        header_params: Vec<String>,
        header_typed_params: Option<Vec<TypedParam>>,
        output_name: Option<String>,
        header_return_type: Option<TypeExpr>,
        arms: Vec<DecArm>,
    },
    SingleArm {
        params: Vec<TypedParam>,
        return_type: TypeExpr,
        body: Vec<Stmt>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecFnDecl {
    pub name: String,
    pub kind: DecFnKind,
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
pub enum AssignTarget {
    Ident(String),
    Field { base: Box<Expr>, name: String },
    Index { base: Box<Expr>, index: Box<Expr> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForTarget {
    Single(String),
    Tuple(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StmtKind {
    Let {
        name: String,
        ty: Option<TypeExpr>,
        value: Expr,
    },
    Mut {
        name: String,
        ty: Option<TypeExpr>,
        value: Expr,
    },
    TypedLocal {
        ty: TypeExpr,
        name: String,
        value: Expr,
    },
    Assign {
        target: AssignTarget,
        value: Expr,
    },
    Assert(Expr),
    Emit(Expr),
    Return(Option<Expr>),
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        elifs: Vec<(Expr, Vec<Stmt>)>,
        else_body: Vec<Stmt>,
    },
    Match {
        scrutinee: Expr,
        arms: Vec<MatchArm>,
    },
    For {
        target: ForTarget,
        iter: Expr,
        body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Break,
    Continue,
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
    Boundary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
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
    And,
    Or,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallArg {
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExprKind {
    IntLit(BigInt),
    BoolLit(bool),
    StrLit(String),
    RatioLit(Ratio),
    Addr8Lit(Addr8),
    SupportLit(SupportForm),
    TritLit(TritLiteral),
    Ident(String),
    Path(Vec<String>),
    List(Vec<Expr>),
    MapLit(Vec<(Expr, Expr)>),
    StructLit {
        path: Vec<String>,
        fields: Vec<(String, Expr)>,
    },
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
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Ternary {
        cond: Box<Expr>,
        yes: Box<Expr>,
        no: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<CallArg>,
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
