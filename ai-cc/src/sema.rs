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
            for (n, t) in &mut f.params {
                self.validate_array_bounds(t, f.span);
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
        self.validate_array_bounds(&mut declaration.ty, declaration.span);
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
                has_static_storage_duration: true,
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
            has_static_storage_duration: false,
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
                    self.validate_array_bounds(ty, span);
                    let symbol = Symbol {
                        name: name.clone(),
                        ty: ty.clone(),
                        is_register: *storage == StorageClass::Register,
                        has_static_storage_duration: matches!(
                            storage,
                            StorageClass::Static | StorageClass::Extern
                        ),
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
                self.validate_array_bounds(t, span);
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
                    self.validate_array_bounds(t, span);
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
                    } else if !self.assignment_compatible(&ret, &got, e) {
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
                if !self.assignment_compatible(&target_ty, &value_ty, value) {
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
                            && !self.equality_comparable(&left, &right, a, b)
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
                            && !self.relationally_comparable(&left, &right)
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
                        if *op == BinOp::Sub
                            && Self::pointer_like(&left)
                            && Self::pointer_like(&right)
                        {
                            if !self.pointer_difference_compatible(&left, &right) {
                                self.errs.push(Diagnostic::error(
                                    "pointer subtraction requires pointers to compatible complete object types",
                                    Some(span),
                                ));
                            }
                            return Type::Long;
                        }
                        let pointer_integer = (Self::pointer_like(&left) && Self::integer(&right))
                            || (*op == BinOp::Add
                                && Self::integer(&left)
                                && Self::pointer_like(&right));
                        if pointer_integer {
                            let pointer_ty = if Self::pointer_like(&left) {
                                &left
                            } else {
                                &right
                            };
                            if !self.pointer_arithmetic_type(pointer_ty) {
                                self.errs.push(Diagnostic::error(
                                    "pointer arithmetic requires a pointer to a complete object type",
                                    Some(span),
                                ));
                            }
                            Self::decay(pointer_ty)
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
            Expr::IncDec(target, _, _) => {
                let ty = self.expr(target, span);
                if !self.is_modifiable_lvalue(target) {
                    self.errs.push(Diagnostic::error(
                        "increment/decrement requires a modifiable lvalue",
                        Some(span),
                    ));
                }
                if (Self::arithmetic(&ty) && !Self::complex(&ty))
                    || (Self::pointer_like(&ty) && self.pointer_arithmetic_type(&ty))
                {
                    ty
                } else {
                    self.errs.push(Diagnostic::error(
                        "increment/decrement requires a real or pointer operand",
                        Some(span),
                    ));
                    ty
                }
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
                if Self::arithmetic(&yes_ty) && Self::arithmetic(&no_ty) {
                    Self::usual_arithmetic_type(&yes_ty, &no_ty)
                } else if Self::compatible(&yes_ty, &no_ty)
                    || (Self::pointer_like(&yes_ty)
                        && Self::integer(&no_ty)
                        && self.is_null_pointer_constant(no, &no_ty))
                {
                    yes_ty
                } else if Self::pointer_like(&no_ty)
                    && Self::integer(&yes_ty)
                    && self.is_null_pointer_constant(yes, &yes_ty)
                {
                    no_ty
                } else if Self::pointer_like(&yes_ty) && Self::pointer_like(&no_ty) {
                    if let Some(common) = Self::common_pointer_type(&yes_ty, &no_ty) {
                        common
                    } else {
                        self.errs.push(Diagnostic::error(
                            "conditional operands have incompatible types",
                            Some(span),
                        ));
                        yes_ty
                    }
                } else {
                    self.errs.push(Diagnostic::error(
                        "conditional operands have incompatible types",
                        Some(span),
                    ));
                    yes_ty
                }
            }
            Expr::Member(base, name, arrow) => {
                let base_ty = self.expr(base, span);
                let member_base = if *arrow {
                    match Self::unqualified(&base_ty) {
                        Type::Pointer(inner) => *inner,
                        _ => {
                            self.errs.push(Diagnostic::error(
                                "arrow operand must be a pointer to a structure or union",
                                Some(span),
                            ));
                            Type::Int
                        }
                    }
                } else {
                    base_ty
                };
                let aggregate = self.resolve_tag(&Self::unqualified(&member_base));
                match aggregate {
                    Type::Struct(_, fields) | Type::Union(_, fields) => fields
                        .iter()
                        .find(|field| field.name.as_deref() == Some(name))
                        .map(|field| {
                            Self::qualify_type(field.ty.clone(), Self::top_qualifiers(&member_base))
                        })
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
                let element = if Self::pointer_like(&base_ty) && Self::integer(&index_ty) {
                    Self::pointee_type(&base_ty)
                } else if Self::integer(&base_ty) && Self::pointer_like(&index_ty) {
                    Self::pointee_type(&index_ty)
                } else {
                    None
                };
                match element {
                    Some(element) => {
                        if !self.valid_object_type(&element) {
                            self.errs.push(Diagnostic::error(
                                "subscript requires a pointer to a complete object type",
                                Some(span),
                            ));
                        }
                        element
                    }
                    None => {
                        self.errs.push(Diagnostic::error(
                            "subscript requires one pointer or array and one integer operand",
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

    fn validate_array_bounds(&mut self, ty: &mut Type, span: crate::diag::Span) {
        match ty {
            Type::Qualified(inner, _) | Type::Pointer(inner) | Type::Array(inner, _, _) => {
                self.validate_array_bounds(inner, span)
            }
            Type::Vla(inner, bound) => {
                let bound_ty = self.expr(bound, span);
                self.require_integer(&bound_ty, "variable-length array bound", span);
                self.validate_array_bounds(inner, span);
            }
            Type::Function(ret, params, _, _) => {
                self.validate_array_bounds(ret, span);
                for parameter in params {
                    self.validate_array_bounds(parameter, span);
                }
            }
            Type::Struct(_, fields) | Type::Union(_, fields) => {
                for field in fields {
                    self.validate_array_bounds(&mut field.ty, span);
                }
            }
            _ => {}
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
        if !self.assignment_compatible(target, &got, expr) {
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
        crate::consteval::integer_constant(expr)
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
                matches!(&**value, Expr::Var(name) if self.syms.lookup(name).is_some_and(|symbol| {
                    symbol.has_static_storage_duration
                        || matches!(Self::unqualified(&symbol.ty), Type::Function(_, _, _, _))
                })) || (self.current_ret.is_none()
                    && matches!(&**value, Expr::CompoundLiteral(_, initializer) if self.constant_initializer_list(initializer)))
            }
            Expr::CompoundLiteral(_, initializer) => {
                self.current_ret.is_none() && self.constant_initializer_list(initializer)
            }
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
            Expr::Subscript(base, index) => {
                let base_ty = self.expr_type(base);
                let index_ty = self.expr_type(index);
                (Self::pointer_like(&base_ty) && Self::integer(&index_ty))
                    || (Self::integer(&base_ty) && Self::pointer_like(&index_ty))
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
        ) && self.valid_object_type(&ty)
            && !Self::top_const(&ty)
            && !self.contains_const_member(&ty)
    }

    fn is_null_pointer_constant(&self, expr: &Expr, ty: &Type) -> bool {
        Self::integer(ty) && Self::integer_constant(expr) == Some(0)
    }

    fn assignment_compatible(&self, target: &Type, source: &Type, value: &Expr) -> bool {
        if Self::arithmetic(target) && Self::arithmetic(source) {
            return true;
        }
        if Self::compatible(target, source) {
            return true;
        }
        match (Self::unqualified(target), Self::unqualified(source)) {
            (Type::Pointer(to), Type::Pointer(from)) => {
                Self::pointer_conversion_compatible(&to, &from)
            }
            (Type::Pointer(_), _) => self.is_null_pointer_constant(value, source),
            _ => false,
        }
    }

    fn pointer_conversion_compatible(target: &Type, source: &Type) -> bool {
        let target_qualifiers = Self::top_qualifiers(target);
        let source_qualifiers = Self::top_qualifiers(source);
        if !Self::qualifiers_include(target_qualifiers, source_qualifiers) {
            return false;
        }
        let target_base = Self::unqualified(target);
        let source_base = Self::unqualified(source);
        if Self::compatible_declaration(&target_base, &source_base) {
            return true;
        }
        match (&target_base, &source_base) {
            (Type::Void, source) => !matches!(
                Self::unqualified(source),
                Type::Void | Type::Function(_, _, _, _)
            ),
            (target, Type::Void) => !matches!(
                Self::unqualified(target),
                Type::Void | Type::Function(_, _, _, _)
            ),
            _ => false,
        }
    }

    fn qualifiers_include(target: Qualifiers, source: Qualifiers) -> bool {
        (!source.is_const || target.is_const)
            && (!source.is_volatile || target.is_volatile)
            && (!source.is_restrict || target.is_restrict)
    }

    fn equality_comparable(&self, left: &Type, right: &Type, a: &Expr, b: &Expr) -> bool {
        match (Self::unqualified(left), Self::unqualified(right)) {
            (Type::Pointer(a_type), Type::Pointer(b_type)) => {
                Self::compatible_pointer_targets(&a_type, &b_type)
                    || Self::void_object_pointer_pair(&a_type, &b_type)
            }
            (Type::Pointer(_), _) => self.is_null_pointer_constant(b, right),
            (_, Type::Pointer(_)) => self.is_null_pointer_constant(a, left),
            _ => false,
        }
    }

    fn relationally_comparable(&self, left: &Type, right: &Type) -> bool {
        let (Type::Pointer(left), Type::Pointer(right)) =
            (Self::unqualified(left), Self::unqualified(right))
        else {
            return false;
        };
        !matches!(
            Self::unqualified(&left),
            Type::Void | Type::Function(_, _, _, _)
        ) && !matches!(
            Self::unqualified(&right),
            Type::Void | Type::Function(_, _, _, _)
        ) && Self::compatible_pointer_targets(&left, &right)
    }

    fn compatible_pointer_targets(left: &Type, right: &Type) -> bool {
        Self::compatible_declaration(&Self::unqualified(left), &Self::unqualified(right))
    }

    fn common_pointer_type(left: &Type, right: &Type) -> Option<Type> {
        let (Type::Pointer(left), Type::Pointer(right)) =
            (Self::unqualified(left), Self::unqualified(right))
        else {
            return None;
        };
        let left_qualifiers = Self::top_qualifiers(&left);
        let right_qualifiers = Self::top_qualifiers(&right);
        let qualifiers = Qualifiers {
            is_const: left_qualifiers.is_const || right_qualifiers.is_const,
            is_volatile: left_qualifiers.is_volatile || right_qualifiers.is_volatile,
            is_restrict: left_qualifiers.is_restrict || right_qualifiers.is_restrict,
            is_static: false,
        };
        let left_base = Self::unqualified(&left);
        let right_base = Self::unqualified(&right);
        let base = if Self::compatible_declaration(&left_base, &right_base) {
            Self::composite(&left_base, &right_base)
        } else if Self::void_object_pointer_pair(&left, &right) {
            Type::Void
        } else {
            return None;
        };
        Some(Type::Pointer(Box::new(Self::qualify_type(
            base, qualifiers,
        ))))
    }

    fn void_object_pointer_pair(left: &Type, right: &Type) -> bool {
        let left = Self::unqualified(left);
        let right = Self::unqualified(right);
        (matches!(left, Type::Void) && !matches!(right, Type::Void | Type::Function(_, _, _, _)))
            || (matches!(right, Type::Void)
                && !matches!(left, Type::Void | Type::Function(_, _, _, _)))
    }

    fn pointer_arithmetic_type(&self, ty: &Type) -> bool {
        let element = match Self::unqualified(ty) {
            Type::Pointer(element) | Type::Array(element, _, _) | Type::Vla(element, _) => element,
            _ => return false,
        };
        self.valid_object_type(&element)
    }

    fn pointer_difference_compatible(&self, left: &Type, right: &Type) -> bool {
        let (Some(left), Some(right)) = (Self::pointee_type(left), Self::pointee_type(right))
        else {
            return false;
        };
        self.valid_object_type(&left)
            && self.valid_object_type(&right)
            && Self::compatible_pointer_targets(&left, &right)
    }

    fn pointee_type(ty: &Type) -> Option<Type> {
        match Self::unqualified(ty) {
            Type::Pointer(inner) | Type::Array(inner, _, _) | Type::Vla(inner, _) => Some(*inner),
            _ => None,
        }
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
            for (index, ((expected, got), arg)) in params
                .iter()
                .zip(arg_types.iter())
                .zip(args.iter())
                .enumerate()
            {
                if !self.assignment_compatible(expected, got, arg) {
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
            Expr::IncDec(target, _, _) => self.expr_type(target),
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
                let member_base = if *arrow {
                    match Self::unqualified(&ty) {
                        Type::Pointer(inner) => *inner,
                        _ => Type::Int,
                    }
                } else {
                    ty
                };
                let aggregate = self.resolve_tag(&Self::unqualified(&member_base));
                let qualifiers = Self::top_qualifiers(&member_base);
                match aggregate {
                    Type::Struct(_, fields) | Type::Union(_, fields) => fields
                        .into_iter()
                        .find(|field| field.name.as_deref() == Some(name))
                        .map(|field| Self::qualify_type(field.ty, qualifiers))
                        .unwrap_or(Type::Int),
                    _ => Type::Int,
                }
            }
            Expr::Subscript(base, index) => {
                let base_ty = self.expr_type(base);
                let index_ty = self.expr_type(index);
                let pointer_ty = if Self::pointer_like(&base_ty) {
                    &base_ty
                } else {
                    &index_ty
                };
                Self::pointee_type(pointer_ty).unwrap_or(Type::Int)
            }
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

    fn top_qualifiers(t: &Type) -> Qualifiers {
        let mut ty = t;
        let mut qualifiers = Qualifiers::default();
        while let Type::Qualified(inner, q) = ty {
            qualifiers.is_const |= q.is_const;
            qualifiers.is_volatile |= q.is_volatile;
            qualifiers.is_restrict |= q.is_restrict;
            qualifiers.is_static |= q.is_static;
            ty = inner;
        }
        qualifiers
    }

    fn qualify_type(ty: Type, qualifiers: Qualifiers) -> Type {
        if qualifiers == Qualifiers::default() {
            return ty;
        }
        match ty {
            Type::Array(element, count, is_static) => Type::Array(
                Box::new(Self::qualify_type(*element, qualifiers)),
                count,
                is_static,
            ),
            Type::Vla(element, bound) => {
                Type::Vla(Box::new(Self::qualify_type(*element, qualifiers)), bound)
            }
            Type::Qualified(inner, mut existing) => {
                existing.is_const |= qualifiers.is_const;
                existing.is_volatile |= qualifiers.is_volatile;
                existing.is_restrict |= qualifiers.is_restrict;
                existing.is_static |= qualifiers.is_static;
                Type::Qualified(inner, existing)
            }
            other => Type::Qualified(Box::new(other), qualifiers),
        }
    }

    fn contains_const_member(&self, ty: &Type) -> bool {
        if Self::top_const(ty) {
            return true;
        }
        match ty {
            Type::Qualified(inner, _) | Type::Array(inner, _, _) | Type::Vla(inner, _) => {
                self.contains_const_member(inner)
            }
            _ => match self.resolve_tag(&Self::unqualified(ty)) {
                Type::Struct(_, fields) | Type::Union(_, fields) => fields
                    .iter()
                    .any(|field| self.contains_const_member(&field.ty)),
                _ => false,
            },
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
            (Type::Pointer(x), Type::Pointer(y)) => Self::compatible_declaration(x, y),
            (Type::Pointer(x), Type::Array(y, _, _))
            | (Type::Array(y, _, _), Type::Pointer(x))
            | (Type::Pointer(x), Type::Vla(y, _))
            | (Type::Vla(y, _), Type::Pointer(x)) => Self::compatible_declaration(x, y),
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

    fn integer_type_at_rank(rank: u8, unsigned: bool) -> Type {
        match (rank, unsigned) {
            (4, true) => Type::UnsignedLong,
            (4, false) => Type::Long,
            (5, true) => Type::UnsignedLongLong,
            (5, false) => Type::LongLong,
            (_, true) => Type::UnsignedInt,
            (_, false) => Type::Int,
        }
    }

    fn integer_precision(rank: u8) -> u8 {
        match rank {
            0 | 1 => 8,
            2 => 16,
            4 | 5 => 64,
            _ => 32,
        }
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
            let a_rank = Self::integer_rank(&a);
            let b_rank = Self::integer_rank(&b);
            let rank = a_rank.max(b_rank);
            let a_unsigned = Self::integer_unsigned(&a);
            let b_unsigned = Self::integer_unsigned(&b);
            let unsigned = if a_unsigned == b_unsigned {
                a_unsigned
            } else {
                let (signed_rank, unsigned_rank) = if a_unsigned {
                    (b_rank, a_rank)
                } else {
                    (a_rank, b_rank)
                };
                signed_rank <= unsigned_rank
                    || Self::integer_precision(signed_rank)
                        <= Self::integer_precision(unsigned_rank)
            };
            if rank < 3 {
                Type::Int
            } else {
                Self::integer_type_at_rank(rank, unsigned)
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
