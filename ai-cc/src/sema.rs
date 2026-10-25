use std::collections::{HashMap, HashSet};

use crate::{ast::*, diag::Diagnostic, symbol::*};

struct InitCursor<'a> {
    items: &'a [InitializerItem],
    position: usize,
}

impl<'a> InitCursor<'a> {
    fn new(items: &'a [InitializerItem]) -> Self {
        Self { items, position: 0 }
    }

    fn next(&mut self) -> Option<InitializerItem> {
        let item = self.items.get(self.position)?.clone();
        self.position += 1;
        Some(item)
    }

    fn is_empty(&self) -> bool {
        self.position >= self.items.len()
    }

    fn peek(&self) -> Option<&'a InitializerItem> {
        self.items.get(self.position)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Linkage {
    External,
    Internal,
}

#[derive(Clone)]
struct FileEntity {
    ty: Type,
    linkage: Linkage,
    is_function: bool,
    defined: bool,
    tentative: bool,
}

pub struct Sema {
    syms: Scopes,
    errs: Vec<Diagnostic>,
    loop_depth: usize,
    current_ret: Option<Type>,
    complete_tags: HashSet<String>,
    tag_defs: HashMap<String, Type>,
    file_entities: HashMap<String, FileEntity>,
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
            file_entities: Default::default(),
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

