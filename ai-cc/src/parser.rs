use crate::{
    ast::*,
    diag::{Diagnostic, Span},
    lex::{Token, TokenKind},
};
use std::collections::HashMap;
pub struct Parser {
    t: Vec<Token>,
    p: usize,
    errors: Vec<Diagnostic>,
    aliases: HashMap<String, Type>,
}
impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            t: tokens,
            p: 0,
            errors: Vec::new(),
            aliases: HashMap::new(),
        }
    }
    fn cur(&self) -> &TokenKind {
        &self.t[self.p].kind
    }
    fn take(&mut self, k: &TokenKind) -> bool {
        if self.cur() == k {
            self.p += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, k: &TokenKind) -> Result<(), Diagnostic> {
        if self.take(k) {
            Ok(())
        } else {
            Err(Diagnostic::error(
                format!("expected {}, found {}", k, self.cur()),
                Some(self.t[self.p].span),
            ))
        }
    }
    fn ident(&mut self) -> Result<String, Diagnostic> {
        if let TokenKind::Ident(x) = self.cur().clone() {
            self.p += 1;
            Ok(x)
        } else {
            Err(Diagnostic::error(
                "expected identifier",
                Some(self.t[self.p].span),
            ))
        }
    }
    fn typ(&mut self) -> Result<Type, Diagnostic> {
        if self.take(&TokenKind::Const)
            || self.take(&TokenKind::Volatile)
            || self.take(&TokenKind::Restrict)
        {
            let q = match &self.t[self.p - 1].kind {
                TokenKind::Const => Qualifiers {
                    is_const: true,
                    ..Default::default()
                },
                TokenKind::Volatile => Qualifiers {
                    is_volatile: true,
                    ..Default::default()
                },
                _ => Qualifiers {
                    is_restrict: true,
                    ..Default::default()
                },
            };
            return Ok(Type::Qualified(Box::new(self.typ()?), q));
        }
        if self.take(&TokenKind::Struct) || self.take(&TokenKind::Union) {
            let union = self.t[self.p - 1].kind == TokenKind::Union;
            let tag = self.ident().unwrap_or_default();
            let mut fields = Vec::new();
            if self.take(&TokenKind::LBrace) {
                while *self.cur() != TokenKind::RBrace && *self.cur() != TokenKind::End {
                    let ft = self.typ()?;
                    let (name, ft) = self.declarator(ft)?;
                    self.expect(&TokenKind::Semicolon)?;
                    if fields.iter().any(|(n, _)| n == &name) {
                        return Err(Diagnostic::error(
                            format!("duplicate member {}", name),
                            None,
                        ));
                    }
                    fields.push((name, ft));
                }
                self.expect(&TokenKind::RBrace)?;
            }
            return Ok(if union {
                Type::Union(tag, fields)
            } else {
                Type::Struct(tag, fields)
            });
        }
        if self.take(&TokenKind::Enum) {
            let tag = if matches!(self.cur(), TokenKind::Ident(_)) {
                self.ident()?
            } else {
                String::new()
            };
            let mut values = Vec::new();
            let mut next = 0;
            if self.take(&TokenKind::LBrace) {
                while *self.cur() != TokenKind::RBrace && *self.cur() != TokenKind::End {
                    let n = self.ident()?;
                    if self.take(&TokenKind::Assign) {
                        if let TokenKind::Number(v) = self.cur().clone() {
                            next = v;
                            self.p += 1;
                        } else {
                            return Err(Diagnostic::error(
                                "enumerator requires integer constant",
                                None,
                            ));
                        }
                    }
                    values.push((n, next));
                    next += 1;
                    if !self.take(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RBrace)?;
            }
            return Ok(Type::Enum(tag, values));
        }
        let x = match self.cur() {
            TokenKind::Int
            | TokenKind::SizeT
            | TokenKind::PtrdiffT
            | TokenKind::WcharT
            | TokenKind::VaList
            | TokenKind::Int8T
            | TokenKind::Uint8T
            | TokenKind::Int16T
            | TokenKind::Uint16T
            | TokenKind::Int32T
            | TokenKind::Uint32T
            | TokenKind::Int64T
            | TokenKind::Uint64T
            | TokenKind::IntmaxT
            | TokenKind::UintmaxT => {
                self.p += 1;
                Type::Int
            }
            TokenKind::Signed => {
                self.p += 1;
                if self.take(&TokenKind::CharKw) {
                    Type::SignedChar
                } else if self.take(&TokenKind::Short) {
                    Type::Short
                } else if self.take(&TokenKind::Long) {
                    if self.take(&TokenKind::Long) {
                        Type::LongLong
                    } else {
                        Type::Long
                    }
                } else {
                    Type::SignedInt
                }
            }
            TokenKind::Unsigned => {
                self.p += 1;
                if self.take(&TokenKind::CharKw) {
                    Type::UnsignedChar
                } else if self.take(&TokenKind::Short) {
                    Type::UnsignedShort
                } else if self.take(&TokenKind::Long) {
                    if self.take(&TokenKind::Long) {
                        Type::UnsignedLongLong
                    } else {
                        Type::UnsignedLong
                    }
                } else {
                    self.take(&TokenKind::Int);
                    Type::UnsignedInt
                }
            }
            TokenKind::Short => {
                self.p += 1;
                Type::Short
            }
            TokenKind::Long => {
                self.p += 1;
                if self.take(&TokenKind::Double) {
                    Type::LongDouble
                } else if self.take(&TokenKind::Long) {
                    Type::LongLong
                } else {
                    Type::Long
                }
            }
            TokenKind::Float => {
                self.p += 1;
                Type::Float
            }
            TokenKind::Double => {
                self.p += 1;
                Type::Double
            }
            TokenKind::Bool => {
                self.p += 1;
                Type::Bool
            }
            TokenKind::CharKw => {
                self.p += 1;
                Type::Char
            }
            TokenKind::Ident(name) if self.aliases.contains_key(name) => {
                let ty = self.aliases.get(name).cloned().unwrap();
                self.p += 1;
                return Ok(ty);
            }
            TokenKind::Void => {
                self.p += 1;
                Type::Void
            }
            _ => {
                return Err(Diagnostic::error(
                    "expected type",
                    Some(self.t[self.p].span),
                ))
            }
        };
        Ok(x)
    }
    pub fn parse_translation_unit(&mut self) -> Result<TranslationUnit, Vec<Diagnostic>> {
        let mut v = Vec::new();
        let mut types = Vec::new();
        let mut e = Vec::new();
        while *self.cur() != TokenKind::End {
            if self.take(&TokenKind::Typedef) {
                match self.typ().and_then(|ty| {
                    let (name, ty) = self.declarator(ty)?;
                    self.expect(&TokenKind::Semicolon)?;
                    Ok((name, ty))
                }) {
                    Ok((name, ty)) => {
                        self.aliases.insert(name, ty);
                        continue;
                    }
                    Err(x) => {
                        e.push(x);
                        self.synchronize(false);
                        continue;
                    }
                }
            }
            let parsed = if matches!(
                self.cur(),
                TokenKind::Struct | TokenKind::Union | TokenKind::Enum
            ) {
                let ty = self.typ();
                match ty {
                    Ok(ty) if self.take(&TokenKind::Semicolon) => {
                        types.push(ty);
                        continue;
                    }
                    Ok(ty) => self.function_after_type(ty),
                    Err(x) => Err(x),
                }
            } else {
                self.function()
            };
            match parsed {
                Ok(x) => v.push(x),
                Err(x) => {
                    e.push(x);
                    self.synchronize(false);
                }
            }
        }
        e.append(&mut self.errors);
        if e.is_empty() {
            Ok(TranslationUnit { items: v, types })
        } else {
            Err(e)
        }
    }
    fn function(&mut self) -> Result<Function, Diagnostic> {
        self.take(&TokenKind::Static);
        let ret = self.typ()?;
        self.function_after_type(ret)
    }
    fn function_after_type(&mut self, ret: Type) -> Result<Function, Diagnostic> {
        let (name, ret) = self.declarator(ret)?;
        self.expect(&TokenKind::LParen)?;
        let mut params = Vec::new();
        if *self.cur() != TokenKind::RParen {
            if self.take(&TokenKind::Void) && *self.cur() == TokenKind::RParen {
                self.expect(&TokenKind::RParen)?;
                return self.finish_function(name, ret, params);
            }
            loop {
                let ty = self.typ()?;
                let (n, ty) = self.declarator(ty)?;
                params.push((n, ty));
                if !self.take(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen)?;
        self.finish_function(name, ret, params)
    }
    fn finish_function(
        &mut self,
        name: String,
        ret: Type,
        params: Vec<(String, Type)>,
    ) -> Result<Function, Diagnostic> {
        let start = self.t[self.p].span.start;
        let external = self.take(&TokenKind::Semicolon);
        let body = if external {
            Block { stmts: Vec::new() }
        } else {
            self.block()?
        };
        Ok(Function {
            name,
            ret,
            params,
            body,
            external,
            span: Span {
                start,
                end: self.t[self.p - 1].span.end,
            },
        })
    }
    fn block(&mut self) -> Result<Block, Diagnostic> {
        self.expect(&TokenKind::LBrace)?;
        let mut v = Vec::new();
        while *self.cur() != TokenKind::RBrace && *self.cur() != TokenKind::End {
            match self.stmt() {
                Ok(stmt) => v.push(stmt),
                Err(error) => {
                    self.errors.push(error);
                    self.synchronize(true);
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Block { stmts: v })
    }
    fn synchronize(&mut self, in_block: bool) {
        while *self.cur() != TokenKind::End
            && (!in_block || *self.cur() != TokenKind::RBrace)
            && *self.cur() != TokenKind::Semicolon
        {
            self.p += 1;
        }
        self.take(&TokenKind::Semicolon);
    }
    fn stmt(&mut self) -> Result<Stmt, Diagnostic> {
        if self.take(&TokenKind::LBrace) {
            self.p -= 1;
            return Ok(Stmt::Block(self.block()?));
        }
        if self.take(&TokenKind::Return) {
            let x = if *self.cur() == TokenKind::Semicolon {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt::Return(x));
        }
        if self.take(&TokenKind::If) {
            self.expect(&TokenKind::LParen)?;
            let c = self.expr()?;
            self.expect(&TokenKind::RParen)?;
            let a = Box::new(self.stmt()?);
            let b = if self.take(&TokenKind::Else) {
                Some(Box::new(self.stmt()?))
            } else {
                None
            };
            return Ok(Stmt::If(c, a, b));
        }
        if self.take(&TokenKind::While) {
            self.expect(&TokenKind::LParen)?;
            let c = self.expr()?;
            self.expect(&TokenKind::RParen)?;
            return Ok(Stmt::While(c, Box::new(self.stmt()?)));
        }
        if self.take(&TokenKind::Do) {
            let body = Box::new(self.stmt()?);
            self.expect(&TokenKind::While)?;
            self.expect(&TokenKind::LParen)?;
            let c = self.expr()?;
            self.expect(&TokenKind::RParen)?;
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt::DoWhile(body, c));
        }
        if self.take(&TokenKind::For) {
            self.expect(&TokenKind::LParen)?;
            let init = if self.take(&TokenKind::Semicolon) {
                None
            } else {
                let s = if self.is_type() {
                    self.declaration()?
                } else {
                    let e = self.expr()?;
                    self.expect(&TokenKind::Semicolon)?;
                    Stmt::Expr(e)
                };
                Some(Box::new(s))
            };
            let cond = if self.take(&TokenKind::Semicolon) {
                None
            } else {
                let e = self.expr()?;
                self.expect(&TokenKind::Semicolon)?;
                Some(e)
            };
            let step = if self.take(&TokenKind::RParen) {
                None
            } else {
                let e = self.expr()?;
                self.expect(&TokenKind::RParen)?;
                Some(e)
            };
            return Ok(Stmt::For(init, cond, step, Box::new(self.stmt()?)));
        }
        if self.take(&TokenKind::Break) {
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt::Break);
        }
        if self.take(&TokenKind::Continue) {
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt::Continue);
        }
        if self.is_type() {
            return self.declaration();
        }
        let x = self.expr()?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Expr(x))
    }
    fn is_type(&self) -> bool {
        matches!(self.cur(), TokenKind::Ident(name) if self.aliases.contains_key(name))
            || matches!(
                self.cur(),
                TokenKind::Int
                    | TokenKind::Bool
                    | TokenKind::CharKw
                    | TokenKind::Void
                    | TokenKind::SizeT
                    | TokenKind::PtrdiffT
                    | TokenKind::WcharT
                    | TokenKind::VaList
                    | TokenKind::Int8T
                    | TokenKind::Uint8T
                    | TokenKind::Int16T
                    | TokenKind::Uint16T
                    | TokenKind::Int32T
                    | TokenKind::Uint32T
                    | TokenKind::Int64T
                    | TokenKind::Uint64T
                    | TokenKind::IntmaxT
                    | TokenKind::UintmaxT
                    | TokenKind::Struct
                    | TokenKind::Union
                    | TokenKind::Enum
                    | TokenKind::Const
                    | TokenKind::Volatile
                    | TokenKind::Restrict
                    | TokenKind::Signed
                    | TokenKind::Unsigned
                    | TokenKind::Short
                    | TokenKind::Long
                    | TokenKind::Float
                    | TokenKind::Double
            )
    }
    fn declaration(&mut self) -> Result<Stmt, Diagnostic> {
        let ty = self.typ()?;
        let (n, ty) = self.declarator(ty)?;
        let x = if self.take(&TokenKind::Assign) {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Decl(n, ty, x))
    }
    fn declarator(&mut self, mut ty: Type) -> Result<(String, Type), Diagnostic> {
        while self.take(&TokenKind::Star) {
            ty = Type::Pointer(Box::new(ty));
        }
        let n = self.ident()?;
        let mut arrays = Vec::new();
        while self.take(&TokenKind::LBracket) {
            let size = if let TokenKind::Number(n) = self.cur().clone() {
                self.p += 1;
                Some(n as usize)
            } else {
                None
            };
            self.expect(&TokenKind::RBracket)?;
            arrays.push(size);
        }
        for size in arrays.into_iter().rev() {
            ty = Type::Array(Box::new(ty), size);
        }
        Ok((n, ty))
    }
    fn expr(&mut self) -> Result<Expr, Diagnostic> {
        let mut x = self.assign()?;
        while self.take(&TokenKind::Comma) {
            x = Expr::Binary(BinOp::Comma, Box::new(x), Box::new(self.assign()?));
        }
        Ok(x)
    }
    fn assign(&mut self) -> Result<Expr, Diagnostic> {
        let x = self.binary(1)?;
        if self.take(&TokenKind::Question) {
            let a = self.expr()?;
            self.expect(&TokenKind::Colon)?;
            let b = self.assign()?;
            return Ok(Expr::Conditional(Box::new(x), Box::new(a), Box::new(b)));
        }
        let op = match self.cur() {
            TokenKind::Assign => Some(None),
            TokenKind::AddAssign => Some(Some(BinOp::Add)),
            TokenKind::SubAssign => Some(Some(BinOp::Sub)),
            TokenKind::MulAssign => Some(Some(BinOp::Mul)),
            TokenKind::DivAssign => Some(Some(BinOp::Div)),
            TokenKind::RemAssign => Some(Some(BinOp::Rem)),
            TokenKind::ShlAssign => Some(Some(BinOp::Shl)),
            TokenKind::ShrAssign => Some(Some(BinOp::Shr)),
            TokenKind::AndAssign => Some(Some(BinOp::BitAnd)),
            TokenKind::XorAssign => Some(Some(BinOp::BitXor)),
            TokenKind::OrAssign => Some(Some(BinOp::BitOr)),
            _ => None,
        };
        if let Some(op) = op {
            self.p += 1;
            if Self::is_lvalue(&x) {
                let rhs = self.assign()?;
                Ok(Expr::Assign(
                    Box::new(x.clone()),
                    if let Some(op) = op {
                        Box::new(Expr::Binary(op, Box::new(x), Box::new(rhs)))
                    } else {
                        Box::new(rhs)
                    },
                ))
            } else {
                Err(Diagnostic::error("assignment target required", None))
            }
        } else {
            Ok(x)
        }
    }
    fn is_lvalue(x: &Expr) -> bool {
        matches!(
            x,
            Expr::Var(_)
                | Expr::Member(_, _, _)
                | Expr::Subscript(_, _)
                | Expr::Unary(UnOp::Deref, _)
        )
    }
    fn prec(k: &TokenKind) -> Option<(u8, BinOp)> {
        Some(match k {
            TokenKind::Or => (1, BinOp::Or),
            TokenKind::And => (2, BinOp::And),
            TokenKind::BitOr => (3, BinOp::BitOr),
            TokenKind::Xor => (4, BinOp::BitXor),
            TokenKind::BitAnd => (5, BinOp::BitAnd),
            TokenKind::Eq => (6, BinOp::Eq),
            TokenKind::Ne => (6, BinOp::Ne),
            TokenKind::Lt => (7, BinOp::Lt),
            TokenKind::Le => (7, BinOp::Le),
            TokenKind::Gt => (7, BinOp::Gt),
            TokenKind::Ge => (7, BinOp::Ge),
            TokenKind::Shl => (8, BinOp::Shl),
            TokenKind::Shr => (8, BinOp::Shr),
            TokenKind::Plus => (9, BinOp::Add),
            TokenKind::Minus => (9, BinOp::Sub),
            TokenKind::Star => (10, BinOp::Mul),
            TokenKind::Slash => (10, BinOp::Div),
            TokenKind::Percent => (10, BinOp::Rem),
            _ => return None,
        })
    }
    fn binary(&mut self, min: u8) -> Result<Expr, Diagnostic> {
        let mut x = self.unary()?;
        loop {
            let Some((p, op)) = Self::prec(self.cur()) else {
                break;
            };
            if p < min {
                break;
            }
            self.p += 1;
            let y = self.binary(p + 1)?;
            x = Expr::Binary(op, Box::new(x), Box::new(y));
        }
        Ok(x)
    }
    fn unary(&mut self) -> Result<Expr, Diagnostic> {
        if self.take(&TokenKind::PlusPlus) || self.take(&TokenKind::MinusMinus) {
            let add = self.t[self.p - 1].kind == TokenKind::PlusPlus;
            let Expr::Var(n) = self.primary()? else {
                return Err(Diagnostic::error("increment target required", None));
            };
            return Ok(Expr::Assign(
                Box::new(Expr::Var(n.clone())),
                Box::new(Expr::Binary(
                    if add { BinOp::Add } else { BinOp::Sub },
                    Box::new(Expr::Var(n)),
                    Box::new(Expr::Int(1)),
                )),
            ));
        }
        if self.take(&TokenKind::Minus) {
            return Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::Plus) {
            return Ok(Expr::Unary(UnOp::Pos, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::Tilde) {
            return Ok(Expr::Unary(UnOp::BitNot, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::Not) {
            return Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::BitAnd) {
            return Ok(Expr::Unary(UnOp::Address, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::Star) {
            return Ok(Expr::Unary(UnOp::Deref, Box::new(self.unary()?)));
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let x = match self.cur().clone() {
            TokenKind::Number(n) => {
                self.p += 1;
                Expr::Int(n)
            }
            TokenKind::Char(c) => {
                self.p += 1;
                Expr::Char(c)
            }
            TokenKind::String(s) => {
                self.p += 1;
                let mut value = s;
                while let TokenKind::String(next) = self.cur().clone() {
                    self.p += 1;
                    value.push_str(&next);
                }
                Expr::String(value)
            }
            TokenKind::Ident(n) => {
                self.p += 1;
                if self.take(&TokenKind::LParen) {
                    let mut a = Vec::new();
                    if *self.cur() != TokenKind::RParen {
                        loop {
                            a.push(self.assign()?);
                            if !self.take(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    Expr::Call(n, a)
                } else {
                    let x = Expr::Var(n.clone());
                    if self.take(&TokenKind::PlusPlus) {
                        Expr::Assign(
                            Box::new(x.clone()),
                            Box::new(Expr::Binary(
                                BinOp::Add,
                                Box::new(x),
                                Box::new(Expr::Int(1)),
                            )),
                        )
                    } else if self.take(&TokenKind::MinusMinus) {
                        Expr::Assign(
                            Box::new(x.clone()),
                            Box::new(Expr::Binary(
                                BinOp::Sub,
                                Box::new(x),
                                Box::new(Expr::Int(1)),
                            )),
                        )
                    } else {
                        x
                    }
                }
            }
            TokenKind::LParen => {
                self.p += 1;
                let x = self.expr()?;
                self.expect(&TokenKind::RParen)?;
                x
            }
            _ => {
                return Err(Diagnostic::error(
                    "expected expression",
                    Some(self.t[self.p].span),
                ))
            }
        };
        let mut x = x;
        loop {
            if self.take(&TokenKind::LBracket) {
                let index = self.expr()?;
                self.expect(&TokenKind::RBracket)?;
                x = Expr::Subscript(Box::new(x), Box::new(index));
                continue;
            }
            let arrow = self.take(&TokenKind::Arrow);
            let dot = arrow || self.take(&TokenKind::Dot);
            if !dot {
                break;
            }
            let name = self.ident()?;
            x = Expr::Member(Box::new(x), name, arrow);
        }
        Ok(x)
    }
}
