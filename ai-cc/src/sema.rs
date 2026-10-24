use std::collections::{HashMap, HashSet};

use crate::{ast::*, diag::Diagnostic, symbol::*};

pub struct Sema {
    syms: Scopes,
    errs: Vec<Diagnostic>,
    loop_depth: usize,
    current_ret: Option<Type>,
    complete_tags: HashSet<String>,
    tag_defs: HashMap<String, Type>,
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

        for declaration in &mut ast.declarations {
            if !self.valid_object_type(&declaration.ty) {
                self.errs.push(Diagnostic::error(
                    format!("invalid file-scope object type for {}", declaration.name),
                    Some(declaration.span),
                ));
            }
            if let Some(previous) = self.syms.lookup_current_scope(&declaration.name) {
                if !Self::compatible(&previous.ty, &declaration.ty) {
                    self.errs.push(Diagnostic::error(
                        format!("conflicting types for {}", declaration.name),
                        Some(declaration.span),
                    ));
                }
            } else if let Err(error) = self.syms.insert(Symbol {
                name: declaration.name.clone(),
                ty: declaration.ty.clone(),
                is_register: false,
            }) {
                self.errs
                    .push(Diagnostic::error(error.0, Some(declaration.span)));
            }
            if let Some(initializer) = &mut declaration.initializer {
                let init_ty = self.expr(initializer, declaration.span);
                let null_pointer_constant = Self::pointer_like(&declaration.ty)
                    && Self::integer(&init_ty)
                    && matches!(initializer, Expr::Int(0));
                if !Self::compatible(&declaration.ty, &init_ty) && !null_pointer_constant {
                    self.errs.push(Diagnostic::error(
                        format!("incompatible initializer for {}", declaration.name),
                        Some(declaration.span),
                    ));
                }
                if !Self::is_constant_initializer(initializer) {
                    self.errs.push(Diagnostic::error(
                        "file-scope initializer is not a supported constant expression",
                        Some(declaration.span),
                    ));
                }
            }
        }

        for f in &ast.items {
            let ty = Type::Function(
                Box::new(f.ret.clone()),
                f.params.iter().map(|(_, ty)| ty.clone()).collect(),
                f.is_variadic,
                !f.params_unspecified,
            );
            if let Err(e) = self.syms.insert(Symbol {
                name: f.name.clone(),
                ty,
                is_register: false,
            }) {
                self.errs.push(Diagnostic::error(e.0, Some(f.span)));
            }
        }

