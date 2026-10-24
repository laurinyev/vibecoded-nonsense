use crate::{ast::*, diag::Diagnostic};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct LoweredProgram {
    pub functions: Vec<LoweredFunction>,
    pub strings: Vec<(String, String)>,
    pub statics: Vec<(String, Type, i64)>,
    pub globals: Vec<(String, Type, DataValue, bool)>,
    pub aggregate_data: Vec<DataObject>,
}

#[derive(Clone, Debug)]
pub enum DataValue {
    Integer(i64),
    Address(String),
}

#[derive(Clone, Debug)]
pub struct DataObject {
    pub name: String,
    pub ty: Type,
    pub values: Vec<(Type, DataValue)>,
    pub internal: bool,
}

#[derive(Clone, Debug)]
pub struct LoweredFunction {
    pub name: String,
    pub ret: Type,
    pub params: Vec<(String, Type)>,
    pub is_static: bool,
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
    statics: Vec<(String, Type, i64)>,
    globals: Vec<(String, Type, DataValue, bool)>,
    aggregate_data: Vec<DataObject>,
    loops: Vec<(String, String)>,
    vars: HashMap<String, Type>,
    slots: HashMap<String, String>,
    functions: HashMap<String, (Type, Vec<Type>)>,
    tags: HashMap<String, Type>,
    function_name: String,
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
            statics: Vec::new(),
            globals: Vec::new(),
            aggregate_data: Vec::new(),
            loops: Vec::new(),
            vars: HashMap::new(),
            slots: HashMap::new(),
            functions: HashMap::new(),
            tags: HashMap::new(),
            function_name: String::new(),
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

    fn resolve_tags(&self, ty: &Type) -> Type {
        match ty {
            Type::Struct(name, fields) if fields.is_empty() => self
                .tags
                .get(&format!("struct:{name}"))
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            Type::Union(name, fields) if fields.is_empty() => self
                .tags
                .get(&format!("union:{name}"))
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            Type::Enum(name, values) if values.is_empty() => self
                .tags
                .get(&format!("enum:{name}"))
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            Type::Pointer(inner) => Type::Pointer(Box::new(self.resolve_tags(inner))),
            Type::Array(inner, count) => Type::Array(Box::new(self.resolve_tags(inner)), *count),
            Type::Qualified(inner, qualifiers) => {
                Type::Qualified(Box::new(self.resolve_tags(inner)), *qualifiers)
            }
            _ => ty.clone(),
        }
    }

