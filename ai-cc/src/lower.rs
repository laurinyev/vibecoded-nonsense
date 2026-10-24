use crate::{ast::*, diag::Diagnostic};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct LoweredProgram {
    pub functions: Vec<LoweredFunction>,
    pub strings: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct LoweredFunction {
    pub name: String,
    pub ret: Type,
    pub params: Vec<(String, Type)>,
    pub body: Vec<Instr>,
}

#[derive(Clone, Debug)]
pub enum Instr {
    Alloc(String, usize),
    Const(String, i64),
    Move(String, String),
    Load(String, String, Type),
    Store(String, String, Type),
    Addr(String, String, i64),
    PtrAdd(String, String, String),
    Bin(String, BinOp, String, String),
    Call(Option<String>, String, Vec<(String, Type)>),
    Ret(Option<String>),
    Label(String),
    Jump(String),
    Branch(String, String, String),
}

pub struct Lowerer {
    n: usize,
    strings: Vec<(String, String)>,
    loops: Vec<(String, String)>,
    vars: HashMap<String, Type>,
    slots: HashMap<String, String>,
    functions: HashMap<String, (Type, Vec<Type>)>,
}

impl Default for Lowerer {
    fn default() -> Self {
        Self::new()
    }
}

impl Lowerer {
    pub fn new() -> Self {
        Self {
            n: 0,
            strings: Vec::new(),
            loops: Vec::new(),
            vars: HashMap::new(),
            slots: HashMap::new(),
            functions: HashMap::new(),
        }
    }

    fn tmp(&mut self) -> String {
        let x = format!("%t{}", self.n);
        self.n += 1;
        x
    }

    fn scalar(t: &Type) -> Type {
        match t {
            Type::Qualified(x, _) => Self::scalar(x),
            _ => t.clone(),
        }
    }

    fn size(t: &Type) -> usize {
        match Self::scalar(t) {
            Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool => 1,
            Type::Array(x, Some(n)) => Self::size(&x) * n,
            Type::Struct(_, fields) | Type::Union(_, fields) => {
                fields.iter().map(|(_, x)| Self::size(x)).sum()
            }
            _ => 8,
        }
    }

    fn is_pointer(t: &Type) -> bool {
        matches!(Self::scalar(t), Type::Pointer(_) | Type::Array(_, _))
    }

    fn pointee(t: &Type) -> Option<Type> {
        match Self::scalar(t) {
            Type::Pointer(x) | Type::Array(x, _) => Some(*x),
            _ => None,
        }
    }

    fn field(t: &Type, name: &str) -> Option<(usize, Type)> {
        let t = Self::scalar(t);
        let fields = match t {
            Type::Struct(_, fields) | Type::Union(_, fields) => fields,
            _ => return None,
        };
        let mut offset = 0;
        for (field, ty) in fields {
            if field == name {
                return Some((offset, ty));
            }
            offset += Self::size(&ty);
        }
        None
    }

    fn type_of(&self, e: &Expr) -> Type {
        match e {
            Expr::Int(_) | Expr::Char(_) | Expr::Binary(_, _, _) | Expr::Conditional(_, _, _) => {
                Type::Int
            }
            Expr::String(_) => Type::Pointer(Box::new(Type::Char)),
            Expr::Var(n) => self.vars.get(n).cloned().unwrap_or(Type::Int),
            Expr::Assign(x, _) => self.type_of(x),
            Expr::Unary(UnOp::Address, x) => Type::Pointer(Box::new(self.type_of(x))),
            Expr::Unary(UnOp::Deref, x) => Self::pointee(&self.type_of(x)).unwrap_or(Type::Int),
            Expr::Unary(_, x) => self.type_of(x),
            Expr::Call(n, _) => self
                .functions
                .get(n)
                .map(|x| x.0.clone())
                .unwrap_or(Type::Int),
            Expr::Member(x, name, arrow) => {
                let base = if *arrow {
                    Self::pointee(&self.type_of(x)).unwrap_or(Type::Int)
                } else {
                    self.type_of(x)
                };
                Self::field(&base, name).map(|x| x.1).unwrap_or(Type::Int)
            }
            Expr::Subscript(x, _) => Self::pointee(&self.type_of(x)).unwrap_or(Type::Int),
        }
    }

    fn lvalue_addr(&mut self, e: &Expr, v: &mut Vec<Instr>) -> String {
        match e {
            Expr::Var(n) => self
                .slots
                .get(n)
                .cloned()
                .unwrap_or_else(|| format!("%{}", n)),
            Expr::Unary(UnOp::Deref, x) => self.expr(x, v),
            Expr::Member(x, name, arrow) => {
                let base_type = if *arrow {
                    Self::pointee(&self.type_of(x)).unwrap_or(Type::Int)
                } else {
                    self.type_of(x)
                };
                let base = if *arrow {
                    self.expr(x, v)
                } else {
                    self.lvalue_addr(x, v)
                };
                let offset = Self::field(&base_type, name).map(|x| x.0).unwrap_or(0) as i64;
                self.add_addr(base, offset, v)
            }
            Expr::Subscript(x, index) => {
                let element = Self::pointee(&self.type_of(x)).unwrap_or(Type::Int);
                let base = if Self::is_pointer(&self.type_of(x)) {
                    if matches!(Self::scalar(&self.type_of(x)), Type::Array(_, _)) {
                        self.lvalue_addr(x, v)
                    } else {
                        self.expr(x, v)
                    }
                } else {
                    self.lvalue_addr(x, v)
                };
                let i = self.expr(index, v);
                let scale = self.tmp();
                v.push(Instr::Const(scale.clone(), Self::size(&element) as i64));
                let offset = self.tmp();
                v.push(Instr::Bin(offset.clone(), BinOp::Mul, i, scale));
                let addr = self.tmp();
                v.push(Instr::PtrAdd(addr.clone(), base, offset));
                addr
            }
            _ => self.expr(e, v),
        }
    }

    fn add_addr(&mut self, base: String, offset: i64, v: &mut Vec<Instr>) -> String {
        if offset == 0 {
            return base;
        }
        let n = self.tmp();
        v.push(Instr::Const(n.clone(), offset));
        let out = self.tmp();
        v.push(Instr::PtrAdd(out.clone(), base, n));
        out
    }

    fn expr(&mut self, e: &Expr, v: &mut Vec<Instr>) -> String {
        match e {
            Expr::Int(n) => {
                let x = self.tmp();
                v.push(Instr::Const(x.clone(), *n));
                x
            }
            Expr::Char(c) => {
                let x = self.tmp();
                v.push(Instr::Const(x.clone(), *c as i64));
                x
            }
            Expr::String(value) => {
                let index = self
                    .strings
                    .iter()
                    .position(|(_, x)| x == value)
                    .unwrap_or_else(|| {
                        let index = self.strings.len();
                        self.strings.push((format!("$str{}", index), value.clone()));
                        index
                    });
                self.strings[index].0.clone()
            }
            Expr::Var(_) | Expr::Member(_, _, _) | Expr::Subscript(_, _) => {
                let ty = self.type_of(e);
                if matches!(Self::scalar(&ty), Type::Array(_, _)) {
                    self.lvalue_addr(e, v)
                } else {
                    let addr = self.lvalue_addr(e, v);
                    let out = self.tmp();
                    v.push(Instr::Load(out.clone(), addr, ty));
                    out
                }
            }
            Expr::Assign(target, e) => {
                let x = self.expr(e, v);
                let addr = self.lvalue_addr(target, v);
                v.push(Instr::Store(x.clone(), addr, self.type_of(target)));
                x
            }
            Expr::Unary(UnOp::Address, e) => self.lvalue_addr(e, v),
            Expr::Unary(UnOp::Deref, e) => {
                let addr = self.expr(e, v);
                let out = self.tmp();
                v.push(Instr::Load(out.clone(), addr, self.type_of(e)));
                out
            }
            Expr::Unary(UnOp::Neg, e) => {
                let x = self.expr(e, v);
                let z = self.tmp();
                v.push(Instr::Bin(z.clone(), BinOp::Sub, "0".into(), x));
                z
            }
            Expr::Unary(UnOp::Not, e) => {
                let x = self.expr(e, v);
                let z = self.tmp();
                v.push(Instr::Bin(z.clone(), BinOp::Eq, x, "0".into()));
                z
            }
            Expr::Unary(UnOp::Pos, e) => self.expr(e, v),
            Expr::Unary(UnOp::BitNot, e) => {
                let x = self.expr(e, v);
                let z = self.tmp();
                v.push(Instr::Bin(z.clone(), BinOp::BitXor, x, "-1".into()));
                z
            }
            Expr::Binary(op, a, b) => {
                let x = self.expr(a, v);
                if matches!(op, BinOp::And | BinOp::Or) {
                    let i = self.n;
                    self.n += 1;
                    let rhs = format!("@logicrhs{}", i);
                    let short = format!("@logicshort{}", i);
                    let end = format!("@logicend{}", i);
                    let z = self.tmp();
                    if matches!(op, BinOp::And) {
                        v.push(Instr::Branch(x, rhs.clone(), short.clone()));
                        v.push(Instr::Label(short));
                        v.push(Instr::Const(z.clone(), 0));
                        v.push(Instr::Jump(end.clone()));
                    } else {
                        v.push(Instr::Branch(x, short.clone(), rhs.clone()));
                        v.push(Instr::Label(short));
                        v.push(Instr::Const(z.clone(), 1));
                        v.push(Instr::Jump(end.clone()));
                    }
                    v.push(Instr::Label(rhs));
                    let y = self.expr(b, v);
                    v.push(Instr::Move(z.clone(), y));
                    v.push(Instr::Jump(end.clone()));
                    v.push(Instr::Label(end));
                    return z;
                }
                let y = self.expr(b, v);
                if matches!(op, BinOp::Comma) {
                    y
                } else {
                    let z = self.tmp();
                    v.push(Instr::Bin(z.clone(), *op, x, y));
                    z
                }
            }
            Expr::Conditional(c, a, b) => {
                let c = self.expr(c, v);
                let i = self.n;
                self.n += 1;
                let t = format!("@condt{}", i);
                let f = format!("@condf{}", i);
                let e = format!("@conde{}", i);
                let z = self.tmp();
                v.push(Instr::Branch(c, t.clone(), f.clone()));
                v.push(Instr::Label(t));
                let x = self.expr(a, v);
                v.push(Instr::Move(z.clone(), x));
                v.push(Instr::Jump(e.clone()));
                v.push(Instr::Label(f));
                let x = self.expr(b, v);
                v.push(Instr::Move(z.clone(), x));
                v.push(Instr::Jump(e.clone()));
                v.push(Instr::Label(e));
                z
            }
            Expr::Call(n, args) => {
                let (ret, params) = self
                    .functions
                    .get(n)
                    .cloned()
                    .unwrap_or((Type::Int, Vec::new()));
                let mut values = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    let value = self.expr(arg, v);
                    let ty = params.get(i).cloned().unwrap_or_else(|| self.type_of(arg));
                    values.push((value, ty));
                }
                let dst = if matches!(Self::scalar(&ret), Type::Void) {
                    None
                } else {
                    Some(self.tmp())
                };
                v.push(Instr::Call(dst.clone(), n.clone(), values));
                dst.unwrap_or_else(|| "0".into())
            }
        }
    }

    fn stmt(&mut self, s: &Stmt, v: &mut Vec<Instr>) {
        match s {
            Stmt::Decl(n, ty, init, _) => {
                let slot = format!("%slot_{}", n);
                let size = Self::size(ty).max(1);
                self.vars.insert(n.clone(), ty.clone());
                self.slots.insert(n.clone(), slot.clone());
                v.push(Instr::Alloc(slot, size));
                if let Some(e) = init {
                    let x = self.expr(e, v);
                    let addr = self.lvalue_addr(&Expr::Var(n.clone()), v);
                    v.push(Instr::Store(x, addr, ty.clone()));
                }
            }
            Stmt::Decls(declarations, _) => {
                for (n, ty, init) in declarations {
                    let slot = format!("%slot_{}", n);
                    let size = Self::size(ty).max(1);
                    self.vars.insert(n.clone(), ty.clone());
                    self.slots.insert(n.clone(), slot.clone());
                    v.push(Instr::Alloc(slot, size));
                    if let Some(e) = init {
                        let x = self.expr(e, v);
                        let addr = self.lvalue_addr(&Expr::Var(n.clone()), v);
                        v.push(Instr::Store(x, addr, ty.clone()));
                    }
                }
            }
            Stmt::Expr(e, _) => {
                self.expr(e, v);
            }
            Stmt::Return(e, _) => {
                let value = e.as_ref().map(|x| self.expr(x, v));
                v.push(Instr::Ret(value));
            }
            Stmt::Block(b, _) => {
                for x in &b.stmts {
                    self.stmt(x, v)
                }
            }
            Stmt::If(c, a, b, _) => {
                let x = self.expr(c, v);
                let i = self.n;
                self.n += 1;
                let t = format!("@then{}", i);
                let f = format!("@else{}", i);
                let e = format!("@end{}", i);
                v.push(Instr::Branch(x, t.clone(), f.clone()));
                v.push(Instr::Label(t));
                self.stmt(a, v);
                v.push(Instr::Jump(e.clone()));
                v.push(Instr::Label(f));
                if let Some(x) = b {
                    self.stmt(x, v)
                }
                v.push(Instr::Jump(e.clone()));
                v.push(Instr::Label(e));
            }
            Stmt::While(c, b, _) => {
                let i = self.n;
                self.n += 1;
                let h = format!("@while{}", i);
                let x = format!("@body{}", i);
                let e = format!("@done{}", i);
                v.push(Instr::Label(h.clone()));
                let y = self.expr(c, v);
                v.push(Instr::Branch(y, x.clone(), e.clone()));
                v.push(Instr::Label(x));
                self.loops.push((e.clone(), h.clone()));
                self.stmt(b, v);
                self.loops.pop();
                v.push(Instr::Jump(h));
                v.push(Instr::Label(e));
            }
            Stmt::DoWhile(b, c, _) => {
                let i = self.n;
                self.n += 1;
                let h = format!("@do{}", i);
                let e = format!("@done{}", i);
                self.loops.push((e.clone(), h.clone()));
                v.push(Instr::Label(h.clone()));
                self.stmt(b, v);
                self.loops.pop();
                let y = self.expr(c, v);
                v.push(Instr::Branch(y, h, e.clone()));
                v.push(Instr::Label(e));
            }
            Stmt::For(init, c, step, b, _) => {
                if let Some(x) = init {
                    self.stmt(x, v);
                }
                let i = self.n;
                self.n += 1;
                let h = format!("@for{}", i);
                let x = format!("@forbody{}", i);
                let e = format!("@fordone{}", i);
                let n = format!("@forstep{}", i);
                v.push(Instr::Label(h.clone()));
                if let Some(c) = c {
                    let y = self.expr(c, v);
                    v.push(Instr::Branch(y, x.clone(), e.clone()));
                } else {
                    v.push(Instr::Jump(x.clone()));
                }
                v.push(Instr::Label(x));
                self.loops.push((e.clone(), n.clone()));
                self.stmt(b, v);
                self.loops.pop();
                v.push(Instr::Label(n.clone()));
                if let Some(s) = step {
                    self.expr(s, v);
                }
                v.push(Instr::Jump(h));
                v.push(Instr::Label(e));
            }
            Stmt::Break(_) => {
                if let Some((x, _)) = self.loops.last() {
                    v.push(Instr::Jump(x.clone()));
                }
            }
            Stmt::Continue(_) => {
                if let Some((_, x)) = self.loops.last() {
                    v.push(Instr::Jump(x.clone()));
                }
            }
        }
    }

    pub fn lower_translation_unit(
        &mut self,
        ast: TranslationUnit,
    ) -> Result<LoweredProgram, Vec<Diagnostic>> {
        for f in &ast.items {
            self.functions.insert(
                f.name.clone(),
                (
                    f.ret.clone(),
                    f.params.iter().map(|x| x.1.clone()).collect(),
                ),
            );
        }
        let mut fs = Vec::new();
        for f in ast.items {
            if f.external {
                continue;
            }
            self.n = 0;
            self.vars.clear();
            self.slots.clear();
            self.loops.clear();
            let mut b = Vec::new();
            for (n, ty) in &f.params {
                let slot = format!("%slot_{}", n);
                self.vars.insert(n.clone(), ty.clone());
                self.slots.insert(n.clone(), slot.clone());
                b.push(Instr::Alloc(slot.clone(), Self::size(ty).max(8)));
                b.push(Instr::Store(format!("%{}", n), slot, ty.clone()));
            }
            for s in &f.body.stmts {
                self.stmt(s, &mut b)
            }
            if !matches!(b.last(), Some(Instr::Ret(_))) {
                b.push(Instr::Ret(if matches!(Self::scalar(&f.ret), Type::Void) {
                    None
                } else {
                    Some("0".into())
                }))
            }
            fs.push(LoweredFunction {
                name: f.name,
                ret: f.ret,
                params: f.params,
                body: b,
            })
        }
        Ok(LoweredProgram {
            functions: fs,
            strings: self.strings.clone(),
        })
    }
}
