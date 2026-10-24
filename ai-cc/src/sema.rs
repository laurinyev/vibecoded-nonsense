use crate::{ast::*, diag::Diagnostic, symbol::*};
pub struct Sema {
    syms: Scopes,
    errs: Vec<Diagnostic>,
    loop_depth: usize,
    current_ret: Option<Type>,
    complete_tags: std::collections::HashSet<String>,
}
impl Sema {
    pub fn new() -> Self {
        Self {
            syms: Scopes::new(),
            errs: Vec::new(),
            loop_depth: 0,
            current_ret: None,
            complete_tags: Default::default(),
        }
    }
    pub fn analyze(&mut self, ast: &mut TranslationUnit) -> Result<(), Vec<Diagnostic>> {
        for t in &ast.types {
            match t {
                Type::Struct(n, f) | Type::Union(n, f) if !f.is_empty() => {
                    self.complete_tags.insert(n.clone());
                }
                Type::Enum(n, v) if !v.is_empty() => {
                    self.complete_tags.insert(n.clone());
                }
                _ => {}
            }
        }
        for f in &ast.items {
            if f.external {
                continue;
            }
            if let Err(e) = self.syms.insert(Symbol {
                name: f.name.clone(),
                ty: f.ret.clone(),
            }) {
                self.errs.push(Diagnostic::error(e.0, None));
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
                self.add(n, t)
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
    fn add(&mut self, n: &str, t: &Type) {
        if let Err(e) = self.syms.insert(Symbol {
            name: n.to_string(),
            ty: t.clone(),
        }) {
            self.errs.push(Diagnostic::error(e.0, None));
        }
    }
    fn block(&mut self, b: &mut Block) {
        for s in &mut b.stmts {
            self.stmt(s)
        }
    }
    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Decl(n, t, x) => {
                if !self.valid_object_type(t) {
                    self.errs.push(Diagnostic::error(
                        format!("invalid object type for {}", n),
                        None,
                    ));
                }
                self.add(n, t);
                if let Some(e) = x {
                    self.expr(e)
                }
            }
            Stmt::Expr(e) => self.expr(e),
            Stmt::Return(e) => match (self.current_ret.clone(), e) {
                (Some(Type::Void), Some(e)) => {
                    self.expr(e);
                    self.errs.push(Diagnostic::error(
                        "void function cannot return a value",
                        None,
                    ));
                }
                (Some(ret), None) if !matches!(ret, Type::Void) => self.errs.push(
                    Diagnostic::error("non-void function requires return value", None),
                ),
                (_, Some(e)) => self.expr(e),
                _ => {}
            },
            Stmt::Block(b) => {
                self.syms.enter_scope();
                self.block(b);
                self.syms.exit_scope()
            }
            Stmt::If(c, a, b) => {
                self.expr(c);
                self.stmt(a);
                if let Some(x) = b {
                    self.stmt(x)
                }
            }
            Stmt::While(c, b) => {
                self.expr(c);
                self.loop_depth += 1;
                self.stmt(b);
                self.loop_depth -= 1
            }
            Stmt::DoWhile(b, c) => {
                self.loop_depth += 1;
                self.stmt(b);
                self.loop_depth -= 1;
                self.expr(c)
            }
            Stmt::For(i, c, step, b) => {
                self.syms.enter_scope();
                if let Some(i) = i {
                    self.stmt(i);
                }
                if let Some(c) = c {
                    self.expr(c);
                }
                self.loop_depth += 1;
                self.stmt(b);
                self.loop_depth -= 1;
                if let Some(x) = step {
                    self.expr(x);
                }
                self.syms.exit_scope();
            }
            Stmt::Break | Stmt::Continue => {
                if self.loop_depth == 0 {
                    self.errs
                        .push(Diagnostic::error("break or continue outside loop", None));
                }
            }
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
    fn expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Var(n) => {
                if self.syms.lookup(n).is_none() {
                    self.errs
                        .push(Diagnostic::error(format!("unknown identifier {}", n), None))
                }
            }
            Expr::Assign(target, x) => {
                self.expr(target);
                self.expr(x)
            }
            Expr::Binary(_, a, b) => {
                self.expr(a);
                self.expr(b)
            }
            Expr::Unary(_, x) => self.expr(x),
            Expr::Conditional(a, b, c) => {
                self.expr(a);
                self.expr(b);
                self.expr(c);
            }
            Expr::Member(x, _, _) => {
                self.expr(x);
            }
            Expr::Subscript(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Call(_, a) => {
                for x in a {
                    self.expr(x)
                }
            }
            _ => {}
        }
    }
}