    fn size(t: &Type) -> usize {
        match Self::scalar(t) {
            Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool => 1,
            Type::Short | Type::UnsignedShort => 2,
            Type::Int | Type::SignedInt | Type::UnsignedInt | Type::Float => 4,
            Type::Long
            | Type::UnsignedLong
            | Type::LongLong
            | Type::UnsignedLongLong
            | Type::Double
            | Type::Pointer(_) => 8,
            Type::LongDouble => 16,
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
            Expr::Cast(ty, _) => ty.clone(),
            Expr::SizeofType(_) | Expr::Unary(UnOp::Sizeof, _) => Type::UnsignedLong,
            Expr::CompoundLiteral(ty, _) => ty.clone(),
            Expr::String(_) => Type::Pointer(Box::new(Type::Char)),
            Expr::Var(n) => self
                .vars
                .get(n)
                .map(|ty| self.resolve_tags(ty))
                .unwrap_or(Type::Int),
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
            Expr::CompoundLiteral(ty, values) => self.compound_literal_addr(ty, values, v),
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

    fn compound_literal_addr(
        &mut self,
        ty: &Type,
        initializer: &Initializer,
        v: &mut Vec<Instr>,
    ) -> String {
        let addr = self.tmp();
        v.push(Instr::Alloc(addr.clone(), Self::size(ty).max(1)));
        self.zero_object(ty, &addr, v);
        self.initialize_object(ty, initializer, addr.clone(), v);
        addr
    }

    fn initialize_object(
        &mut self,
        ty: &Type,
        initializer: &Initializer,
        addr: String,
        v: &mut Vec<Instr>,
    ) {
        match initializer {
            Initializer::Expr(expr) => {
                let value = self.expr(expr, v);
                v.push(Instr::Store(value, addr, ty.clone()));
            }
            Initializer::List(items) => match Self::scalar(ty) {
                Type::Qualified(inner, _) => self.initialize_object(&inner, initializer, addr, v),
                Type::Array(element, _) => {
                    let mut next = 0usize;
                    for item in items {
                        let index = match item.designators.first() {
                            Some(Designator::Index(expr)) => {
                                Self::constant_value(expr).unwrap_or(next as i64) as usize
                            }
                            _ => next,
                        };
                        let target =
                            self.add_addr(addr.clone(), (index * Self::size(&element)) as i64, v);
                        if item.designators.len() > 1 {
                            self.initialize_designated(
                                &element,
                                &item.designators[1..],
                                &item.value,
                                target,
                                v,
                            );
                        } else {
                            self.initialize_object(&element, &item.value, target, v);
                        }
                        next = index + 1;
                    }
                }
                Type::Struct(_, fields) => {
                    let mut next = 0usize;
                    for item in items {
                        let index = match item.designators.first() {
                            Some(Designator::Member(name)) => fields
                                .iter()
                                .position(|(field, _)| field == name)
                                .unwrap_or(next),
                            _ => next,
                        };
                        if let Some((_, field)) = fields.get(index) {
                            let offset = fields
                                .iter()
                                .take(index)
                                .map(|(_, field)| Self::size(field))
                                .sum::<usize>();
                            let target = self.add_addr(addr.clone(), offset as i64, v);
                            if item.designators.len() > 1 {
                                self.initialize_designated(
                                    field,
                                    &item.designators[1..],
                                    &item.value,
                                    target,
                                    v,
                                );
                            } else {
                                self.initialize_object(field, &item.value, target, v);
                            }
                        }
                        next = index + 1;
                    }
                }
                Type::Union(_, fields) => {
                    for item in items {
                        let index = match item.designators.first() {
                            Some(Designator::Member(name)) => fields
                                .iter()
                                .position(|(field, _)| field == name)
                                .unwrap_or(0),
                            _ => 0,
                        };
                        if let Some((_, field)) = fields.get(index) {
                            if item.designators.len() > 1 {
                                self.initialize_designated(
                                    field,
                                    &item.designators[1..],
                                    &item.value,
                                    addr.clone(),
                                    v,
                                );
                            } else {
                                self.initialize_object(field, &item.value, addr.clone(), v);
                            }
                        }
                    }
                }
                _ => {
                    if let Some(item) = items.first() {
                        self.initialize_object(ty, &item.value, addr, v);
                    }
                }
            },
        }
    }

    fn initialize_designated(
        &mut self,
        ty: &Type,
        designators: &[Designator],
        value: &Initializer,
        addr: String,
        v: &mut Vec<Instr>,
    ) {
        if designators.is_empty() {
            self.initialize_object(ty, value, addr, v);
            return;
        }
        match (ty, &designators[0]) {
            (Type::Qualified(inner, _), _) => {
                self.initialize_designated(inner, designators, value, addr, v)
            }
            (Type::Array(element, _), Designator::Index(index)) => {
                let index = Self::constant_value(index).unwrap_or(0) as usize;
                let target = self.add_addr(addr, (index * Self::size(element)) as i64, v);
                self.initialize_designated(element, &designators[1..], value, target, v);
            }
            (Type::Struct(_, fields) | Type::Union(_, fields), Designator::Member(name)) => {
                if let Some((index, (_, field))) = fields
                    .iter()
                    .enumerate()
                    .find(|(_, (field, _))| field == name)
                {
                    let offset = if matches!(ty, Type::Union(_, _)) {
                        0
                    } else {
                        fields
                            .iter()
                            .take(index)
                            .map(|(_, field)| Self::size(field))
                            .sum::<usize>()
                    };
                    let target = self.add_addr(addr, offset as i64, v);
                    self.initialize_designated(field, &designators[1..], value, target, v);
                }
            }
            _ => {}
        }
    }

    fn zero_object(&mut self, ty: &Type, addr: &str, v: &mut Vec<Instr>) {
        match Self::scalar(ty) {
            Type::Array(ref element, Some(count)) => {
                for i in 0..count {
                    let target =
                        self.add_addr(addr.to_owned(), (i * Self::size(element)) as i64, v);
                    self.zero_object(element, &target, v);
                }
            }
            Type::Struct(_, ref fields) => {
                let mut offset = 0;
                for (_, field) in fields {
                    let target = self.add_addr(addr.to_owned(), offset as i64, v);
                    self.zero_object(field, &target, v);
                    offset += Self::size(field);
                }
            }
            Type::Union(_, ref fields) => {
                if let Some((_, field)) = fields.first() {
                    self.zero_object(field, addr, v);
                }
            }
            _ => {
                let zero = self.tmp();
                v.push(Instr::Const(zero.clone(), 0));
                v.push(Instr::Store(zero, addr.to_owned(), ty.clone()));
            }
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
            Expr::Cast(_, value) => self.expr(value, v),
            Expr::CompoundLiteral(ty, values) => {
                let addr = self.compound_literal_addr(ty, values, v);
                if matches!(Self::scalar(ty), Type::Array(_, _)) {
                    addr
                } else {
                    let out = self.tmp();
                    v.push(Instr::Load(out.clone(), addr, ty.clone()));
                    out
                }
            }
            Expr::SizeofType(ty) => {
                let x = self.tmp();
                v.push(Instr::Const(x.clone(), Self::size(ty) as i64));
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
            Expr::Unary(UnOp::Sizeof, e) => {
                let x = self.tmp();
                v.push(Instr::Const(x.clone(), Self::size(&self.type_of(e)) as i64));
                x
            }
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
            Stmt::StorageDecl(storage, declarations, _) => {
                for (name, ty, init) in declarations {
                    self.vars.insert(name.clone(), ty.clone());
                    match storage {
                        StorageClass::Static => {
                            let symbol =
                                format!("{}_{}_{}", self.function_name, name, self.statics.len());
                            let value = init
                                .as_ref()
                                .and_then(|e| Self::constant_value(e).ok())
                                .unwrap_or(0);
                            self.statics.push((symbol.clone(), ty.clone(), value));
                            self.slots
                                .insert(name.clone(), format!("$static_{}", symbol));
                        }
                        StorageClass::Extern => {
                            self.slots.insert(name.clone(), format!("${name}"));
                        }
                        StorageClass::Auto | StorageClass::Register => {
                            let slot = format!("%slot_{}", name);
                            self.slots.insert(name.clone(), slot.clone());
                            v.push(Instr::Alloc(slot, Self::size(ty).max(1)));
                            if let Some(e) = init {
                                let x = self.expr(e, v);
                                let addr = self.lvalue_addr(&Expr::Var(name.clone()), v);
                                v.push(Instr::Store(x, addr, ty.clone()));
                            }
                        }
                    }
                }
            }
            Stmt::Typedef(_, _, _) => {}
            Stmt::Typedefs(_, _) => {}
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
        for ty in &ast.types {
            match ty {
                Type::Struct(name, fields) if !fields.is_empty() => {
                    self.tags.insert(format!("struct:{name}"), ty.clone());
                }
                Type::Union(name, fields) if !fields.is_empty() => {
                    self.tags.insert(format!("union:{name}"), ty.clone());
                }
                Type::Enum(name, values) if !values.is_empty() => {
                    self.tags.insert(format!("enum:{name}"), ty.clone());
                }
                _ => {}
            }
        }
        for declaration in &ast.declarations {
            let declaration_ty = self.resolve_tags(&declaration.ty);
            if declaration.tentative_definition || declaration.initializer.is_some() {
                let internal = declaration.storage == Some(StorageClass::Static);
                if Self::aggregate(&declaration_ty) {
                    let mut values = Self::zero_data(&declaration_ty);
                    if let Some(Expr::CompoundLiteral(ty, initializer)) = &declaration.initializer {
                        self.data_initialize(ty, initializer, &mut values, 0)
                            .map_err(|_| {
                                vec![Diagnostic::error(
                                    format!(
                                        "unsupported file-scope initializer for {}",
                                        declaration.name
                                    ),
                                    Some(declaration.span),
                                )]
                            })?;
                    } else if declaration.initializer.is_some() {
                        return Err(vec![Diagnostic::error(
                            format!(
                                "unsupported file-scope initializer for {}",
                                declaration.name
                            ),
                            Some(declaration.span),
                        )]);
                    }
                    self.aggregate_data.push(DataObject {
                        name: declaration.name.clone(),
                        ty: declaration_ty.clone(),
                        values,
                        internal,
                    });
                    continue;
                }
                let value = match &declaration.initializer {
                    Some(expr) => self.global_value(expr).map_err(|_| {
                        vec![Diagnostic::error(
                            format!(
                                "unsupported file-scope initializer for {}",
                                declaration.name
                            ),
                            Some(declaration.span),
                        )]
                    })?,
                    None => DataValue::Integer(0),
                };
                if let Some(existing) = self
                    .globals
                    .iter_mut()
                    .find(|global| global.0 == declaration.name)
                {
                    if declaration.initializer.is_some() {
                        existing.1 = declaration_ty.clone();
                        existing.2 = value;
                        existing.3 = internal;
                    }
                } else {
                    self.globals
                        .push((declaration.name.clone(), declaration_ty, value, internal));
                }
            }
        }
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
            self.function_name = f.name.clone();
            self.vars.clear();
            self.slots.clear();
            self.loops.clear();
            for declaration in &ast.declarations {
                self.vars
                    .insert(declaration.name.clone(), declaration.ty.clone());
                self.slots
                    .insert(declaration.name.clone(), format!("${}", declaration.name));
            }
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
                is_static: f.is_static,
                body: b,
            })
        }
        Ok(LoweredProgram {
            functions: fs,
            strings: self.strings.clone(),
            statics: self.statics.clone(),
            globals: self.globals.clone(),
            aggregate_data: self.aggregate_data.clone(),
        })
    }

    fn aggregate(ty: &Type) -> bool {
        matches!(
            Self::scalar(ty),
            Type::Array(_, _) | Type::Struct(_, _) | Type::Union(_, _)
        )
    }

    fn zero_data(ty: &Type) -> Vec<(Type, DataValue)> {
        let mut values = Vec::new();
        Self::data_leaves(ty, &mut values);
        values
    }

    fn data_leaves(ty: &Type, values: &mut Vec<(Type, DataValue)>) {
        match Self::scalar(ty) {
            Type::Array(element, Some(count)) => {
                for _ in 0..count {
                    Self::data_leaves(&element, values);
                }
            }
            Type::Struct(_, fields) => {
                for (_, field) in fields {
                    Self::data_leaves(&field, values);
                }
            }
            Type::Union(_, fields) => {
                if let Some((_, field)) = fields.first() {
                    Self::data_leaves(field, values);
                }
            }
            scalar => values.push((scalar, DataValue::Integer(0))),
        }
    }

    fn data_initialize(
        &mut self,
        ty: &Type,
        initializer: &Initializer,
        values: &mut [(Type, DataValue)],
        start: usize,
    ) -> Result<(), ()> {
        match initializer {
            Initializer::Expr(expr) => {
                if Self::aggregate(ty) {
                    return Err(());
                }
                values[start].1 = self.global_value(expr)?;
            }
            Initializer::List(items) => match Self::scalar(ty) {
                Type::Array(element, _) => {
                    let stride = Self::zero_data(&element).len();
                    let mut next = 0usize;
                    for item in items {
                        let index = match item.designators.first() {
                            Some(Designator::Index(index)) => Self::constant_value(index)? as usize,
                            _ => next,
                        };
                        if item.designators.len() > 1 {
                            self.data_initialize_designated(
                                &element,
                                &item.designators[1..],
                                &item.value,
                                values,
                                start + index * stride,
                            )?;
                        } else {
                            self.data_initialize(
                                &element,
                                &item.value,
                                values,
                                start + index * stride,
                            )?;
                        }
                        next = index + 1;
                    }
                }
                Type::Struct(_, fields) => {
                    let mut next = 0usize;
                    for item in items {
                        let index = match item.designators.first() {
                            Some(Designator::Member(name)) => fields
                                .iter()
                                .position(|(field, _)| field == name)
                                .ok_or(())?,
                            _ => next,
                        };
                        let offset: usize = fields
                            .iter()
                            .take(index)
                            .map(|(_, field)| Self::zero_data(field).len())
                            .sum();
                        let field = &fields[index].1;
                        if item.designators.len() > 1 {
                            self.data_initialize_designated(
                                field,
                                &item.designators[1..],
                                &item.value,
                                values,
                                start + offset,
                            )?;
                        } else {
                            self.data_initialize(field, &item.value, values, start + offset)?;
                        }
                        next = index + 1;
                    }
                }
                Type::Union(_, fields) => {
                    if let Some((_, field)) = fields.first() {
                        if let Some(item) = items.first() {
                            self.data_initialize(field, &item.value, values, start)?;
                        }
                    }
                }
                _ => {
                    if let Some(item) = items.first() {
                        self.data_initialize(ty, &item.value, values, start)?;
                    }
                }
            },
        }
        Ok(())
    }

    fn data_initialize_designated(
        &mut self,
        ty: &Type,
        designators: &[Designator],
        value: &Initializer,
        values: &mut [(Type, DataValue)],
        start: usize,
    ) -> Result<(), ()> {
        if designators.is_empty() {
            return self.data_initialize(ty, value, values, start);
        }
        match (Self::scalar(ty), &designators[0]) {
            (Type::Array(element, _), Designator::Index(index)) => self.data_initialize_designated(
                &element,
                &designators[1..],
                value,
                values,
                start + Self::constant_value(index)? as usize * Self::zero_data(&element).len(),
            ),
            (Type::Struct(_, fields), Designator::Member(name)) => {
                let index = fields
                    .iter()
                    .position(|(field, _)| field == name)
                    .ok_or(())?;
                let offset: usize = fields
                    .iter()
                    .take(index)
                    .map(|(_, field)| Self::zero_data(field).len())
                    .sum();
                self.data_initialize_designated(
                    &fields[index].1,
                    &designators[1..],
                    value,
                    values,
                    start + offset,
                )
            }
            (Type::Union(_, fields), Designator::Member(name)) => {
                let field = fields.iter().find(|(field, _)| field == name).ok_or(())?;
                self.data_initialize_designated(&field.1, &designators[1..], value, values, start)
            }
            _ => Err(()),
        }
    }

    fn global_value(&mut self, expr: &Expr) -> Result<DataValue, ()> {
        match expr {
            Expr::Unary(UnOp::Address, value) => match &**value {
                Expr::Var(name) => Ok(DataValue::Address(name.clone())),
                Expr::CompoundLiteral(ty, initializer) => {
                    let name = format!("compound_literal_{}", self.aggregate_data.len());
                    let mut values = Self::zero_data(ty);
                    self.data_initialize(ty, initializer, &mut values, 0)?;
                    self.aggregate_data.push(DataObject {
                        name: name.clone(),
                        ty: ty.clone(),
                        values,
                        internal: true,
                    });
                    Ok(DataValue::Address(name))
                }
                _ => Err(()),
            },
            Expr::CompoundLiteral(ty, initializer) if Self::aggregate(ty) => {
                let name = format!("compound_literal_{}", self.aggregate_data.len());
                let mut values = Self::zero_data(ty);
                self.data_initialize(ty, initializer, &mut values, 0)?;
                self.aggregate_data.push(DataObject {
                    name: name.clone(),
                    ty: ty.clone(),
                    values,
                    internal: true,
                });
                Ok(DataValue::Address(name))
            }
            _ => Ok(DataValue::Integer(Self::constant_value(expr)?)),
        }
    }

    fn constant_value(expr: &Expr) -> Result<i64, ()> {
        match expr {
            Expr::Int(n) => Ok(*n),
            Expr::Char(c) => Ok(*c as i64),
            Expr::Cast(_, x) | Expr::Unary(UnOp::Pos, x) => Self::constant_value(x),
            Expr::Unary(UnOp::Not, x) => Ok((Self::constant_value(x)? == 0) as i64),
            Expr::Unary(UnOp::Neg, x) => Self::constant_value(x)?.checked_neg().ok_or(()),
            Expr::Unary(UnOp::BitNot, x) => Ok(!Self::constant_value(x)?),
            Expr::Binary(op, a, b) => {
                let (x, y) = (Self::constant_value(a)?, Self::constant_value(b)?);
                match op {
                    BinOp::Add => x.checked_add(y).ok_or(()),
                    BinOp::Sub => x.checked_sub(y).ok_or(()),
                    BinOp::Mul => x.checked_mul(y).ok_or(()),
                    BinOp::Div => x.checked_div(y).ok_or(()),
                    BinOp::Rem => x.checked_rem(y).ok_or(()),
                    BinOp::BitOr => Ok(x | y),
                    BinOp::BitXor => Ok(x ^ y),
                    BinOp::BitAnd => Ok(x & y),
                    BinOp::Shl => x.checked_shl(y as u32).ok_or(()),
                    BinOp::Shr => x.checked_shr(y as u32).ok_or(()),
                    BinOp::Eq => Ok((x == y) as i64),
                    BinOp::Ne => Ok((x != y) as i64),
                    BinOp::Lt => Ok((x < y) as i64),
                    BinOp::Le => Ok((x <= y) as i64),
                    BinOp::Gt => Ok((x > y) as i64),
                    BinOp::Ge => Ok((x >= y) as i64),
                    BinOp::And => Ok((x != 0 && y != 0) as i64),
                    BinOp::Or => Ok((x != 0 || y != 0) as i64),
                    _ => Err(()),
                }
            }
            Expr::Conditional(condition, yes, no) => {
                if Self::constant_value(condition)? != 0 {
                    Self::constant_value(yes)
                } else {
                    Self::constant_value(no)
                }
            }
            _ => Err(()),
        }
    }
}