        for f in &mut ast.items {
            if f.is_variadic && !f.external {
                self.errs.push(Diagnostic::error(
                    "variadic function definitions are not yet supported by lowering",
                    Some(f.span),
                ));
            }
            if matches!(f.ret, Type::Array(_, _) | Type::Function(_, _, _, _)) {
                self.errs.push(Diagnostic::error(
                    "function cannot return array or function type",
                    Some(f.span),
                ));
            }
            self.current_ret = Some(f.ret.clone());
            self.syms.enter_scope();
            for (n, t) in &f.params {
                if matches!(Self::unqualified(t), Type::Void) {
                    self.errs.push(Diagnostic::error(
                        "function parameter cannot have void type",
                        Some(f.span),
                    ));
                }
                if !n.is_empty() {
                    self.add(n, t, f.span);
                }
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
            is_register: false,
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
            Stmt::StorageDecl(storage, declarations, _) => {
                for (name, ty, init) in declarations {
                    if !self.valid_object_type(ty) {
                        self.errs.push(Diagnostic::error(
                            format!("invalid object type for {}", name),
                            Some(span),
                        ));
                    }
                    if *storage == StorageClass::Extern && init.is_some() {
                        self.errs.push(Diagnostic::error(
                            "block-scope extern declaration cannot have an initializer",
                            Some(span),
                        ));
                    }
                    if *storage == StorageClass::Static
                        && init
                            .as_ref()
                            .is_some_and(|e| !Self::is_constant_initializer(e))
                    {
                        self.errs.push(Diagnostic::error(
                            "static local initializer must be a constant expression",
                            Some(span),
                        ));
                    }
                    if *storage == StorageClass::Static
                        && !Self::integer(ty)
                        && !matches!(Self::unqualified(ty), Type::Pointer(_))
                    {
                        self.errs.push(Diagnostic::error(
                            "static local objects currently require an integer or pointer type",
                            Some(span),
                        ));
                    }
                    let symbol = Symbol {
                        name: name.clone(),
                        ty: ty.clone(),
                        is_register: *storage == StorageClass::Register,
                    };
                    if let Err(e) = self.syms.insert(symbol) {
                        self.errs.push(Diagnostic::error(e.0, Some(span)));
                    }
                    if let Some(e) = init {
                        let got = self.expr(e, span);
                        let null_pointer_init = matches!(Self::unqualified(ty), Type::Pointer(_))
                            && matches!(e, Expr::Int(0));
                        if !Self::compatible(ty, &got) && !null_pointer_init {
                            self.errs.push(Diagnostic::error(
                                format!("initializer is incompatible with {}", Self::describe(ty)),
                                Some(span),
                            ));
                        }
                    }
                }
            }
            Stmt::Typedef(_, _, _) => {}
            Stmt::Typedefs(_, _) => {}
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
            Stmt::Decls(declarations, _) => {
                for (n, t, x) in declarations {
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
                        if let Expr::Var(name) = &**x {
                            if self
                                .syms
                                .lookup(name)
                                .is_some_and(|symbol| symbol.is_register)
                            {
                                self.errs.push(Diagnostic::error(
                                    "cannot take the address of a register object",
                                    Some(span),
                                ));
                            }
                        }
                        if !self.is_lvalue(x)
                            && !matches!(Self::unqualified(&ty), Type::Function(_, _, _, _))
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
                    UnOp::Sizeof => {
                        if matches!(
                            Self::unqualified(&ty),
                            Type::Void | Type::Function(_, _, _, _)
                        ) {
                            self.errs.push(Diagnostic::error(
                                "sizeof requires an object type",
                                Some(span),
                            ));
                        }
                        Type::UnsignedLong
                    }
                }
            }
            Expr::Cast(target, value) => {
                let source = self.expr(value, span);
                if !matches!(Self::unqualified(target), Type::Void)
                    && !(Self::arithmetic(target) && Self::arithmetic(&source))
                    && !(Self::pointer_like(target)
                        && (Self::pointer_like(&source) || Self::integer(&source)))
                {
                    self.errs
                        .push(Diagnostic::error("invalid cast operands", Some(span)));
                }
                target.clone()
            }
            Expr::SizeofType(ty) => {
                if matches!(
                    Self::unqualified(ty),
                    Type::Void | Type::Function(_, _, _, _)
                ) || !self.valid_object_type(ty)
                {
                    self.errs.push(Diagnostic::error(
                        "sizeof requires an object type",
                        Some(span),
                    ));
                }
                Type::UnsignedLong
            }
            Expr::CompoundLiteral(ty, initializer) => {
                *ty = self.resolve_tag(ty);
                self.check_initializer(ty, initializer, span);
                if !self.valid_object_type(ty) {
                    self.errs.push(Diagnostic::error(
                        "compound literal requires a complete object type",
                        Some(span),
                    ));
                }
                ty.clone()
            }
            Expr::Call(n, args) => {
                let arg_types: Vec<_> = args.iter_mut().map(|x| self.expr(x, span)).collect();
                match self.syms.lookup(n).map(|x| x.ty.clone()) {
                    Some(Type::Function(ret, params, variadic, has_prototype)) => {
                        if has_prototype
                            && ((!variadic && params.len() != arg_types.len())
                                || (variadic && params.len() > arg_types.len()))
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
                        if has_prototype {
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

    fn check_initializer(
        &mut self,
        target: &mut Type,
        initializer: &mut Initializer,
        span: crate::diag::Span,
    ) {
        match initializer {
            Initializer::Expr(expr) => {
                let got = self.expr(expr, span);
                if !Self::compatible(target, &got)
                    && !(Self::arithmetic(target) && Self::arithmetic(&got))
                {
                    self.errs.push(Diagnostic::error(
                        "compound literal initializer has incompatible type",
                        Some(span),
                    ));
                }
            }
            Initializer::List(items) => match target {
                Type::Qualified(inner, _) => self.check_initializer(inner, initializer, span),
                Type::Array(element, count) => {
                    let mut next = 0usize;
                    let mut high = 0usize;
                    for item in items {
                        let index = if item.designators.is_empty() {
                            next
                        } else {
                            match item.designators.first_mut() {
                                Some(Designator::Index(expr)) => {
                                    let got = self.expr(expr, span);
                                    let value = Self::integer_constant(expr);
                                    if !Self::integer(&got) || value.is_none() {
                                        self.errs.push(Diagnostic::error(
                                            "array designator requires an integer constant expression",
                                            Some(span),
                                        ));
                                    }
                                    value.and_then(|x| usize::try_from(x).ok()).unwrap_or(next)
                                }
                                _ => {
                                    self.errs.push(Diagnostic::error(
                                        "array initializer requires an array designator",
                                        Some(span),
                                    ));
                                    next
                                }
                            }
                        };
                        if count.is_some_and(|n| index >= n) {
                            self.errs.push(Diagnostic::error(
                                "array initializer index is out of bounds",
                                Some(span),
                            ));
                        }
                        if item.designators.len() > 1 {
                            self.check_designated(
                                element,
                                &mut item.designators[1..],
                                &mut item.value,
                                span,
                            );
                        } else {
                            self.check_initializer(element, &mut item.value, span);
                        }
                        next = index.saturating_add(1);
                        high = high.max(next);
                    }
                    if count.is_none() {
                        *count = Some(high);
                    }
                }
                Type::Struct(_, fields) => {
                    let mut next = 0usize;
                    for item in items {
                        let index = if item.designators.is_empty() {
                            next
                        } else {
                            match item.designators.first_mut() {
                                Some(Designator::Member(name)) => fields
                                    .iter()
                                    .position(|(field, _)| field == name)
                                    .unwrap_or_else(|| {
                                        self.errs.push(Diagnostic::error(
                                            format!("unknown member {} in initializer", name),
                                            Some(span),
                                        ));
                                        next
                                    }),
                                _ => {
                                    self.errs.push(Diagnostic::error(
                                        "structure initializer requires a member designator",
                                        Some(span),
                                    ));
                                    next
                                }
                            }
                        };
                        if let Some((_, field)) = fields.get_mut(index) {
                            if item.designators.len() > 1 {
                                self.check_designated(
                                    field,
                                    &mut item.designators[1..],
                                    &mut item.value,
                                    span,
                                );
                            } else {
                                self.check_initializer(field, &mut item.value, span);
                            }
                        } else {
                            self.errs.push(Diagnostic::error(
                                "too many structure initializers",
                                Some(span),
                            ));
                        }
                        next = index.saturating_add(1);
                    }
                }
                Type::Union(_, fields) => {
                    for (position, item) in items.iter_mut().enumerate() {
                        let index = if item.designators.is_empty() {
                            0
                        } else {
                            match item.designators.first_mut() {
                                Some(Designator::Member(name)) => fields
                                    .iter()
                                    .position(|(field, _)| field == name)
                                    .unwrap_or_else(|| {
                                        self.errs.push(Diagnostic::error(
                                            format!("unknown member {} in initializer", name),
                                            Some(span),
                                        ));
                                        0
                                    }),
                                _ => {
                                    self.errs.push(Diagnostic::error(
                                        "union initializer requires a member designator",
                                        Some(span),
                                    ));
                                    0
                                }
                            }
                        };
                        if position > 0 {
                            self.errs.push(Diagnostic::error(
                                "union initializer has more than one member",
                                Some(span),
                            ));
                        }
                        if let Some((_, field)) = fields.get_mut(index) {
                            if item.designators.len() > 1 {
                                self.check_designated(
                                    field,
                                    &mut item.designators[1..],
                                    &mut item.value,
                                    span,
                                );
                            } else {
                                self.check_initializer(field, &mut item.value, span);
                            }
                        }
                    }
                }
                _ => {
                    if items.len() != 1 {
                        self.errs.push(Diagnostic::error(
                            "scalar initializer has too many values",
                            Some(span),
                        ));
                    }
                    if let Some(item) = items.first_mut() {
                        if item.designators.is_empty() {
                            self.check_initializer(target, &mut item.value, span);
                        } else {
                            self.errs.push(Diagnostic::error(
                                "scalar initializer cannot use a designator",
                                Some(span),
                            ));
                        }
                    }
                }
            },
        }
    }

    fn check_designated(
        &mut self,
        target: &mut Type,
        designators: &mut [Designator],
        value: &mut Initializer,
        span: crate::diag::Span,
    ) {
        if designators.is_empty() {
            self.check_initializer(target, value, span);
            return;
        }
        match (&mut designators[0], target) {
            (Designator::Index(expr), Type::Array(element, count)) => {
                let got = self.expr(expr, span);
                let index = Self::integer_constant(expr).and_then(|x| usize::try_from(x).ok());
                if !Self::integer(&got) || index.is_none() {
                    self.errs.push(Diagnostic::error(
                        "array designator requires an integer constant expression",
                        Some(span),
                    ));
                }
                if let Some(index) = index {
                    if count.is_some_and(|n| index >= n) {
                        self.errs.push(Diagnostic::error(
                            "array initializer index is out of bounds",
                            Some(span),
                        ));
                    }
                    if count.is_none() {
                        *count = Some(index + 1);
                    }
                }
                self.check_designated(element, &mut designators[1..], value, span);
            }
            (Designator::Member(name), Type::Struct(_, fields) | Type::Union(_, fields)) => {
                if let Some((_, field)) = fields.iter_mut().find(|(field, _)| field == name) {
                    self.check_designated(field, &mut designators[1..], value, span);
                } else {
                    self.errs.push(Diagnostic::error(
                        format!("unknown member {} in initializer", name),
                        Some(span),
                    ));
                }
            }
            (_, Type::Qualified(inner, _)) => {
                self.check_designated(inner, designators, value, span)
            }
            _ => self.errs.push(Diagnostic::error(
                "invalid initializer designator",
                Some(span),
            )),
        }
    }

    fn integer_constant(expr: &Expr) -> Option<i64> {
        match expr {
            Expr::Int(n) => Some(*n),
            Expr::Char(c) => Some(*c as i64),
            Expr::Cast(_, x) | Expr::Unary(UnOp::Pos, x) => Self::integer_constant(x),
            Expr::Unary(UnOp::Neg, x) => Self::integer_constant(x)?.checked_neg(),
            Expr::Unary(UnOp::BitNot, x) => Some(!Self::integer_constant(x)?),
            Expr::Binary(op, a, b) => {
                let (a, b) = (Self::integer_constant(a)?, Self::integer_constant(b)?);
                match op {
                    BinOp::Add => a.checked_add(b),
                    BinOp::Sub => a.checked_sub(b),
                    BinOp::Mul => a.checked_mul(b),
                    BinOp::Div => a.checked_div(b),
                    BinOp::Rem => a.checked_rem(b),
                    BinOp::Shl => Some(a << b),
                    BinOp::Shr => Some(a >> b),
                    BinOp::BitAnd => Some(a & b),
                    BinOp::BitOr => Some(a | b),
                    BinOp::BitXor => Some(a ^ b),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn is_constant_initializer(expr: &Expr) -> bool {
        match expr {
            Expr::Int(_) | Expr::Char(_) => true,
            Expr::Cast(_, value)
            | Expr::Unary(UnOp::Neg | UnOp::Pos | UnOp::Not | UnOp::BitNot, value) => {
                Self::is_constant_initializer(value)
            }
            Expr::Binary(op, left, right) if *op != BinOp::Comma => {
                Self::is_constant_initializer(left) && Self::is_constant_initializer(right)
            }
            Expr::Conditional(condition, yes, no) => {
                Self::is_constant_initializer(condition)
                    && Self::is_constant_initializer(yes)
                    && Self::is_constant_initializer(no)
            }
            Expr::Unary(UnOp::Address, value) => {
                matches!(&**value, Expr::Var(_))
                    || matches!(&**value, Expr::CompoundLiteral(_, initializer) if Self::constant_initializer_list(initializer))
            }
            Expr::CompoundLiteral(_, initializer) => Self::constant_initializer_list(initializer),
            _ => false,
        }
    }

    fn constant_initializer_list(initializer: &Initializer) -> bool {
        match initializer {
            Initializer::Expr(expr) => Self::is_constant_initializer(expr),
            Initializer::List(items) => items.iter().all(|item| {
                item.designators.iter().all(|designator| match designator {
                    Designator::Index(expr) => Self::is_constant_initializer(expr),
                    Designator::Member(_) => true,
                }) && Self::constant_initializer_list(&item.value)
            }),
        }
    }

    fn is_lvalue(&self, e: &Expr) -> bool {
        match e {
            Expr::Var(n) => self
                .syms
                .lookup(n)
                .map(|x| !matches!(Self::unqualified(&x.ty), Type::Function(_, _, _, _)))
                .unwrap_or(false),
            Expr::Member(base, _, _) => self.is_lvalue(base),
            Expr::Subscript(base, _) => {
                let ty = self.expr_type(base);
                matches!(Self::unqualified(&ty), Type::Pointer(_) | Type::Array(_, _))
            }
            Expr::Unary(UnOp::Deref, _) => true,
            Expr::CompoundLiteral(_, _) => true,
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
            Type::Array(_, _) | Type::Function(_, _, _, _) | Type::Void
        ) && !Self::top_const(&ty)
    }

    fn expr_type(&self, e: &Expr) -> Type {
        match e {
            Expr::Int(_) | Expr::Char(_) => Type::Int,
            Expr::Cast(ty, _) => ty.clone(),
            Expr::SizeofType(_) | Expr::Unary(UnOp::Sizeof, _) => Type::UnsignedLong,
            Expr::CompoundLiteral(ty, _) => ty.clone(),
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
                Some(Type::Function(ret, _, _, _)) => *ret,
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
            Type::Void | Type::Function(_, _, _, _) => false,
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
            (Type::Struct(a, _), Type::Struct(b, _)) | (Type::Union(a, _), Type::Union(b, _)) => {
                a == b
            }
            (Type::Enum(a, _), Type::Enum(b, _)) => a == b,
            (x, y) if Self::integer(x) && Self::integer(y) => true,
            (x, y) if Self::arithmetic(x) && Self::arithmetic(y) => true,
            (Type::Pointer(x), Type::Pointer(y)) => Self::compatible(x, y),
            (Type::Pointer(x), Type::Array(y, _)) | (Type::Array(y, _), Type::Pointer(x)) => {
                Self::compatible(x, y)
            }
            (Type::Pointer(x), Type::Void) | (Type::Void, Type::Pointer(x)) => {
                matches!(Self::unqualified(x), Type::Void)
            }
            (Type::Function(ar, ap, av, aproto), Type::Function(br, bp, bv, bproto)) => {
                Self::compatible(ar, br)
                    && if !aproto || !bproto {
                        !av && !bv
                    } else {
                        av == bv
                            && ap.len() == bp.len()
                            && ap.iter().zip(bp).all(|(a, b)| Self::compatible(a, b))
                    }
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
            Type::Function(_, _, _, _) => "function",
            Type::Float | Type::Double | Type::LongDouble => "floating type",
            _ => "integer type",
        }
    }
}
