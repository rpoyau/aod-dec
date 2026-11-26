use aod_core::bigint::BigInt;
use aod_core::ternary::{B64, B8, Q4, T1};

use crate::ast::{
    AxisDir, BinaryOp, ChamberDecl, ClockDecl, ConstDecl, DecArm, DecArmParam,
    DecFnDecl, EnumDecl, EnumVariantDecl, Expr, ExprKind, InjectDecl, Item, LetPattern,
    MatchExprArm, ObserveDecl, Pattern, PipeStage, PrepareDecl, ProbeDecl, ProbeKind, Program,
    QuarantineModDecl, Stmt, StmtKind, StructDecl, TritLiteral, TypeExpr, UnaryOp, UseDecl,
    WallPolicyAst,
};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::lexer::{tokenize, Token, TokenKind};

pub const MAX_SOURCE_BYTES: usize = 1_000_000;

pub fn parse_source(source: &str) -> DecResult<Program> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Diagnostic::new(
            DiagnosticCode::ParseError,
            "source exceeds the 1 MB core limit",
        ));
    }
    let tokens = tokenize(source)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        pending_gt: false,
        generic_depth: 0,
        in_observe: false,
        just_closed_block: false,
    };
    parser.parse_program()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Supports nested generic closings like `ReadOnly<QExt<ratio>>` where lexer emitted `>>`.
    pending_gt: bool,
    generic_depth: usize,
    in_observe: bool,
    /// Set when an expression (such as indented `match`) already consumed its trailing `Dedent`.
    just_closed_block: bool,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_n(&self, n: usize) -> &Token {
        &self.tokens[(self.pos + n).min(self.tokens.len() - 1)]
    }

    fn line(&self) -> usize {
        self.peek().line
    }

    fn advance(&mut self) -> Token {
        self.just_closed_block = false;
        let tok = self.peek().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, expected: &TokenKind) -> bool {
        &self.peek().kind == expected
    }

    fn expect(&mut self, expected: &TokenKind, ctx: &str) -> DecResult<Token> {
        if self.check(expected) {
            Ok(self.advance())
        } else {
            Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                self.peek().line,
                self.peek().col,
                format!("expected {ctx}, got {:?}", self.peek().kind),
            ))
        }
    }

    fn expect_gt_closing(&mut self) -> DecResult<()> {
        if self.pending_gt {
            self.pending_gt = false;
            return Ok(());
        }
        if self.check(&TokenKind::Gt) {
            self.advance();
            Ok(())
        } else if self.check(&TokenKind::Shr) {
            self.advance();
            self.pending_gt = true;
            Ok(())
        } else {
            Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                self.peek().line,
                self.peek().col,
                format!("expected '>', got {:?}", self.peek().kind),
            ))
        }
    }

    fn skip_newlines(&mut self) {
        while self.check(&TokenKind::Newline) {
            self.advance();
        }
    }

    fn expect_ident(&mut self, ctx: &str) -> DecResult<String> {
        match &self.peek().kind {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            other => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                self.peek().line,
                self.peek().col,
                format!("expected identifier ({ctx}), got {other:?}"),
            )),
        }
    }

    fn check_ident_eq(&self, name: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(s) if s == name)
    }

    fn expect_positive_usize(&mut self, ctx: &str) -> DecResult<usize> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::IntLit(n) => {
                self.advance();
                let u = n.to_u64().ok_or_else(|| {
                    Diagnostic::at(
                        DiagnosticCode::ParseError,
                        tok.line,
                        tok.col,
                        format!("{ctx} out of range: {n}"),
                    )
                })?;
                if u == 0 || u > 1_000_000 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::TypeError,
                        tok.line,
                        tok.col,
                        format!("{ctx} must be in 1..=1_000_000, got {u}"),
                    ));
                }
                Ok(u as usize)
            }
            other => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                tok.line,
                tok.col,
                format!("expected positive integer ({ctx}), got {other:?}"),
            )),
        }
    }

    fn parse_program(&mut self) -> DecResult<Program> {
        let mut items = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Eof) {
            items.push(self.parse_top_item(false)?);
            self.skip_newlines();
        }
        Ok(Program { items })
    }

    fn parse_top_item(&mut self, in_quarantine_mod: bool) -> DecResult<Item> {
        let line = self.line();
        match &self.peek().kind {
            TokenKind::Use => {
                if in_quarantine_mod {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "use declarations must appear at module top level",
                    ));
                }
                Ok(Item::Use(self.parse_use()?))
            }
            TokenKind::Const => Ok(Item::Const(self.parse_const()?)),
            TokenKind::Struct => {
                if in_quarantine_mod {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "struct declarations are not supported inside quarantine Q_ext modules",
                    ));
                }
                Ok(Item::Struct(self.parse_struct()?))
            }
            TokenKind::Enum => {
                if in_quarantine_mod {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "enum declarations are not supported inside quarantine Q_ext modules",
                    ));
                }
                Ok(Item::Enum(self.parse_enum()?))
            }
            TokenKind::Dec => Ok(Item::DecFn(self.parse_dec_fn()?)),
            TokenKind::Chamber => {
                if in_quarantine_mod {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "chamber cannot be declared inside quarantine Q_ext",
                    ));
                }
                Ok(Item::Chamber(self.parse_chamber()?))
            }
            TokenKind::Prepare => {
                if in_quarantine_mod {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "prepare cannot be declared inside quarantine Q_ext",
                    ));
                }
                Ok(Item::Prepare(self.parse_prepare()?))
            }
            TokenKind::Observe => {
                if in_quarantine_mod {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "observe cannot be declared inside quarantine Q_ext",
                    ));
                }
                Ok(Item::Observe(self.parse_observe()?))
            }
            TokenKind::Quarantine => {
                if in_quarantine_mod {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "nested quarantine modules are unsupported",
                    ));
                }
                Ok(Item::QuarantineMod(self.parse_quarantine_mod()?))
            }
            other => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                line,
                self.peek().col,
                format!("unexpected top-level token {other:?}"),
            )),
        }
    }

    fn parse_use(&mut self) -> DecResult<UseDecl> {
        let line = self.line();
        self.expect(&TokenKind::Use, "'use'")?;
        let mut path = vec![self.expect_ident("module path")?];
        while self.check(&TokenKind::ColonColon) {
            self.advance();
            path.push(self.expect_ident("path segment")?);
        }
        if self.check(&TokenKind::Colon) {
            self.advance();
            self.expect(&TokenKind::Newline, "newline after 'use path:'")?;
            self.expect(&TokenKind::Indent, "indented imported items")?;
            self.skip_newlines();
            let mut list = Vec::new();
            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                loop {
                    list.push(self.expect_ident("imported item")?);
                    if self.check(&TokenKind::Comma) {
                        self.advance();
                        if self.check(&TokenKind::Newline)
                            || self.check(&TokenKind::Dedent)
                            || self.check(&TokenKind::Eof)
                        {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                self.expect_line_end()?;
                self.skip_newlines();
            }
            self.expect(&TokenKind::Dedent, "dedent after use block")?;
            if list.is_empty() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "indented use block must list at least one imported symbol",
                ));
            }
            Ok(UseDecl {
                path,
                items: Some(list),
                line,
            })
        } else {
            self.expect_line_end()?;
            Ok(UseDecl {
                path,
                items: None,
                line,
            })
        }
    }

    fn parse_const(&mut self) -> DecResult<ConstDecl> {
        let line = self.line();
        self.expect(&TokenKind::Const, "'const'")?;
        let name = self.expect_ident("const name")?;
        let ty = if self.check(&TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };
        self.expect(&TokenKind::Eq, "'='")?;
        let value = self.parse_expr()?;
        self.expect_line_end()?;
        Ok(ConstDecl {
            name,
            ty,
            value,
            line,
        })
    }

    fn parse_struct(&mut self) -> DecResult<StructDecl> {
        let line = self.line();
        self.expect(&TokenKind::Struct, "'struct'")?;
        let name = self.expect_ident("struct name")?;
        self.expect(&TokenKind::Colon, "':' after struct name")?;
        self.expect(&TokenKind::Newline, "newline after struct header")?;
        self.expect(&TokenKind::Indent, "indented struct fields")?;
        self.skip_newlines();
        let mut fields = Vec::new();
        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            let fname = self.expect_ident("struct field name")?;
            self.expect(&TokenKind::Colon, "':' after field name")?;
            let fty = self.parse_type_expr()?;
            fields.push((fname, fty));
            self.expect_line_end()?;
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent, "dedent after struct")?;
        Ok(StructDecl { name, fields, line })
    }

    fn parse_enum(&mut self) -> DecResult<EnumDecl> {
        let line = self.line();
        self.expect(&TokenKind::Enum, "'enum'")?;
        let name = self.expect_ident("enum name")?;
        self.expect(&TokenKind::Colon, "':' after enum name")?;
        self.expect(&TokenKind::Newline, "newline after enum header")?;
        self.expect(&TokenKind::Indent, "indented enum variants")?;
        self.skip_newlines();
        let mut variants = Vec::new();
        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            variants.push(self.parse_enum_variant()?);
            self.expect_line_end()?;
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent, "dedent after enum")?;
        if variants.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                format!("enum {name} must declare at least one variant"),
            ));
        }
        Ok(EnumDecl {
            name,
            variants,
            line,
        })
    }

    fn parse_enum_variant(&mut self) -> DecResult<EnumVariantDecl> {
        let line = self.line();
        let name = self.expect_ident("enum variant name")?;
        let mut payloads = Vec::new();
        if self.check(&TokenKind::LParen) {
            self.advance();
            while !self.check(&TokenKind::RParen) {
                payloads.push(self.parse_type_expr()?);
                if self.check(&TokenKind::Comma) {
                    self.advance();
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RParen, "')'")?;
        }
        Ok(EnumVariantDecl {
            name,
            payloads,
            line,
        })
    }

    fn parse_type_expr(&mut self) -> DecResult<TypeExpr> {
        let ty = self.parse_type_expr_inner()?;
        if self.generic_depth == 0 && self.pending_gt {
            self.pending_gt = false;
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                self.line(),
                "unmatched extra '>' in generic type expression",
            ));
        }
        Ok(ty)
    }

    fn parse_type_expr_inner(&mut self) -> DecResult<TypeExpr> {
        let line = self.line();
        if self.check(&TokenKind::Null) {
            self.advance();
            return Ok(TypeExpr::Named("null".to_string()));
        }
        if self.check(&TokenKind::LParen) {
            self.advance();
            if self.check(&TokenKind::RParen) {
                self.advance();
                return Ok(TypeExpr::Unit);
            }
            let first = self.parse_type_expr_inner()?;
            if self.check(&TokenKind::Comma) {
                let mut elems = vec![first];
                while self.check(&TokenKind::Comma) {
                    self.advance();
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                    elems.push(self.parse_type_expr_inner()?);
                }
                self.expect(&TokenKind::RParen, "')'")?;
                return Ok(TypeExpr::Tuple(elems));
            }
            self.expect(&TokenKind::RParen, "')'")?;
            return Ok(first);
        }
        if self.check(&TokenKind::LBracket) {
            self.advance();
            let elem_ty = self.parse_type_expr_inner()?;
            if self.check(&TokenKind::Semicolon) {
                self.advance();
                let mut dims = Vec::new();
                loop {
                    dims.push(self.expect_positive_usize("array dimension")?);
                    if self.check(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket, "']' after shaped array type")?;
                let mut built = elem_ty;
                for &d in dims.iter().rev() {
                    built = TypeExpr::Array(Box::new(built), d);
                }
                return Ok(built);
            }
            self.expect(&TokenKind::RBracket, "']'")?;
            return Ok(TypeExpr::List(Box::new(elem_ty)));
        }
        let mut name = self.expect_ident("type name")?;
        while self.check(&TokenKind::ColonColon) {
            self.advance();
            let seg = self.expect_ident("qualified type segment")?;
            name.push_str("::");
            name.push_str(&seg);
        }
        if name == "array" && self.check(&TokenKind::LBracket) {
            self.advance();
            let elem_ty = self.parse_type_expr_inner()?;
            self.expect(&TokenKind::Comma, "',' in array[T, N]")?;
            let n = self.expect_positive_usize("array dimension")?;
            self.expect(&TokenKind::RBracket, "']' in array[T, N]")?;
            return Ok(TypeExpr::Array(Box::new(elem_ty), n));
        }
        if name == "map" && self.check(&TokenKind::LBracket) {
            self.advance();
            let k = self.parse_type_expr_inner()?;
            self.expect(&TokenKind::Comma, "',' in map[K, V]")?;
            let v = self.parse_type_expr_inner()?;
            self.expect(&TokenKind::RBracket, "']' in map[K, V]")?;
            return Ok(TypeExpr::Map(Box::new(k), Box::new(v)));
        }
        if self.check(&TokenKind::Lt) {
            self.advance();
            self.generic_depth += 1;
            let inner = self.parse_type_expr_inner()?;
            self.generic_depth -= 1;
            self.expect_gt_closing()?;
            match name.as_str() {
                "ReadOnly" => Ok(TypeExpr::ReadOnly(Box::new(inner))),
                "QExt" => Ok(TypeExpr::QExt(Box::new(inner))),
                "List" => Ok(TypeExpr::List(Box::new(inner))),
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    format!("unknown generic type constructor {name}"),
                )),
            }
        } else {
            Ok(TypeExpr::Named(name))
        }
    }

    fn parse_dec_fn(&mut self) -> DecResult<DecFnDecl> {
        let line = self.line();
        self.expect(&TokenKind::Dec, "'dec'")?;
        let name = self.expect_ident("function name")?;

        // 1. Canonical signature-matrix header: `dec <name>:` + indented `(params) [where g] -> Ret:` arms
        if self.check(&TokenKind::Colon) {
            self.advance();
            self.expect(&TokenKind::Newline, "newline after 'dec <name>:'")?;
            self.expect(&TokenKind::Indent, "indented dec signature arms")?;
            self.skip_newlines();
            let mut arms = Vec::new();
            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                arms.push(self.parse_dec_arm()?);
                self.skip_newlines();
            }
            self.expect(&TokenKind::Dedent, "dedent after dec arms")?;
            if arms.is_empty() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    format!("dec {name}: has no signature arms"),
                ));
            }
            return Ok(DecFnDecl { name, arms, line });
        }

        // 2. Concise single-arm shorthand: `dec <name>(params) [where g] -> Ret:`
        if self.check(&TokenKind::LParen) {
            let arm = self.parse_dec_arm()?;
            return Ok(DecFnDecl {
                name,
                arms: vec![arm],
                line,
            });
        }

        Err(Diagnostic::at(
            DiagnosticCode::ParseError,
            self.peek().line,
            self.peek().col,
            "expected ':' or '(' after dec function name",
        ))
    }

    fn parse_dec_arm(&mut self) -> DecResult<DecArm> {
        let line = self.line();
        self.expect(&TokenKind::LParen, "'(' for arm signature")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                params.push(self.parse_dec_arm_param()?);
                if self.check(&TokenKind::Comma) {
                    self.advance();
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "')'")?;

        let mut guard = None;
        if self.check(&TokenKind::Where) {
            self.advance();
            guard = Some(self.parse_expr()?);
        }

        let (has_arrow, return_type) = if self.check(&TokenKind::Arrow) {
            self.advance();
            (true, self.parse_type_expr()?)
        } else {
            (false, TypeExpr::Unit)
        };

        if self.check(&TokenKind::Where) {
            if guard.is_some() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "duplicate 'where' guard on dec arm",
                ));
            }
            self.advance();
            guard = Some(self.parse_expr()?);
        }

        self.expect(&TokenKind::Colon, "':' after arm signature")?;
        let mut body = if self.check(&TokenKind::Newline) {
            self.advance();
            self.expect(&TokenKind::Indent, "indented arm body")?;
            self.parse_block_stmts_already_indented()?
        } else {
            // Single-line arm expression/statement shorthand: `(0: int) -> int: 1`
            let s = self.parse_stmt()?;
            vec![s]
        };

        promote_tail_return(&mut body, &return_type);

        Ok(DecArm {
            params,
            guard,
            has_arrow,
            return_type,
            body,
            line,
        })
    }

    fn parse_dec_arm_param(&mut self) -> DecResult<DecArmParam> {
        if let TokenKind::Ident(ref name) = self.peek().kind {
            if name != "_" && self.peek_n(1).kind == TokenKind::Colon {
                let pname = self.expect_ident("parameter name")?;
                self.expect(&TokenKind::Colon, "':'")?;
                let ty = self.parse_type_expr()?;
                return Ok(DecArmParam::TypedVar { name: pname, ty });
            }
        }
        let pattern = self.parse_pattern()?;
        if self.check(&TokenKind::Colon) {
            self.advance();
            let ty = self.parse_type_expr()?;
            Ok(DecArmParam::TypedPattern { pattern, ty })
        } else {
            Ok(DecArmParam::Pattern(pattern))
        }
    }

    fn parse_pattern(&mut self) -> DecResult<Pattern> {
        let tok = self.peek().clone();
        let line = tok.line;
        match tok.kind {
            TokenKind::Ident(s) if s == "_" => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            TokenKind::Null => {
                self.advance();
                Ok(Pattern::NullLit)
            }
            TokenKind::True => {
                self.advance();
                Ok(Pattern::BoolLit(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Pattern::BoolLit(false))
            }
            TokenKind::BytesLit(bs) => {
                self.advance();
                Ok(Pattern::BytesLit(bs))
            }
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(Pattern::IntLit(n))
            }
            TokenKind::RatioLit(r) => {
                self.advance();
                Ok(Pattern::RatioLit(r))
            }
            TokenKind::Addr8Lit(a) => {
                self.advance();
                Ok(Pattern::Addr8Lit(a))
            }
            TokenKind::SupportLit(sf) => {
                self.advance();
                Ok(Pattern::SupportLit(sf))
            }
            TokenKind::TritLit(tl) => {
                self.advance();
                Ok(Pattern::TritLit(tl.to_t1()))
            }
            TokenKind::Plus | TokenKind::Minus => {
                let neg = tok.kind == TokenKind::Minus;
                self.advance();
                let inner = self.peek().clone();
                match inner.kind {
                    TokenKind::IntLit(n) => {
                        self.advance();
                        Ok(Pattern::IntLit(if neg { -n } else { n }))
                    }
                    TokenKind::RatioLit(r) => {
                        self.advance();
                        Ok(Pattern::RatioLit(if neg { -r } else { r }))
                    }
                    TokenKind::TritLit(tl) => {
                        self.advance();
                        let base: T1 = tl.to_t1();
                        Ok(Pattern::TritLit(if neg { base.checked_neg() } else { base }))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        format!("expected numeric or trit literal after +/- in pattern, got {other:?}"),
                    )),
                }
            }
            TokenKind::LParen => {
                self.advance();
                let mut pats = Vec::new();
                while !self.check(&TokenKind::RParen) {
                    pats.push(self.parse_pattern()?);
                    if self.check(&TokenKind::Comma) {
                        self.advance();
                        if self.check(&TokenKind::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "')' in tuple pattern")?;
                Ok(Pattern::Tuple(pats))
            }
            TokenKind::Ident(first) => {
                self.advance();
                if matches!(
                    first.as_str(),
                    "b8" | "b64" | "b64_bits" | "t1" | "q4"
                ) && self.check(&TokenKind::LParen)
                {
                    self.advance();
                    let neg = if self.check(&TokenKind::Minus) {
                        self.advance();
                        true
                    } else if self.check(&TokenKind::Plus) {
                        self.advance();
                        false
                    } else {
                        false
                    };
                    if first == "t1" {
                        if let TokenKind::TritLit(tl) = self.peek().kind {
                            self.advance();
                            let base = tl.to_t1();
                            self.expect(&TokenKind::RParen, "')' after t1(...) pattern")?;
                            return Ok(Pattern::TritLit(if neg { base.checked_neg() } else { base }));
                        }
                    }
                    let arg_tok = self.peek().clone();
                    let raw_int = match arg_tok.kind {
                        TokenKind::IntLit(n) => {
                            self.advance();
                            if neg { -n } else { n }
                        }
                        other => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ParseError,
                                line,
                                format!(
                                    "expected integer literal inside {first}(...) pattern, got {other:?}"
                                ),
                            ));
                        }
                    };
                    self.expect(&TokenKind::RParen, "')' after register pattern literal")?;
                    return match first.as_str() {
                        "b8" => {
                            let u = raw_int.to_u64().filter(|&v| v <= 255).ok_or_else(|| {
                                Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("b8 pattern literal outside 0..=255: {raw_int}"),
                                )
                            })?;
                            Ok(Pattern::B8Lit(B8(u as u8)))
                        }
                        "b64" => {
                            let b = B64::from_trit_int(&raw_int).map_err(|e| {
                                Diagnostic::at_line(DiagnosticCode::TypeError, line, e.to_string())
                            })?;
                            Ok(Pattern::B64Lit(b))
                        }
                        "b64_bits" => {
                            let b = B64::from_unsigned_int(&raw_int).map_err(|e| {
                                Diagnostic::at_line(DiagnosticCode::TypeError, line, e.to_string())
                            })?;
                            Ok(Pattern::B64Lit(b))
                        }
                        "t1" => {
                            let t = T1::from_bigint(&raw_int).map_err(|e| {
                                Diagnostic::at_line(
                                    DiagnosticCode::InvalidTritLiteral,
                                    line,
                                    e.to_string(),
                                )
                            })?;
                            Ok(Pattern::TritLit(t))
                        }
                        "q4" => {
                            let q = Q4::from_bigint(&raw_int).map_err(|e| {
                                Diagnostic::at_line(DiagnosticCode::TypeError, line, e.to_string())
                            })?;
                            Ok(Pattern::Q4Lit(q))
                        }
                        _ => unreachable!(),
                    };
                }
                if self.check(&TokenKind::ColonColon) {
                    let mut path = vec![first];
                    while self.check(&TokenKind::ColonColon) {
                        self.advance();
                        path.push(self.expect_ident("enum variant segment")?);
                    }
                    let mut bindings = Vec::new();
                    if self.check(&TokenKind::LParen) {
                        self.advance();
                        while !self.check(&TokenKind::RParen) {
                            bindings.push(self.parse_pattern()?);
                            if self.check(&TokenKind::Comma) {
                                self.advance();
                                if self.check(&TokenKind::RParen) {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        self.expect(&TokenKind::RParen, "')' in variant pattern")?;
                    }
                    Ok(Pattern::EnumVariant { path, bindings })
                } else {
                    Ok(Pattern::Binding(first))
                }
            }
            other => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                tok.line,
                tok.col,
                format!("unexpected token in pattern: {other:?}"),
            )),
        }
    }

    fn parse_chamber(&mut self) -> DecResult<ChamberDecl> {
        let line = self.line();
        self.expect(&TokenKind::Chamber, "'chamber'")?;
        let name = self.expect_ident("chamber name")?;
        self.expect(&TokenKind::Colon, "':'")?;
        self.expect(&TokenKind::Newline, "newline after chamber header")?;
        self.expect(&TokenKind::Indent, "indented chamber body")?;
        self.skip_newlines();

        let mut space: Option<Expr> = None;
        let mut walls = WallPolicyAst::Open;
        let mut engine: Option<String> = None;
        let mut boundary_step: Option<Expr> = None;
        let mut contact_ttl: Option<Expr> = None;
        let mut fuse_on_resonance: Option<Expr> = None;

        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            let fline = self.line();
            let key = self.expect_ident("chamber field (space/walls/engine)")?;
            if self.check(&TokenKind::Eq) || self.check(&TokenKind::Colon) {
                self.advance();
            } else {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    fline,
                    "expected '=' or ':' in chamber field",
                ));
            }
            match key.as_str() {
                "space" => {
                    space = Some(self.parse_expr()?);
                }
                "walls" => {
                    let w = self.expect_ident("wall policy (open/closed)")?;
                    walls = match w.as_str() {
                        "open" => WallPolicyAst::Open,
                        "closed" => WallPolicyAst::Closed,
                        _ => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ParseError,
                                fline,
                                format!("unknown wall policy {w:?}; expected open or closed"),
                            ));
                        }
                    };
                }
                "engine" => {
                    let eng = match &self.peek().kind {
                        TokenKind::Ident(s) => {
                            let s = s.clone();
                            self.advance();
                            s
                        }
                        TokenKind::BytesLit(bs) => {
                            let s = std::str::from_utf8(bs).map_err(|_| {
                                Diagnostic::at_line(
                                    DiagnosticCode::ParseError,
                                    fline,
                                    "chamber engine bytes literal must be valid UTF-8",
                                )
                            })?.to_string();
                            self.advance();
                            s
                        }
                        other => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ParseError,
                                fline,
                                format!("expected engine identifier or byte literal, got {other:?}"),
                            ));
                        }
                    };
                    engine = Some(eng);
                }
                "boundary_step" => {
                    boundary_step = Some(self.parse_expr()?);
                }
                "contact_ttl" => {
                    contact_ttl = Some(self.parse_expr()?);
                }
                "fuse_on_resonance" => {
                    fuse_on_resonance = Some(self.parse_expr()?);
                }
                other => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        fline,
                        format!("unknown chamber property {other:?}"),
                    ));
                }
            }
            self.expect_line_end()?;
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent, "dedent after chamber")?;
        let space = space.ok_or_else(|| {
            Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                format!("chamber {name} missing required `space` declaration"),
            )
        })?;
        Ok(ChamberDecl {
            name,
            space,
            walls,
            engine,
            boundary_step,
            contact_ttl,
            fuse_on_resonance,
            line,
        })
    }

    fn parse_prepare(&mut self) -> DecResult<PrepareDecl> {
        let line = self.line();
        self.expect(&TokenKind::Prepare, "'prepare'")?;
        let name = self.expect_ident("prepare name")?;
        self.expect(&TokenKind::In, "'in'")?;
        let chamber = self.expect_ident("chamber name")?;
        self.expect(&TokenKind::Colon, "':'")?;
        self.expect(&TokenKind::Newline, "newline after prepare header")?;
        self.expect(&TokenKind::Indent, "indented prepare body")?;
        self.skip_newlines();

        let mut lets = Vec::new();
        let mut clocks = Vec::new();
        let mut injections = Vec::new();

        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            let sline = self.line();
            if self.check(&TokenKind::Let) {
                lets.push(self.parse_stmt()?);
            } else if self.check(&TokenKind::Quarantine) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    sline,
                    "quarantine Q_ext is forbidden inside prepare blocks",
                ));
            } else if self.check_ident_eq("clock") {
                self.advance();
                let cname = self.expect_ident("clock name")?;
                if self.check(&TokenKind::Eq) || self.check(&TokenKind::Colon) {
                    self.advance();
                }
                let period = self.parse_expr()?;
                self.expect_line_end()?;
                clocks.push(ClockDecl {
                    name: cname,
                    period,
                    line: sline,
                });
            } else if self.check_ident_eq("inject") {
                injections.push(self.parse_inject()?);
            } else {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    sline,
                    format!(
                        "expected `let`, `clock`, or `inject` inside prepare block, got {:?}",
                        self.peek().kind
                    ),
                ));
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent, "dedent after prepare")?;
        Ok(PrepareDecl {
            name,
            chamber,
            lets,
            clocks,
            injections,
            line,
        })
    }

    fn parse_inject(&mut self) -> DecResult<InjectDecl> {
        let line = self.line();
        self.expect_ident("inject")?;
        let support = self.parse_expr()?;

        let mut pos: Option<Expr> = None;
        let mut dir: Option<AxisDir> = None;
        let mut orient: Option<Expr> = None;
        let mut shells: Option<Expr> = None;

        while !self.check(&TokenKind::Newline)
            && !self.check(&TokenKind::Dedent)
            && !self.check(&TokenKind::Eof)
        {
            if self.check(&TokenKind::Comma) {
                self.advance();
                continue;
            }
            let clause = self.expect_ident("inject clause (at/dir/orient/shells)")?;
            if self.check(&TokenKind::Colon) || self.check(&TokenKind::Eq) {
                self.advance();
            }
            match clause.as_str() {
                "at" => pos = Some(self.parse_expr()?),
                "dir" => dir = Some(self.parse_axis_dir()?),
                "orient" => orient = Some(self.parse_expr()?),
                "shells" => shells = Some(self.parse_expr()?),
                other => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        format!("unknown inject clause {other:?}"),
                    ));
                }
            }
        }
        self.expect_line_end()?;

        Ok(InjectDecl {
            support,
            pos: pos.ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "inject missing `at (x, y, z)`",
                )
            })?,
            dir: dir.unwrap_or(AxisDir::PosX),
            orient: orient.unwrap_or(Expr {
                kind: ExprKind::IntLit(BigInt::one()),
                line,
            }),
            shells: shells.unwrap_or(Expr {
                kind: ExprKind::IntLit(BigInt::one()),
                line,
            }),
            line,
        })
    }

    fn parse_axis_dir(&mut self) -> DecResult<AxisDir> {
        let line = self.line();
        let positive = match self.peek().kind {
            TokenKind::Plus => {
                self.advance();
                true
            }
            TokenKind::Minus => {
                self.advance();
                false
            }
            _ => true,
        };
        let axis = self.expect_ident("axis direction (X/Y/Z)")?;
        match (positive, axis.as_str()) {
            (true, "X") => Ok(AxisDir::PosX),
            (false, "X") => Ok(AxisDir::NegX),
            (true, "Y") => Ok(AxisDir::PosY),
            (false, "Y") => Ok(AxisDir::NegY),
            (true, "Z") => Ok(AxisDir::PosZ),
            (false, "Z") => Ok(AxisDir::NegZ),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                format!("invalid axis direction {axis:?}; expected +X/-X/+Y/-Y/+Z/-Z"),
            )),
        }
    }

    fn parse_observe(&mut self) -> DecResult<ObserveDecl> {
        let line = self.line();
        self.expect(&TokenKind::Observe, "'observe'")?;
        let name = self.expect_ident("observe name")?;
        self.expect(&TokenKind::Colon, "':'")?;
        self.expect(&TokenKind::Newline, "newline after observe header")?;
        self.expect(&TokenKind::Indent, "indented observe body")?;
        self.skip_newlines();

        let prev_in_observe = self.in_observe;
        self.in_observe = true;
        let res = (|| {
            let mut probes = Vec::new();
            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                let pline = self.line();
                if self.check(&TokenKind::Quarantine) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        pline,
                        "quarantine Q_ext is forbidden inside observe blocks",
                    ));
                }
                let kind_word = self.expect_ident("tracer or sensor")?;
                let kind = match kind_word.as_str() {
                    "tracer" => ProbeKind::Tracer,
                    "sensor" => ProbeKind::Sensor,
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ParseError,
                            pline,
                            format!("expected `tracer` or `sensor` inside observe, got {other:?}"),
                        ));
                    }
                };
                let pname = self.expect_ident("probe name")?;
                if self.check(&TokenKind::LParen) {
                    self.advance();
                    let param_name = self.expect_ident("observer view parameter")?;
                    let param_ty = if self.check(&TokenKind::Colon) {
                        self.advance();
                        Some(self.parse_type_expr()?)
                    } else {
                        None
                    };
                    self.expect(&TokenKind::RParen, "')'")?;
                    let return_type = if self.check(&TokenKind::Arrow) {
                        self.advance();
                        self.parse_type_expr()?
                    } else {
                        TypeExpr::Unit
                    };
                    self.expect(&TokenKind::Colon, "':'")?;
                    self.expect(&TokenKind::Newline, "newline after procedural probe header")?;
                    self.expect(&TokenKind::Indent, "indented procedural probe body")?;
                    let mut body = self.parse_block_stmts_already_indented()?;
                    promote_tail_return(&mut body, &return_type);
                    probes.push(ProbeDecl::Procedural {
                        kind,
                        name: pname,
                        param_name,
                        param_ty,
                        return_type,
                        body,
                        line: pline,
                    });
                } else {
                    self.expect(&TokenKind::Colon, "':'")?;
                    self.expect(&TokenKind::Newline, "newline after probe header")?;
                    self.expect(&TokenKind::Indent, "indented probe body")?;
                    self.skip_newlines();
                    if self.check(&TokenKind::LParen) {
                        self.advance();
                        let param_name = self.expect_ident("observer view parameter")?;
                        let param_ty = if self.check(&TokenKind::Colon) {
                            self.advance();
                            Some(self.parse_type_expr()?)
                        } else {
                            None
                        };
                        self.expect(&TokenKind::RParen, "')'")?;
                        let return_type = if self.check(&TokenKind::Arrow) {
                            self.advance();
                            self.parse_type_expr()?
                        } else {
                            TypeExpr::Unit
                        };
                        self.expect(&TokenKind::Colon, "':'")?;
                        let mut body = if self.check(&TokenKind::Newline) {
                            self.advance();
                            self.expect(&TokenKind::Indent, "indented procedural probe body")?;
                            self.parse_block_stmts_already_indented()?
                        } else {
                            let s = self.parse_stmt()?;
                            vec![s]
                        };
                        self.skip_newlines();
                        self.expect(&TokenKind::Dedent, "dedent after indented procedural probe")?;
                        promote_tail_return(&mut body, &return_type);
                        probes.push(ProbeDecl::Procedural {
                            kind,
                            name: pname,
                            param_name,
                            param_ty,
                            return_type,
                            body,
                            line: pline,
                        });
                    } else {
                        let mut region: Option<Expr> = None;
                        let mut capture: Vec<String> = Vec::new();
                        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                            let fline = self.line();
                            let field = self.expect_ident("region or capture")?;
                            if self.check(&TokenKind::Eq) || self.check(&TokenKind::Colon) {
                                self.advance();
                            }
                            match field.as_str() {
                                "region" => region = Some(self.parse_expr()?),
                                "capture" => {
                                    self.expect(&TokenKind::LBracket, "'[' for capture list")?;
                                    while !self.check(&TokenKind::RBracket) {
                                        capture.push(self.expect_ident("capture channel")?);
                                        if self.check(&TokenKind::Comma) {
                                            self.advance();
                                        } else {
                                            break;
                                        }
                                    }
                                    self.expect(&TokenKind::RBracket, "']'")?;
                                }
                                other => {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::ParseError,
                                        fline,
                                        format!("unknown declarative probe field {other:?}"),
                                    ));
                                }
                            }
                            self.expect_line_end()?;
                            self.skip_newlines();
                        }
                        self.expect(&TokenKind::Dedent, "dedent after probe")?;
                        let region = region.ok_or_else(|| {
                            Diagnostic::at_line(
                                DiagnosticCode::ParseError,
                                pline,
                                format!("probe {pname} missing required `region`"),
                            )
                        })?;
                        probes.push(ProbeDecl::Declarative {
                            kind,
                            name: pname,
                            region,
                            capture,
                            line: pline,
                        });
                    }
                }
                self.skip_newlines();
            }
            self.expect(&TokenKind::Dedent, "dedent after observe")?;
            Ok(ObserveDecl { name, probes, line })
        })();
        self.in_observe = prev_in_observe;
        res
    }

    fn parse_quarantine_mod(&mut self) -> DecResult<QuarantineModDecl> {
        let line = self.line();
        self.expect(&TokenKind::Quarantine, "'quarantine'")?;
        let tag = self.expect_ident("'Q_ext'")?;
        if tag != "Q_ext" {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                format!("expected `quarantine Q_ext <Name>:`, got {tag:?}"),
            ));
        }
        let name = self.expect_ident("quarantine module name")?;
        self.expect(&TokenKind::Colon, "':'")?;
        self.expect(&TokenKind::Newline, "newline after quarantine header")?;
        self.expect(&TokenKind::Indent, "indented quarantine module")?;
        self.skip_newlines();
        let mut items = Vec::new();
        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            items.push(self.parse_top_item(true)?);
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent, "dedent after quarantine module")?;
        Ok(QuarantineModDecl { name, items, line })
    }

    fn parse_block_stmts_already_indented(&mut self) -> DecResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            stmts.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent, "dedent after block")?;
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> DecResult<Stmt> {
        let line = self.line();
        match &self.peek().kind {
            TokenKind::Let => {
                self.advance();
                let pattern = if self.check(&TokenKind::LParen) {
                    self.advance();
                    let mut elems = Vec::new();
                    while !self.check(&TokenKind::RParen) {
                        let bname = self.expect_ident("tuple binding name")?;
                        let elem_ty = if self.check(&TokenKind::Colon) {
                            self.advance();
                            Some(self.parse_type_expr()?)
                        } else {
                            None
                        };
                        elems.push((bname, elem_ty));
                        if self.check(&TokenKind::Comma) {
                            self.advance();
                            if self.check(&TokenKind::RParen) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen, "')' in let tuple pattern")?;
                    if elems.len() < 2 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ParseError,
                            line,
                            "let tuple destructuring requires at least 2 bindings",
                        ));
                    }
                    LetPattern::Tuple(elems)
                } else {
                    LetPattern::Single(self.expect_ident("binding name")?)
                };
                let ty = if self.check(&TokenKind::Colon) {
                    self.advance();
                    Some(self.parse_type_expr()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Eq, "'='")?;
                let value = self.parse_expr()?;
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: StmtKind::Let { pattern, ty, value },
                    line,
                })
            }
            TokenKind::Assert => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: StmtKind::Assert(expr),
                    line,
                })
            }
            TokenKind::Emit => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: StmtKind::Emit(expr),
                    line,
                })
            }
            TokenKind::Return => {
                self.advance();
                let val = if self.check(&TokenKind::Newline)
                    || self.check(&TokenKind::Dedent)
                    || self.check(&TokenKind::Eof)
                {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: StmtKind::Return(val),
                    line,
                })
            }
            TokenKind::Pass => {
                self.advance();
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: StmtKind::Pass,
                    line,
                })
            }
            TokenKind::Quarantine => {
                self.advance();
                let tag = self.expect_ident("'Q_ext'")?;
                if tag != "Q_ext" {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        format!("expected `quarantine Q_ext:`, got {tag:?}"),
                    ));
                }
                self.expect(&TokenKind::Colon, "':'")?;
                self.expect(&TokenKind::Newline, "newline")?;
                self.expect(&TokenKind::Indent, "indent")?;
                let body = self.parse_block_stmts_already_indented()?;
                Ok(Stmt {
                    kind: StmtKind::QuarantineBlock { body },
                    line,
                })
            }
            _ => {
                let lhs = self.parse_expr()?;
                if self.check(&TokenKind::Eq) {
                    if self.in_observe {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ObserverMutationForbidden,
                            line,
                            "observer cannot mutate chamber view or bindings",
                        ));
                    }
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "imperative assignment / mutation is removed; use immutable 'let', 'walk', or 'settle'",
                    ));
                }
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: StmtKind::ExprStmt(lhs),
                    line,
                })
            }
        }
    }

    fn expect_line_end(&mut self) -> DecResult<()> {
        if self.just_closed_block {
            self.just_closed_block = false;
            return Ok(());
        }
        if self.check(&TokenKind::Newline) {
            self.advance();
            Ok(())
        } else if self.check(&TokenKind::Dedent) || self.check(&TokenKind::Eof) {
            Ok(())
        } else {
            Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                self.peek().line,
                self.peek().col,
                format!("expected end of line, got {:?}", self.peek().kind),
            ))
        }
    }

    // ==================== Expressions ====================

    fn parse_expr(&mut self) -> DecResult<Expr> {
        self.parse_pipe()
    }

    fn parse_pipe(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_or()?;
        while self.check(&TokenKind::PipeGt) {
            let line = self.line();
            self.advance();
            let stage = self.parse_pipe_stage()?;
            lhs = Expr {
                kind: ExprKind::Pipe {
                    input: Box::new(lhs),
                    stage: Box::new(stage),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_pipe_stage(&mut self) -> DecResult<PipeStage> {
        if let TokenKind::Adverb(op) = self.peek().kind {
            self.advance();
            return Ok(PipeStage::Adverb(op));
        }
        if self.check(&TokenKind::BoundaryOp) {
            self.advance();
            return Ok(PipeStage::Boundary);
        }
        // Check parenthesized operator section `(+ k)`, `(== -1t)`, `(min 0)`, etc.
        if self.check(&TokenKind::LParen) {
            if let Some(op) = self.peek_section_op_after_lparen() {
                self.advance(); // consume '('
                self.advance(); // consume operator token
                let rhs = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "')' after pipeline operator section")?;
                return Ok(PipeStage::Section { op, rhs });
            }
        }

        // Otherwise parse `f` or `path::f` optionally followed by `(arg1, arg2, ...)`
        let tok = self.peek().clone();
        let line = tok.line;
        let callee = match tok.kind {
            TokenKind::Ident(first) => {
                self.advance();
                let mut segs = vec![first];
                while self.check(&TokenKind::ColonColon) {
                    self.advance();
                    segs.push(self.expect_ident("path segment")?);
                }
                if segs.len() == 1 {
                    Expr {
                        kind: ExprKind::Ident(segs.into_iter().next().unwrap()),
                        line,
                    }
                } else {
                    Expr {
                        kind: ExprKind::Path(segs),
                        line,
                    }
                }
            }
            other => {
                return Err(Diagnostic::at(
                    DiagnosticCode::ParseError,
                    tok.line,
                    tok.col,
                    format!("expected pipeline stage (adverb, section, or function), got {other:?}"),
                ));
            }
        };

        let mut extra_args = Vec::new();
        if self.check(&TokenKind::LParen) {
            self.advance();
            while !self.check(&TokenKind::RParen) {
                extra_args.push(self.parse_expr()?);
                if self.check(&TokenKind::Comma) {
                    self.advance();
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RParen, "')' in pipeline stage call")?;
        }
        Ok(PipeStage::Call { callee, extra_args })
    }

    fn peek_section_op_after_lparen(&self) -> Option<BinaryOp> {
        match &self.peek_n(1).kind {
            TokenKind::Plus => Some(BinaryOp::Add),
            TokenKind::Minus => Some(BinaryOp::Sub),
            TokenKind::Star => Some(BinaryOp::Mul),
            TokenKind::Slash => Some(BinaryOp::Div),
            TokenKind::SlashSlash => Some(BinaryOp::FloorDiv),
            TokenKind::Percent => Some(BinaryOp::Mod),
            TokenKind::EqEq => Some(BinaryOp::Eq),
            TokenKind::NotEq => Some(BinaryOp::NotEq),
            TokenKind::Lt => Some(BinaryOp::Lt),
            TokenKind::LtEq => Some(BinaryOp::LtEq),
            TokenKind::Gt => Some(BinaryOp::Gt),
            TokenKind::GtEq => Some(BinaryOp::GtEq),
            TokenKind::Cmp3 => Some(BinaryOp::Cmp3),
            TokenKind::Amp => Some(BinaryOp::BitAnd),
            TokenKind::Pipe => Some(BinaryOp::BitOr),
            TokenKind::Caret => Some(BinaryOp::BitXor),
            TokenKind::Shl => Some(BinaryOp::Shl),
            TokenKind::Shr => Some(BinaryOp::Shr),
            TokenKind::And => Some(BinaryOp::And),
            TokenKind::Or => Some(BinaryOp::Or),
            TokenKind::OuterAdd => Some(BinaryOp::OuterAdd),
            TokenKind::OuterMul => Some(BinaryOp::OuterMul),
            TokenKind::Ident(s) if s == "min" && self.peek_n(2).kind != TokenKind::LParen => {
                Some(BinaryOp::Min)
            }
            TokenKind::Ident(s) if s == "max" && self.peek_n(2).kind != TokenKind::LParen => {
                Some(BinaryOp::Max)
            }
            _ => None,
        }
    }

    fn parse_or(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_and()?;
        while self.check(&TokenKind::Or) {
            let line = self.line();
            self.advance();
            let rhs = self.parse_and()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::Or,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_not()?;
        while self.check(&TokenKind::And) {
            let line = self.line();
            self.advance();
            let rhs = self.parse_not()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::And,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_not(&mut self) -> DecResult<Expr> {
        if self.check(&TokenKind::Not) {
            let line = self.line();
            self.advance();
            let operand = self.parse_not()?;
            Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                },
                line,
            })
        } else {
            self.parse_sieve()
        }
    }

    fn parse_sieve(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_compare()?;
        while self.check(&TokenKind::Sieve) {
            let line = self.line();
            self.advance();
            let rhs = self.parse_compare()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::Sieve,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_compare(&mut self) -> DecResult<Expr> {
        let lhs = self.parse_outer()?;
        let op = match self.peek().kind {
            TokenKind::EqEq => Some(BinaryOp::Eq),
            TokenKind::NotEq => Some(BinaryOp::NotEq),
            TokenKind::Lt => Some(BinaryOp::Lt),
            TokenKind::LtEq => Some(BinaryOp::LtEq),
            TokenKind::Gt => Some(BinaryOp::Gt),
            TokenKind::GtEq => Some(BinaryOp::GtEq),
            TokenKind::Cmp3 => Some(BinaryOp::Cmp3),
            _ => None,
        };
        if let Some(op) = op {
            let line = self.line();
            self.advance();
            let rhs = self.parse_outer()?;
            if matches!(
                self.peek().kind,
                TokenKind::EqEq
                    | TokenKind::NotEq
                    | TokenKind::Lt
                    | TokenKind::LtEq
                    | TokenKind::Gt
                    | TokenKind::GtEq
                    | TokenKind::Cmp3
            ) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "chained comparisons are not supported",
                ));
            }
            Ok(Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            })
        } else {
            Ok(lhs)
        }
    }

    fn parse_outer(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_range()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::OuterAdd => BinaryOp::OuterAdd,
                TokenKind::OuterMul => BinaryOp::OuterMul,
                _ => break,
            };
            let line = self.line();
            self.advance();
            let rhs = self.parse_range()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_range(&mut self) -> DecResult<Expr> {
        let lhs = self.parse_bitor()?;
        if self.check(&TokenKind::DotDot) {
            let line = self.line();
            self.advance();
            let rhs = self.parse_bitor()?;
            Ok(Expr {
                kind: ExprKind::Range {
                    start: Box::new(lhs),
                    end: Box::new(rhs),
                },
                line,
            })
        } else {
            Ok(lhs)
        }
    }

    fn parse_bitor(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_bitxor()?;
        while self.check(&TokenKind::Pipe) {
            let line = self.line();
            self.advance();
            let rhs = self.parse_bitxor()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::BitOr,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_bitxor(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_bitand()?;
        while self.check(&TokenKind::Caret) {
            let line = self.line();
            self.advance();
            let rhs = self.parse_bitand()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::BitXor,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_bitand(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_shift()?;
        while self.check(&TokenKind::Amp) {
            let line = self.line();
            self.advance();
            let rhs = self.parse_shift()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::BitAnd,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_minmax()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Shl => BinaryOp::Shl,
                TokenKind::Shr => BinaryOp::Shr,
                _ => break,
            };
            let line = self.line();
            self.advance();
            let rhs = self.parse_minmax()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_minmax(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = if self.check_ident_eq("min")
                && !matches!(self.peek_n(1).kind, TokenKind::Colon | TokenKind::Eq)
            {
                BinaryOp::Min
            } else if self.check_ident_eq("max")
                && !matches!(self.peek_n(1).kind, TokenKind::Colon | TokenKind::Eq)
            {
                BinaryOp::Max
            } else {
                break;
            };
            let line = self.line();
            self.advance();
            let rhs = self.parse_add()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            let line = self.line();
            self.advance();
            let rhs = self.parse_mul()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> DecResult<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::SlashSlash => BinaryOp::FloorDiv,
                TokenKind::Percent => BinaryOp::Mod,
                _ => break,
            };
            let line = self.line();
            self.advance();
            let rhs = self.parse_unary()?;
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> DecResult<Expr> {
        let line = self.line();
        match self.peek().kind {
            TokenKind::Plus => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    kind: ExprKind::Unary {
                        op: UnaryOp::Pos,
                        operand: Box::new(operand),
                    },
                    line,
                })
            }
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                if let ExprKind::TritLit(TritLiteral::PosOne) = operand.kind {
                    return Ok(Expr {
                        kind: ExprKind::TritLit(TritLiteral::NegOne),
                        line,
                    });
                }
                Ok(Expr {
                    kind: ExprKind::Unary {
                        op: UnaryOp::Neg,
                        operand: Box::new(operand),
                    },
                    line,
                })
            }
            TokenKind::Tilde => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    kind: ExprKind::Unary {
                        op: UnaryOp::BitNot,
                        operand: Box::new(operand),
                    },
                    line,
                })
            }
            TokenKind::BoundaryOp => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    kind: ExprKind::Unary {
                        op: UnaryOp::Boundary,
                        operand: Box::new(operand),
                    },
                    line,
                })
            }
            TokenKind::Adverb(op) => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    kind: ExprKind::Adverb {
                        op,
                        operand: Box::new(operand),
                    },
                    line,
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> DecResult<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            let line = self.line();
            if self.check(&TokenKind::LParen) {
                if let ExprKind::Ident(ref name) = expr.kind {
                    if name == "box" {
                        self.advance();
                        let mut dims = Vec::new();
                        if self.check(&TokenKind::LBracket) {
                            self.advance();
                            while !self.check(&TokenKind::RBracket) {
                                dims.push(self.parse_expr()?);
                                if self.check(&TokenKind::Comma) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            self.expect(&TokenKind::RBracket, "']'")?;
                        } else {
                            while !self.check(&TokenKind::RParen) {
                                dims.push(self.parse_expr()?);
                                if self.check(&TokenKind::Comma) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                        }
                        self.expect(&TokenKind::RParen, "')'")?;
                        expr = Expr {
                            kind: ExprKind::BoxExpr(dims),
                            line,
                        };
                        continue;
                    }
                }
                self.advance();
                let mut args = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.check(&TokenKind::Comma) {
                            self.advance();
                            if self.check(&TokenKind::RParen) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RParen, "')'")?;
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    line,
                };
            } else if self.check(&TokenKind::Dot) {
                self.advance();
                let field = self.expect_ident("field name")?;
                expr = Expr {
                    kind: ExprKind::Field {
                        base: Box::new(expr),
                        name: field,
                    },
                    line,
                };
            } else if self.check(&TokenKind::LBracket) {
                self.advance();
                let index = self.parse_expr()?;
                self.expect(&TokenKind::RBracket, "']'")?;
                expr = Expr {
                    kind: ExprKind::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    },
                    line,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> DecResult<Expr> {
        let tok = self.peek().clone();
        let line = tok.line;
        match tok.kind {
            TokenKind::Null => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::NullLit,
                    line,
                })
            }
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::IntLit(n),
                    line,
                })
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::BoolLit(true),
                    line,
                })
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::BoolLit(false),
                    line,
                })
            }
            TokenKind::BytesLit(bs) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::BytesLit(bs),
                    line,
                })
            }
            TokenKind::RatioLit(r) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::RatioLit(r),
                    line,
                })
            }
            TokenKind::Addr8Lit(a) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Addr8Lit(a),
                    line,
                })
            }
            TokenKind::SupportLit(sf) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::SupportLit(sf),
                    line,
                })
            }
            TokenKind::TritLit(t) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::TritLit(t),
                    line,
                })
            }
            TokenKind::Match => self.parse_match_expr(),
            TokenKind::Simulate => {
                self.advance();
                let prepare = self.expect_ident("prepare name")?;
                let mut observers = Vec::new();
                if self.check_ident_eq("with") {
                    self.advance();
                    loop {
                        observers.push(self.expect_ident("observer name")?);
                        if self.check(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                if self.check_ident_eq("over") || self.check_ident_eq("for") {
                    self.advance();
                }
                let ticks = self.parse_expr()?;
                if self.check_ident_eq("ticks") {
                    self.advance();
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        self.line(),
                        "expected `ticks` after simulate count (e.g. `simulate Prep over 12 ticks`)",
                    ));
                }
                if self.check_ident_eq("with") {
                    self.advance();
                    loop {
                        observers.push(self.expect_ident("observer name")?);
                        if self.check(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                Ok(Expr {
                    kind: ExprKind::Simulate {
                        prepare,
                        ticks: Box::new(ticks),
                        observers,
                    },
                    line,
                })
            }
            TokenKind::Ident(first) => {
                self.advance();
                // Map literal: `map[(k1, v1), ...]` or `map[k1: v1, ...]` or `map[]`
                if first == "map" && self.check(&TokenKind::LBracket) {
                    self.advance();
                    let mut entries = Vec::new();
                    while !self.check(&TokenKind::RBracket) {
                        if self.check(&TokenKind::LParen) {
                            self.advance();
                            let k = self.parse_expr()?;
                            self.expect(&TokenKind::Comma, "',' in map[(k, v)] entry")?;
                            let v = self.parse_expr()?;
                            self.expect(&TokenKind::RParen, "')' in map[(k, v)] entry")?;
                            entries.push((k, v));
                        } else {
                            let k = self.parse_expr()?;
                            self.expect(&TokenKind::Colon, "':' in map[k: v] entry")?;
                            let v = self.parse_expr()?;
                            entries.push((k, v));
                        }
                        if self.check(&TokenKind::Comma) {
                            self.advance();
                            if self.check(&TokenKind::RBracket) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RBracket, "']' in map literal")?;
                    return Ok(Expr {
                        kind: ExprKind::MapLit(entries),
                        line,
                    });
                }
                let mut segs = vec![first];
                while self.check(&TokenKind::ColonColon) {
                    self.advance();
                    segs.push(self.expect_ident("path segment")?);
                }
                if segs.len() == 1 {
                    Ok(Expr {
                        kind: ExprKind::Ident(segs.into_iter().next().unwrap()),
                        line,
                    })
                } else {
                    Ok(Expr {
                        kind: ExprKind::Path(segs),
                        line,
                    })
                }
            }
            TokenKind::LParen => {
                self.advance();
                if self.check(&TokenKind::RParen) {
                    self.advance();
                    return Ok(Expr {
                        kind: ExprKind::Tuple(Vec::new()),
                        line,
                    });
                }
                let e1 = self.parse_expr()?;
                if self.check(&TokenKind::Comma) {
                    self.advance();
                    let e2 = self.parse_expr()?;
                    if self.check(&TokenKind::RBracket) {
                        self.advance();
                        return Ok(Expr {
                            kind: ExprKind::WindowLit {
                                start: Box::new(e1),
                                end: Box::new(e2),
                            },
                            line,
                        });
                    }
                    let mut elems = vec![e1, e2];
                    while self.check(&TokenKind::Comma) {
                        self.advance();
                        if self.check(&TokenKind::RParen) {
                            break;
                        }
                        elems.push(self.parse_expr()?);
                    }
                    self.expect(&TokenKind::RParen, "')'")?;
                    Ok(Expr {
                        kind: ExprKind::Tuple(elems),
                        line,
                    })
                } else {
                    self.expect(&TokenKind::RParen, "')'")?;
                    Ok(e1)
                }
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                while !self.check(&TokenKind::RBracket) {
                    items.push(self.parse_expr()?);
                    if self.check(&TokenKind::Comma) {
                        self.advance();
                        if self.check(&TokenKind::RBracket) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket, "']'")?;
                Ok(Expr {
                    kind: ExprKind::SeqLit(items),
                    line,
                })
            }
            other => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                tok.line,
                tok.col,
                format!("unexpected token in expression: {other:?}"),
            )),
        }
    }

    fn parse_match_expr(&mut self) -> DecResult<Expr> {
        let line = self.line();
        self.expect(&TokenKind::Match, "'match'")?;
        let scrutinee = self.parse_expr()?;
        self.expect(&TokenKind::Colon, "':' after match scrutinee")?;
        self.expect(&TokenKind::Newline, "newline after match header")?;
        self.expect(&TokenKind::Indent, "indented match arms")?;
        self.skip_newlines();
        let mut arms = Vec::new();
        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            let aline = self.line();
            let pattern = self.parse_pattern()?;
            let guard = if self.check(&TokenKind::Where) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            if self.check(&TokenKind::Arrow) || self.check(&TokenKind::Colon) {
                self.advance();
            } else {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    aline,
                    "expected '->' after match pattern",
                ));
            }
            let body = self.parse_expr()?;
            self.expect_line_end()?;
            self.skip_newlines();
            arms.push(MatchExprArm {
                pattern,
                guard,
                body,
                line: aline,
            });
        }
        self.expect(&TokenKind::Dedent, "dedent after match")?;
        self.just_closed_block = true;
        if arms.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                "match expression requires at least one arm",
            ));
        }
        Ok(Expr {
            kind: ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            line,
        })
    }
}

fn promote_tail_return(body: &mut [Stmt], return_type: &TypeExpr) {
    if matches!(return_type, TypeExpr::Unit) {
        return;
    }
    if let Some(last) = body.last_mut() {
        if let StmtKind::ExprStmt(ref expr) = last.kind {
            last.kind = StmtKind::Return(Some(expr.clone()));
        }
    }
}
