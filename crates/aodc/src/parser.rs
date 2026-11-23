use aod_core::bigint::BigInt;
use aod_core::ternary::T1;

use crate::ast::{
    AssignTarget, AxisDir, BinaryOp, CallArg, ChamberDecl, ClockDecl, ConstDecl, DecArm,
    DecArmParam, DecFnDecl, DecFnKind, EnumDecl, EnumVariantDecl, Expr, ExprKind, ForTarget,
    InjectDecl, Item, MatchArm, ObserveDecl, Pattern, PrepareDecl, ProbeDecl, ProbeKind, Program,
    QuarantineModDecl, Stmt, StmtKind, StructDecl, TritLiteral, TypeExpr, TypedParam, UnaryOp,
    UseDecl, WallPolicyAst,
};
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode};
use crate::lexer::{tokenize, Token, TokenKind};

pub const MAX_SOURCE_BYTES: usize = 1_000_000;

pub fn parse_source(source: &str, compat: CompatMode) -> DecResult<Program> {
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
        compat,
        pending_gt: false,
        generic_depth: 0,
    };
    parser.parse_program()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    compat: CompatMode,
    /// `P2-2`: supports nested generic closings like `ReadOnly<QExt<ratio>>` where lexer emitted `>>`.
    pending_gt: bool,
    generic_depth: usize,
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

    fn parse_program(&mut self) -> DecResult<Program> {
        let mut items = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Eof) {
            items.push(self.parse_top_item(false)?);
            self.skip_newlines();
        }
        if self.compat == CompatMode::V1 && items.is_empty() {
            return Err(Diagnostic::new(
                DiagnosticCode::ParseError,
                "empty source: no Dec declarations",
            ));
        }
        Ok(Program { items })
    }

    fn parse_top_item(&mut self, in_quarantine_mod: bool) -> DecResult<Item> {
        let line = self.line();
        if self.compat == CompatMode::V1 && !self.check(&TokenKind::Dec) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                "expected Dec name(parameters) -> result:; unsupported top-level syntax in --compat v1",
            ));
        }
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
        let mut items = None;
        while self.check(&TokenKind::ColonColon) {
            self.advance();
            if self.check(&TokenKind::LBrace) {
                self.advance();
                let mut list = Vec::new();
                while !self.check(&TokenKind::RBrace) {
                    list.push(self.expect_ident("imported item")?);
                    if self.check(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RBrace, "'}'")?;
                items = Some(list);
                break;
            } else {
                path.push(self.expect_ident("path segment")?);
            }
        }
        self.expect_line_end()?;
        Ok(UseDecl { path, items, line })
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
        let mut fields = Vec::new();

        if self.check(&TokenKind::LBrace) {
            self.advance();
            while !self.check(&TokenKind::RBrace) {
                let fname = self.expect_ident("struct field name")?;
                self.expect(&TokenKind::Colon, "':' after field name")?;
                let fty = self.parse_type_expr()?;
                fields.push((fname, fty));
                if self.check(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RBrace, "'}'")?;
            self.expect_line_end()?;
        } else {
            self.expect(&TokenKind::Colon, "':' or '{' after struct name")?;
            self.expect(&TokenKind::Newline, "newline after struct header")?;
            self.expect(&TokenKind::Indent, "indented struct fields")?;
            self.skip_newlines();
            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                let fname = self.expect_ident("struct field name")?;
                self.expect(&TokenKind::Colon, "':' after field name")?;
                let fty = self.parse_type_expr()?;
                fields.push((fname, fty));
                self.expect_line_end()?;
                self.skip_newlines();
            }
            self.expect(&TokenKind::Dedent, "dedent after struct")?;
        }
        Ok(StructDecl { name, fields, line })
    }

    fn parse_enum(&mut self) -> DecResult<EnumDecl> {
        let line = self.line();
        self.expect(&TokenKind::Enum, "'enum'")?;
        let name = self.expect_ident("enum name")?;
        let mut variants = Vec::new();

        if self.check(&TokenKind::LBrace) {
            self.advance();
            while !self.check(&TokenKind::RBrace) {
                variants.push(self.parse_enum_variant()?);
                if self.check(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RBrace, "'}'")?;
            self.expect_line_end()?;
        } else {
            self.expect(&TokenKind::Colon, "':' or '{' after enum name")?;
            self.expect(&TokenKind::Newline, "newline after enum header")?;
            self.expect(&TokenKind::Indent, "indented enum variants")?;
            self.skip_newlines();
            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                variants.push(self.parse_enum_variant()?);
                self.expect_line_end()?;
                self.skip_newlines();
            }
            self.expect(&TokenKind::Dedent, "dedent after enum")?;
        }
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
            let inner = self.parse_type_expr_inner()?;
            self.expect(&TokenKind::RBracket, "']'")?;
            return Ok(TypeExpr::List(Box::new(inner)));
        }
        let name = self.expect_ident("type name")?;
        if name == "map" && self.check(&TokenKind::LBracket) {
            if self.compat == CompatMode::V1 {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "map[K, V] is not in --compat v1",
                ));
            }
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
        self.expect(&TokenKind::Dec, "'Dec'")?;
        let name = self.expect_ident("function name")?;
        self.expect(&TokenKind::LParen, "'('")?;

        let mut bare_params: Vec<String> = Vec::new();
        let mut typed_params: Vec<TypedParam> = Vec::new();
        let mut saw_typed = false;
        let mut saw_bare = false;

        if !self.check(&TokenKind::RParen) {
            loop {
                let first = self.expect_ident("parameter")?;
                if self.check(&TokenKind::Colon) {
                    saw_typed = true;
                    self.advance();
                    let ty = self.parse_type_expr()?;
                    typed_params.push(TypedParam { name: first, ty });
                } else if matches!(&self.peek().kind, TokenKind::Ident(_)) {
                    saw_typed = true;
                    let pname = self.expect_ident("parameter name")?;
                    typed_params.push(TypedParam {
                        name: pname,
                        ty: TypeExpr::Named(first),
                    });
                } else {
                    saw_bare = true;
                    bare_params.push(first);
                }
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
        if saw_typed && saw_bare {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                "cannot mix typed and untyped parameters in Dec header",
            ));
        }

        let mut arrow_ident: Option<String> = None;
        let mut arrow_type: Option<TypeExpr> = None;
        if self.check(&TokenKind::Arrow) {
            self.advance();
            if self.check(&TokenKind::LParen) && self.peek_n(1).kind == TokenKind::RParen {
                self.advance();
                self.advance();
                arrow_type = Some(TypeExpr::Unit);
            } else if matches!(&self.peek().kind, TokenKind::Ident(_))
                && self.peek_n(1).kind == TokenKind::Colon
            {
                let id = self.expect_ident("return binding or type")?;
                arrow_ident = Some(id.clone());
                arrow_type = Some(TypeExpr::Named(id));
            } else {
                arrow_type = Some(self.parse_type_expr()?);
            }
        }

        self.expect(&TokenKind::Colon, "':'")?;
        self.expect(&TokenKind::Newline, "newline after Dec header")?;
        self.expect(&TokenKind::Indent, "indented Dec body")?;
        self.skip_newlines();

        let is_multi_arm = self.looks_like_dec_arm();
        if self.compat == CompatMode::V1 && !is_multi_arm {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                self.line(),
                "expected a typed signature row in --compat v1",
            ));
        }

        if is_multi_arm {
            if self.compat == CompatMode::V1 && saw_typed {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "multi-arm Dec header must list untyped parameter names in --compat v1",
                ));
            }
            if self.compat == CompatMode::V1 && arrow_ident.is_none() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "multi-arm Dec declaration requires `-> output_name:`",
                ));
            }
            let header_params = if saw_typed {
                typed_params.iter().map(|p| p.name.clone()).collect()
            } else {
                bare_params
            };
            let header_typed_params = if saw_typed { Some(typed_params) } else { None };
            let output_name = if saw_typed { None } else { arrow_ident };
            let header_return_type = if saw_typed { arrow_type } else { None };

            let mut arms = Vec::new();
            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                arms.push(self.parse_dec_arm()?);
                self.skip_newlines();
            }
            self.expect(&TokenKind::Dedent, "dedent after Dec arms")?;
            if arms.is_empty() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "Dec declaration has no signature rows",
                ));
            }
            Ok(DecFnDecl {
                name,
                kind: DecFnKind::MultiArm {
                    header_params,
                    header_typed_params,
                    output_name,
                    header_return_type,
                    arms,
                },
                line,
            })
        } else {
            if saw_bare {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "single-arm Dec parameters require explicit types (`name: Type`)",
                ));
            }
            let return_type = arrow_type.unwrap_or(TypeExpr::Unit);
            let body = self.parse_block_stmts_already_indented()?;
            Ok(DecFnDecl {
                name,
                kind: DecFnKind::SingleArm {
                    params: typed_params,
                    return_type,
                    body,
                },
                line,
            })
        }
    }

    fn looks_like_dec_arm(&self) -> bool {
        if !self.check(&TokenKind::LParen) {
            return false;
        }
        let mut depth = 0usize;
        let mut i = self.pos;
        while i < self.tokens.len() {
            match self.tokens[i].kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        // Look for `->` directly OR `if ... ->` on the same line
                        let mut j = i + 1;
                        while j < self.tokens.len() {
                            match self.tokens[j].kind {
                                TokenKind::Arrow => return true,
                                TokenKind::Newline | TokenKind::Eof | TokenKind::Colon => {
                                    return false
                                }
                                _ => j += 1,
                            }
                        }
                        return false;
                    }
                }
                TokenKind::Newline | TokenKind::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
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

        let mut guard: Option<Expr> = None;
        if self.check(&TokenKind::If) {
            if self.compat == CompatMode::V1 {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "arm guards are not in --compat v1",
                ));
            }
            self.advance();
            guard = Some(self.parse_expr()?);
        }

        self.expect(&TokenKind::Arrow, "'->'")?;
        let return_type = self.parse_type_expr()?;
        let output_name = if matches!(&self.peek().kind, TokenKind::Ident(_)) {
            Some(self.expect_ident("arm result binding")?)
        } else {
            if self.compat == CompatMode::V1 {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "arm signature requires a result binding in --compat v1",
                ));
            }
            None
        };

        if self.check(&TokenKind::If) {
            if self.compat == CompatMode::V1 {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "arm guards are not in --compat v1",
                ));
            }
            if guard.is_some() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "duplicate guard on Dec arm",
                ));
            }
            self.advance();
            guard = Some(self.parse_expr()?);
        }

        self.expect(&TokenKind::Colon, "':'")?;
        self.expect(&TokenKind::Newline, "newline after arm signature")?;
        self.expect(&TokenKind::Indent, "indented arm body")?;
        let body = self.parse_block_stmts_already_indented()?;
        Ok(DecArm {
            params,
            guard,
            return_type,
            output_name,
            body,
            line,
        })
    }

    fn parse_dec_arm_param(&mut self) -> DecResult<DecArmParam> {
        let line = self.line();
        if self.compat == CompatMode::V1 {
            let ty = self.parse_type_expr()?;
            let name = self.expect_ident("arm parameter name")?;
            return Ok(DecArmParam::TypedVar { ty, name });
        }

        // Check for pure pattern params: `_`, `Enum::Var(...)`, literals, `+`/`-` literals, or bare ident before `,`/`)`
        let is_pure_pattern = match &self.peek().kind {
            TokenKind::IntLit(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::StrLit(_)
            | TokenKind::RatioLit(_)
            | TokenKind::Addr8Lit(_)
            | TokenKind::SupportLit(_)
            | TokenKind::TritLit(_)
            | TokenKind::Plus
            | TokenKind::Minus => true,
            TokenKind::Ident(s) => {
                s == "_"
                    || self.peek_n(1).kind == TokenKind::ColonColon
                    || matches!(self.peek_n(1).kind, TokenKind::Comma | TokenKind::RParen)
            }
            _ => false,
        };
        if is_pure_pattern {
            let pat = self.parse_pattern()?;
            return Ok(DecArmParam::Pattern(pat));
        }

        // Otherwise parse `TypeExpr` followed by either binding name or literal/wildcard/enum pattern!
        let ty = self.parse_type_expr()?;
        if let TokenKind::Ident(ref s) = self.peek().kind {
            if s != "_" && self.peek_n(1).kind != TokenKind::ColonColon {
                let name = self.expect_ident("arm parameter name")?;
                return Ok(DecArmParam::TypedVar { ty, name });
            }
        }
        let pat = self.parse_pattern().map_err(|_| {
            Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                "expected parameter name or pattern after type in Dec arm",
            )
        })?;
        Ok(DecArmParam::TypedPattern { ty, pattern: pat })
    }

    fn parse_pattern(&mut self) -> DecResult<Pattern> {
        let tok = self.peek().clone();
        let line = tok.line;
        match tok.kind {
            TokenKind::Ident(s) if s == "_" => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            TokenKind::True => {
                self.advance();
                Ok(Pattern::BoolLit(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Pattern::BoolLit(false))
            }
            TokenKind::StrLit(s) => {
                self.advance();
                Ok(Pattern::StrLit(s))
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
                let t = match tl {
                    TritLiteral::One => T1::POS,
                    TritLiteral::Zero | TritLiteral::Hinge0H | TritLiteral::Dwell0D => T1::ZERO,
                };
                Ok(Pattern::TritLit(t))
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
                        let base = match tl {
                            TritLiteral::One => T1::POS,
                            TritLiteral::Zero | TritLiteral::Hinge0H | TritLiteral::Dwell0D => {
                                T1::ZERO
                            }
                        };
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
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "')' in tuple pattern")?;
                Ok(Pattern::Tuple(pats))
            }
            TokenKind::Ident(first) => {
                self.advance();
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
                        TokenKind::StrLit(s) | TokenKind::Ident(s) => {
                            let s = s.clone();
                            self.advance();
                            s
                        }
                        other => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ParseError,
                                fline,
                                format!("expected engine identifier or string, got {other:?}"),
                            ));
                        }
                    };
                    engine = Some(eng);
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
                if self.check(&TokenKind::Colon) {
                    self.advance();
                    let _ = self.parse_type_expr()?;
                }
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
                let body = self.parse_block_stmts_already_indented()?;
                probes.push(ProbeDecl::Procedural {
                    kind,
                    name: pname,
                    param_name,
                    return_type,
                    body,
                    line: pline,
                });
            } else {
                self.expect(&TokenKind::Colon, "':'")?;
                self.expect(&TokenKind::Newline, "newline after probe header")?;
                self.expect(&TokenKind::Indent, "indented probe body")?;
                self.skip_newlines();
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
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent, "dedent after observe")?;
        Ok(ObserveDecl { name, probes, line })
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
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`let` is not in --compat v1",
                    ));
                }
                self.advance();
                let name = self.expect_ident("binding name")?;
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
                    kind: StmtKind::Let { name, ty, value },
                    line,
                })
            }
            TokenKind::Mut => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`mut` is not in --compat v1",
                    ));
                }
                self.advance();
                let name = self.expect_ident("mutable binding name")?;
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
                    kind: StmtKind::Mut { name, ty, value },
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
            TokenKind::If => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "nested `if` blocks are not in --compat v1",
                    ));
                }
                self.advance();
                let cond = self.parse_expr()?;
                self.expect(&TokenKind::Colon, "':'")?;
                self.expect(&TokenKind::Newline, "newline")?;
                self.expect(&TokenKind::Indent, "indent")?;
                let then_body = self.parse_block_stmts_already_indented()?;
                self.skip_newlines();
                let mut elifs = Vec::new();
                while self.check(&TokenKind::Elif) {
                    self.advance();
                    let ec = self.parse_expr()?;
                    self.expect(&TokenKind::Colon, "':'")?;
                    self.expect(&TokenKind::Newline, "newline")?;
                    self.expect(&TokenKind::Indent, "indent")?;
                    let eb = self.parse_block_stmts_already_indented()?;
                    elifs.push((ec, eb));
                    self.skip_newlines();
                }
                let else_body = if self.check(&TokenKind::Else) {
                    self.advance();
                    self.expect(&TokenKind::Colon, "':'")?;
                    self.expect(&TokenKind::Newline, "newline")?;
                    self.expect(&TokenKind::Indent, "indent")?;
                    self.parse_block_stmts_already_indented()?
                } else {
                    Vec::new()
                };
                Ok(Stmt {
                    kind: StmtKind::If {
                        cond,
                        then_body,
                        elifs,
                        else_body,
                    },
                    line,
                })
            }
            TokenKind::Match => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`match` is not in --compat v1",
                    ));
                }
                self.advance();
                let scrutinee = self.parse_expr()?;
                self.expect(&TokenKind::Colon, "':' after match expression")?;
                self.expect(&TokenKind::Newline, "newline after match header")?;
                self.expect(&TokenKind::Indent, "indented match arms")?;
                self.skip_newlines();
                let mut arms = Vec::new();
                while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                    let aline = self.line();
                    let pattern = self.parse_pattern()?;
                    let guard = if self.check(&TokenKind::If) {
                        self.advance();
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    self.expect(&TokenKind::Colon, "':' after match pattern")?;
                    let body = if self.check(&TokenKind::Newline) {
                        self.advance();
                        self.expect(&TokenKind::Indent, "indented match arm body")?;
                        self.parse_block_stmts_already_indented()?
                    } else {
                        vec![self.parse_stmt()?]
                    };
                    arms.push(MatchArm {
                        pattern,
                        guard,
                        body,
                        line: aline,
                    });
                    self.skip_newlines();
                }
                self.expect(&TokenKind::Dedent, "dedent after match")?;
                if arms.is_empty() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "match requires at least one arm",
                    ));
                }
                Ok(Stmt {
                    kind: StmtKind::Match { scrutinee, arms },
                    line,
                })
            }
            TokenKind::For => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`for` loops are not in --compat v1",
                    ));
                }
                self.advance();
                let target = if self.check(&TokenKind::LParen) {
                    self.advance();
                    let mut vars = Vec::new();
                    while !self.check(&TokenKind::RParen) {
                        vars.push(self.expect_ident("loop tuple variable")?);
                        if self.check(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen, "')'")?;
                    ForTarget::Tuple(vars)
                } else {
                    ForTarget::Single(self.expect_ident("loop variable")?)
                };
                self.expect(&TokenKind::In, "'in'")?;
                let iter = self.parse_expr()?;
                self.expect(&TokenKind::Colon, "':'")?;
                self.expect(&TokenKind::Newline, "newline")?;
                self.expect(&TokenKind::Indent, "indent")?;
                let body = self.parse_block_stmts_already_indented()?;
                Ok(Stmt {
                    kind: StmtKind::For { target, iter, body },
                    line,
                })
            }
            TokenKind::While => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`while` loops are not in --compat v1",
                    ));
                }
                self.advance();
                let cond = self.parse_expr()?;
                self.expect(&TokenKind::Colon, "':'")?;
                self.expect(&TokenKind::Newline, "newline")?;
                self.expect(&TokenKind::Indent, "indent")?;
                let body = self.parse_block_stmts_already_indented()?;
                Ok(Stmt {
                    kind: StmtKind::While { cond, body },
                    line,
                })
            }
            TokenKind::Break => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`break` is not in --compat v1",
                    ));
                }
                self.advance();
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: StmtKind::Break,
                    line,
                })
            }
            TokenKind::Continue => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`continue` is not in --compat v1",
                    ));
                }
                self.advance();
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: StmtKind::Continue,
                    line,
                })
            }
            TokenKind::Pass => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`pass` is not in --compat v1",
                    ));
                }
                self.advance();
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: StmtKind::Pass,
                    line,
                })
            }
            TokenKind::Quarantine => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`quarantine Q_ext` is not in --compat v1",
                    ));
                }
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
                if matches!(&self.peek().kind, TokenKind::Ident(_))
                    && matches!(&self.peek_n(1).kind, TokenKind::Ident(_))
                    && self.peek_n(2).kind == TokenKind::Eq
                {
                    let ty_name = self.expect_ident("type")?;
                    let name = self.expect_ident("variable")?;
                    self.expect(&TokenKind::Eq, "'='")?;
                    let value = self.parse_expr()?;
                    self.expect_line_end()?;
                    return Ok(Stmt {
                        kind: StmtKind::TypedLocal {
                            ty: TypeExpr::Named(ty_name),
                            name,
                            value,
                        },
                        line,
                    });
                }

                let lhs = self.parse_expr()?;
                if self.check(&TokenKind::Eq) {
                    self.advance();
                    let value = self.parse_expr()?;
                    self.expect_line_end()?;
                    let target = match lhs.kind {
                        ExprKind::Ident(name) => AssignTarget::Ident(name),
                        ExprKind::Field { base, name } => AssignTarget::Field { base, name },
                        ExprKind::Index { base, index } => AssignTarget::Index { base, index },
                        _ => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ParseError,
                                line,
                                "invalid assignment target",
                            ));
                        }
                    };
                    if self.compat == CompatMode::V1 && !matches!(target, AssignTarget::Ident(_)) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ParseError,
                            line,
                            "field/index assignment is not in --compat v1",
                        ));
                    }
                    Ok(Stmt {
                        kind: StmtKind::Assign { target, value },
                        line,
                    })
                } else {
                    if self.compat == CompatMode::V1 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ParseError,
                            line,
                            "unsupported statement in --compat v1; expected assignment, assert, emit or return",
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
    }

    fn expect_line_end(&mut self) -> DecResult<()> {
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
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> DecResult<Expr> {
        let yes = self.parse_or()?;
        if self.check(&TokenKind::If) {
            let line = self.line();
            self.advance();
            let cond = self.parse_or()?;
            self.expect(&TokenKind::Else, "'else' in conditional expression")?;
            let no = self.parse_ternary()?;
            Ok(Expr {
                kind: ExprKind::Ternary {
                    cond: Box::new(cond),
                    yes: Box::new(yes),
                    no: Box::new(no),
                },
                line,
            })
        } else {
            Ok(yes)
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
            self.parse_compare()
        }
    }

    fn parse_compare(&mut self) -> DecResult<Expr> {
        let lhs = self.parse_range()?;
        let op = match self.peek().kind {
            TokenKind::EqEq => Some(BinaryOp::Eq),
            TokenKind::NotEq => Some(BinaryOp::NotEq),
            TokenKind::Lt => Some(BinaryOp::Lt),
            TokenKind::LtEq => Some(BinaryOp::LtEq),
            TokenKind::Gt => Some(BinaryOp::Gt),
            TokenKind::GtEq => Some(BinaryOp::GtEq),
            _ => None,
        };
        if let Some(op) = op {
            let line = self.line();
            self.advance();
            let rhs = self.parse_range()?;
            if matches!(
                self.peek().kind,
                TokenKind::EqEq
                    | TokenKind::NotEq
                    | TokenKind::Lt
                    | TokenKind::LtEq
                    | TokenKind::Gt
                    | TokenKind::GtEq
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

    fn parse_range(&mut self) -> DecResult<Expr> {
        let lhs = self.parse_bitor()?;
        if self.check(&TokenKind::DotDot) {
            let line = self.line();
            if self.compat == CompatMode::V1 {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "range literals are not in --compat v1",
                ));
            }
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
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Shl => BinaryOp::Shl,
                TokenKind::Shr => BinaryOp::Shr,
                _ => break,
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
                Ok(Expr {
                    kind: ExprKind::Unary {
                        op: UnaryOp::Neg,
                        operand: Box::new(operand),
                    },
                    line,
                })
            }
            TokenKind::BoundaryOp => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "boundary operator ∂ is not in --compat v1",
                    ));
                }
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
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> DecResult<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            let line = self.line();
            if self.check(&TokenKind::LParen) {
                if let ExprKind::Ident(ref name) = expr.kind {
                    if name == "box" && self.compat == CompatMode::V2 {
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
                        if matches!(&self.peek().kind, TokenKind::Ident(_))
                            && self.peek_n(1).kind == TokenKind::Colon
                        {
                            if self.compat == CompatMode::V1 {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ParseError,
                                    self.line(),
                                    "named call arguments are not in --compat v1",
                                ));
                            }
                            let arg_name = self.expect_ident("named argument")?;
                            self.advance();
                            let val = self.parse_expr()?;
                            args.push(CallArg {
                                name: Some(arg_name),
                                value: val,
                            });
                        } else {
                            let val = self.parse_expr()?;
                            args.push(CallArg {
                                name: None,
                                value: val,
                            });
                        }
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
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "field access is not in --compat v1",
                    ));
                }
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
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "indexing is not in --compat v1",
                    ));
                }
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
            TokenKind::StrLit(s) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::StrLit(s),
                    line,
                })
            }
            TokenKind::RatioLit(r) => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "ratio literals (a/br) are not in --compat v1",
                    ));
                }
                self.advance();
                Ok(Expr {
                    kind: ExprKind::RatioLit(r),
                    line,
                })
            }
            TokenKind::Addr8Lit(a) => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "octal anchor literals are not in --compat v1",
                    ));
                }
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Addr8Lit(a),
                    line,
                })
            }
            TokenKind::SupportLit(sf) => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "support literals are not in --compat v1",
                    ));
                }
                self.advance();
                Ok(Expr {
                    kind: ExprKind::SupportLit(sf),
                    line,
                })
            }
            TokenKind::TritLit(t) => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "trit literals (0t/1t/0_H/0_D) are not in --compat v1",
                    ));
                }
                self.advance();
                Ok(Expr {
                    kind: ExprKind::TritLit(t),
                    line,
                })
            }
            TokenKind::Simulate => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "`simulate` is not in --compat v1",
                    ));
                }
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
                self.expect(&TokenKind::For, "'for'")?;
                let ticks = self.parse_expr()?;
                if self.check_ident_eq("ticks") {
                    self.advance();
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        self.line(),
                        "expected `ticks` after simulate count",
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
                let mut segs = vec![first];
                if self.check(&TokenKind::ColonColon) {
                    if self.compat == CompatMode::V1 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ParseError,
                            line,
                            "module/enum paths are not in --compat v1",
                        ));
                    }
                    while self.check(&TokenKind::ColonColon) {
                        self.advance();
                        segs.push(self.expect_ident("path segment")?);
                    }
                }
                if self.check(&TokenKind::LBrace) {
                    if self.compat == CompatMode::V1 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ParseError,
                            line,
                            "struct literals are not in --compat v1",
                        ));
                    }
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.check(&TokenKind::RBrace) {
                        let fname = self.expect_ident("struct field name")?;
                        self.expect(&TokenKind::Colon, "':' in struct literal")?;
                        let val = self.parse_expr()?;
                        fields.push((fname, val));
                        if self.check(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RBrace, "'}' in struct literal")?;
                    return Ok(Expr {
                        kind: ExprKind::StructLit { path: segs, fields },
                        line,
                    });
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
            TokenKind::LBrace => {
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "map literals are not in --compat v1",
                    ));
                }
                self.advance();
                let mut entries = Vec::new();
                while !self.check(&TokenKind::RBrace) {
                    let k = self.parse_expr()?;
                    self.expect(&TokenKind::Colon, "':' in map literal")?;
                    let v = self.parse_expr()?;
                    entries.push((k, v));
                    if self.check(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RBrace, "'}' in map literal")?;
                Ok(Expr {
                    kind: ExprKind::MapLit(entries),
                    line,
                })
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
                    if self.compat == CompatMode::V1 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ParseError,
                            line,
                            "tuples and window literals are not in --compat v1",
                        ));
                    }
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
                if self.compat == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "list literals are not in --compat v1",
                    ));
                }
                self.advance();
                let mut items = Vec::new();
                while !self.check(&TokenKind::RBracket) {
                    items.push(self.parse_expr()?);
                    if self.check(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket, "']'")?;
                Ok(Expr {
                    kind: ExprKind::List(items),
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
}
