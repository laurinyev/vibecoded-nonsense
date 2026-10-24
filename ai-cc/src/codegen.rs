use crate::{ast::Type, diag::Diagnostic, lower::*};

pub struct QbeCodegen;

impl Default for QbeCodegen {
    fn default() -> Self {
        Self::new()
    }
}

impl QbeCodegen {
    pub fn new() -> Self {
        Self
    }

    fn ty(t: &Type) -> &'static str {
        match t {
            Type::Pointer(_) | Type::Array(_, _) => "l",
            Type::Qualified(x, _) => Self::ty(x),
            _ => "w",
        }
    }

    fn value(x: &str) -> &str {
        if x.starts_with("$str") {
            "l"
        } else {
            "w"
        }
    }

    pub fn emit_program(&mut self, p: &LoweredProgram) -> Result<String, Vec<Diagnostic>> {
        let mut o = String::new();
        for (name, ty, value, internal) in &p.globals {
            let mut base_ty = ty;
            while let Type::Qualified(inner, _) = base_ty {
                base_ty = inner;
            }
            let (directive, alignment) = match base_ty {
                Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool => ("b", 1),
                Type::Short | Type::UnsignedShort => ("h", 2),
                Type::Long
                | Type::UnsignedLong
                | Type::LongLong
                | Type::UnsignedLongLong
                | Type::Pointer(_) => ("l", 8),
                _ => ("w", 4),
            };
            let value = Self::data_value(value, directive);
            let linkage = if *internal { "data" } else { "export data" };
            o += &format!(
                "{} ${} = align {} {{ {} {} }}\n",
                linkage, name, alignment, directive, value
            );
        }
        for data in &p.aggregate_data {
            let alignment = Self::alignment(&data.ty);
            let values = data
                .values
                .iter()
                .map(|(ty, value)| {
                    let directive = Self::data_directive(ty);
                    format!("{} {}", directive, Self::data_value(value, directive))
                })
                .collect::<Vec<_>>()
                .join(", ");
            let linkage = if data.internal { "data" } else { "export data" };
            o += &format!(
                "{} ${} = align {} {{ {} }}\n",
                linkage, data.name, alignment, values
            );
        }
        for (name, ty, value) in &p.statics {
            let mut base_ty = ty;
            while let Type::Qualified(inner, _) = base_ty {
                base_ty = inner;
            }
            let alignment = match base_ty {
                Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool => 1,
                Type::Short | Type::UnsignedShort => 2,
                Type::Long
                | Type::UnsignedLong
                | Type::LongLong
                | Type::UnsignedLongLong
                | Type::Pointer(_) => 8,
                _ => 4,
            };
            let (directive, value) = match base_ty {
                Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool => {
                    ("b", *value as u8 as i64)
                }
                Type::Short | Type::UnsignedShort => ("h", *value as u16 as i64),
                Type::Long
                | Type::UnsignedLong
                | Type::LongLong
                | Type::UnsignedLongLong
                | Type::Pointer(_) => ("l", *value),
                _ => ("w", *value as i32 as i64),
            };
            o += &format!(
                "data $static_{} = align {} {{ {} {} }}\n",
                name, alignment, directive, value
            );
        }
        for (name, value) in &p.strings {
            o += &format!("data {} = {{ b \"{}\", b 0 }}\n", name, qbe_string(value));
        }
        if !p.strings.is_empty() {
            o.push('\n');
        }
        for f in &p.functions {
            let params = f
                .params
                .iter()
                .map(|(x, t)| format!("{} %{}", Self::ty(t), x))
                .collect::<Vec<_>>()
                .join(", ");
            let ret = if matches!(f.ret, Type::Void) {
                String::new()
            } else {
                format!("{} ", Self::ty(&f.ret))
            };
            let linkage = if f.is_static {
                "function"
            } else {
                "export function"
            };
            o += &format!("{} {}${}({}) {{\n@start\n", linkage, ret, f.name, params);
            let mut terminated = false;
            for x in &f.body {
                match x {
                    Instr::Alloc(n, size) => o += &format!("{} =l alloc4 {}\n", n, size),
                    Instr::Const(n, v) => o += &format!("{} =w copy {}\n", n, v),
                    Instr::Move(n, x) => o += &format!("{} ={} copy {}\n", n, Self::value(x), x),
                    Instr::Load(n, addr, ty) => {
                        let op = if matches!(
                            ty,
                            Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool
                        ) {
                            "loadub"
                        } else if Self::ty(ty) == "l" {
                            "loadl"
                        } else {
                            "loadw"
                        };
                        o += &format!("{} ={} {} {}\n", n, Self::ty(ty), op, addr);
                    }
                    Instr::Store(value, addr, ty) => {
                        let op = if matches!(
                            ty,
                            Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool
                        ) {
                            "storeb"
                        } else if Self::ty(ty) == "l" {
                            "storel"
                        } else {
                            "storew"
                        };
                        o += &format!("{} {}, {}\n", op, value, addr);
                    }
                    Instr::Addr(n, base, offset) => {
                        o += &format!("{} =l add {}, {}\n", n, base, offset);
                    }
                    Instr::PtrAdd(n, base, offset) => {
                        let wide = format!("{}w", n.trim_start_matches('%'));
                        o += &format!(
                            "%{} =l extsw {}\n{} =l add {}, %{}\n",
                            wide, offset, n, base, wide
                        );
                    }
                    Instr::Bin(n, op, a, b) => {
                        o += &format!(
                            "{} =w {} {}, {}\n",
                            n,
                            match op {
                                crate::ast::BinOp::Add => "add",
                                crate::ast::BinOp::Sub => "sub",
                                crate::ast::BinOp::Mul => "mul",
                                crate::ast::BinOp::Div => "div",
                                crate::ast::BinOp::Rem => "rem",
                                crate::ast::BinOp::Eq => "ceqw",
                                crate::ast::BinOp::Ne => "cnew",
                                crate::ast::BinOp::Lt => "csltw",
                                crate::ast::BinOp::Le => "cslew",
                                crate::ast::BinOp::Gt => "csgtw",
                                crate::ast::BinOp::Ge => "csgew",
                                crate::ast::BinOp::And | crate::ast::BinOp::BitAnd => "and",
                                crate::ast::BinOp::Or | crate::ast::BinOp::BitOr => "or",
                                crate::ast::BinOp::BitXor => "xor",
                                crate::ast::BinOp::Shl => "shl",
                                crate::ast::BinOp::Shr => "sar",
                                crate::ast::BinOp::Comma => "copy",
                            },
                            a,
                            b
                        );
                    }
                    Instr::Call(dst, name, args) => {
                        let args = args
                            .iter()
                            .map(|(x, t)| format!("{} {}", Self::ty(t), x))
                            .collect::<Vec<_>>()
                            .join(", ");
                        if let Some(dst) = dst {
                            o += &format!("{} =w call ${}({})\n", dst, name, args);
                        } else {
                            o += &format!("call ${}({})\n", name, args);
                        }
                    }
                    Instr::Ret(Some(x)) => {
                        o += &format!("ret {}\n", x);
                        terminated = true;
                    }
                    Instr::Ret(None) => {
                        o.push_str("ret\n");
                        terminated = true;
                    }
                    Instr::Label(x) => {
                        o += &format!("{}\n", x);
                        terminated = false;
                    }
                    Instr::Jump(x) => {
                        if !terminated {
                            o += &format!("jmp {}\n", x);
                            terminated = true;
                        }
                    }
                    Instr::Branch(x, a, b) => o += &format!("jnz {}, {}, {}\n", x, a, b),
                }
            }
            o.push_str("}\n\n");
        }
        Ok(o)
    }

    fn data_directive(ty: &Type) -> &'static str {
        match ty {
            Type::Qualified(inner, _) => Self::data_directive(inner),
            Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool => "b",
            Type::Short | Type::UnsignedShort => "h",
            Type::Long
            | Type::UnsignedLong
            | Type::LongLong
            | Type::UnsignedLongLong
            | Type::Pointer(_) => "l",
            _ => "w",
        }
    }

    fn alignment(ty: &Type) -> usize {
        match ty {
            Type::Qualified(inner, _) => Self::alignment(inner),
            Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool => 1,
            Type::Short | Type::UnsignedShort => 2,
            Type::Long
            | Type::UnsignedLong
            | Type::LongLong
            | Type::UnsignedLongLong
            | Type::Pointer(_) => 8,
            Type::Array(inner, _) => Self::alignment(inner),
            Type::Struct(_, fields) | Type::Union(_, fields) => fields
                .iter()
                .map(|(_, ty)| Self::alignment(ty))
                .max()
                .unwrap_or(1),
            _ => 4,
        }
    }

    fn data_value(value: &DataValue, directive: &str) -> String {
        match value {
            DataValue::Address(name) => format!("${name}"),
            DataValue::Integer(value) => match directive {
                "b" => (*value as u8 as i64).to_string(),
                "h" => (*value as u16 as i64).to_string(),
                "w" => (*value as i32 as i64).to_string(),
                _ => value.to_string(),
            },
        }
    }
}

fn qbe_string(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push('\\');
                out.push(next);
            } else {
                out.push_str("\\\\");
            }
        } else if c == '"' {
            out.push_str("\\\"");
        } else {
            out.push(c);
        }
    }
    out
}
