use crate::{ast::*, diag::Diagnostic};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct LoweredProgram {
    pub functions: Vec<LoweredFunction>,
    pub strings: Vec<(String, String)>,
    pub statics: Vec<(String, Type, DataValue)>,
    pub globals: Vec<(String, Type, DataValue, bool)>,
    pub aggregate_data: Vec<DataObject>,
}

#[derive(Clone, Debug)]
pub enum DataValue {
    Integer(i64),
    Float(f64, Type),
    Address(String),
}

#[derive(Clone, Debug)]
pub struct DataObject {
    pub name: String,
    pub ty: Type,
    pub bytes: Vec<u8>,
    pub relocations: Vec<(usize, String)>,
    pub internal: bool,
}

#[derive(Clone, Debug)]
pub struct LoweredFunction {
    pub name: String,
    pub ret: Type,
    pub params: Vec<(String, Type)>,
    pub is_static: bool,
    pub is_variadic: bool,
    pub body: Vec<Instr>,
}

#[derive(Clone, Debug)]
pub enum Instr {
    Alloc(String, usize),
    AllocVla(String, String, usize),
    Const(String, i64),
    ConstLong(String, i64),
    FuncAddr(String, String),
    ConstFloat(String, f64, Type),
    Move(String, String, Type),
    Load(String, String, Type),
    LoadZero(String, String, usize),
    Store(String, String, Type),
    StoreSize(String, String, usize),
    Addr(String, String, i64),
    PtrAdd(String, String, String),
    Bin(String, BinOp, String, String),
    BinTyped(String, BinOp, String, String, Type),
    FloatBin(String, BinOp, String, String, Type),
    Convert(String, String, Type, Type),
    Call(
        Option<String>,
        String,
        Vec<(String, Type)>,
        Option<usize>,
        Type,
    ),
    CallIndirect(
        Option<String>,
        String,
        Vec<(String, Type)>,
        Option<usize>,
        Type,
    ),
    VaStart(String),
    VaArg(String, String, Type),
    Ret(Option<String>),
    Label(String),
    Jump(String),
    Branch(String, String, String),
}

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