        enum FileEvent<'a> {
            Object(&'a mut GlobalDecl),
            Function(&'a mut Function),
        }
        let mut events = Vec::new();
        for f in &mut ast.items {
            events.push((f.span.start, 0usize, FileEvent::Function(f)));
        }
        for declaration in &mut ast.declarations {
            events.push((
                declaration.span.start,
                1usize,
                FileEvent::Object(declaration),
            ));
        }
        events.sort_by_key(|(start, order, _)| (*start, *order));
        for (_, _, event) in events {
            match event {
                FileEvent::Object(declaration) => self.file_object(declaration),
                FileEvent::Function(function) => self.file_function(function),
            }
        }

        for entity in self.file_entities.values_mut() {
            if !entity.is_function
                && entity.tentative
                && matches!(entity.ty, Type::Array(_, None, _))
            {
                if let Type::Array(element, None, is_static) = entity.ty.clone() {
                    entity.ty = Type::Array(element, Some(1), is_static);
                }
            }
        }
        for declaration in &mut ast.declarations {
            if let Some(entity_ty) = self
                .file_entities
                .get(&declaration.name)
                .filter(|entity| !entity.is_function)
                .map(|entity| entity.ty.clone())
            {
                declaration.ty = entity_ty.clone();
                self.update_symbol_type(&declaration.name, &entity_ty);
            }
        }

        for f in &mut ast.items {
            if matches!(
                f.ret,
                Type::Array(_, _, _) | Type::Vla(_, _) | Type::Function(_, _, _, _)
            ) {
                self.errs.push(Diagnostic::error(
                    "function cannot return array or function type",
                    Some(f.span),
                ));
            }
            if !matches!(Self::unqualified(&f.ret), Type::Void) && !self.valid_object_type(&f.ret) {
                self.errs.push(Diagnostic::error(
                    "function return type is incomplete or invalid",
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
                } else if !self.valid_object_type(t) {
                    self.errs.push(Diagnostic::error(
                        "function parameter has incomplete or invalid type",
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

    fn file_object(&mut self, declaration: &mut GlobalDecl) {
        let linkage = match declaration.storage {
            Some(StorageClass::Static) => Linkage::Internal,
            Some(StorageClass::Extern) => self
                .file_entities
                .get(&declaration.name)
                .map(|entity| entity.linkage)
                .unwrap_or(Linkage::External),
            _ => Linkage::External,
        };
        self.merge_file_entity(
            &declaration.name,
            &declaration.ty,
            false,
            linkage,
            declaration.initializer.is_some(),
            declaration.tentative_definition,
            declaration.span,
        );
        if let Some(entity_ty) = self
            .file_entities
            .get(&declaration.name)
            .filter(|entity| !entity.is_function)
            .map(|entity| entity.ty.clone())
        {
            declaration.ty = entity_ty;
        }
        if linkage == Linkage::Internal {
            declaration.storage = Some(StorageClass::Static);
        }
        if let Some(initializer) = &mut declaration.initializer {
            self.check_initializer(&mut declaration.ty, initializer, declaration.span);
            if !self.constant_initializer_list(initializer) {
                self.errs.push(Diagnostic::error(
                    "file-scope initializer is not a supported constant expression",
                    Some(declaration.span),
                ));
            }
            if let Some(entity) = self.file_entities.get_mut(&declaration.name) {
                if Self::compatible_declaration(&entity.ty, &declaration.ty) {
                    entity.ty = Self::composite(&entity.ty, &declaration.ty);
                }
                declaration.ty = entity.ty.clone();
            }
        }
        self.update_symbol_type(&declaration.name, &declaration.ty);
        if Self::contains_vla(&declaration.ty) {
            self.errs.push(Diagnostic::error(
                "file-scope object cannot have variable-length array type",
                Some(declaration.span),
            ));
        }
        let is_definition = declaration.tentative_definition;
        if !self.valid_file_object_type(&declaration.ty, is_definition) {
            self.errs.push(Diagnostic::error(
                format!("invalid file-scope object type for {}", declaration.name),
                Some(declaration.span),
            ));
        }
    }

    fn file_function(&mut self, function: &mut Function) {
        let ty = Type::Function(
            Box::new(function.ret.clone()),
            function.params.iter().map(|(_, ty)| ty.clone()).collect(),
            function.is_variadic,
            !function.params_unspecified,
        );
        let linkage = if function.is_static {
            Linkage::Internal
        } else {
            self.file_entities
                .get(&function.name)
                .map(|entity| entity.linkage)
                .unwrap_or(Linkage::External)
        };
        self.merge_file_entity(
            &function.name,
            &ty,
            true,
            linkage,
            !function.external,
            false,
            function.span,
        );
        function.is_static = linkage == Linkage::Internal;
    }

    fn merge_file_entity(
        &mut self,
        name: &str,
        ty: &Type,
        is_function: bool,
        linkage: Linkage,
        is_definition: bool,
        is_tentative: bool,
        span: crate::diag::Span,
    ) {
        if let Some(previous) = self.file_entities.get(name).cloned() {
            if previous.is_function != is_function
                || !Self::compatible_declaration(&previous.ty, ty)
            {
                self.errs.push(Diagnostic::error(
                    format!("conflicting types for {}", name),
                    Some(span),
                ));
            }
            if previous.linkage != linkage {
                let message = match (previous.linkage, linkage) {
                    (Linkage::External, Linkage::Internal) => format!(
                        "static declaration of {} follows non-static declaration",
                        name
                    ),
                    (Linkage::Internal, Linkage::External) => format!(
                        "non-static declaration of {} follows static declaration",
                        name
                    ),
                    _ => unreachable!(),
                };
                self.errs.push(Diagnostic::error(message, Some(span)));
            }
            if is_definition && previous.defined {
                self.errs.push(Diagnostic::error(
                    format!("redefinition of {}", name),
                    Some(span),
                ));
            }
            let compatible = self
                .file_entities
                .get(name)
                .is_some_and(|entity| Self::compatible_declaration(&entity.ty, ty));
            let entity = self.file_entities.get_mut(name).unwrap();
            if compatible {
                entity.ty = Self::composite(&entity.ty, ty);
            }
            entity.linkage = linkage;
            entity.defined |= is_definition;
            entity.tentative |= is_tentative;
        } else {
            self.file_entities.insert(
                name.to_string(),
                FileEntity {
                    ty: ty.clone(),
                    linkage,
                    is_function,
                    defined: is_definition,
                    tentative: is_tentative,
                },
            );
        }
        if self.syms.lookup_current_scope(name).is_none() {
            if let Err(error) = self.syms.insert(Symbol {
                name: name.to_string(),
                ty: ty.clone(),
                is_register: false,
            }) {
                self.errs.push(Diagnostic::error(error.0, Some(span)));
            }
        }
        let ty = self
            .file_entities
            .get(name)
            .map(|entity| entity.ty.clone())
            .unwrap_or_else(|| ty.clone());
        self.update_symbol_type(name, &ty);
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

    fn update_symbol_type(&mut self, name: &str, ty: &Type) {
        if let Some(scope) = self.syms.scopes.last_mut() {
            if let Some(symbol) = scope.get_mut(name) {
                symbol.ty = ty.clone();
            }
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
                    let symbol = Symbol {
                        name: name.clone(),
                        ty: ty.clone(),
                        is_register: *storage == StorageClass::Register,
                    };
                    if let Err(e) = self.syms.insert(symbol) {
                        self.errs.push(Diagnostic::error(e.0, Some(span)));
                    }
                    if let Some(initializer) = init {
                        self.check_initializer(ty, initializer, span);
                        self.update_symbol_type(name, ty);
                    }
                    let valid = if *storage == StorageClass::Extern {
                        self.valid_extern_object_type(ty)
                    } else {
                        self.valid_object_type(ty)
                    };
                    if !valid {
                        self.errs.push(Diagnostic::error(
                            format!("invalid object type for {}", name),
                            Some(span),
                        ));
                    }
                    if Self::contains_vla(ty) {
                        if init.is_some() {
                            self.errs.push(Diagnostic::error(
                                "variable-length array cannot be initialized",
                                Some(span),
                            ));
                        }
                        if *storage == StorageClass::Static {
                            self.errs.push(Diagnostic::error(
                                "static local object cannot have variable-length array type",
                                Some(span),
                            ));
                        }
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
                            .is_some_and(|initializer| !self.constant_initializer_list(initializer))
                    {
                        self.errs.push(Diagnostic::error(
                            "static local initializer must be a constant expression",
                            Some(span),
                        ));
                    }
                    if *storage == StorageClass::Static
                        && !Self::arithmetic(ty)
                        && !matches!(Self::unqualified(ty), Type::Pointer(_))
                    {
                        self.errs.push(Diagnostic::error(
                            "static local objects currently require a scalar type",
                            Some(span),
                        ));
                    }
                }
            }
            Stmt::Typedef(_, _, _) => {}
            Stmt::Typedefs(_, _) => {}
            Stmt::Decl(n, t, x, _) => {
                self.add(n, t, span);
                if let Some(initializer) = x {
                    self.check_initializer(t, initializer, span);
                    self.update_symbol_type(n, t);
                }
                if !self.valid_object_type(t) {
                    self.errs.push(Diagnostic::error(
                        format!("invalid object type for {}", n),
                        Some(span),
                    ));
                }
                if x.is_some() && Self::contains_vla(t) {
                    self.errs.push(Diagnostic::error(
                        "variable-length array cannot be initialized",
                        Some(span),
                    ));
                }
            }
            Stmt::Decls(declarations, _) => {
                for (n, t, x) in declarations {
                    self.add(n, t, span);
                    if let Some(initializer) = x {
                        self.check_initializer(t, initializer, span);
                        self.update_symbol_type(n, t);
                    }
                    if !self.valid_object_type(t) {
                        self.errs.push(Diagnostic::error(
                            format!("invalid object type for {}", n),
                            Some(span),
                        ));
                    }
                    if x.is_some() && Self::contains_vla(t) {
                        self.errs.push(Diagnostic::error(
                            "variable-length array cannot be initialized",
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
                    let got = Self::decay(&self.expr(e, span));
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
            // BUG: suffixes are ignored; only the value's range selects a wider type.
            Expr::Int(n) => Self::integer_literal_type(*n),
            Expr::Char(_) => Type::Int,
            Expr::Float(_, ty) => ty.clone(),
            Expr::Complex(_, _, ty) => Type::Complex(Box::new(ty.clone())),
            // BUG: string literals have array type in C99, not char pointer.
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
                let value_ty = Self::decay(&self.expr(value, span));
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
                let left = Self::decay(&self.expr(a, span));
                let right = Self::decay(&self.expr(b, span));
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
                        if Self::complex(&left) || Self::complex(&right) {
                            self.errs.push(Diagnostic::error(
                                "relational operands cannot have complex type",
                                Some(span),
                            ));
                        } else if !(Self::arithmetic(&left) && Self::arithmetic(&right))
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
                        Self::usual_arithmetic_type(&left, &right)
                    }
                    BinOp::Shl | BinOp::Shr => {
                        self.require_integer(&left, "shift operand", span);
                        self.require_integer(&right, "shift operand", span);
                        Self::integer_promote(&left)
                    }
                    BinOp::Add | BinOp::Sub => {
                        let pointer_integer = (Self::pointer_like(&left) && Self::integer(&right))
                            || (*op == BinOp::Add
                                && Self::integer(&left)
                                && Self::pointer_like(&right));
                        if pointer_integer {
                            if Self::pointer_like(&left) {
                                Self::decay(&left)
                            } else {
                                Self::decay(&right)
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
                        if matches!(op, BinOp::Rem)
                            && (Self::complex(&left) || Self::complex(&right))
                        {
                            self.errs.push(Diagnostic::error(
                                "remainder operands cannot have complex type",
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
                        if self.is_bitfield_member(x) {
                            self.errs.push(Diagnostic::error(
                                "cannot take the address of a bit-field",
                                Some(span),
                            ));
                        }
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
                        if self.is_bitfield_member(x) {
                            self.errs.push(Diagnostic::error(
                                "sizeof cannot be applied to a bit-field",
                                Some(span),
                            ));
                        } else if !self.valid_object_type(&ty) {
                            self.errs.push(Diagnostic::error(
                                "sizeof requires a complete object type",
                                Some(span),
                            ));
                        }
                        Type::UnsignedLong
                    }
                }
            }
            Expr::Cast(target, value) => {
                let source = Self::decay(&self.expr(value, span));
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
            Expr::VaStart(ap) | Expr::VaEnd(ap) => {
                self.expr(ap, span);
                Type::Void
            }
            Expr::VaCopy(destination, source) => {
                self.expr(destination, span);
                self.expr(source, span);
                Type::Void
            }
            Expr::VaArg(ap, ty) => {
                self.expr(ap, span);
                *ty = self.resolve_tag(ty);
                if !Self::arithmetic(ty) && !matches!(Self::unqualified(ty), Type::Pointer(_)) {
                    self.errs.push(Diagnostic::error(
                        "va_arg currently supports scalar types",
                        Some(span),
                    ));
                }
                ty.clone()
            }
            Expr::Call(n, args) => {
                if let Some(ty) = self.syms.lookup(n).map(|symbol| symbol.ty.clone()) {
                    self.finish_call(n, &ty, args, span)
                } else {
                    for arg in args.iter_mut() {
                        self.expr(arg, span);
                    }
                    // HACK HACK: undeclared callees are silently accepted as an implicit-int extension.
                    // Keep the existing external-call extension for hosted libc symbols.
                    Type::Int
                }
            }
            Expr::IndirectCall(callee, args) => {
                let ty = self.expr(callee, span);
                self.finish_call("called expression", &ty, args, span)
            }
            Expr::Conditional(condition, yes, no) => {
                let condition_ty = self.expr(condition, span);
                self.require_scalar(&condition_ty, "conditional condition", span);
                let yes_ty = Self::decay(&self.expr(yes, span));
                let no_ty = Self::decay(&self.expr(no, span));
                if !Self::compatible(&yes_ty, &no_ty) {
                    self.errs.push(Diagnostic::error(
                        "conditional operands have incompatible types",
                        Some(span),
                    ));
                }
                if Self::arithmetic(&yes_ty) && Self::arithmetic(&no_ty) {
                    Self::usual_arithmetic_type(&yes_ty, &no_ty)
                } else {
                    yes_ty
                }
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
                        .find(|field| field.name.as_deref() == Some(name))
                        .map(|field| field.ty.clone())
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
                    Type::Pointer(inner) | Type::Array(inner, _, _) | Type::Vla(inner, _) => *inner,
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
        let resolved = self.resolve_tag(target);
        *target = resolved;
        match initializer {
            Initializer::Expr(expr) => self.check_initializer_expr(target, expr, span),
            Initializer::List(items) => {
                let mut cursor = InitCursor::new(items);
                self.check_initializer_items(target, &mut cursor, span);
            }
        }
    }

    fn check_initializer_expr(
        &mut self,
        target: &mut Type,
        expr: &mut Expr,
        span: crate::diag::Span,
    ) {
        Self::infer_array_bound(target, expr, &mut self.errs, span);
        let got = Self::decay(&self.expr(expr, span));
        let null_pointer =
            Self::pointer_like(target) && Self::integer(&got) && matches!(expr, Expr::Int(0));
        if !Self::compatible(target, &got)
            && !null_pointer
            && !(Self::arithmetic(target) && Self::arithmetic(&got))
        {
            self.errs.push(Diagnostic::error(
                format!(
                    "initializer is incompatible with {}",
                    Self::describe(target)
                ),
                Some(span),
            ));
        }
    }

    fn check_initializer_from_value(
        &mut self,
        target: &mut Type,
        value: &mut Initializer,
        cursor: &mut InitCursor,
        span: crate::diag::Span,
    ) {
        match value {
            Initializer::List(items) => {
                let mut nested = InitCursor::new(items);
                self.check_initializer_items(target, &mut nested, span);
            }
            Initializer::Expr(expr) => {
                let string_array = matches!(Self::unqualified(target), Type::Array(_, _, _))
                    && matches!(**expr, Expr::String(_));
                if Self::aggregate(target)
                    && !string_array
                    && !matches!(&**expr, Expr::CompoundLiteral(_, _))
                {
                    self.check_initializer_elided(target, expr, cursor, span);
                } else {
                    self.check_initializer_expr(target, expr, span);
                }
            }
        }
    }

    fn check_initializer_items(
        &mut self,
        target: &mut Type,
        cursor: &mut InitCursor,
        span: crate::diag::Span,
    ) {
        match target {
            Type::Qualified(inner, _) => self.check_initializer_items(inner, cursor, span),
            Type::Array(element, count, _) => {
                let mut next = 0usize;
                let mut high = 0usize;
                while !cursor.is_empty()
                    && (!count.is_some_and(|n| next >= n)
                        || cursor
                            .peek()
                            .is_some_and(|item| !item.designators.is_empty()))
                {
                    let Some(item) = cursor.next() else { break };
                    let index = match item.designators.first() {
                        Some(Designator::Index(expr)) => {
                            Self::check_index_designator(self, expr, next, *count, span)
                        }
                        Some(Designator::Member(_)) => {
                            self.errs.push(Diagnostic::error(
                                "array initializer requires an array designator",
                                Some(span),
                            ));
                            next
                        }
                        None => next,
                    };
                    if item.designators.len() > 1 {
                        let mut designators = item.designators.clone();
                        let mut value = item.value.clone();
                        self.check_designated(element, &mut designators[1..], &mut value, span);
                    } else {
                        let mut value = item.value.clone();
                        self.check_initializer_from_value(element, &mut value, cursor, span);
                    }
                    next = index.saturating_add(1);
                    high = high.max(next);
                }
                if count.is_none() {
                    *count = Some(high);
                }
            }
            Type::Struct(_, fields) => {
                let mut next = Self::next_initializable(fields, 0);
                while !cursor.is_empty()
                    && (next < fields.len()
                        || cursor
                            .peek()
                            .is_some_and(|item| !item.designators.is_empty()))
                {
                    let Some(item) = cursor.next() else { break };
                    let index = match item.designators.first() {
                        Some(Designator::Member(name)) => fields
                            .iter()
                            .position(|field| field.name.as_deref() == Some(name))
                            .unwrap_or_else(|| {
                                self.errs.push(Diagnostic::error(
                                    format!("unknown member {} in initializer", name),
                                    Some(span),
                                ));
                                next
                            }),
                        _ => next,
                    };
                    if let Some(field) = fields.get_mut(index) {
                        if item.designators.len() > 1 {
                            let mut designators = item.designators.clone();
                            let mut value = item.value.clone();
                            self.check_designated(
                                &mut field.ty,
                                &mut designators[1..],
                                &mut value,
                                span,
                            );
                        } else {
                            let mut value = item.value.clone();
                            self.check_initializer_from_value(
                                &mut field.ty,
                                &mut value,
                                cursor,
                                span,
                            );
                        }
                    } else {
                        self.errs.push(Diagnostic::error(
                            "too many structure initializers",
                            Some(span),
                        ));
                    }
                    next = Self::next_initializable(fields, index.saturating_add(1));
                }
            }
            Type::Union(_, fields) => {
                let Some(item) = cursor.next() else { return };
                // BUG: undesignated union initializer blindly selects field 0 and ignores extras.
                let index = match item.designators.first() {
                    Some(Designator::Member(name)) => fields
                        .iter()
                        .position(|field| field.name.as_deref() == Some(name))
                        .unwrap_or_else(|| {
                            self.errs.push(Diagnostic::error(
                                format!("unknown member {} in initializer", name),
                                Some(span),
                            ));
                            0
                        }),
                    _ => 0,
                };
                if let Some(field) = fields.get_mut(index) {
                    if item.designators.len() > 1 {
                        let mut designators = item.designators.clone();
                        let mut value = item.value.clone();
                        self.check_designated(
                            &mut field.ty,
                            &mut designators[1..],
                            &mut value,
                            span,
                        );
                    } else {
                        let mut value = item.value.clone();
                        self.check_initializer_from_value(&mut field.ty, &mut value, cursor, span);
                    }
                }
            }
            _ => {
                let Some(item) = cursor.next() else { return };
                if item.designators.is_empty() {
                    let mut value = item.value.clone();
                    self.check_initializer_from_value(target, &mut value, cursor, span);
                    if !cursor.is_empty() {
                        self.errs.push(Diagnostic::error(
                            "excess elements in scalar initializer",
                            Some(span),
                        ));
                    }
                } else {
                    self.errs.push(Diagnostic::error(
                        "scalar initializer cannot use a designator",
                        Some(span),
                    ));
                }
            }
        }
    }

    fn next_initializable(fields: &[Field], mut index: usize) -> usize {
        while index < fields.len() && fields[index].name.is_none() {
            index += 1;
        }
        index
    }

    fn check_initializer_elided(
        &mut self,
        target: &mut Type,
        first: &mut Expr,
        cursor: &mut InitCursor,
        span: crate::diag::Span,
    ) {
        match target {
            Type::Qualified(inner, _) => self.check_initializer_elided(inner, first, cursor, span),
            Type::Array(element, count, _) => {
                self.check_initializer_elided_value(element, first, cursor, span);
                let mut next = 1usize;
                let mut high = 1usize;
                while !cursor.is_empty()
                    && (!count.is_some_and(|n| next >= n)
                        || cursor
                            .peek()
                            .is_some_and(|item| !item.designators.is_empty()))
                {
                    let Some(item) = cursor.next() else { break };
                    let index = match item.designators.first() {
                        Some(Designator::Index(expr)) => {
                            Self::check_index_designator(self, expr, next, *count, span)
                        }
                        Some(Designator::Member(_)) => {
                            self.errs.push(Diagnostic::error(
                                "array initializer requires an array designator",
                                Some(span),
                            ));
                            next
                        }
                        None => next,
                    };
                    if item.designators.len() > 1 {
                        let mut designators = item.designators.clone();
                        let mut value = item.value.clone();
                        self.check_designated(element, &mut designators[1..], &mut value, span);
                    } else {
                        let mut value = item.value.clone();
                        self.check_initializer_from_value(element, &mut value, cursor, span);
                    }
                    next = index.saturating_add(1);
                    high = high.max(next);
                }
                if count.is_none() {
                    *count = Some(high);
                }
            }
            Type::Struct(_, fields) => {
                let first_index = Self::next_initializable(fields, 0);
                let Some(field) = fields.get_mut(first_index) else {
                    return;
                };
                self.check_initializer_elided_value(&mut field.ty, first, cursor, span);
                let mut next = Self::next_initializable(fields, first_index + 1);
                while !cursor.is_empty()
                    && (next < fields.len()
                        || cursor
                            .peek()
                            .is_some_and(|item| !item.designators.is_empty()))
                {
                    let Some(item) = cursor.next() else { break };
                    let index = match item.designators.first() {
                        Some(Designator::Member(name)) => fields
                            .iter()
                            .position(|field| field.name.as_deref() == Some(name))
                            .unwrap_or_else(|| {
                                self.errs.push(Diagnostic::error(
                                    format!("unknown member {} in initializer", name),
                                    Some(span),
                                ));
                                next
                            }),
                        _ => next,
                    };
                    if let Some(field) = fields.get_mut(index) {
                        if item.designators.len() > 1 {
                            let mut designators = item.designators.clone();
                            let mut value = item.value.clone();
                            self.check_designated(
                                &mut field.ty,
                                &mut designators[1..],
                                &mut value,
                                span,
                            );
                        } else {
                            let mut value = item.value.clone();
                            self.check_initializer_from_value(
                                &mut field.ty,
                                &mut value,
                                cursor,
                                span,
                            );
                        }
                    }
                    next = Self::next_initializable(fields, index.saturating_add(1));
                }
            }
            Type::Union(_, fields) => {
                if let Some(field) = fields.first_mut() {
                    self.check_initializer_elided_value(&mut field.ty, first, cursor, span);
                }
            }
            _ => self.check_initializer_expr(target, first, span),
        }
    }

    fn check_initializer_elided_value(
        &mut self,
        target: &mut Type,
        first: &mut Expr,
        cursor: &mut InitCursor,
        span: crate::diag::Span,
    ) {
        let string_array = matches!(Self::unqualified(target), Type::Array(_, _, _))
            && matches!(first, Expr::String(_));
        if Self::aggregate(target) && !string_array && !matches!(first, Expr::CompoundLiteral(_, _))
        {
            self.check_initializer_elided(target, first, cursor, span);
        } else {
            self.check_initializer_expr(target, first, span);
        }
    }

    fn check_index_designator(
        &mut self,
        expr: &Expr,
        next: usize,
        count: Option<usize>,
        span: crate::diag::Span,
    ) -> usize {
        let mut expr = expr.clone();
        let got = self.expr(&mut expr, span);
        let value = Self::integer_constant(&expr);
        let index = value.and_then(|x| usize::try_from(x).ok());
        if value.is_some_and(|x| x < 0) {
            self.errs.push(Diagnostic::error(
                "array designator is negative",
                Some(span),
            ));
        } else if !Self::integer(&got) || index.is_none() {
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
            index
        } else {
            next
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
            (Designator::Index(expr), Type::Array(element, count, _)) => {
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
                if let Some(field) = fields
                    .iter_mut()
                    .find(|field| field.name.as_deref() == Some(name))
                {
                    self.check_designated(&mut field.ty, &mut designators[1..], value, span);
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
                    BinOp::Shl => u32::try_from(b).ok().and_then(|b| a.checked_shl(b)),
                    BinOp::Shr => u32::try_from(b).ok().and_then(|b| a.checked_shr(b)),
                    BinOp::BitAnd => Some(a & b),
                    BinOp::BitOr => Some(a | b),
                    BinOp::BitXor => Some(a ^ b),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn is_constant_initializer(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Var(name) => self
                .syms
                .lookup(name)
                .is_some_and(|symbol| matches!(Self::unqualified(&symbol.ty), Type::Function(..))),
            Expr::Int(_)
            | Expr::Float(_, _)
            | Expr::Complex(_, _, _)
            | Expr::Char(_)
            | Expr::String(_) => true,
            Expr::Cast(_, value)
            | Expr::Unary(UnOp::Neg | UnOp::Pos | UnOp::Not | UnOp::BitNot, value) => {
                self.is_constant_initializer(value)
            }
            Expr::Binary(op, left, right) if *op != BinOp::Comma => {
                self.is_constant_initializer(left) && self.is_constant_initializer(right)
            }
            Expr::Conditional(condition, yes, no) => {
                self.is_constant_initializer(condition)
                    && self.is_constant_initializer(yes)
                    && self.is_constant_initializer(no)
            }
            Expr::Unary(UnOp::Address, value) => {
                // BUG: any address-of variable is treated as constant, including automatic objects.
                matches!(&**value, Expr::Var(_))
                    || matches!(&**value, Expr::CompoundLiteral(_, initializer) if self.constant_initializer_list(initializer))
            }
            Expr::CompoundLiteral(_, initializer) => self.constant_initializer_list(initializer),
            _ => false,
        }
    }

    fn constant_initializer_list(&self, initializer: &Initializer) -> bool {
        match initializer {
            Initializer::Expr(expr) => self.is_constant_initializer(expr),
            Initializer::List(items) => items.iter().all(|item| {
                item.designators.iter().all(|designator| match designator {
                    Designator::Index(expr) => self.is_constant_initializer(expr),
                    Designator::Member(_) => true,
                }) && self.constant_initializer_list(&item.value)
            }),
        }
    }

    fn is_bitfield_member(&self, e: &Expr) -> bool {
        let Expr::Member(base, name, arrow) = e else {
            return false;
        };
        let base_ty = self.expr_type(base);
        let base_ty = if *arrow {
            match Self::unqualified(&base_ty) {
                Type::Pointer(inner) => *inner,
                _ => return false,
            }
        } else {
            base_ty
        };
        let base_ty = self.resolve_tag(&Self::unqualified(&base_ty));
        match base_ty {
            Type::Struct(_, fields) | Type::Union(_, fields) => fields
                .iter()
                .find(|field| field.name.as_deref() == Some(name))
                .and_then(|field| field.bit_width)
                .is_some_and(|width| width > 0),
            _ => false,
        }
    }

    fn is_lvalue(&self, e: &Expr) -> bool {
        match e {
            Expr::Var(n) => self
                .syms
                .lookup(n)
                .map(|x| !matches!(Self::unqualified(&x.ty), Type::Function(_, _, _, _)))
                .unwrap_or(false),
            Expr::Member(base, _, arrow) => {
                if *arrow {
                    true
                } else {
                    self.is_lvalue(base)
                }
            }
            Expr::Subscript(base, _) => {
                let ty = self.expr_type(base);
                matches!(
                    Self::unqualified(&ty),
                    Type::Pointer(_) | Type::Array(_, _, _) | Type::Vla(_, _)
                )
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
            Type::Array(_, _, _) | Type::Vla(_, _) | Type::Function(_, _, _, _) | Type::Void
        ) && !Self::top_const(&ty)
    }

    fn finish_call(
        &mut self,
        label: &str,
        ty: &Type,
        args: &mut [Expr],
        span: crate::diag::Span,
    ) -> Type {
        let arg_types: Vec<_> = args
            .iter_mut()
            .map(|arg| Self::decay(&self.expr(arg, span)))
            .collect();
        let function = match Self::unqualified(ty) {
            function @ Type::Function(_, _, _, _) => function,
            Type::Pointer(inner) => match Self::unqualified(&inner) {
                function @ Type::Function(_, _, _, _) => function,
                _ => {
                    self.errs.push(Diagnostic::error(
                        format!("{} is not a function", label),
                        Some(span),
                    ));
                    return Type::Int;
                }
            },
            _ => {
                self.errs.push(Diagnostic::error(
                    format!("{} is not a function", label),
                    Some(span),
                ));
                return Type::Int;
            }
        };
        let Type::Function(ret, params, variadic, has_prototype) = function else {
            unreachable!()
        };
        if has_prototype
            && ((!variadic && params.len() != arg_types.len())
                || (variadic && params.len() > arg_types.len()))
        {
            self.errs.push(Diagnostic::error(
                format!(
                    "function {} expects {} argument(s), got {}",
                    label,
                    params.len(),
                    arg_types.len()
                ),
                Some(span),
            ));
        }
        if has_prototype {
            for (index, (expected, got)) in params.iter().zip(arg_types.iter()).enumerate() {
                if !Self::compatible(expected, got) {
                    self.errs.push(Diagnostic::error(
                        format!("argument {} to {} has incompatible type", index + 1, label),
                        Some(span),
                    ));
                }
            }
        }
        *ret
    }

    fn expr_type(&self, e: &Expr) -> Type {
        match e {
            Expr::Int(n) => Self::integer_literal_type(*n),
            Expr::Char(_) => Type::Int,
            Expr::Float(_, ty) => ty.clone(),
            Expr::Complex(_, _, ty) => Type::Complex(Box::new(ty.clone())),
            Expr::Cast(ty, _) => ty.clone(),
            Expr::SizeofType(_) | Expr::Unary(UnOp::Sizeof, _) => Type::UnsignedLong,
            Expr::CompoundLiteral(ty, _) => ty.clone(),
            Expr::VaStart(_) | Expr::VaEnd(_) | Expr::VaCopy(_, _) => Type::Void,
            Expr::VaArg(_, ty) => ty.clone(),
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
                BinOp::Mul | BinOp::Div | BinOp::Rem => {
                    Self::usual_arithmetic_type(&self.expr_type(a), &self.expr_type(b))
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
                Some(ty) => Self::call_return_type(&ty),
                None => Type::Int,
            },
            Expr::IndirectCall(callee, _) => Self::call_return_type(&self.expr_type(callee)),
            Expr::Conditional(_, yes, no) => {
                let yes = self.expr_type(yes);
                let no = self.expr_type(no);
                if Self::pointer_like(&yes) {
                    yes
                } else if Self::pointer_like(&no) {
                    no
                } else {
                    Self::usual_arithmetic_type(&yes, &no)
                }
            }
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
                        .find(|field| field.name.as_deref() == Some(name))
                        .map(|field| field.ty)
                        .unwrap_or(Type::Int),
                    _ => Type::Int,
                }
            }
            Expr::Subscript(base, _) => match Self::unqualified(&self.expr_type(base)) {
                Type::Pointer(inner) | Type::Array(inner, _, _) | Type::Vla(inner, _) => *inner,
                _ => Type::Int,
            },
        }
    }

    fn contains_vla(t: &Type) -> bool {
        match t {
            Type::Vla(_, _) => true,
            Type::Qualified(inner, _) => Self::contains_vla(inner),
            Type::Array(inner, _, _) => Self::contains_vla(inner),
            _ => false,
        }
    }

    fn valid_extern_object_type(&self, t: &Type) -> bool {
        match Self::unqualified(t) {
            Type::Void | Type::Function(_, _, _, _) => false,
            Type::Array(element, None, _) => self.valid_object_type(&element),
            Type::Struct(n, fields) | Type::Union(n, fields)
                if fields.is_empty() && !self.complete_tags.contains(&n) =>
            {
                true
            }
            _ => self.valid_object_type(t),
        }
    }

    fn valid_file_object_type(&self, t: &Type, is_definition: bool) -> bool {
        if !is_definition || matches!(t, Type::Array(_, None, _)) {
            return self.valid_extern_object_type(t);
        }
        self.valid_object_type(t)
    }

    fn valid_object_type(&self, t: &Type) -> bool {
        match t {
            Type::Void | Type::Function(_, _, _, _) => false,
            Type::Array(x, Some(n), false) => *n > 0 && self.valid_object_type(x),
            Type::Array(_, _, true) | Type::Array(_, None, _) => false,
            Type::Vla(x, _) => self.valid_object_type(x),
            Type::Pointer(_) => true,
            Type::Qualified(x, q) => {
                let inner = Self::unqualified(x);
                if matches!(inner, Type::Array(_, _, _) | Type::Vla(_, _)) {
                    return false;
                }
                if q.is_restrict && !matches!(inner, Type::Pointer(_)) {
                    return false;
                }
                self.valid_object_type(x)
            }
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

    fn decay(t: &Type) -> Type {
        match Self::unqualified(t) {
            Type::Function(_, _, _, _) => Type::Pointer(Box::new(t.clone())),
            Type::Array(element, _, _) | Type::Vla(element, _) => Type::Pointer(element),
            _ => t.clone(),
        }
    }

    fn call_return_type(t: &Type) -> Type {
        match Self::unqualified(t) {
            Type::Function(ret, _, _, _) => *ret,
            Type::Pointer(inner) => match Self::unqualified(&inner) {
                Type::Function(ret, _, _, _) => *ret,
                _ => Type::Int,
            },
            _ => Type::Int,
        }
    }

    fn character(t: &Type) -> bool {
        matches!(
            Self::unqualified(t),
            Type::Char | Type::SignedChar | Type::UnsignedChar
        )
    }

    fn infer_array_bound(
        ty: &mut Type,
        initializer: &Expr,
        errs: &mut Vec<Diagnostic>,
        span: crate::diag::Span,
    ) {
        let Expr::String(value) = initializer else {
            return;
        };
        match ty {
            Type::Qualified(inner, _) => Self::infer_array_bound(inner, initializer, errs, span),
            Type::Array(element, count, _) if Self::character(element) => {
                let needed = value.len() + 1;
                match *count {
                    None => *count = Some(needed),
                    Some(bound) if value.len() > bound => errs.push(Diagnostic::error(
                        "initializer string is too long for array",
                        Some(span),
                    )),
                    _ => {}
                }
            }
            _ => {}
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

    fn complex(t: &Type) -> bool {
        matches!(Self::unqualified(t), Type::Complex(_))
    }

    fn real_type(t: &Type) -> Type {
        match Self::unqualified(t) {
            Type::Complex(inner) => *inner,
            other => other,
        }
    }

    fn arithmetic(t: &Type) -> bool {
        Self::integer(t)
            || matches!(
                Self::unqualified(t),
                Type::Float | Type::Double | Type::LongDouble | Type::Complex(_)
            )
    }

    fn aggregate(t: &Type) -> bool {
        matches!(
            Self::unqualified(t),
            Type::Array(_, _, _) | Type::Struct(_, _) | Type::Union(_, _)
        )
    }

    fn scalar(t: &Type) -> bool {
        Self::arithmetic(t) || Self::pointer_like(t)
    }

    fn pointer_like(t: &Type) -> bool {
        matches!(
            Self::unqualified(t),
            Type::Pointer(_) | Type::Array(_, _, _) | Type::Vla(_, _)
        )
    }

    fn compatible(a: &Type, b: &Type) -> bool {
        // BUG: stripping qualifiers here makes const/volatile pointers falsely compatible.
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
            (Type::Pointer(x), Type::Array(y, _, _))
            | (Type::Array(y, _, _), Type::Pointer(x))
            | (Type::Pointer(x), Type::Vla(y, _))
            | (Type::Vla(y, _), Type::Pointer(x)) => Self::compatible(x, y),
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

    fn same_qualifiers(a: &Qualifiers, b: &Qualifiers) -> bool {
        a.is_const == b.is_const && a.is_volatile == b.is_volatile && a.is_restrict == b.is_restrict
    }

    fn compatible_declaration(a: &Type, b: &Type) -> bool {
        match (a, b) {
            (Type::Qualified(ai, aq), Type::Qualified(bi, bq)) => {
                Self::same_qualifiers(aq, bq) && Self::compatible_declaration(ai, bi)
            }
            (Type::Qualified(_, _), _) | (_, Type::Qualified(_, _)) => false,
            (Type::Int | Type::SignedInt, Type::Int | Type::SignedInt) => true,
            (Type::Struct(a, _), Type::Struct(b, _)) | (Type::Union(a, _), Type::Union(b, _)) => {
                a == b
            }
            (Type::Enum(a, _), Type::Enum(b, _)) => a == b,
            (Type::Array(ae, ac, _), Type::Array(be, bc, _)) => {
                (ac.is_none() || bc.is_none() || ac == bc) && Self::compatible_declaration(ae, be)
            }
            (Type::Vla(ae, _), Type::Vla(be, _)) => Self::compatible_declaration(ae, be),
            (Type::Pointer(ai), Type::Pointer(bi)) => Self::compatible_declaration(ai, bi),
            (Type::Function(ar, ap, av, aproto), Type::Function(br, bp, bv, bproto)) => {
                Self::compatible_declaration(ar, br)
                    && if !aproto || !bproto {
                        !av && !bv
                    } else {
                        av == bv
                            && ap.len() == bp.len()
                            && ap.iter().zip(bp).all(|(a, b)| {
                                Self::compatible_declaration(
                                    &Self::unqualified(a),
                                    &Self::unqualified(b),
                                )
                            })
                    }
            }
            (a, b) => a == b,
        }
    }

    fn composite(a: &Type, b: &Type) -> Type {
        if a == b {
            return a.clone();
        }
        match (a, b) {
            (Type::Qualified(ai, aq), Type::Qualified(bi, _)) => {
                Type::Qualified(Box::new(Self::composite(ai, bi)), *aq)
            }
            (Type::Array(ae, ac, astatic), Type::Array(be, bc, bstatic)) => Type::Array(
                Box::new(Self::composite(ae, be)),
                ac.or(*bc),
                *astatic || *bstatic,
            ),
            (Type::Vla(ae, bound), Type::Vla(be, _)) => {
                Type::Vla(Box::new(Self::composite(ae, be)), bound.clone())
            }
            (Type::Pointer(ai), Type::Pointer(bi)) => {
                Type::Pointer(Box::new(Self::composite(ai, bi)))
            }
            (Type::Function(ar, ap, av, aproto), Type::Function(br, bp, bv, bproto)) => {
                let ret = Self::composite(ar, br);
                let params = if *aproto && *bproto {
                    ap.iter()
                        .zip(bp)
                        .map(|(a, b)| Self::composite(a, b))
                        .collect()
                } else if *aproto {
                    ap.clone()
                } else if *bproto {
                    bp.clone()
                } else {
                    Vec::new()
                };
                Type::Function(Box::new(ret), params, *av || *bv, *aproto || *bproto)
            }
            _ => a.clone(),
        }
    }

    fn integer_literal_type(value: i64) -> Type {
        if i32::try_from(value).is_ok() {
            Type::Int
        } else {
            Type::Long
        }
    }

    fn integer_promote(t: &Type) -> Type {
        if Self::integer_rank(t) < 3 {
            Type::Int
        } else {
            Self::unqualified(t)
        }
    }

    fn integer_rank(t: &Type) -> u8 {
        match Self::unqualified(t) {
            Type::Bool => 0,
            Type::Char | Type::SignedChar | Type::UnsignedChar => 1,
            Type::Short | Type::UnsignedShort => 2,
            Type::Int | Type::SignedInt | Type::UnsignedInt | Type::Enum(_, _) => 3,
            Type::Long | Type::UnsignedLong => 4,
            Type::LongLong | Type::UnsignedLongLong => 5,
            _ => 3,
        }
    }

    fn integer_unsigned(t: &Type) -> bool {
        matches!(
            Self::unqualified(t),
            Type::Bool
                | Type::UnsignedChar
                | Type::UnsignedShort
                | Type::UnsignedInt
                | Type::UnsignedLong
                | Type::UnsignedLongLong
        )
    }

    fn usual_arithmetic_type(a: &Type, b: &Type) -> Type {
        let complex = Self::complex(a) || Self::complex(b);
        let a = Self::real_type(&Self::unqualified(a));
        let b = Self::real_type(&Self::unqualified(b));
        let base = if matches!(a, Type::LongDouble) || matches!(b, Type::LongDouble) {
            Type::LongDouble
        } else if matches!(a, Type::Double) || matches!(b, Type::Double) {
            Type::Double
        } else if matches!(a, Type::Float) || matches!(b, Type::Float) {
            Type::Float
        } else {
            let rank = Self::integer_rank(&a).max(Self::integer_rank(&b));
            if rank < 3 {
                Type::Int
            } else if Self::integer_unsigned(&a) || Self::integer_unsigned(&b) {
                match rank {
                    4 => Type::UnsignedLong,
                    5 => Type::UnsignedLongLong,
                    _ => Type::UnsignedInt,
                }
            } else {
                match rank {
                    4 => Type::Long,
                    5 => Type::LongLong,
                    _ => Type::Int,
                }
            }
        };
        if complex {
            Type::Complex(Box::new(base))
        } else {
            base
        }
    }

    fn describe(t: &Type) -> &'static str {
        match Self::unqualified(t) {
            Type::Void => "void",
            Type::Pointer(_) => "pointer",
            Type::Array(_, _, _) | Type::Vla(_, _) => "array",
            Type::Function(_, _, _, _) => "function",
            Type::Float | Type::Double | Type::LongDouble => "floating type",
            Type::Complex(_) => "complex type",
            _ => "integer type",
        }
    }
}
