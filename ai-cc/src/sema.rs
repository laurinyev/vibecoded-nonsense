use std::collections::{HashMap, HashSet};

use crate::{ast::*, diag::Diagnostic, symbol::*};

pub struct Sema {
    syms: Scopes,
    errs: Vec<Diagnostic>,
    loop_depth: usize,
    current_ret: Option<Type>,
    complete_tags: HashSet<String>,
    tag_defs: HashMap<String, Type>,
    unspecified_functions: HashSet<String>,
}

impl Default for Sema {
    fn default() -> Self {
        Self::new()
    }
}

impl Sema {
    pub fn new() -> Self {
        Self {
            syms: Scopes::new(),
            errs: Vec::new(),
            loop_depth: 0,
            current_ret: None,
            complete_tags: Default::default(),
            tag_defs: Default::default(),
            unspecified_functions: Default::default(),
        }
    }

    pub fn analyze(&mut self, ast: &mut TranslationUnit) -> Result<(), Vec<Diagnostic>> {
        for t in &ast.types {
            match t {
                Type::Struct(n, f) | Type::Union(n, f) if !f.is_empty() => {
                    self.complete_tags.insert(n.clone());
                    self.tag_defs.insert(Self::tag_key(t), t.clone());
                }
                Type::Enum(n, v) if !v.is_empty() => {
                    self.complete_tags.insert(n.clone());
                    self.tag_defs.insert(Self::tag_key(t), t.clone());
                }
                _ => {}
            }
        }

        for f in &ast.items {
            if f.params_unspecified {
                self.unspecified_functions.insert(f.name.clone());
            }
            let ty = Type::Function(
                Box::new(f.ret.clone()),
                f.params.iter().map(|(_, ty)| ty.clone()).collect(),
            );
            if let Err(e) = self.syms.insert(Symbol {
                name: f.name.clone(),
                ty,
            }) {
                self.errs.push(Diagnostic::error(e.0, Some(f.span)));
            }
        }

        for f in &mut ast.items {
            if matches!(f.ret, Type::Array(_, _) | Type::Function(_, _)) {
                self.errs.push(Diagnostic::error(
                    "function cannot return array or function type",
                    Some(f.span),
                ));
            }
            self.current_ret = Some(f.ret.clone());
            self.syms.enter_scope();
            for (n, t) in &f.params {
                self.add(n, t, f.span);
            }
            self.block(&mut f.body);
            self.syms.exit_scope();
        }
        self.current_ret = None;

        if self.errs.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errs))
        }
    }

    fn add(&mut self, n: &str, t: &Type, span: crate::diag::Span) {
        if let Err(e) = self.syms.insert(Symbol {
            name: n.to_string(),
            ty: t.clone(),
        }) {
            self.errs.push(Diagnostic::error(e.0, Some(span)));
        }
    }

    fn block(&mut self, b: &mut Block) {
        for s in &mut b.stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        let span = s.span();
        match s {
            Stmt::Decl(n, t, x, _) => {
                if !self.valid_object_type(t) {
                    self.errs.push(Diagnostic::error(
                        format!("invalid object type for {}", n),
                        Some(span),
                    ));
                }
                self.add(n, t, span);
                if let Some(e) = x {
                    let init = self.expr(e, span);
                    if !Self::compatible(t, &init) && !matches!(t, Type::Array(_, _)) {
                        self.errs.push(Diagnostic::error(
                            format!("initializer is incompatible with {}", Self::describe(t)),
                            Some(span),
                        ));
                    }
                }
            }
            Stmt::Expr(e, _) => {
                self.expr(e, span);
            }
            Stmt::Return(e, _) => match (self.current_ret.clone(), e) {
                (Some(ret), Some(e)) => {
                    let got = self.expr(e, span);
                    if matches!(Self::unqualified(&ret), Type::Void) {
                        self.errs.push(Diagnostic::error(
                            "void function cannot return a value",
                            Some(span),
                        ));
                    } else if !Self::compatible(&ret, &got) {
                        self.errs.push(Diagnostic::error(
                            format!("return value is incompatible with {}", Self::describe(&ret)),
                            Some(span),
                        ));
                    }
                }
                (Some(ret), None) if !matches!(Self::unqualified(&ret), Type::Void) => {
                    self.errs.push(Diagnostic::error(
                        "non-void function requires return value",
                        Some(span),
                    ));
                }
                _ => {}
            },
            Stmt::Block(b, _) => {
                self.syms.enter_scope();
                self.block(b);
                self.syms.exit_scope();
            }
            Stmt::If(c, a, b, _) => {
                let ty = self.expr(c, span);
                self.require_scalar(&ty, "if condition", span);
                self.stmt(a);
                if let Some(x) = b {
                    self.stmt(x);
                }
            }
            Stmt::While(c, b, _) => {
                let ty = self.expr(c, span);
                self.require_scalar(&ty, "while condition", span);
                self.loop_depth += 1;
                self.stmt(b);
                self.loop_depth -= 1;
            }
            Stmt::DoWhile(b, c, _) => {
                self.loop_depth += 1;
                self.stmt(b);
                self.loop_depth -= 1;
                let ty = self.expr(c, span);
                self.require_scalar(&ty, "do-while condition", span);
            }
            Stmt::For(i, c, step, b, _) => {
                self.syms.enter_scope();
                if let Some(i) = i {
                    self.stmt(i);
                }
                if let Some(c) = c {
                    let ty = self.expr(c, span);
                    self.require_scalar(&ty, "for condition", span);
                }
                self.loop_depth += 1;
                self.stmt(b);
                self.loop_depth -= 1;
                if let Some(x) = step {
                    self.expr(x, span);
                }
                self.syms.exit_scope();
            }
            Stmt::Break(_) | Stmt::Continue(_) => {
                if self.loop_depth == 0 {
                    self.errs.push(Diagnostic::error(
                        "break or continue outside loop",
                        Some(span),
                    ));
                }
            }
        }
    }

    fn expr(&mut self, e: &mut Expr, span: crate::diag::Span) -> Type {
        match e {
            Expr::Int(_) | Expr::Char(_) => Type::Int,
            Expr::String(_) => Type::Pointer(Box::new(Type::Char)),
            Expr::Var(n) => self
                .syms
                .lookup(n)
                .map(|x| x.ty.clone())
                .unwrap_or_else(|| {
                    self.errs.push(Diagnostic::error(
                        format!("unknown identifier {}", n),
                        Some(span),
                    ));
                    Type::Int
                }),
            Expr::Assign(target, value) => {
                let target_ty = self.expr(target, span);
                let value_ty = self.expr(value, span);
                if !self.is_modifiable_lvalue(target) {
                    self.errs.push(Diagnostic::error(
                        "assignment requires a modifiable lvalue",
                        Some(span),
                    ));
                }
                if !Self::compatible(&target_ty, &value_ty) {
                    self.errs.push(Diagnostic::error(
                        format!(
                            "cannot assign {} to {}",
                            Self::describe(&value_ty),
                            Self::describe(&target_ty)
                        ),
                        Some(span),
                    ));
                }
                target_ty
            }
            Expr::Binary(op, a, b) => {
                let left = self.expr(a, span);
                let right = self.expr(b, span);
                match op {
                    BinOp::Comma => right,
                    BinOp::And | BinOp::Or => {
                        self.require_scalar(&left, "logical operand", span);
                        self.require_scalar(&right, "logical operand", span);
                        Type::Int
                    }
                    BinOp::Eq | BinOp::Ne => {
                        if !(Self::arithmetic(&left) && Self::arithmetic(&right))
                            && !(Self::pointer_like(&left)
                                && Self::pointer_like(&right)
                                && Self::compatible(&left, &right))
                        {
                            self.errs.push(Diagnostic::error(
                                "equality operands must have compatible scalar types",
                                Some(span),
                            ));
                        }
                        Type::Int
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        if !(Self::arithmetic(&left) && Self::arithmetic(&right))
                            && !(Self::pointer_like(&left)
                                && Self::pointer_like(&right)
                                && Self::compatible(&left, &right))
                        {
                            self.errs.push(Diagnostic::error(
                                "relational operands must be arithmetic or compatible pointers",
                                Some(span),
                            ));
                        }
                        Type::Int
                    }
                    BinOp::BitOr | BinOp::BitXor | BinOp::BitAnd => {
                        self.require_integer(&left, "bitwise operand", span);
                        self.require_integer(&right, "bitwise operand", span);
                        Type::Int
                    }
                    BinOp::Shl | BinOp::Shr => {
                        self.require_integer(&left, "shift operand", span);
                        self.require_integer(&right, "shift operand", span);
                        Type::Int
                    }
                    BinOp::Add | BinOp::Sub => {
                        let pointer_integer = (Self::pointer_like(&left) && Self::integer(&right))
                            || (*op == BinOp::Add
                                && Self::integer(&left)
                                && Self::pointer_like(&right));
                        if pointer_integer {
                            if Self::pointer_like(&left) {
                                left
                            } else {
                                right
                            }
                        } else if Self::arithmetic(&left) && Self::arithmetic(&right) {
                            Self::usual_arithmetic_type(&left, &right)
                        } else {
                            self.errs.push(Diagnostic::error(
                                "additive operands have incompatible types",
                                Some(span),
                            ));
                            Type::Int
                        }
                    }
                    BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        if !Self::arithmetic(&left) || !Self::arithmetic(&right) {
                            self.errs.push(Diagnostic::error(
                                "multiplicative operands must be arithmetic",
                                Some(span),
                            ));
                        }
                        Self::usual_arithmetic_type(&left, &right)
                    }
                }
            }
            Expr::Unary(op, x) => {
                let ty = self.expr(x, span);
                match op {
                    UnOp::Neg | UnOp::Pos => {
                        self.require_arithmetic(&ty, "unary operand", span);
                        ty
                    }
                    UnOp::Not => {
                        self.require_scalar(&ty, "logical operand", span);
                        Type::Int
                    }
                    UnOp::BitNot => {
                        self.require_integer(&ty, "bitwise operand", span);
                        Type::Int
                    }
                    UnOp::Address => {
                        if !self.is_lvalue(x)
                            && !matches!(Self::unqualified(&ty), Type::Function(_, _))
                        {
                            self.errs.push(Diagnostic::error(
                                "address-of operand must be an lvalue or function",
                                Some(span),
                            ));
                        }
                        Type::Pointer(Box::new(ty))
                    }
                    UnOp::Deref => match Self::unqualified(&ty) {
                        Type::Pointer(inner) if !matches!(*inner, Type::Void) => *inner,
                        _ => {
                            self.errs.push(Diagnostic::error(
                                "dereference operand must be a pointer to an object",
                                Some(span),
                            ));
                            Type::Int
                        }
                    },
                }
            }
            Expr::Call(n, args) => {
                let arg_types: Vec<_> = args.iter_mut().map(|x| self.expr(x, span)).collect();
                match self.syms.lookup(n).map(|x| x.ty.clone()) {
                    Some(Type::Function(ret, params)) => {
                        if !self.unspecified_functions.contains(n)
                            && params.len() != arg_types.len()
                        {
                            self.errs.push(Diagnostic::error(
                                format!(
                                    "function {} expects {} argument(s), got {}",
                                    n,
                                    params.len(),
                                    arg_types.len()
                                ),
                                Some(span),
                            ));
                        }
                        if !self.unspecified_functions.contains(n) {
                            for (index, (expected, got)) in
                                params.iter().zip(arg_types.iter()).enumerate()
                            {
                                if !Self::compatible(expected, got) {
                                    self.errs.push(Diagnostic::error(
                                        format!(
                                            "argument {} to {} has incompatible type",
                                            index + 1,
                                            n
                                        ),
                                        Some(span),
                                    ));
                                }
                            }
                        }
                        *ret
                    }
                    Some(_) => {
                        self.errs.push(Diagnostic::error(
                            format!("{} is not a function", n),
                            Some(span),
                        ));
                        Type::Int
                    }
                    None => {
                        // Keep the existing external-call extension for hosted libc symbols.
                        Type::Int
                    }
                }
            }
            Expr::Conditional(condition, yes, no) => {
                let condition_ty = self.expr(condition, span);
                self.require_scalar(&condition_ty, "conditional condition", span);
                let yes_ty = self.expr(yes, span);
                let no_ty = self.expr(no, span);
                if !Self::compatible(&yes_ty, &no_ty) {
                    self.errs.push(Diagnostic::error(
                        "conditional operands have incompatible types",
                        Some(span),
                    ));
                }
                yes_ty
            }
            Expr::Member(base, name, arrow) => {
                let base_ty = self.expr(base, span);
                let aggregate = if *arrow {
                    match Self::unqualified(&base_ty) {
                        Type::Pointer(inner) => self.resolve_tag(&Self::unqualified(&inner)),
                        _ => {
                            self.errs.push(Diagnostic::error(
                                "arrow operand must be a pointer to a structure or union",
                                Some(span),
                            ));
                            Type::Int
                        }
                    }
                } else {
                    self.resolve_tag(&Self::unqualified(&base_ty))
                };
                match aggregate {
                    Type::Struct(_, fields) | Type::Union(_, fields) => fields
                        .iter()
                        .find(|(field, _)| field == name)
                        .map(|(_, ty)| ty.clone())
                        .unwrap_or_else(|| {
                            self.errs.push(Diagnostic::error(
                                format!("no member named {}", name),
                                Some(span),
                            ));
                            Type::Int
                        }),
                    _ => {
                        self.errs.push(Diagnostic::error(
                            "member access requires a structure or union",
                            Some(span),
                        ));
                        Type::Int
                    }
                }
            }
            Expr::Subscript(base, index) => {
                let base_ty = self.expr(base, span);
                let index_ty = self.expr(index, span);
                self.require_integer(&index_ty, "subscript index", span);
                match Self::unqualified(&base_ty) {
                    Type::Pointer(inner) | Type::Array(inner, _) => *inner,
                    _ => {
                        self.errs.push(Diagnostic::error(
                            "subscript operand must be a pointer or array",
                            Some(span),
                        ));
                        Type::Int
                    }
                }
            }
        }
    }

    fn is_lvalue(&self, e: &Expr) -> bool {
        match e {
            Expr::Var(n) => self
                .syms
                .lookup(n)
                .map(|x| !matches!(Self::unqualified(&x.ty), Type::Function(_, _)))
                .unwrap_or(false),
            Expr::Member(base, _, _) => self.is_lvalue(base),
            Expr::Subscript(base, _) => {
                let ty = self.expr_type(base);
                matches!(Self::unqualified(&ty), Type::Pointer(_) | Type::Array(_, _))
            }
            Expr::Unary(UnOp::Deref, _) => true,
            _ => false,
        }
    }

    fn is_modifiable_lvalue(&self, e: &Expr) -> bool {
        if !self.is_lvalue(e) {
            return false;
        }
        let ty = self.expr_type(e);
        !matches!(
            Self::unqualified(&ty),
            Type::Array(_, _) | Type::Function(_, _) | Type::Void
        ) && !Self::top_const(&ty)
    }

    fn expr_type(&self, e: &Expr) -> Type {
        match e {
            Expr::Int(_) | Expr::Char(_) => Type::Int,
            Expr::String(_) => Type::Pointer(Box::new(Type::Char)),
            Expr::Var(n) => self
                .syms
                .lookup(n)
                .map(|x| x.ty.clone())
                .unwrap_or(Type::Int),
            Expr::Assign(x, _) => self.expr_type(x),
            Expr::Binary(op, a, b) => match op {
                BinOp::Comma => self.expr_type(b),
                BinOp::Add | BinOp::Sub => {
                    let left = self.expr_type(a);
                    let right = self.expr_type(b);
                    if Self::pointer_like(&left) {
                        left
                    } else if Self::pointer_like(&right) {
                        right
                    } else {
                        Self::usual_arithmetic_type(&left, &right)
                    }
                }
                _ => Type::Int,
            },
            Expr::Unary(UnOp::Address, x) => Type::Pointer(Box::new(self.expr_type(x))),
            Expr::Unary(UnOp::Deref, x) => match Self::unqualified(&self.expr_type(x)) {
                Type::Pointer(inner) => *inner,
                _ => Type::Int,
            },
            Expr::Unary(_, x) => self.expr_type(x),
            Expr::Call(n, _) => match self.syms.lookup(n).map(|x| x.ty.clone()) {
                Some(Type::Function(ret, _)) => *ret,
                _ => Type::Int,
            },
            Expr::Conditional(_, yes, _) => self.expr_type(yes),
            Expr::Member(base, name, arrow) => {
                let ty = self.expr_type(base);
                let ty = if *arrow {
                    match Self::unqualified(&ty) {
                        Type::Pointer(inner) => *inner,
                        _ => Type::Int,
                    }
                } else {
                    ty
                };
                let ty = self.resolve_tag(&Self::unqualified(&ty));
                match ty {
                    Type::Struct(_, fields) | Type::Union(_, fields) => fields
                        .into_iter()
                        .find(|(field, _)| field == name)
                        .map(|(_, ty)| ty)
                        .unwrap_or(Type::Int),
                    _ => Type::Int,
                }
            }
            Expr::Subscript(base, _) => match Self::unqualified(&self.expr_type(base)) {
                Type::Pointer(inner) | Type::Array(inner, _) => *inner,
                _ => Type::Int,
            },
        }
    }

    fn valid_object_type(&self, t: &Type) -> bool {
        match t {
            Type::Void | Type::Function(_, _) => false,
            Type::Array(x, Some(n)) => *n > 0 && self.valid_object_type(x),
            Type::Array(_, None) => false,
            Type::Pointer(_) => true,
            Type::Qualified(x, _) => self.valid_object_type(x),
            Type::Struct(n, fields) | Type::Union(n, fields) => {
                !fields.is_empty() || self.complete_tags.contains(n)
            }
            Type::Enum(n, values) => !values.is_empty() || self.complete_tags.contains(n),
            _ => true,
        }
    }

    fn tag_key(t: &Type) -> String {
        match t {
            Type::Struct(n, _) => format!("struct:{n}"),
            Type::Union(n, _) => format!("union:{n}"),
            Type::Enum(n, _) => format!("enum:{n}"),
            _ => String::new(),
        }
    }

    fn resolve_tag(&self, t: &Type) -> Type {
        match t {
            Type::Struct(n, fields) if fields.is_empty() => self
                .tag_defs
                .get(&format!("struct:{n}"))
                .cloned()
                .unwrap_or_else(|| t.clone()),
            Type::Union(n, fields) if fields.is_empty() => self
                .tag_defs
                .get(&format!("union:{n}"))
                .cloned()
                .unwrap_or_else(|| t.clone()),
            Type::Enum(n, values) if values.is_empty() => self
                .tag_defs
                .get(&format!("enum:{n}"))
                .cloned()
                .unwrap_or_else(|| t.clone()),
            _ => t.clone(),
        }
    }

    fn require_scalar(&mut self, t: &Type, context: &str, span: crate::diag::Span) {
        if !Self::scalar(t) {
            self.errs.push(Diagnostic::error(
                format!("{} requires a scalar operand", context),
                Some(span),
            ));
        }
    }

    fn require_integer(&mut self, t: &Type, context: &str, span: crate::diag::Span) {
        if !Self::integer(t) {
            self.errs.push(Diagnostic::error(
                format!("{} requires an integer operand", context),
                Some(span),
            ));
        }
    }

    fn require_arithmetic(&mut self, t: &Type, context: &str, span: crate::diag::Span) {
        if !Self::arithmetic(t) {
            self.errs.push(Diagnostic::error(
                format!("{} requires an arithmetic operand", context),
                Some(span),
            ));
        }
    }

    fn unqualified(t: &Type) -> Type {
        match t {
            Type::Qualified(inner, _) => Self::unqualified(inner),
            _ => t.clone(),
        }
    }

    fn top_const(t: &Type) -> bool {
        match t {
            Type::Qualified(_, q) => q.is_const,
            _ => false,
        }
    }

    fn integer(t: &Type) -> bool {
        matches!(
            Self::unqualified(t),
            Type::Int
                | Type::SignedInt
                | Type::UnsignedInt
                | Type::Short
                | Type::UnsignedShort
                | Type::Long
                | Type::UnsignedLong
                | Type::LongLong
                | Type::UnsignedLongLong
                | Type::Bool
                | Type::Char
                | Type::SignedChar
                | Type::UnsignedChar
                | Type::Enum(_, _)
        )
    }

    fn arithmetic(t: &Type) -> bool {
        Self::integer(t)
            || matches!(
                Self::unqualified(t),
                Type::Float | Type::Double | Type::LongDouble
            )
    }

    fn scalar(t: &Type) -> bool {
        Self::arithmetic(t) || Self::pointer_like(t)
    }

    fn pointer_like(t: &Type) -> bool {
        matches!(Self::unqualified(t), Type::Pointer(_) | Type::Array(_, _))
    }

    fn compatible(a: &Type, b: &Type) -> bool {
        let a = Self::unqualified(a);
        let b = Self::unqualified(b);
        match (&a, &b) {
            (x, y) if x == y => true,
            (x, y) if Self::integer(x) && Self::integer(y) => true,
            (x, y) if Self::arithmetic(x) && Self::arithmetic(y) => true,
            (Type::Pointer(x), Type::Pointer(y)) => Self::compatible(x, y),
            (Type::Pointer(x), Type::Array(y, _)) | (Type::Array(y, _), Type::Pointer(x)) => {
                Self::compatible(x, y)
            }
            (Type::Pointer(x), Type::Void) | (Type::Void, Type::Pointer(x)) => {
                matches!(Self::unqualified(x), Type::Void)
            }
            _ => false,
        }
    }

    fn usual_arithmetic_type(a: &Type, b: &Type) -> Type {
        let a = Self::unqualified(a);
        let b = Self::unqualified(b);
        if matches!(a, Type::LongDouble) || matches!(b, Type::LongDouble) {
            Type::LongDouble
        } else if matches!(a, Type::Double) || matches!(b, Type::Double) {
            Type::Double
        } else if matches!(a, Type::Float) || matches!(b, Type::Float) {
            Type::Float
        } else {
            Type::Int
        }
    }

    fn describe(t: &Type) -> &'static str {
        match Self::unqualified(t) {
            Type::Void => "void",
            Type::Pointer(_) => "pointer",
            Type::Array(_, _) => "array",
            Type::Function(_, _) => "function",
            Type::Float | Type::Double | Type::LongDouble => "floating type",
            _ => "integer type",
        }
    }
}