pub struct Lowerer {
    n: usize,
    strings: Vec<(String, String)>,
    statics: Vec<(String, Type, DataValue)>,
    globals: Vec<(String, Type, DataValue, bool)>,
    aggregate_data: Vec<DataObject>,
    loops: Vec<(String, String)>,
    local_id: usize,
    vars: HashMap<String, Type>,
    slots: HashMap<String, String>,
    functions: HashMap<String, (Type, Vec<Type>, bool)>,
    tags: HashMap<String, Type>,
    function_name: String,
    return_type: Type,
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
            local_id: 0,
            vars: HashMap::new(),
            slots: HashMap::new(),
            functions: HashMap::new(),
            tags: HashMap::new(),
            function_name: String::new(),
            return_type: Type::Int,
        }
    }

    fn tmp(&mut self) -> String {
        let x = format!("%t{}", self.n);
        self.n += 1;
        x
    }

    fn intern_string(&mut self, value: &str) -> String {
        if let Some((name, _)) = self.strings.iter().find(|(_, text)| text == value) {
            return name.clone();
        }
        let name = format!("$str{}", self.strings.len());
        self.strings.push((name.clone(), value.to_string()));
        name
    }

    fn scalar(t: &Type) -> Type {
        match t {
            Type::Qualified(x, _) => Self::scalar(x),
            _ => t.clone(),
        }
    }

    fn resolve_tags(&self, ty: &Type) -> Type {
        self.resolve_tags_inner(ty, &mut HashSet::new())
    }

    fn resolve_tags_inner(&self, ty: &Type, seen: &mut HashSet<String>) -> Type {
        match ty {
            Type::Struct(name, fields) if fields.is_empty() => {
                let key = format!("struct:{name}");
                if !seen.insert(key.clone()) {
                    return ty.clone();
                }
                let resolved = self
                    .tags
                    .get(&key)
                    .map(|tag| {
                        if let Type::Struct(name, fields) = tag {
                            Type::Struct(
                                name.clone(),
                                fields
                                    .iter()
                                    .map(|field| Field {
                                        name: field.name.clone(),
                                        ty: self.resolve_tags_inner(&field.ty, seen),
                                        bit_width: field.bit_width,
                                    })
                                    .collect(),
                            )
                        } else {
                            tag.clone()
                        }
                    })
                    .unwrap_or_else(|| ty.clone());
                seen.remove(&key);
                resolved
            }
            Type::Union(name, fields) if fields.is_empty() => {
                let key = format!("union:{name}");
                if !seen.insert(key.clone()) {
                    return ty.clone();
                }
                let resolved = self
                    .tags
                    .get(&key)
                    .map(|tag| {
                        if let Type::Union(name, fields) = tag {
                            Type::Union(
                                name.clone(),
                                fields
                                    .iter()
                                    .map(|field| Field {
                                        name: field.name.clone(),
                                        ty: self.resolve_tags_inner(&field.ty, seen),
                                        bit_width: field.bit_width,
                                    })
                                    .collect(),
                            )
                        } else {
                            tag.clone()
                        }
                    })
                    .unwrap_or_else(|| ty.clone());
                seen.remove(&key);
                resolved
            }
            Type::Enum(name, values) if values.is_empty() => self
                .tags
                .get(&format!("enum:{name}"))
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            Type::Struct(name, fields) => Type::Struct(
                name.clone(),
                fields
                    .iter()
                    .map(|field| Field {
                        name: field.name.clone(),
                        ty: self.resolve_tags_inner(&field.ty, seen),
                        bit_width: field.bit_width,
                    })
                    .collect(),
            ),
            Type::Union(name, fields) => Type::Union(
                name.clone(),
                fields
                    .iter()
                    .map(|field| Field {
                        name: field.name.clone(),
                        ty: self.resolve_tags_inner(&field.ty, seen),
                        bit_width: field.bit_width,
                    })
                    .collect(),
            ),
            Type::Pointer(inner) => Type::Pointer(Box::new(self.resolve_tags_inner(inner, seen))),
            Type::Array(inner, count, is_static) => Type::Array(
                Box::new(self.resolve_tags_inner(inner, seen)),
                *count,
                *is_static,
            ),
            Type::Vla(inner, bound) => Type::Vla(
                Box::new(self.resolve_tags_inner(inner, seen)),
                bound.clone(),
            ),
            Type::Qualified(inner, qualifiers) => {
                Type::Qualified(Box::new(self.resolve_tags_inner(inner, seen)), *qualifiers)
            }
            _ => ty.clone(),
        }
    }

    fn size(t: &Type) -> usize {
        crate::layout::size(t)
    }

    fn sizeof_value(&mut self, ty: &Type, v: &mut Vec<Instr>) -> String {
        let scalar = Self::scalar(ty);
        if let Type::Vla(element, bound) = scalar {
            let count = self.expr(&bound, v);
            let count_ty = self.type_of(&bound);
            let count = self.convert_value(count, &count_ty, &Type::UnsignedLong, v);
            let unit = self.tmp();
            v.push(Instr::ConstLong(
                unit.clone(),
                Self::size(&element).max(1) as i64,
            ));
            let out = self.tmp();
            v.push(Instr::BinTyped(
                out.clone(),
                BinOp::Mul,
                count,
                unit,
                Type::UnsignedLong,
            ));
            out
        } else {
            let out = self.tmp();
            v.push(Instr::ConstLong(out.clone(), Self::size(&scalar) as i64));
            out
        }
    }

    fn is_pointer(t: &Type) -> bool {
        matches!(
            Self::scalar(t),
            Type::Pointer(_) | Type::Array(_, _, _) | Type::Vla(_, _)
        )
    }

    fn pointee(t: &Type) -> Option<Type> {
        match Self::scalar(t) {
            Type::Pointer(x) | Type::Array(x, _, _) | Type::Vla(x, _) => Some(*x),
            _ => None,
        }
    }

    fn field(t: &Type, name: &str) -> Option<crate::layout::FieldLayout> {
        crate::layout::field(t, name)
    }

    fn type_of(&self, e: &Expr) -> Type {
        match e {
            Expr::Int(n) => Self::integer_literal_type(*n),
            Expr::Char(_) => Type::Int,
            Expr::Float(_, ty) => ty.clone(),
            Expr::Complex(_, _, ty) => Type::Complex(Box::new(ty.clone())),
            Expr::Cast(ty, _) => ty.clone(),
            Expr::IncDec(target, _, _) => self.type_of(target),
            Expr::SizeofType(_) | Expr::Unary(UnOp::Sizeof, _) => Type::UnsignedLong,
            Expr::CompoundLiteral(ty, _) => ty.clone(),
            Expr::VaStart(_) | Expr::VaEnd(_) | Expr::VaCopy(_, _) => Type::Void,
            Expr::VaArg(_, ty) => ty.clone(),
            Expr::String(value) => Type::Array(Box::new(Type::Char), Some(value.len() + 1), false),
            Expr::Var(n) => {
                if let Some(ty) = self.vars.get(n) {
                    self.resolve_tags(ty)
                } else if let Some((ret, params, variadic)) = self.functions.get(n) {
                    Type::Function(Box::new(ret.clone()), params.clone(), *variadic, true)
                } else {
                    Type::Int
                }
            }
            Expr::Assign(x, _) => self.type_of(x),
            Expr::CompoundAssign(x, _, _) => self.type_of(x),
            Expr::Unary(UnOp::Address, x) => Type::Pointer(Box::new(self.type_of(x))),
            Expr::Unary(UnOp::Deref, x) => Self::pointee(&self.type_of(x)).unwrap_or(Type::Int),
            Expr::Unary(UnOp::Not | UnOp::BitNot, _) => Type::Int,
            Expr::Unary(_, x) => self.type_of(x),
            Expr::Binary(op, a, b) => match op {
                BinOp::Comma => self.type_of(b),
                BinOp::And
                | BinOp::Or
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge => Type::Int,
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                    Self::usual_arithmetic_type(&self.type_of(a), &self.type_of(b))
                }
                BinOp::Shl | BinOp::Shr => Self::integer_promote(&self.type_of(a)),
                BinOp::Add | BinOp::Sub => {
                    let left = self.type_of(a);
                    let right = self.type_of(b);
                    if Self::is_pointer(&left) {
                        left
                    } else if Self::is_pointer(&right) {
                        right
                    } else {
                        Self::usual_arithmetic_type(&left, &right)
                    }
                }
                BinOp::Mul | BinOp::Div | BinOp::Rem => {
                    Self::usual_arithmetic_type(&self.type_of(a), &self.type_of(b))
                }
            },
            Expr::Conditional(_, yes, no) => {
                let yes = self.type_of(yes);
                let no = self.type_of(no);
                if Self::is_pointer(&yes) {
                    yes
                } else if Self::is_pointer(&no) {
                    no
                } else {
                    Self::usual_arithmetic_type(&yes, &no)
                }
            }
            Expr::Call(n, _) => self
                .functions
                .get(n)
                .map(|x| x.0.clone())
                .unwrap_or(Type::Int),
            Expr::IndirectCall(callee, _) => self
                .function_signature(&self.type_of(callee))
                .map(|x| x.0)
                .unwrap_or(Type::Int),
            Expr::Member(x, name, arrow) => {
                let base = if *arrow {
                    Self::pointee(&self.type_of(x)).unwrap_or(Type::Int)
                } else {
                    self.type_of(x)
                };
                Self::field(&base, name).map(|x| x.ty).unwrap_or(Type::Int)
            }
            Expr::Subscript(left, right) => {
                let left = self.type_of(left);
                let right = self.type_of(right);
                if Self::is_pointer(&left) {
                    Self::pointee(&left).unwrap_or(Type::Int)
                } else {
                    Self::pointee(&right).unwrap_or(Type::Int)
                }
            }
        }
    }

    fn is_float(t: &Type) -> bool {
        matches!(
            Self::scalar(t),
            Type::Float | Type::Double | Type::LongDouble
        )
    }

    fn is_complex(t: &Type) -> bool {
        matches!(Self::scalar(t), Type::Complex(_))
    }

    fn complex_base(t: &Type) -> Option<Type> {
        match Self::scalar(t) {
            Type::Complex(inner) => Some(*inner),
            _ => None,
        }
    }

    fn real_type(t: &Type) -> Type {
        match Self::scalar(t) {
            Type::Complex(inner) => *inner,
            other => other,
        }
    }

    fn arithmetic(t: &Type) -> bool {
        matches!(
            Self::scalar(t),
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
                | Type::Float
                | Type::Double
                | Type::LongDouble
                | Type::Complex(_)
        )
    }

    fn integer_literal_type(value: i64) -> Type {
        if i32::try_from(value).is_ok() {
            Type::Int
        } else {
            Type::Long
        }
    }

    fn is_integer(t: &Type) -> bool {
        matches!(
            Self::scalar(t),
            Type::Bool
                | Type::Char
                | Type::SignedChar
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort
                | Type::Int
                | Type::SignedInt
                | Type::UnsignedInt
                | Type::Long
                | Type::UnsignedLong
                | Type::LongLong
                | Type::UnsignedLongLong
                | Type::Enum(_, _)
        )
    }

    fn integer_promote(t: &Type) -> Type {
        if matches!(Self::scalar(t), Type::Enum(_, _)) || Self::integer_rank(t) < 3 {
            Type::Int
        } else {
            Self::scalar(t)
        }
    }

    fn integer_rank(t: &Type) -> u8 {
        match Self::scalar(t) {
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
            Self::scalar(t),
            Type::Bool
                | Type::UnsignedChar
                | Type::UnsignedShort
                | Type::UnsignedInt
                | Type::UnsignedLong
                | Type::UnsignedLongLong
        )
    }

    fn integer_precision(rank: u8) -> u8 {
        match rank {
            0 | 1 => 8,
            2 => 16,
            4 | 5 => 64,
            _ => 32,
        }
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

    fn usual_arithmetic_type(a: &Type, b: &Type) -> Type {
        let complex = Self::is_complex(a) || Self::is_complex(b);
        let a = Self::real_type(a);
        let b = Self::real_type(b);
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

    fn lvalue_addr(&mut self, e: &Expr, v: &mut Vec<Instr>) -> String {
        match e {
            Expr::CompoundLiteral(ty, values) => self.compound_literal_addr(ty, values, v),
            Expr::Var(n) => {
                if let Some(slot) = self.slots.get(n) {
                    slot.clone()
                } else if self.functions.contains_key(n) {
                    let out = self.tmp();
                    v.push(Instr::FuncAddr(out.clone(), n.clone()));
                    out
                } else {
                    format!("%{}", n)
                }
            }
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
                let offset = Self::field(&base_type, name).map(|x| x.offset).unwrap_or(0) as i64;
                self.add_addr(base, offset, v)
            }
            Expr::Subscript(x, index) => {
                let (pointer, index) = if Self::is_pointer(&self.type_of(x)) {
                    (x.as_ref(), index.as_ref())
                } else {
                    (index.as_ref(), x.as_ref())
                };
                let pointer_ty = self.type_of(pointer);
                let element = Self::pointee(&pointer_ty).unwrap_or(Type::Int);
                let base = if Self::is_pointer(&pointer_ty) {
                    if matches!(
                        Self::scalar(&pointer_ty),
                        Type::Array(_, _, _) | Type::Vla(_, _)
                    ) {
                        self.lvalue_addr(pointer, v)
                    } else {
                        self.expr(pointer, v)
                    }
                } else {
                    self.lvalue_addr(pointer, v)
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
        let ty = &self.resolve_tags(ty);
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
        let ty = &self.resolve_tags(ty);
        match initializer {
            Initializer::Expr(expr) => {
                let mut cursor = InitCursor::new(&[]);
                self.initialize_object_value(ty, expr, &mut cursor, addr, v);
            }
            Initializer::List(items) => {
                let mut cursor = InitCursor::new(items);
                self.initialize_object_items(ty, &mut cursor, addr, v);
            }
        }
    }

    fn initialize_object_value(
        &mut self,
        ty: &Type,
        expr: &Expr,
        cursor: &mut InitCursor,
        addr: String,
        v: &mut Vec<Instr>,
    ) {
        let scalar = Self::scalar(ty);
        if let (Type::Array(_, Some(_), _), Expr::String(value)) = (&scalar, expr) {
            self.store_string_initializer(&scalar, value, &addr, v);
            return;
        }
        if Self::aggregate(&scalar) {
            if let Expr::CompoundLiteral(_, initializer) = expr {
                self.initialize_object(&scalar, initializer, addr, v);
            } else {
                self.initialize_object_elided(&scalar, expr, cursor, addr, v);
            }
            return;
        }
        let value = self.expr_as(expr, &scalar, v);
        if Self::is_complex(&scalar) {
            self.copy_storage(addr, value, Self::size(&scalar), v);
        } else {
            v.push(Instr::Store(value, addr, scalar));
        }
    }

    fn initialize_object_from_value(
        &mut self,
        ty: &Type,
        value: &Initializer,
        cursor: &mut InitCursor,
        addr: String,
        v: &mut Vec<Instr>,
    ) {
        match value {
            Initializer::Expr(expr) => self.initialize_object_value(ty, expr, cursor, addr, v),
            Initializer::List(items) => {
                let mut nested = InitCursor::new(items);
                self.initialize_object_items(ty, &mut nested, addr, v);
            }
        }
    }

    fn initialize_object_items(
        &mut self,
        ty: &Type,
        cursor: &mut InitCursor,
        addr: String,
        v: &mut Vec<Instr>,
    ) {
        match Self::scalar(ty) {
            Type::Qualified(inner, _) => self.initialize_object_items(&inner, cursor, addr, v),
            Type::Array(element, count, _) => {
                let stride = Self::size(&element);
                let mut next = 0usize;
                while !cursor.is_empty()
                    && (!count.is_some_and(|n| next >= n)
                        || cursor
                            .peek()
                            .is_some_and(|item| !item.designators.is_empty()))
                {
                    let Some(item) = cursor.next() else { break };
                    let index = match item.designators.first() {
                        Some(Designator::Index(expr)) => {
                            Self::constant_value(expr).unwrap_or(next as i64).max(0) as usize
                        }
                        Some(Designator::Member(_)) => return,
                        None => next,
                    };
                    let target = self.add_addr(addr.clone(), (index * stride) as i64, v);
                    if item.designators.len() > 1 {
                        self.initialize_object_designated(
                            &element,
                            &item.designators[1..],
                            &item.value,
                            target,
                            v,
                        );
                    } else {
                        self.initialize_object_from_value(&element, &item.value, cursor, target, v);
                    }
                    next = index + 1;
                }
            }
            Type::Struct(_, fields) => {
                let layouts = crate::layout::struct_layout(&fields).1;
                let mut next = Self::next_initializable(&fields, 0);
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
                            .unwrap_or(next),
                        _ => next,
                    };
                    if let Some(layout) = layouts.get(index) {
                        let target = self.add_addr(addr.clone(), layout.offset as i64, v);
                        if layout.width.is_some_and(|width| width > 0) {
                            self.initialize_bitfield(layout, &item.value, target, v);
                        } else if item.designators.len() > 1 {
                            self.initialize_object_designated(
                                &layout.ty,
                                &item.designators[1..],
                                &item.value,
                                target,
                                v,
                            );
                        } else {
                            self.initialize_object_from_value(
                                &layout.ty,
                                &item.value,
                                cursor,
                                target,
                                v,
                            );
                        }
                    }
                    next = Self::next_initializable(&fields, index + 1);
                }
            }
            Type::Union(_, fields) => {
                let Some(item) = cursor.next() else { return };
                let index = match item.designators.first() {
                    Some(Designator::Member(name)) => fields
                        .iter()
                        .position(|field| field.name.as_deref() == Some(name))
                        .unwrap_or(0),
                    _ => 0,
                };
                if let Some(layout) = crate::layout::union_layout(&fields).1.get(index) {
                    if layout.width.is_some_and(|width| width > 0) {
                        self.initialize_bitfield(layout, &item.value, addr, v);
                    } else if item.designators.len() > 1 {
                        self.initialize_object_designated(
                            &layout.ty,
                            &item.designators[1..],
                            &item.value,
                            addr,
                            v,
                        );
                    } else {
                        self.initialize_object_from_value(&layout.ty, &item.value, cursor, addr, v);
                    }
                }
            }
            _ => {
                let Some(item) = cursor.next() else { return };
                if item.designators.is_empty() {
                    self.initialize_object_from_value(ty, &item.value, cursor, addr, v);
                }
            }
        }
    }

    fn initialize_object_elided(
        &mut self,
        ty: &Type,
        first: &Expr,
        cursor: &mut InitCursor,
        addr: String,
        v: &mut Vec<Instr>,
    ) {
        match Self::scalar(ty) {
            Type::Array(element, count, _) => {
                let stride = Self::size(&element);
                self.initialize_object_value(&element, first, cursor, addr.clone(), v);
                let mut next = 1usize;
                while !cursor.is_empty()
                    && (!count.is_some_and(|n| next >= n)
                        || cursor
                            .peek()
                            .is_some_and(|item| !item.designators.is_empty()))
                {
                    let Some(item) = cursor.next() else { break };
                    let index = match item.designators.first() {
                        Some(Designator::Index(expr)) => {
                            Self::constant_value(expr).unwrap_or(next as i64).max(0) as usize
                        }
                        Some(Designator::Member(_)) => return,
                        None => next,
                    };
                    let target = self.add_addr(addr.clone(), (index * stride) as i64, v);
                    if item.designators.len() > 1 {
                        self.initialize_object_designated(
                            &element,
                            &item.designators[1..],
                            &item.value,
                            target,
                            v,
                        );
                    } else {
                        self.initialize_object_from_value(&element, &item.value, cursor, target, v);
                    }
                    next = index + 1;
                }
            }
            Type::Struct(_, fields) => {
                let layouts = crate::layout::struct_layout(&fields).1;
                let first_index = Self::next_initializable(&fields, 0);
                let Some(layout) = layouts.get(first_index) else {
                    return;
                };
                let target = self.add_addr(addr.clone(), layout.offset as i64, v);
                if layout.width.is_some_and(|width| width > 0) {
                    self.initialize_bitfield_expr(layout, first, target, v);
                } else {
                    self.initialize_object_value(&layout.ty, first, cursor, target, v);
                }
                let mut next = Self::next_initializable(&fields, first_index + 1);
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
                            .unwrap_or(next),
                        _ => next,
                    };
                    if let Some(layout) = layouts.get(index) {
                        let target = self.add_addr(addr.clone(), layout.offset as i64, v);
                        if layout.width.is_some_and(|width| width > 0) {
                            self.initialize_bitfield(layout, &item.value, target, v);
                        } else if item.designators.len() > 1 {
                            self.initialize_object_designated(
                                &layout.ty,
                                &item.designators[1..],
                                &item.value,
                                target,
                                v,
                            );
                        } else {
                            self.initialize_object_from_value(
                                &layout.ty,
                                &item.value,
                                cursor,
                                target,
                                v,
                            );
                        }
                    }
                    next = Self::next_initializable(&fields, index + 1);
                }
            }
            Type::Union(_, fields) => {
                if let Some(layout) = crate::layout::union_layout(&fields).1.first().cloned() {
                    if layout.width.is_some_and(|width| width > 0) {
                        self.initialize_bitfield_expr(&layout, first, addr, v);
                    } else {
                        self.initialize_object_value(&layout.ty, first, cursor, addr, v);
                    }
                }
            }
            _ => self.initialize_object_value(ty, first, cursor, addr, v),
        }
    }

    fn initialize_object_designated(
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
        match (Self::scalar(ty), &designators[0]) {
            (Type::Qualified(inner, _), _) => {
                self.initialize_object_designated(&inner, designators, value, addr, v)
            }
            (Type::Array(element, _, _), Designator::Index(index)) => {
                let index = Self::constant_value(index).unwrap_or(0).max(0) as usize;
                let target = self.add_addr(addr, (index * Self::size(&element)) as i64, v);
                self.initialize_object_designated(&element, &designators[1..], value, target, v);
            }
            (Type::Struct(_, fields) | Type::Union(_, fields), Designator::Member(name)) => {
                if let Some(index) = fields
                    .iter()
                    .position(|field| field.name.as_deref() == Some(name))
                {
                    let layout =
                        crate::layout::member(ty, index).unwrap_or(crate::layout::FieldLayout {
                            offset: 0,
                            bit_offset: 0,
                            width: None,
                            ty: Type::Int,
                        });
                    let target = self.add_addr(addr, layout.offset as i64, v);
                    if layout.width.is_some_and(|width| width > 0) {
                        self.initialize_bitfield(&layout, value, target, v);
                    } else {
                        self.initialize_object_designated(
                            &layout.ty,
                            &designators[1..],
                            value,
                            target,
                            v,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn store_string_initializer(&mut self, ty: &Type, value: &str, addr: &str, v: &mut Vec<Instr>) {
        self.zero_object(ty, addr, v);
        let Type::Array(element, Some(count), _) = Self::scalar(ty) else {
            return;
        };
        for (index, value) in value.bytes().take(count).enumerate() {
            let target = self.add_addr(addr.to_owned(), index as i64, v);
            let byte = self.tmp();
            v.push(Instr::Const(byte.clone(), value as i64));
            v.push(Instr::Store(byte, target, (*element).clone()));
        }
    }

    fn allocate_local(&mut self, slot: &str, ty: &Type, v: &mut Vec<Instr>) {
        if let Type::Vla(element, bound) = Self::scalar(ty) {
            let count = self.expr(&bound, v);
            v.push(Instr::AllocVla(
                slot.to_string(),
                count,
                Self::size(&element).max(1),
            ));
        } else {
            v.push(Instr::Alloc(slot.to_string(), Self::size(ty).max(1)));
        }
    }

    fn initialize_local(
        &mut self,
        name: &str,
        ty: &Type,
        init: Option<&Initializer>,
        v: &mut Vec<Instr>,
    ) {
        let Some(initializer) = init else {
            return;
        };
        let addr = self.lvalue_addr(&Expr::Var(name.to_string()), v);
        if matches!(initializer, Initializer::List(_)) || Self::aggregate(ty) {
            self.zero_object(ty, &addr, v);
        }
        self.initialize_object(ty, initializer, addr, v);
    }

    fn zero_object(&mut self, ty: &Type, addr: &str, v: &mut Vec<Instr>) {
        match Self::scalar(ty) {
            Type::Array(ref element, Some(count), _) => {
                for i in 0..count {
                    let target =
                        self.add_addr(addr.to_owned(), (i * Self::size(element)) as i64, v);
                    self.zero_object(element, &target, v);
                }
            }
            Type::Struct(_, ref fields) => {
                let (_, layouts) = crate::layout::struct_layout(fields);
                for layout in layouts {
                    if let Some(width) = layout.width {
                        if width > 0 {
                            let target = self.add_addr(addr.to_owned(), layout.offset as i64, v);
                            self.zero_storage(target, Self::size(&layout.ty), v);
                        }
                    } else {
                        let target = self.add_addr(addr.to_owned(), layout.offset as i64, v);
                        self.zero_object(&layout.ty, &target, v);
                    }
                }
            }
            Type::Complex(_) => {
                self.zero_storage(addr.to_owned(), Self::size(ty), v);
            }
            Type::Union(_, ref fields) => {
                let Some(layout) = crate::layout::union_layout(fields).1.first().cloned() else {
                    return;
                };
                if let Some(width) = layout.width {
                    if width > 0 {
                        self.zero_storage(addr.to_owned(), Self::size(&layout.ty), v);
                    }
                } else {
                    self.zero_object(&layout.ty, addr, v);
                }
            }
            _ => self.zero_storage(addr.to_owned(), Self::size(ty).max(1), v),
        }
    }

    fn zero_storage(&mut self, addr: String, size: usize, v: &mut Vec<Instr>) {
        let mut offset = 0usize;
        while offset < size {
            let chunk = (size - offset).min(8);
            let target = self.add_addr(addr.clone(), offset as i64, v);
            let zero = self.tmp();
            if chunk == 8 {
                v.push(Instr::ConstLong(zero.clone(), 0));
            } else {
                v.push(Instr::Const(zero.clone(), 0));
            }
            v.push(Instr::StoreSize(zero, target, chunk));
            offset += chunk;
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

    fn member_layout(&self, e: &Expr) -> Option<crate::layout::FieldLayout> {
        let Expr::Member(base, name, arrow) = e else {
            return None;
        };
        let base_ty = if *arrow {
            Self::pointee(&self.type_of(base)).unwrap_or(Type::Int)
        } else {
            self.type_of(base)
        };
        let base_ty = self.resolve_tags(&base_ty);
        Self::field(&base_ty, name)
    }

    fn bitfield_signed(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Char
                | Type::SignedChar
                | Type::Short
                | Type::Int
                | Type::SignedInt
                | Type::Enum(_, _)
                | Type::Qualified(_, _)
        ) && !matches!(
            Self::scalar(ty),
            Type::Bool | Type::UnsignedChar | Type::UnsignedShort | Type::UnsignedInt
        )
    }

    fn bitfield_width_mask(width: u32) -> i64 {
        if width >= 32 {
            -1
        } else {
            ((1u64 << width) - 1) as u32 as i32 as i64
        }
    }

    fn bitfield_unit_mask(size: usize) -> i64 {
        if size >= 8 {
            -1
        } else {
            ((1u64 << (size * 8)) - 1) as u32 as i32 as i64
        }
    }

    fn load_bitfield(
        &mut self,
        e: &Expr,
        layout: &crate::layout::FieldLayout,
        v: &mut Vec<Instr>,
    ) -> String {
        let addr = self.lvalue_addr(e, v);
        self.load_bitfield_at(layout, addr, v)
    }

    fn load_bitfield_at(
        &mut self,
        layout: &crate::layout::FieldLayout,
        addr: String,
        v: &mut Vec<Instr>,
    ) -> String {
        let width = layout.width.unwrap_or(0);
        let unit = self.tmp();
        v.push(Instr::LoadZero(unit.clone(), addr, Self::size(&layout.ty)));
        let mut value = unit;
        if layout.bit_offset > 0 {
            let shift = self.tmp();
            v.push(Instr::Const(shift.clone(), layout.bit_offset as i64));
            let shifted = self.tmp();
            v.push(Instr::Bin(shifted.clone(), BinOp::Shr, value, shift));
            value = shifted;
        }
        if width < 32 {
            let mask = self.tmp();
            v.push(Instr::Const(mask.clone(), Self::bitfield_width_mask(width)));
            let masked = self.tmp();
            v.push(Instr::Bin(masked.clone(), BinOp::BitAnd, value, mask));
            value = masked;
        }
        if Self::bitfield_signed(&layout.ty) && width < 32 {
            let shift = 32 - width;
            let shift_value = self.tmp();
            v.push(Instr::Const(shift_value.clone(), shift as i64));
            let left = self.tmp();
            v.push(Instr::Bin(
                left.clone(),
                BinOp::Shl,
                value,
                shift_value.clone(),
            ));
            let right = self.tmp();
            v.push(Instr::Bin(right.clone(), BinOp::Shr, left, shift_value));
            value = right;
        }
        value
    }

    fn store_bitfield(
        &mut self,
        layout: &crate::layout::FieldLayout,
        value: String,
        addr: String,
        v: &mut Vec<Instr>,
    ) {
        let width = layout.width.unwrap_or(0);
        if width == 0 {
            return;
        }
        let size = Self::size(&layout.ty);
        let mut value = value;
        if width < 32 {
            let mask = self.tmp();
            v.push(Instr::Const(mask.clone(), Self::bitfield_width_mask(width)));
            let masked = self.tmp();
            v.push(Instr::Bin(masked.clone(), BinOp::BitAnd, value, mask));
            value = masked;
        }
        if layout.bit_offset > 0 {
            let shift = self.tmp();
            v.push(Instr::Const(shift.clone(), layout.bit_offset as i64));
            let shifted = self.tmp();
            v.push(Instr::Bin(shifted.clone(), BinOp::Shl, value, shift));
            value = shifted;
        }
        let field_mask = (Self::bitfield_width_mask(width) as u64) << layout.bit_offset;
        let clear_mask =
            (!field_mask & (Self::bitfield_unit_mask(size) as u64)) as u32 as i32 as i64;
        let old = self.tmp();
        v.push(Instr::LoadZero(old.clone(), addr.clone(), size));
        let mut merged = old;
        if clear_mask != 0 {
            let clear = self.tmp();
            v.push(Instr::Const(clear.clone(), clear_mask));
            let cleared = self.tmp();
            v.push(Instr::Bin(cleared.clone(), BinOp::BitAnd, merged, clear));
            merged = cleared;
        }
        let combined = self.tmp();
        v.push(Instr::Bin(combined.clone(), BinOp::BitOr, merged, value));
        v.push(Instr::StoreSize(combined, addr, size));
    }

    fn initializer_scalar_expr(initializer: &Initializer) -> Option<&Expr> {
        match initializer {
            Initializer::Expr(expr) => Some(expr),
            Initializer::List(items) => items
                .first()
                .filter(|item| item.designators.is_empty())
                .and_then(|item| Self::initializer_scalar_expr(&item.value)),
        }
    }

    fn next_initializable(fields: &[Field], mut index: usize) -> usize {
        while index < fields.len() && fields[index].name.is_none() {
            index += 1;
        }
        index
    }

    fn initialize_bitfield(
        &mut self,
        layout: &crate::layout::FieldLayout,
        initializer: &Initializer,
        addr: String,
        v: &mut Vec<Instr>,
    ) {
        let Some(expr) = Self::initializer_scalar_expr(initializer) else {
            return;
        };
        let value = self.expr(expr, v);
        self.store_bitfield(layout, value, addr, v);
    }

    fn initialize_bitfield_expr(
        &mut self,
        layout: &crate::layout::FieldLayout,
        expr: &Expr,
        addr: String,
        v: &mut Vec<Instr>,
    ) {
        let value = self.expr_as(expr, &layout.ty, v);
        self.store_bitfield(layout, value, addr, v);
    }

    fn function_signature(&self, ty: &Type) -> Option<(Type, Vec<Type>, bool)> {
        match Self::scalar(ty) {
            Type::Function(ret, params, variadic, _) => Some((*ret, params, variadic)),
            Type::Pointer(inner) => match Self::scalar(&inner) {
                Type::Function(ret, params, variadic, _) => Some((*ret, params, variadic)),
                _ => None,
            },
            _ => None,
        }
    }

    fn call_arguments(
        &mut self,
        args: &[Expr],
        params: &[Type],
        v: &mut Vec<Instr>,
    ) -> Vec<(String, Type)> {
        let mut values = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            if i < params.len() {
                let ty = params[i].clone();
                let value = self.expr_as(arg, &ty, v);
                values.push((value, ty));
            } else {
                let mut ty = self.type_of(arg);
                let mut value = self.expr(arg, v);
                if matches!(Self::scalar(&ty), Type::Float) {
                    let promoted = self.tmp();
                    v.push(Instr::Convert(
                        promoted.clone(),
                        value,
                        ty.clone(),
                        Type::Double,
                    ));
                    value = promoted;
                    ty = Type::Double;
                } else {
                    let promoted_ty = Self::integer_promote(&ty);
                    if promoted_ty != ty {
                        value = self.convert_value(value, &ty, &promoted_ty, v);
                        ty = promoted_ty;
                    }
                }
                values.push((value, ty));
            }
        }
        values
    }

    fn increment_decrement(
        &mut self,
        target: &Expr,
        increment: bool,
        prefix: bool,
        v: &mut Vec<Instr>,
    ) -> String {
        let ty = self.type_of(target);
        let scalar_ty = Self::scalar(&ty);
        let addr = self.lvalue_addr(target, v);
        let bitfield = self
            .member_layout(target)
            .filter(|layout| layout.width.is_some_and(|width| width > 0));
        let old = if let Some(layout) = &bitfield {
            self.load_bitfield_at(layout, addr.clone(), v)
        } else {
            let old = self.tmp();
            v.push(Instr::Load(old.clone(), addr.clone(), ty.clone()));
            old
        };
        let new = self.tmp();
        if Self::is_pointer(&ty) {
            let element_size = Self::pointee(&ty)
                .map(|element| Self::size(&element).max(1))
                .unwrap_or(1) as i64;
            let offset = if increment {
                element_size
            } else {
                -element_size
            };
            v.push(Instr::Addr(new.clone(), old.clone(), offset));
        } else if Self::is_float(&scalar_ty) {
            let one = self.tmp();
            v.push(Instr::ConstFloat(one.clone(), 1.0, scalar_ty.clone()));
            v.push(Instr::FloatBin(
                new.clone(),
                if increment { BinOp::Add } else { BinOp::Sub },
                old.clone(),
                one,
                scalar_ty.clone(),
            ));
        } else {
            let one = self.tmp();
            if Self::size(&scalar_ty) >= 8 {
                v.push(Instr::ConstLong(one.clone(), 1));
            } else {
                v.push(Instr::Const(one.clone(), 1));
            }
            v.push(Instr::BinTyped(
                new.clone(),
                if increment { BinOp::Add } else { BinOp::Sub },
                old.clone(),
                one,
                scalar_ty.clone(),
            ));
            if matches!(scalar_ty, Type::Bool) {
                let normalized = self.tmp();
                v.push(Instr::BinTyped(
                    normalized.clone(),
                    BinOp::Ne,
                    new,
                    "0".into(),
                    Type::Bool,
                ));
                self.store_increment_value(&ty, &addr, &bitfield, normalized.clone(), v);
                return if prefix { normalized } else { old };
            }
        }
        self.store_increment_value(&ty, &addr, &bitfield, new.clone(), v);
        if prefix {
            if let Some(layout) = &bitfield {
                self.load_bitfield_at(layout, addr, v)
            } else if Self::is_integer(&scalar_ty) && Self::size(&scalar_ty) < 4 {
                let converted = self.tmp();
                v.push(Instr::Load(converted.clone(), addr, ty));
                converted
            } else {
                new
            }
        } else {
            old
        }
    }

    fn compound_assign(
        &mut self,
        target: &Expr,
        op: BinOp,
        value: &Expr,
        v: &mut Vec<Instr>,
    ) -> String {
        let target_ty = self.type_of(target);
        let scalar_ty = Self::scalar(&target_ty);
        let value_ty = self.type_of(value);
        let addr = self.lvalue_addr(target, v);
        let bitfield = self
            .member_layout(target)
            .filter(|layout| layout.width.is_some_and(|width| width > 0));
        let old = if let Some(layout) = &bitfield {
            self.load_bitfield_at(layout, addr.clone(), v)
        } else {
            let old = self.tmp();
            v.push(Instr::Load(old.clone(), addr.clone(), target_ty.clone()));
            old
        };
        let result = if Self::is_pointer(&target_ty) {
            let element = Self::pointee(&target_ty).unwrap_or(Type::Char);
            let offset = self.expr_as(value, &Type::Long, v);
            let stride = Self::size(&element).max(1);
            let offset = if stride == 1 {
                offset
            } else {
                let scale = self.tmp();
                v.push(Instr::ConstLong(scale.clone(), stride as i64));
                let scaled = self.tmp();
                v.push(Instr::BinTyped(
                    scaled.clone(),
                    BinOp::Mul,
                    offset,
                    scale,
                    Type::Long,
                ));
                scaled
            };
            let old = self.convert_value(old, &target_ty, &Type::Long, v);
            let result = self.tmp();
            v.push(Instr::BinTyped(result.clone(), op, old, offset, Type::Long));
            result
        } else {
            let shift = matches!(op, BinOp::Shl | BinOp::Shr);
            let common = if shift {
                Self::integer_promote(&target_ty)
            } else {
                Self::usual_arithmetic_type(&target_ty, &value_ty)
            };
            let old = self.convert_value(old, &target_ty, &common, v);
            let value_target = if shift {
                Self::integer_promote(&value_ty)
            } else {
                common.clone()
            };
            let right = self.expr_as(value, &value_target, v);
            let result = self.tmp();
            if Self::is_float(&common) {
                v.push(Instr::FloatBin(
                    result.clone(),
                    op,
                    old,
                    right,
                    common.clone(),
                ));
            } else {
                v.push(Instr::BinTyped(
                    result.clone(),
                    op,
                    old,
                    right,
                    common.clone(),
                ));
            }
            if matches!(scalar_ty, Type::Bool) {
                let normalized = self.tmp();
                if Self::is_float(&common) {
                    let zero = self.tmp();
                    v.push(Instr::ConstFloat(zero.clone(), 0.0, common.clone()));
                    v.push(Instr::FloatBin(
                        normalized.clone(),
                        BinOp::Ne,
                        result,
                        zero,
                        common,
                    ));
                } else {
                    v.push(Instr::BinTyped(
                        normalized.clone(),
                        BinOp::Ne,
                        result,
                        "0".into(),
                        common,
                    ));
                }
                normalized
            } else {
                self.convert_value(result, &common, &target_ty, v)
            }
        };
        self.store_increment_value(&target_ty, &addr, &bitfield, result.clone(), v);
        if let Some(layout) = &bitfield {
            self.load_bitfield_at(layout, addr, v)
        } else if Self::is_integer(&scalar_ty) && Self::size(&scalar_ty) < 4 {
            let converted = self.tmp();
            v.push(Instr::Load(converted.clone(), addr, target_ty));
            converted
        } else {
            result
        }
    }

    fn store_increment_value(
        &mut self,
        ty: &Type,
        addr: &str,
        bitfield: &Option<crate::layout::FieldLayout>,
        value: String,
        v: &mut Vec<Instr>,
    ) {
        if let Some(layout) = bitfield {
            self.store_bitfield(layout, value, addr.to_owned(), v);
        } else {
            v.push(Instr::Store(value, addr.to_owned(), ty.clone()));
        }
    }

    fn expr(&mut self, e: &Expr, v: &mut Vec<Instr>) -> String {
        match e {
            Expr::Int(n) => {
                let x = self.tmp();
                if i32::try_from(*n).is_ok() {
                    v.push(Instr::Const(x.clone(), *n));
                } else {
                    v.push(Instr::ConstLong(x.clone(), *n));
                }
                x
            }
            Expr::Float(value, ty) => {
                let x = self.tmp();
                v.push(Instr::ConstFloat(x.clone(), *value, ty.clone()));
                x
            }
            Expr::Complex(real, imaginary, base) => {
                let real = {
                    let x = self.tmp();
                    v.push(Instr::ConstFloat(x.clone(), *real, base.clone()));
                    x
                };
                let imaginary = {
                    let x = self.tmp();
                    v.push(Instr::ConstFloat(x.clone(), *imaginary, base.clone()));
                    x
                };
                self.make_complex(base, real, imaginary, v)
            }
            Expr::Cast(target, value) => self.expr_as(value, target, v),
            Expr::CompoundAssign(target, op, value) => self.compound_assign(target, *op, value, v),
            Expr::IncDec(target, increment, prefix) => {
                self.increment_decrement(target, *increment, *prefix, v)
            }
            Expr::CompoundLiteral(ty, values) => {
                let ty = self.resolve_tags(ty);
                let addr = self.compound_literal_addr(&ty, values, v);
                if matches!(Self::scalar(&ty), Type::Array(_, _, _)) || Self::is_complex(&ty) {
                    addr
                } else {
                    let out = self.tmp();
                    v.push(Instr::Load(out.clone(), addr, ty));
                    out
                }
            }
            Expr::SizeofType(ty) => {
                let ty = self.resolve_tags(ty);
                self.sizeof_value(&ty, v)
            }
            Expr::Char(c) => {
                let x = self.tmp();
                v.push(Instr::Const(x.clone(), *c as i64));
                x
            }
            Expr::String(value) => self.intern_string(value),
            Expr::Member(_, _, _) => {
                if let Some(layout) = self.member_layout(e) {
                    if layout.width.is_some_and(|width| width > 0) {
                        return self.load_bitfield(e, &layout, v);
                    }
                }
                let ty = self.type_of(e);
                if Self::is_complex(&ty) {
                    return self.lvalue_addr(e, v);
                }
                let addr = self.lvalue_addr(e, v);
                let out = self.tmp();
                v.push(Instr::Load(out.clone(), addr, ty));
                out
            }
            Expr::Var(n) if !self.vars.contains_key(n) && self.functions.contains_key(n) => {
                let out = self.tmp();
                v.push(Instr::FuncAddr(out.clone(), n.clone()));
                out
            }
            Expr::Var(_) | Expr::Subscript(_, _) => {
                let ty = self.type_of(e);
                if matches!(Self::scalar(&ty), Type::Array(_, _, _) | Type::Vla(_, _))
                    || Self::is_complex(&ty)
                {
                    self.lvalue_addr(e, v)
                } else {
                    let addr = self.lvalue_addr(e, v);
                    let out = self.tmp();
                    v.push(Instr::Load(out.clone(), addr, ty));
                    out
                }
            }
            Expr::Assign(target, e) => {
                let target_ty = self.type_of(target);
                let x = self.expr_as(e, &target_ty, v);
                if Self::is_complex(&target_ty) {
                    let addr = self.lvalue_addr(target, v);
                    self.copy_storage(addr.clone(), x, Self::size(&target_ty), v);
                    return addr;
                }
                if let Some(layout) = self.member_layout(target) {
                    if layout.width.is_some_and(|width| width > 0) {
                        let addr = self.lvalue_addr(target, v);
                        self.store_bitfield(&layout, x.clone(), addr, v);
                        return x;
                    }
                }
                let addr = self.lvalue_addr(target, v);
                v.push(Instr::Store(x.clone(), addr, target_ty));
                x
            }
            Expr::Unary(UnOp::Address, e) => self.lvalue_addr(e, v),
            Expr::Unary(UnOp::Deref, e) => {
                let pointer_ty = self.type_of(e);
                let ty = Self::pointee(&pointer_ty).unwrap_or(pointer_ty);
                let addr = self.expr(e, v);
                if matches!(Self::scalar(&ty), Type::Function(_, _, _, _)) {
                    return addr;
                }
                if Self::is_complex(&ty) {
                    return addr;
                }
                let out = self.tmp();
                v.push(Instr::Load(out.clone(), addr, ty));
                out
            }
            Expr::Unary(UnOp::Neg, e) => {
                let ty = self.type_of(e);
                if let Some(base) = Self::complex_base(&ty) {
                    let addr = self.expr(e, v);
                    let (real, imaginary) = self.load_complex_parts(&addr, &base, v);
                    let real = {
                        let zero = self.tmp();
                        let out = self.tmp();
                        v.push(Instr::ConstFloat(zero.clone(), 0.0, base.clone()));
                        v.push(Instr::FloatBin(
                            out.clone(),
                            BinOp::Sub,
                            zero,
                            real,
                            base.clone(),
                        ));
                        out
                    };
                    let imaginary = {
                        let zero = self.tmp();
                        let out = self.tmp();
                        v.push(Instr::ConstFloat(zero.clone(), 0.0, base.clone()));
                        v.push(Instr::FloatBin(
                            out.clone(),
                            BinOp::Sub,
                            zero,
                            imaginary,
                            base.clone(),
                        ));
                        out
                    };
                    return self.make_complex(&base, real, imaginary, v);
                }
                let x = self.expr(e, v);
                let z = self.tmp();
                if Self::is_float(&ty) {
                    let zero = self.tmp();
                    v.push(Instr::ConstFloat(zero.clone(), 0.0, ty.clone()));
                    v.push(Instr::FloatBin(z.clone(), BinOp::Sub, zero, x, ty));
                } else {
                    v.push(Instr::Bin(z.clone(), BinOp::Sub, "0".into(), x));
                }
                z
            }
            Expr::Unary(UnOp::Not, e) => {
                let ty = self.type_of(e);
                if Self::is_complex(&ty) {
                    let condition = self.condition(e, v);
                    let z = self.tmp();
                    v.push(Instr::Bin(z.clone(), BinOp::Eq, condition, "0".into()));
                    return z;
                }
                let x = self.expr(e, v);
                let z = self.tmp();
                if Self::is_float(&ty) {
                    let zero = self.tmp();
                    v.push(Instr::ConstFloat(zero.clone(), 0.0, ty.clone()));
                    v.push(Instr::FloatBin(z.clone(), BinOp::Eq, x, zero, ty));
                } else {
                    v.push(Instr::Bin(z.clone(), BinOp::Eq, x, "0".into()));
                }
                z
            }
            Expr::Unary(UnOp::Pos, e) => self.expr(e, v),
            Expr::Unary(UnOp::BitNot, e) => {
                let x = self.expr(e, v);
                let z = self.tmp();
                v.push(Instr::Bin(z.clone(), BinOp::BitXor, x, "-1".into()));
                z
            }
            Expr::Unary(UnOp::Sizeof, e) => {
                let ty = self.resolve_tags(&self.type_of(e));
                self.sizeof_value(&ty, v)
            }
            Expr::Binary(op, a, b) => {
                let left_ty = self.type_of(a);
                let right_ty = self.type_of(b);
                if matches!(
                    op,
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                ) && (Self::is_pointer(&left_ty) || Self::is_pointer(&right_ty))
                {
                    let compare_ty = if matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
                    {
                        Type::UnsignedLong
                    } else if Self::is_pointer(&left_ty) {
                        left_ty.clone()
                    } else {
                        right_ty.clone()
                    };
                    let x = self.expr_as(a, &compare_ty, v);
                    let y = self.expr_as(b, &compare_ty, v);
                    let out = self.tmp();
                    v.push(Instr::BinTyped(out.clone(), *op, x, y, compare_ty));
                    return out;
                }
                if *op == BinOp::Sub && Self::is_pointer(&left_ty) && Self::is_pointer(&right_ty) {
                    let element = Self::pointee(&left_ty).unwrap_or(Type::Char);
                    let left = self.expr_as(a, &Type::Long, v);
                    let right = self.expr_as(b, &Type::Long, v);
                    let bytes = self.tmp();
                    v.push(Instr::BinTyped(
                        bytes.clone(),
                        BinOp::Sub,
                        left,
                        right,
                        Type::Long,
                    ));
                    let stride = Self::size(&element).max(1);
                    if stride == 1 {
                        return bytes;
                    }
                    let divisor = self.tmp();
                    v.push(Instr::ConstLong(divisor.clone(), stride as i64));
                    let elements = self.tmp();
                    v.push(Instr::BinTyped(
                        elements.clone(),
                        BinOp::Div,
                        bytes,
                        divisor,
                        Type::Long,
                    ));
                    return elements;
                }
                if matches!(op, BinOp::Add | BinOp::Sub) {
                    if Self::is_pointer(&left_ty) && Self::is_integer(&right_ty) {
                        return self.pointer_offset(*op, a, b, &right_ty, v);
                    }
                    if *op == BinOp::Add
                        && Self::is_integer(&left_ty)
                        && Self::is_pointer(&right_ty)
                    {
                        return self.pointer_offset(BinOp::Add, b, a, &left_ty, v);
                    }
                }
                if Self::is_complex(&left_ty) || Self::is_complex(&right_ty) {
                    match op {
                        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                            let result_ty = self.type_of(e);
                            if let Some(base) = Self::complex_base(&result_ty) {
                                return self.complex_arithmetic(*op, a, b, &base, v);
                            }
                        }
                        BinOp::Eq | BinOp::Ne => {
                            return self.complex_equality(*op, a, b, v);
                        }
                        _ => {}
                    }
                }
                if matches!(op, BinOp::And | BinOp::Or) {
                    let x = self.condition(a, v);
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
                    let y = self.condition(b, v);
                    let normalized = self.tmp();
                    v.push(Instr::Bin(normalized.clone(), BinOp::Ne, y, "0".into()));
                    v.push(Instr::Move(z.clone(), normalized, Type::Int));
                    v.push(Instr::Jump(end.clone()));
                    v.push(Instr::Label(end));
                    return z;
                }
                if matches!(op, BinOp::Comma) {
                    self.expr(a, v);
                    return self.expr(b, v);
                }
                let arithmetic = matches!(
                    op,
                    BinOp::Add
                        | BinOp::Sub
                        | BinOp::Mul
                        | BinOp::Div
                        | BinOp::Rem
                        | BinOp::Eq
                        | BinOp::Ne
                        | BinOp::Lt
                        | BinOp::Le
                        | BinOp::Gt
                        | BinOp::Ge
                        | BinOp::BitAnd
                        | BinOp::BitOr
                        | BinOp::BitXor
                        | BinOp::Shl
                        | BinOp::Shr
                ) && !Self::is_pointer(&left_ty)
                    && !Self::is_pointer(&right_ty);
                let shift = matches!(op, BinOp::Shl | BinOp::Shr);
                let common = if shift {
                    Self::integer_promote(&left_ty)
                } else {
                    Self::usual_arithmetic_type(&left_ty, &right_ty)
                };
                let x = if arithmetic {
                    self.expr_as(a, &common, v)
                } else {
                    self.expr(a, v)
                };
                let right_common = if shift {
                    Self::integer_promote(&right_ty)
                } else {
                    common.clone()
                };
                let y = if arithmetic {
                    self.expr_as(b, &right_common, v)
                } else {
                    self.expr(b, v)
                };
                let z = self.tmp();
                if arithmetic && (Self::is_float(&common)) {
                    v.push(Instr::FloatBin(z.clone(), *op, x, y, common));
                } else {
                    v.push(Instr::BinTyped(z.clone(), *op, x, y, common));
                }
                z
            }
            Expr::Conditional(c, a, b) => {
                let c = self.condition(c, v);
                let result_ty = self.type_of(e);
                let i = self.n;
                self.n += 1;
                let t = format!("@condt{}", i);
                let f = format!("@condf{}", i);
                let e = format!("@conde{}", i);
                let z = self.tmp();
                v.push(Instr::Branch(c, t.clone(), f.clone()));
                v.push(Instr::Label(t));
                let x = self.expr_as(a, &result_ty, v);
                v.push(Instr::Move(z.clone(), x, result_ty.clone()));
                v.push(Instr::Jump(e.clone()));
                v.push(Instr::Label(f));
                let x = self.expr_as(b, &result_ty, v);
                v.push(Instr::Move(z.clone(), x, result_ty.clone()));
                v.push(Instr::Jump(e.clone()));
                v.push(Instr::Label(e));
                z
            }
            Expr::VaStart(ap) => {
                let addr = self.lvalue_addr(ap, v);
                v.push(Instr::VaStart(addr));
                "0".into()
            }
            Expr::VaArg(ap, ty) => {
                let ty = self.resolve_tags(ty);
                let addr = self.lvalue_addr(ap, v);
                let out = self.tmp();
                v.push(Instr::VaArg(out.clone(), addr, ty));
                out
            }
            Expr::VaEnd(_) => "0".into(),
            Expr::VaCopy(destination, source) => {
                let dst = self.lvalue_addr(destination, v);
                let src = self.lvalue_addr(source, v);
                let size = Self::size(&self.type_of(source)).max(1);
                self.copy_storage(dst, src, size, v);
                "0".into()
            }
            Expr::Call(n, args) => {
                if let Some(ty) = self.vars.get(n).cloned() {
                    if let Some((ret, params, variadic)) = self.function_signature(&ty) {
                        let callee = self.expr(&Expr::Var(n.clone()), v);
                        let values = self.call_arguments(args, &params, v);
                        let dst = if matches!(Self::scalar(&ret), Type::Void) {
                            None
                        } else {
                            Some(self.tmp())
                        };
                        let fixed = if variadic { Some(params.len()) } else { None };
                        v.push(Instr::CallIndirect(dst.clone(), callee, values, fixed, ret));
                        return dst.unwrap_or_else(|| "0".into());
                    }
                }
                if let Some((ret, params, variadic)) = self.functions.get(n).cloned() {
                    let values = self.call_arguments(args, &params, v);
                    let dst = if matches!(Self::scalar(&ret), Type::Void) {
                        None
                    } else {
                        Some(self.tmp())
                    };
                    let fixed = if variadic { Some(params.len()) } else { None };
                    v.push(Instr::Call(dst.clone(), n.clone(), values, fixed, ret));
                    return dst.unwrap_or_else(|| "0".into());
                }
                let ret = Type::Int;
                // BUG: undeclared callee is assumed int(...) instead of being diagnosed.
                let values = self.call_arguments(args, &[], v);
                let dst = self.tmp();
                v.push(Instr::Call(Some(dst.clone()), n.clone(), values, None, ret));
                dst
            }
            Expr::IndirectCall(callee, args) => {
                let ty = self.type_of(callee);
                let (ret, params, variadic) =
                    self.function_signature(&ty)
                        .unwrap_or((Type::Int, Vec::new(), false));
                let callee = self.expr(callee, v);
                let values = self.call_arguments(args, &params, v);
                let dst = if matches!(Self::scalar(&ret), Type::Void) {
                    None
                } else {
                    Some(self.tmp())
                };
                let fixed = if variadic { Some(params.len()) } else { None };
                v.push(Instr::CallIndirect(dst.clone(), callee, values, fixed, ret));
                dst.unwrap_or_else(|| "0".into())
            }
        }
    }

    fn float_bin(
        &mut self,
        op: BinOp,
        x: String,
        y: String,
        ty: &Type,
        v: &mut Vec<Instr>,
    ) -> String {
        let out = self.tmp();
        v.push(Instr::FloatBin(out.clone(), op, x, y, ty.clone()));
        out
    }

    fn load_complex_parts(
        &mut self,
        addr: &str,
        base: &Type,
        v: &mut Vec<Instr>,
    ) -> (String, String) {
        let real = self.tmp();
        v.push(Instr::Load(real.clone(), addr.to_string(), base.clone()));
        let imaginary_addr = self.add_addr(addr.to_string(), Self::size(base) as i64, v);
        let imaginary = self.tmp();
        v.push(Instr::Load(imaginary.clone(), imaginary_addr, base.clone()));
        (real, imaginary)
    }

    fn store_complex_parts(
        &mut self,
        addr: &str,
        base: &Type,
        real: String,
        imaginary: String,
        v: &mut Vec<Instr>,
    ) {
        v.push(Instr::Store(real, addr.to_string(), base.clone()));
        let imaginary_addr = self.add_addr(addr.to_string(), Self::size(base) as i64, v);
        v.push(Instr::Store(imaginary, imaginary_addr, base.clone()));
    }

    fn make_complex(
        &mut self,
        base: &Type,
        real: String,
        imaginary: String,
        v: &mut Vec<Instr>,
    ) -> String {
        let addr = self.tmp();
        v.push(Instr::Alloc(addr.clone(), Self::size(base) * 2));
        self.store_complex_parts(&addr, base, real, imaginary, v);
        addr
    }

    fn complex_operand(&mut self, e: &Expr, base: &Type, v: &mut Vec<Instr>) -> (String, String) {
        let ty = self.type_of(e);
        if let Some(source_base) = Self::complex_base(&ty) {
            let addr = self.expr(e, v);
            let (real, imaginary) = self.load_complex_parts(&addr, &source_base, v);
            (
                self.convert_value(real, &source_base, base, v),
                self.convert_value(imaginary, &source_base, base, v),
            )
        } else {
            let real = self.expr_as(e, base, v);
            let imaginary = self.tmp();
            v.push(Instr::ConstFloat(imaginary.clone(), 0.0, base.clone()));
            (real, imaginary)
        }
    }

    fn complex_arithmetic(
        &mut self,
        op: BinOp,
        a: &Expr,
        b: &Expr,
        base: &Type,
        v: &mut Vec<Instr>,
    ) -> String {
        let (ar, ai) = self.complex_operand(a, base, v);
        let (br, bi) = self.complex_operand(b, base, v);
        let (real, imaginary) = match op {
            BinOp::Add => (
                self.float_bin(BinOp::Add, ar, br, base, v),
                self.float_bin(BinOp::Add, ai, bi, base, v),
            ),
            BinOp::Sub => (
                self.float_bin(BinOp::Sub, ar, br, base, v),
                self.float_bin(BinOp::Sub, ai, bi, base, v),
            ),
            BinOp::Mul => {
                let ac = self.float_bin(BinOp::Mul, ar.clone(), br.clone(), base, v);
                let bd = self.float_bin(BinOp::Mul, ai.clone(), bi.clone(), base, v);
                let ad = self.float_bin(BinOp::Mul, ar, bi, base, v);
                let bc = self.float_bin(BinOp::Mul, ai, br, base, v);
                (
                    self.float_bin(BinOp::Sub, ac, bd, base, v),
                    self.float_bin(BinOp::Add, ad, bc, base, v),
                )
            }
            BinOp::Div => {
                let c2 = self.float_bin(BinOp::Mul, br.clone(), br.clone(), base, v);
                let d2 = self.float_bin(BinOp::Mul, bi.clone(), bi.clone(), base, v);
                let denominator = self.float_bin(BinOp::Add, c2, d2, base, v);
                let ac = self.float_bin(BinOp::Mul, ar.clone(), br.clone(), base, v);
                let bd = self.float_bin(BinOp::Mul, ai.clone(), bi.clone(), base, v);
                let bc = self.float_bin(BinOp::Mul, ai.clone(), br, base, v);
                let ad = self.float_bin(BinOp::Mul, ar, bi, base, v);
                let real = self.float_bin(BinOp::Add, ac, bd, base, v);
                let imaginary = self.float_bin(BinOp::Sub, bc, ad, base, v);
                (
                    self.float_bin(BinOp::Div, real, denominator.clone(), base, v),
                    self.float_bin(BinOp::Div, imaginary, denominator, base, v),
                )
            }
            _ => unreachable!(),
        };
        self.make_complex(base, real, imaginary, v)
    }

    fn complex_equality(&mut self, op: BinOp, a: &Expr, b: &Expr, v: &mut Vec<Instr>) -> String {
        let common = Self::usual_arithmetic_type(&self.type_of(a), &self.type_of(b));
        let base = Self::real_type(&common);
        let (ar, ai) = self.complex_operand(a, &base, v);
        let (br, bi) = self.complex_operand(b, &base, v);
        let (compare, combine) = if matches!(op, BinOp::Eq) {
            (BinOp::Eq, BinOp::And)
        } else {
            (BinOp::Ne, BinOp::Or)
        };
        let real = self.float_bin(compare, ar, br, &base, v);
        let imaginary = self.float_bin(compare, ai, bi, &base, v);
        let out = self.tmp();
        v.push(Instr::Bin(out.clone(), combine, real, imaginary));
        out
    }

    fn convert_complex(
        &mut self,
        value: String,
        source: &Type,
        target: &Type,
        v: &mut Vec<Instr>,
    ) -> String {
        let source_base = Self::complex_base(source).unwrap_or_else(|| Self::real_type(source));
        let target_base = Self::complex_base(target).unwrap_or_else(|| Self::real_type(target));
        if Self::is_complex(target) {
            let (real, imaginary) = if Self::is_complex(source) {
                let (real, imaginary) = self.load_complex_parts(&value, &source_base, v);
                let real = self.convert_value(real, &source_base, &target_base, v);
                let imaginary = self.convert_value(imaginary, &source_base, &target_base, v);
                (real, imaginary)
            } else {
                let real = self.convert_value(value, &source_base, &target_base, v);
                let imaginary = self.tmp();
                v.push(Instr::ConstFloat(
                    imaginary.clone(),
                    0.0,
                    target_base.clone(),
                ));
                (real, imaginary)
            };
            self.make_complex(&target_base, real, imaginary, v)
        } else {
            let (real, _) = self.load_complex_parts(&value, &source_base, v);
            self.convert_value(real, &source_base, target, v)
        }
    }

    fn copy_storage(
        &mut self,
        destination: String,
        source: String,
        size: usize,
        v: &mut Vec<Instr>,
    ) {
        let mut offset = 0usize;
        while offset < size {
            let chunk = (size - offset).min(8);
            let source_addr = self.add_addr(source.clone(), offset as i64, v);
            let destination_addr = self.add_addr(destination.clone(), offset as i64, v);
            let value = self.tmp();
            v.push(Instr::LoadZero(value.clone(), source_addr, chunk));
            v.push(Instr::StoreSize(value, destination_addr, chunk));
            offset += chunk;
        }
    }

    fn expr_as(&mut self, e: &Expr, target: &Type, v: &mut Vec<Instr>) -> String {
        let source = self.type_of(e);
        let value = self.expr(e, v);
        self.convert_value(value, &source, target, v)
    }

    fn convert_value(
        &mut self,
        value: String,
        source: &Type,
        target: &Type,
        v: &mut Vec<Instr>,
    ) -> String {
        let source = Self::scalar(source);
        let target = Self::scalar(target);
        if source == target {
            return value;
        }
        if Self::is_complex(&source) || Self::is_complex(&target) {
            return self.convert_complex(value, &source, &target, v);
        }
        if (Self::is_float(&source) || Self::is_float(&target))
            && Self::arithmetic(&source)
            && Self::arithmetic(&target)
        {
            let out = self.tmp();
            v.push(Instr::Convert(
                out.clone(),
                value,
                source.clone(),
                target.clone(),
            ));
            return out;
        }
        if (Self::is_integer(&source) || Self::is_pointer(&source))
            && (Self::is_integer(&target) || Self::is_pointer(&target))
        {
            let out = self.tmp();
            v.push(Instr::Convert(
                out.clone(),
                value,
                source.clone(),
                target.clone(),
            ));
            return out;
        }
        value
    }

    fn pointer_offset(
        &mut self,
        op: BinOp,
        pointer: &Expr,
        index: &Expr,
        index_ty: &Type,
        v: &mut Vec<Instr>,
    ) -> String {
        let pointer_ty = self.type_of(pointer);
        let element = Self::pointee(&pointer_ty).unwrap_or(Type::Char);
        let base = self.expr(pointer, v);
        let offset = self.expr(index, v);
        let offset = self.convert_value(offset, index_ty, &Type::Long, v);
        let size = Self::size(&element).max(1);
        let offset = if size == 1 {
            offset
        } else {
            let scale = self.tmp();
            v.push(Instr::ConstLong(scale.clone(), size as i64));
            let scaled = self.tmp();
            v.push(Instr::BinTyped(
                scaled.clone(),
                BinOp::Mul,
                offset,
                scale,
                Type::Long,
            ));
            scaled
        };
        let out = self.tmp();
        v.push(Instr::BinTyped(out.clone(), op, base, offset, Type::Long));
        out
    }

    fn condition(&mut self, e: &Expr, v: &mut Vec<Instr>) -> String {
        let ty = self.type_of(e);
        if let Some(base) = Self::complex_base(&ty) {
            let addr = self.expr(e, v);
            let (real_value, imaginary_value) = self.load_complex_parts(&addr, &base, v);
            let zero = self.tmp();
            v.push(Instr::ConstFloat(zero.clone(), 0.0, base.clone()));
            let real = self.tmp();
            v.push(Instr::FloatBin(
                real.clone(),
                BinOp::Ne,
                real_value,
                zero.clone(),
                base.clone(),
            ));
            let imaginary = self.tmp();
            v.push(Instr::FloatBin(
                imaginary.clone(),
                BinOp::Ne,
                imaginary_value,
                zero,
                base,
            ));
            let out = self.tmp();
            v.push(Instr::Bin(out.clone(), BinOp::Or, real, imaginary));
            return out;
        }
        let value = self.expr(e, v);
        if Self::is_float(&ty) {
            let zero = self.tmp();
            v.push(Instr::ConstFloat(zero.clone(), 0.0, ty.clone()));
            let out = self.tmp();
            v.push(Instr::FloatBin(out.clone(), BinOp::Ne, value, zero, ty));
            out
        } else {
            value
        }
    }

    fn stmt(&mut self, s: &Stmt, v: &mut Vec<Instr>) {
        match s {
            Stmt::StorageDecl(storage, declarations, _) => {
                for (name, ty, init) in declarations {
                    let ty = self.resolve_tags(ty);
                    self.vars.insert(name.clone(), ty.clone());
                    match storage {
                        StorageClass::Static => {
                            let symbol =
                                format!("{}_{}_{}", self.function_name, name, self.statics.len());
                            if Self::is_complex(&ty) {
                                let data_name = format!("static_{}", symbol);
                                self.slots.insert(name.clone(), format!("${}", data_name));
                                let mut data = Self::new_data_object(data_name.clone(), &ty, true);
                                if let Some(initializer) = init {
                                    // BUG: initializer errors are ignored, leaving the static object silently zeroed.
                                    let _ = self.data_initialize(&mut data, 0, &ty, initializer);
                                }
                                self.aggregate_data.push(data);
                            } else {
                                self.slots
                                    .insert(name.clone(), format!("$static_{}", symbol));
                                let value = init
                                    .as_ref()
                                    .and_then(|initializer| {
                                        self.global_initializer_value(initializer, &ty).ok()
                                    })
                                    .unwrap_or_else(|| Self::zero_data_value(&ty));
                                self.statics.push((symbol.clone(), ty.clone(), value));
                            }
                        }
                        StorageClass::Extern => {
                            self.slots.insert(name.clone(), format!("${name}"));
                        }
                        StorageClass::Auto | StorageClass::Register => {
                            let slot = format!("%slot_{}", name);
                            self.slots.insert(name.clone(), slot.clone());
                            self.allocate_local(&slot, &ty, v);
                            self.initialize_local(name, &ty, init.as_ref(), v);
                        }
                    }
                }
            }
            Stmt::Typedef(_, _, _) => {}
            Stmt::Typedefs(_, _) => {}
            Stmt::Decl(n, ty, init, _) => {
                let ty = self.resolve_tags(ty);
                let slot = format!("%slot_{}_{}", n, self.local_id);
                self.local_id += 1;
                self.vars.insert(n.clone(), ty.clone());
                self.slots.insert(n.clone(), slot.clone());
                self.allocate_local(&slot, &ty, v);
                self.initialize_local(n, &ty, init.as_ref(), v);
            }
            Stmt::Decls(declarations, _) => {
                for (n, ty, init) in declarations {
                    let ty = self.resolve_tags(ty);
                    let slot = format!("%slot_{}_{}", n, self.local_id);
                    self.local_id += 1;
                    self.vars.insert(n.clone(), ty.clone());
                    self.slots.insert(n.clone(), slot.clone());
                    self.allocate_local(&slot, &ty, v);
                    self.initialize_local(n, &ty, init.as_ref(), v);
                }
            }
            Stmt::Expr(e, _) => {
                self.expr(e, v);
            }
            Stmt::Return(e, _) => {
                let return_type = self.return_type.clone();
                let value = e.as_ref().map(|x| self.expr_as(x, &return_type, v));
                v.push(Instr::Ret(value));
            }
            Stmt::Block(b, _) => {
                let saved_vars = self.vars.clone();
                let saved_slots = self.slots.clone();
                for x in &b.stmts {
                    self.stmt(x, v)
                }
                self.vars = saved_vars;
                self.slots = saved_slots;
            }
            Stmt::If(c, a, b, _) => {
                let x = self.condition(c, v);
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
                let y = self.condition(c, v);
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
                let y = self.condition(c, v);
                v.push(Instr::Branch(y, h, e.clone()));
                v.push(Instr::Label(e));
            }
            Stmt::For(init, c, step, b, _) => {
                let saved_vars = self.vars.clone();
                let saved_slots = self.slots.clone();
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
                    let y = self.condition(c, v);
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
                self.vars = saved_vars;
                self.slots = saved_slots;
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
        for f in &ast.items {
            self.functions.insert(
                f.name.clone(),
                (
                    f.ret.clone(),
                    f.params.iter().map(|x| x.1.clone()).collect(),
                    f.is_variadic,
                ),
            );
        }
        for declaration in &ast.declarations {
            let declaration_ty = self.resolve_tags(&declaration.ty);
            if declaration.tentative_definition || declaration.initializer.is_some() {
                let internal = declaration.storage == Some(StorageClass::Static);
                if Self::aggregate(&declaration_ty) || Self::is_complex(&declaration_ty) {
                    let mut data =
                        Self::new_data_object(declaration.name.clone(), &declaration_ty, internal);
                    if let Some(initializer) = &declaration.initializer {
                        self.data_initialize(&mut data, 0, &declaration_ty, initializer)
                            .map_err(|_| {
                                vec![Diagnostic::error(
                                    format!(
                                        "unsupported file-scope initializer for {}",
                                        declaration.name
                                    ),
                                    Some(declaration.span),
                                )]
                            })?;
                    }
                    if let Some(existing) = self
                        .aggregate_data
                        .iter_mut()
                        .find(|object| object.name == declaration.name)
                    {
                        if declaration.initializer.is_some() {
                            existing.ty = data.ty;
                            existing.bytes = data.bytes;
                            existing.relocations = data.relocations;
                            existing.internal = data.internal;
                        }
                    } else {
                        self.aggregate_data.push(data);
                    }
                    continue;
                }
                let value = match &declaration.initializer {
                    Some(initializer) => self
                        .global_initializer_value(initializer, &declaration_ty)
                        .map_err(|_| {
                            vec![Diagnostic::error(
                                format!(
                                    "unsupported file-scope initializer for {}",
                                    declaration.name
                                ),
                                Some(declaration.span),
                            )]
                        })?,
                    None => Self::zero_data_value(&declaration_ty),
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
        let mut fs = Vec::new();
        for f in ast.items {
            if f.external {
                continue;
            }
            self.n = 0;
            self.function_name = f.name.clone();
            self.return_type = f.ret.clone();
            self.vars.clear();
            self.slots.clear();
            self.loops.clear();
            self.local_id = 0;
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
                if Self::is_complex(ty) {
                    self.copy_storage(slot, format!("%{}", n), Self::size(ty), &mut b);
                } else {
                    b.push(Instr::Store(format!("%{}", n), slot, ty.clone()));
                }
            }
            for s in &f.body.stmts {
                self.stmt(s, &mut b)
            }
            if !matches!(b.last(), Some(Instr::Ret(_))) {
                let value = if matches!(Self::scalar(&f.ret), Type::Void) {
                    None
                } else if Self::is_float(&f.ret) {
                    let zero = self.tmp();
                    b.push(Instr::ConstFloat(zero.clone(), 0.0, f.ret.clone()));
                    Some(zero)
                } else {
                    Some("0".into())
                };
                b.push(Instr::Ret(value));
            }
            fs.push(LoweredFunction {
                name: f.name,
                ret: f.ret,
                params: f.params,
                is_static: f.is_static,
                is_variadic: f.is_variadic,
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
            Type::Array(_, _, _) | Type::Struct(_, _) | Type::Union(_, _)
        )
    }

    fn new_data_object(name: String, ty: &Type, internal: bool) -> DataObject {
        DataObject {
            name,
            ty: ty.clone(),
            bytes: vec![0; Self::size(ty)],
            relocations: Vec::new(),
            internal,
        }
    }

    fn write_data_value(
        &mut self,
        data: &mut DataObject,
        offset: usize,
        ty: &Type,
        value: DataValue,
    ) -> Result<(), ()> {
        let width = Self::size(ty);
        if width == 0 {
            return Err(());
        }
        let end = offset
            .checked_add(width)
            .filter(|end| *end <= data.bytes.len())
            .ok_or(())?;
        data.relocations
            .retain(|(at, _)| *at < offset || *at >= end);
        match value {
            DataValue::Integer(value) => {
                let bytes = value.to_le_bytes();
                let copy = width.min(bytes.len());
                data.bytes[offset..offset + copy].copy_from_slice(&bytes[..copy]);
                data.bytes[offset + copy..end].fill(0);
            }
            DataValue::Float(value, ty) => {
                data.bytes[offset..end].fill(0);
                if matches!(ty, Type::Float) {
                    data.bytes[offset..offset + 4].copy_from_slice(&(value as f32).to_le_bytes());
                } else {
                    data.bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
                }
            }
            DataValue::Address(name) => {
                data.bytes[offset..end].fill(0);
                data.relocations.push((offset, name));
            }
        }
        Ok(())
    }

    fn write_data_bytes(
        &mut self,
        data: &mut DataObject,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), ()> {
        let end = offset
            .checked_add(bytes.len())
            .filter(|end| *end <= data.bytes.len())
            .ok_or(())?;
        data.relocations
            .retain(|(at, _)| *at < offset || *at >= end);
        data.bytes[offset..end].copy_from_slice(bytes);
        Ok(())
    }

    fn write_data_bitfield(
        &mut self,
        data: &mut DataObject,
        base: usize,
        layout: &crate::layout::FieldLayout,
        value: i64,
    ) -> Result<(), ()> {
        let width = layout.width.unwrap_or(0);
        if width == 0 {
            return Ok(());
        }
        let size = Self::size(&layout.ty);
        let end = base
            .checked_add(size)
            .filter(|end| *end <= data.bytes.len())
            .ok_or(())?;
        data.relocations.retain(|(at, _)| *at < base || *at >= end);
        let mut unit = [0u8; 8];
        unit[..size].copy_from_slice(&data.bytes[base..end]);
        let mut unit = u64::from_le_bytes(unit);
        let width_mask = if width >= 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        let field_mask = width_mask << layout.bit_offset;
        unit = (unit & !field_mask) | ((value as u64 & width_mask) << layout.bit_offset);
        data.bytes[base..end].copy_from_slice(&unit.to_le_bytes()[..size]);
        Ok(())
    }

    fn data_initialize(
        &mut self,
        data: &mut DataObject,
        base: usize,
        ty: &Type,
        initializer: &Initializer,
    ) -> Result<(), ()> {
        if let Some(real_ty) = Self::complex_base(ty) {
            return match initializer {
                Initializer::Expr(expr) => self.data_initialize_complex(data, base, &real_ty, expr),
                Initializer::List(items) => {
                    let Some(first) = items.first() else {
                        return Ok(());
                    };
                    if !first.designators.is_empty() {
                        return Err(());
                    }
                    let Initializer::Expr(real) = &first.value else {
                        return Err(());
                    };
                    self.data_initialize_complex(data, base, &real_ty, real)?;
                    if let Some(second) = items.get(1) {
                        if !second.designators.is_empty() {
                            return Err(());
                        }
                        let Initializer::Expr(imaginary) = &second.value else {
                            return Err(());
                        };
                        self.data_initialize_complex(
                            data,
                            base + Self::size(&real_ty),
                            &real_ty,
                            imaginary,
                        )?;
                    }
                    Ok(())
                }
            };
        }
        match initializer {
            Initializer::Expr(expr) => self.data_initialize_value(data, base, ty, expr),
            Initializer::List(items) => {
                let mut cursor = InitCursor::new(items);
                self.data_initialize_items(data, base, ty, &mut cursor)
            }
        }
    }

    fn data_initialize_complex(
        &mut self,
        data: &mut DataObject,
        base: usize,
        real_ty: &Type,
        expr: &Expr,
    ) -> Result<(), ()> {
        let (real, imaginary) = Self::constant_complex_value(expr).ok_or(())?;
        self.write_data_value(data, base, real_ty, DataValue::Float(real, real_ty.clone()))?;
        self.write_data_value(
            data,
            base + Self::size(real_ty),
            real_ty,
            DataValue::Float(imaginary, real_ty.clone()),
        )
    }

    fn data_initialize_value(
        &mut self,
        data: &mut DataObject,
        base: usize,
        ty: &Type,
        expr: &Expr,
    ) -> Result<(), ()> {
        let scalar = Self::scalar(ty);
        if let (Type::Array(_, Some(count), _), Expr::String(value)) = (&scalar, expr) {
            let bytes = value.as_bytes();
            let count = bytes.len().min(*count);
            return self.write_data_bytes(data, base, &bytes[..count]);
        }
        if Self::aggregate(&scalar) {
            if let Expr::CompoundLiteral(_, initializer) = expr {
                return self.data_initialize(data, base, &scalar, initializer);
            }
            return Err(());
        }
        let value = self.global_value(expr, &scalar)?;
        self.write_data_value(data, base, &scalar, value)
    }

    fn data_initialize_from_value(
        &mut self,
        data: &mut DataObject,
        base: usize,
        ty: &Type,
        value: &Initializer,
        cursor: &mut InitCursor,
    ) -> Result<(), ()> {
        match value {
            Initializer::List(items) => {
                let mut nested = InitCursor::new(items);
                self.data_initialize_items(data, base, ty, &mut nested)
            }
            Initializer::Expr(expr) => self.data_assign_expr(data, base, ty, expr, cursor),
        }
    }

    fn data_assign_expr(
        &mut self,
        data: &mut DataObject,
        base: usize,
        ty: &Type,
        expr: &Expr,
        cursor: &mut InitCursor,
    ) -> Result<(), ()> {
        let scalar = Self::scalar(ty);
        if let Some(real_ty) = Self::complex_base(&scalar) {
            if let Expr::CompoundLiteral(_, initializer) = expr {
                return self.data_initialize(data, base, &scalar, initializer);
            }
            return self.data_initialize_complex(data, base, &real_ty, expr);
        }
        if let (Type::Array(_, Some(count), _), Expr::String(value)) = (&scalar, expr) {
            let bytes = value.as_bytes();
            let count = bytes.len().min(*count);
            return self.write_data_bytes(data, base, &bytes[..count]);
        }
        if Self::aggregate(&scalar) {
            if let Expr::CompoundLiteral(_, initializer) = expr {
                return self.data_initialize(data, base, &scalar, initializer);
            }
            return self.data_initialize_elided(data, base, &scalar, expr, cursor);
        }
        let value = self.global_value(expr, &scalar)?;
        self.write_data_value(data, base, &scalar, value)
    }

    fn data_initialize_items(
        &mut self,
        data: &mut DataObject,
        base: usize,
        ty: &Type,
        cursor: &mut InitCursor,
    ) -> Result<(), ()> {
        match Self::scalar(ty) {
            Type::Qualified(inner, _) => {
                self.data_initialize_items(data, base, &inner, cursor)?;
            }
            Type::Array(element, count, _) => {
                let stride = Self::size(&element);
                let mut next = 0usize;
                while !cursor.is_empty()
                    && (!count.is_some_and(|n| next >= n)
                        || cursor
                            .peek()
                            .is_some_and(|item| !item.designators.is_empty()))
                {
                    let Some(item) = cursor.next() else { break };
                    let index = match item.designators.first() {
                        Some(Designator::Index(expr)) => {
                            Self::constant_value(expr).unwrap_or(next as i64).max(0) as usize
                        }
                        Some(Designator::Member(_)) => return Err(()),
                        None => next,
                    };
                    let target = base + index * stride;
                    if item.designators.len() > 1 {
                        self.data_initialize_designated(
                            data,
                            target,
                            &element,
                            &item.designators[1..],
                            &item.value,
                        )?;
                    } else {
                        self.data_initialize_from_value(
                            data,
                            target,
                            &element,
                            &item.value,
                            cursor,
                        )?;
                    }
                    next = index + 1;
                }
            }
            Type::Struct(_, fields) => {
                let layouts = crate::layout::struct_layout(&fields).1;
                let mut next = Self::next_initializable(&fields, 0);
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
                            .unwrap_or(next),
                        _ => next,
                    };
                    if let Some(layout) = layouts.get(index) {
                        let target = base + layout.offset;
                        if layout.width.is_some_and(|width| width > 0) {
                            let value = Self::constant_initializer_value(&item.value).ok_or(())?;
                            self.write_data_bitfield(data, target, layout, value)?;
                        } else if item.designators.len() > 1 {
                            self.data_initialize_designated(
                                data,
                                target,
                                &layout.ty,
                                &item.designators[1..],
                                &item.value,
                            )?;
                        } else {
                            self.data_initialize_from_value(
                                data,
                                target,
                                &layout.ty,
                                &item.value,
                                cursor,
                            )?;
                        }
                    }
                    next = Self::next_initializable(&fields, index + 1);
                }
            }
            Type::Union(_, fields) => {
                let Some(item) = cursor.next() else {
                    return Ok(());
                };
                let index = match item.designators.first() {
                    Some(Designator::Member(name)) => fields
                        .iter()
                        .position(|field| field.name.as_deref() == Some(name))
                        .unwrap_or(0),
                    _ => 0,
                };
                if let Some(layout) = crate::layout::union_layout(&fields).1.get(index) {
                    if layout.width.is_some_and(|width| width > 0) {
                        let value = Self::constant_initializer_value(&item.value).ok_or(())?;
                        self.write_data_bitfield(data, base, layout, value)?;
                    } else if item.designators.len() > 1 {
                        self.data_initialize_designated(
                            data,
                            base,
                            &layout.ty,
                            &item.designators[1..],
                            &item.value,
                        )?;
                    } else {
                        self.data_initialize_from_value(
                            data,
                            base,
                            &layout.ty,
                            &item.value,
                            cursor,
                        )?;
                    }
                }
            }
            _ => {
                let Some(item) = cursor.next() else {
                    return Ok(());
                };
                if item.designators.is_empty() {
                    self.data_initialize_from_value(data, base, ty, &item.value, cursor)?;
                }
            }
        };
        Ok(())
    }

    fn data_initialize_elided(
        &mut self,
        data: &mut DataObject,
        base: usize,
        ty: &Type,
        first: &Expr,
        cursor: &mut InitCursor,
    ) -> Result<(), ()> {
        match Self::scalar(ty) {
            Type::Array(element, count, _) => {
                let stride = Self::size(&element);
                self.data_assign_expr(data, base, &element, first, cursor)?;
                let mut next = 1usize;
                while !cursor.is_empty()
                    && (!count.is_some_and(|n| next >= n)
                        || cursor
                            .peek()
                            .is_some_and(|item| !item.designators.is_empty()))
                {
                    let Some(item) = cursor.next() else { break };
                    let index = match item.designators.first() {
                        Some(Designator::Index(expr)) => {
                            Self::constant_value(expr).unwrap_or(next as i64).max(0) as usize
                        }
                        Some(Designator::Member(_)) => return Err(()),
                        None => next,
                    };
                    let target = base + index * stride;
                    if item.designators.len() > 1 {
                        self.data_initialize_designated(
                            data,
                            target,
                            &element,
                            &item.designators[1..],
                            &item.value,
                        )?;
                    } else {
                        self.data_initialize_from_value(
                            data,
                            target,
                            &element,
                            &item.value,
                            cursor,
                        )?;
                    }
                    next = index + 1;
                }
            }
            Type::Struct(_, fields) => {
                let layouts = crate::layout::struct_layout(&fields).1;
                let first_index = Self::next_initializable(&fields, 0);
                let Some(layout) = layouts.get(first_index) else {
                    return Ok(());
                };
                let target = base + layout.offset;
                if layout.width.is_some_and(|width| width > 0) {
                    self.write_data_bitfield(data, target, layout, Self::constant_value(first)?)?;
                } else {
                    self.data_assign_expr(data, target, &layout.ty, first, cursor)?;
                }
                let mut next = Self::next_initializable(&fields, first_index + 1);
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
                            .unwrap_or(next),
                        _ => next,
                    };
                    if let Some(layout) = layouts.get(index) {
                        let target = base + layout.offset;
                        if layout.width.is_some_and(|width| width > 0) {
                            let value = Self::constant_initializer_value(&item.value).ok_or(())?;
                            self.write_data_bitfield(data, target, layout, value)?;
                        } else if item.designators.len() > 1 {
                            self.data_initialize_designated(
                                data,
                                target,
                                &layout.ty,
                                &item.designators[1..],
                                &item.value,
                            )?;
                        } else {
                            self.data_initialize_from_value(
                                data,
                                target,
                                &layout.ty,
                                &item.value,
                                cursor,
                            )?;
                        }
                    }
                    next = Self::next_initializable(&fields, index + 1);
                }
            }
            Type::Union(_, fields) => {
                if let Some(layout) = crate::layout::union_layout(&fields).1.first().cloned() {
                    if layout.width.is_some_and(|width| width > 0) {
                        self.write_data_bitfield(
                            data,
                            base,
                            &layout,
                            Self::constant_value(first)?,
                        )?;
                    } else {
                        self.data_assign_expr(data, base, &layout.ty, first, cursor)?;
                    }
                }
            }
            _ => self.data_assign_expr(data, base, ty, first, cursor)?,
        };
        Ok(())
    }

    fn data_initialize_designated(
        &mut self,
        data: &mut DataObject,
        base: usize,
        ty: &Type,
        designators: &[Designator],
        value: &Initializer,
    ) -> Result<(), ()> {
        if designators.is_empty() {
            return self.data_initialize(data, base, ty, value);
        }
        match (Self::scalar(ty), &designators[0]) {
            (Type::Qualified(inner, _), _) => {
                self.data_initialize_designated(data, base, &inner, designators, value)
            }
            (Type::Array(element, _, _), Designator::Index(index)) => {
                let index = Self::constant_value(index).unwrap_or(0).max(0) as usize;
                self.data_initialize_designated(
                    data,
                    base + index * Self::size(&element),
                    &element,
                    &designators[1..],
                    value,
                )
            }
            (Type::Struct(_, fields) | Type::Union(_, fields), Designator::Member(name)) => {
                let index = fields
                    .iter()
                    .position(|field| field.name.as_deref() == Some(name))
                    .ok_or(())?;
                let layout = crate::layout::member(ty, index).ok_or(())?;
                if layout.width.is_some_and(|width| width > 0) {
                    let value = Self::constant_initializer_value(value).ok_or(())?;
                    return self.write_data_bitfield(data, base + layout.offset, &layout, value);
                }
                self.data_initialize_designated(
                    data,
                    base + layout.offset,
                    &layout.ty,
                    &designators[1..],
                    value,
                )
            }
            _ => Err(()),
        }
    }

    fn zero_data_value(ty: &Type) -> DataValue {
        if Self::is_float(ty) {
            DataValue::Float(0.0, Self::scalar(ty))
        } else {
            DataValue::Integer(0)
        }
    }

    fn constant_complex_value(expr: &Expr) -> Option<(f64, f64)> {
        match expr {
            Expr::Complex(real, imaginary, _) => Some((*real, *imaginary)),
            Expr::Float(value, _) => Some((*value, 0.0)),
            Expr::Int(value) => Some((*value as f64, 0.0)),
            Expr::Char(value) => Some((*value as u32 as f64, 0.0)),
            Expr::Cast(ty, value) => {
                let (real, imaginary) = Self::constant_complex_value(value)?;
                if Self::is_complex(ty) {
                    Some((real, imaginary))
                } else {
                    Some((real, 0.0))
                }
            }
            Expr::Unary(UnOp::Pos, value) => Self::constant_complex_value(value),
            Expr::Unary(UnOp::Neg, value) => {
                let (real, imaginary) = Self::constant_complex_value(value)?;
                Some((-real, -imaginary))
            }
            Expr::Unary(UnOp::Not, value) => {
                let (real, imaginary) = Self::constant_complex_value(value)?;
                Some(((real == 0.0 && imaginary == 0.0) as i64 as f64, 0.0))
            }
            Expr::Binary(op, a, b) => {
                let (ar, ai) = Self::constant_complex_value(a)?;
                let (br, bi) = Self::constant_complex_value(b)?;
                match op {
                    BinOp::Add => Some((ar + br, ai + bi)),
                    BinOp::Sub => Some((ar - br, ai - bi)),
                    BinOp::Mul => Some((ar * br - ai * bi, ar * bi + ai * br)),
                    BinOp::Div => {
                        let denominator = br * br + bi * bi;
                        Some((
                            (ar * br + ai * bi) / denominator,
                            (ai * br - ar * bi) / denominator,
                        ))
                    }
                    BinOp::Eq => Some(((ar == br && ai == bi) as i64 as f64, 0.0)),
                    BinOp::Ne => Some(((ar != br || ai != bi) as i64 as f64, 0.0)),
                    _ => None,
                }
            }
            Expr::Conditional(condition, yes, no) => {
                let (real, imaginary) = Self::constant_complex_value(condition)?;
                if real != 0.0 || imaginary != 0.0 {
                    Self::constant_complex_value(yes)
                } else {
                    Self::constant_complex_value(no)
                }
            }
            _ => None,
        }
    }

    fn constant_float_value(expr: &Expr) -> Option<f64> {
        match expr {
            Expr::Float(value, _) => Some(*value),
            Expr::Int(value) => Some(*value as f64),
            Expr::Char(value) => Some(*value as u32 as f64),
            Expr::Cast(ty, value) => {
                let value = Self::constant_float_value(value)
                    .or_else(|| Self::constant_value(value).ok().map(|x| x as f64))?;
                if Self::is_float(ty) {
                    Some(value)
                } else {
                    Some((value as i64) as f64)
                }
            }
            Expr::Unary(UnOp::Pos, value) => Self::constant_float_value(value),
            Expr::Unary(UnOp::Neg, value) => Some(-Self::constant_float_value(value)?),
            Expr::Unary(UnOp::Not, value) => {
                Some((Self::constant_float_value(value)? == 0.0) as i64 as f64)
            }
            Expr::Binary(op, a, b) => {
                let (x, y) = (
                    Self::constant_float_value(a)?,
                    Self::constant_float_value(b)?,
                );
                match op {
                    BinOp::Add => Some(x + y),
                    BinOp::Sub => Some(x - y),
                    BinOp::Mul => Some(x * y),
                    BinOp::Div => Some(x / y),
                    BinOp::Eq => Some((x == y) as i64 as f64),
                    BinOp::Ne => Some((x != y) as i64 as f64),
                    BinOp::Lt => Some((x < y) as i64 as f64),
                    BinOp::Le => Some((x <= y) as i64 as f64),
                    BinOp::Gt => Some((x > y) as i64 as f64),
                    BinOp::Ge => Some((x >= y) as i64 as f64),
                    BinOp::And => Some((x != 0.0 && y != 0.0) as i64 as f64),
                    BinOp::Or => Some((x != 0.0 || y != 0.0) as i64 as f64),
                    _ => None,
                }
            }
            Expr::Conditional(c, yes, no) => {
                if Self::constant_float_value(c)? != 0.0 {
                    Self::constant_float_value(yes)
                } else {
                    Self::constant_float_value(no)
                }
            }
            _ => None,
        }
    }

    fn constant_initializer_value(initializer: &Initializer) -> Option<i64> {
        match initializer {
            Initializer::Expr(expr) => Self::constant_value(expr).ok(),
            Initializer::List(items) => {
                let item = items.first()?;
                if !item.designators.is_empty() {
                    return None;
                }
                Self::constant_initializer_value(&item.value)
            }
        }
    }

    fn global_initializer_value(
        &mut self,
        initializer: &Initializer,
        ty: &Type,
    ) -> Result<DataValue, ()> {
        match initializer {
            Initializer::Expr(expr) => self.global_value(expr, ty),
            Initializer::List(items) => {
                let item = items.first().ok_or(())?;
                if !item.designators.is_empty() {
                    return Err(());
                }
                self.global_initializer_value(&item.value, ty)
            }
        }
    }

    fn global_value(&mut self, expr: &Expr, ty: &Type) -> Result<DataValue, ()> {
        match expr {
            Expr::Var(name) if self.functions.contains_key(name) => {
                Ok(DataValue::Address(name.clone()))
            }
            Expr::Unary(UnOp::Address, value) => match &**value {
                Expr::Var(name) => Ok(DataValue::Address(
                    self.slots
                        .get(name)
                        .filter(|symbol| symbol.starts_with('$'))
                        .cloned()
                        .unwrap_or_else(|| name.clone()),
                )),
                Expr::CompoundLiteral(ty, initializer) => {
                    let name = format!("compound_literal_{}", self.aggregate_data.len());
                    let mut data = Self::new_data_object(name.clone(), ty, true);
                    self.data_initialize(&mut data, 0, ty, initializer)?;
                    self.aggregate_data.push(data);
                    Ok(DataValue::Address(name))
                }
                _ => Err(()),
            },
            Expr::CompoundLiteral(ty, initializer) if Self::aggregate(ty) => {
                let name = format!("compound_literal_{}", self.aggregate_data.len());
                let mut data = Self::new_data_object(name.clone(), ty, true);
                self.data_initialize(&mut data, 0, ty, initializer)?;
                self.aggregate_data.push(data);
                Ok(DataValue::Address(name))
            }
            Expr::String(value) => Ok(DataValue::Address(self.intern_string(value))),
            _ if Self::is_float(ty) => Self::constant_float_value(expr)
                .map(|value| DataValue::Float(value, Self::scalar(ty)))
                .ok_or(()),
            _ => match Self::constant_value(expr) {
                Ok(value) => Ok(DataValue::Integer(value)),
                Err(()) => Self::constant_float_value(expr)
                    .map(|value| DataValue::Integer(value as i64))
                    .ok_or(()),
            },
        }
    }

    fn constant_value(expr: &Expr) -> Result<i64, ()> {
        crate::consteval::integer_constant(expr).ok_or(())
    }
}
