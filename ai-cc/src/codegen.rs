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
            Type::Pointer(_) | Type::Array(_, _, _) | Type::Vla(_, _) => "l",
            Type::Float => "s",
            Type::Double | Type::LongDouble => "d",
            Type::Complex(_) => "l",
            Type::Qualified(x, _) => Self::ty(x),
            _ => "w",
        }
    }

    fn abi_ty(t: &Type) -> String {
        match t {
            Type::Qualified(inner, _) => Self::abi_ty(inner),
            Type::Complex(inner) => {
                if Self::ty(inner) == "s" {
                    ":c_s".to_string()
                } else {
                    ":c_d".to_string()
                }
            }
            _ => Self::ty(t).to_string(),
        }
    }

    fn float_literal(value: f64, ty: &Type) -> String {
        let value = if matches!(ty, Type::Float) {
            value as f32 as f64
        } else {
            value
        };
        let text = format!("{value:?}");
        if text.contains('.') || text.contains('e') || text.contains('E') {
            text
        } else {
            format!("{text}.0")
        }
    }

    pub fn emit_program(&mut self, p: &LoweredProgram) -> Result<String, Vec<Diagnostic>> {
        let mut o = String::new();
        o.push_str("type :c_s = { s, s }\ntype :c_d = { d, d }\n\n");
        for (name, ty, value, internal) in &p.globals {
            let mut base_ty = ty;
            while let Type::Qualified(inner, _) = base_ty {
                base_ty = inner;
            }
            let (directive, alignment) = match base_ty {
                Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool => ("b", 1),
                Type::Short | Type::UnsignedShort => ("h", 2),
                Type::Float => ("s", 4),
                Type::Double | Type::LongDouble => ("d", 8),
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
            let alignment = crate::layout::align(&data.ty);
            let items = Self::data_items(data);
            let linkage = if data.internal { "data" } else { "export data" };
            o += &format!(
                "{} ${} = align {} {{ {} }}\n",
                linkage, data.name, alignment, items
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
                Type::Float => 4,
                Type::Double | Type::LongDouble => 8,
                Type::Long
                | Type::UnsignedLong
                | Type::LongLong
                | Type::UnsignedLongLong
                | Type::Pointer(_) => 8,
                _ => 4,
            };
            let directive = match base_ty {
                Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool => "b",
                Type::Short | Type::UnsignedShort => "h",
                Type::Float => "s",
                Type::Double | Type::LongDouble => "d",
                Type::Long
                | Type::UnsignedLong
                | Type::LongLong
                | Type::UnsignedLongLong
                | Type::Pointer(_) => "l",
                _ => "w",
            };
            let value = Self::data_value(value, directive);
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
                .map(|(x, t)| format!("{} %{}", Self::abi_ty(t), x))
                .collect::<Vec<_>>()
                .join(", ");
            let params = if f.is_variadic {
                if params.is_empty() {
                    "...".to_string()
                } else {
                    format!("{}, ...", params)
                }
            } else {
                params
            };
            let ret = if matches!(f.ret, Type::Void) {
                String::new()
            } else {
                format!("{} ", Self::abi_ty(&f.ret))
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
                    Instr::Alloc(n, size) => {
                        let op = if *size > 4 { "alloc8" } else { "alloc4" };
                        o += &format!("{} =l {} {}\n", n, op, size)
                    }
                    Instr::AllocVla(n, count, element) => {
                        let base = n.trim_start_matches('%');
                        let wide = format!("%{}_count", base);
                        let bytes = format!("%{}_bytes", base);
                        o += &format!("{} =l extsw {}\n", wide, count);
                        o += &format!("{} =l mul {}, {}\n", bytes, wide, element.max(&1));
                        o += &format!("{} =l alloc8 {}\n", n, bytes);
                    }
                    Instr::Const(n, v) => o += &format!("{} =w copy {}\n", n, v),
                    Instr::ConstLong(n, v) => o += &format!("{} =l copy {}\n", n, v),
                    Instr::ConstFloat(n, v, ty) => {
                        let qbe_ty = Self::ty(ty);
                        let literal = Self::float_literal(*v, ty);
                        o += &format!("{} ={} copy {}_{}\n", n, qbe_ty, qbe_ty, literal)
                    }
                    Instr::Move(n, x, ty) => o += &format!("{} ={} copy {}\n", n, Self::ty(ty), x),
                    Instr::Load(n, addr, ty) => {
                        let op = match Self::ty(ty) {
                            "l" => "loadl",
                            "s" => "loads",
                            "d" => "loadd",
                            _ if matches!(
                                ty,
                                Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool
                            ) =>
                            {
                                "loadub"
                            }
                            _ => "loadw",
                        };
                        o += &format!("{} ={} {} {}\n", n, Self::ty(ty), op, addr);
                    }
                    Instr::LoadZero(n, addr, size) => {
                        let op = match size {
                            1 => "loadub",
                            2 => "loaduh",
                            4 => "loaduw",
                            _ => "loadl",
                        };
                        let ty = if *size == 8 { "l" } else { "w" };
                        o += &format!("{} ={} {} {}\n", n, ty, op, addr);
                    }
                    Instr::Store(value, addr, ty) => {
                        let op = match Self::ty(ty) {
                            "l" => "storel",
                            "s" => "stores",
                            "d" => "stored",
                            _ if matches!(
                                ty,
                                Type::Char | Type::SignedChar | Type::UnsignedChar | Type::Bool
                            ) =>
                            {
                                "storeb"
                            }
                            _ => "storew",
                        };
                        o += &format!("{} {}, {}\n", op, value, addr);
                    }
                    Instr::StoreSize(value, addr, size) => {
                        let op = match size {
                            1 => "storeb",
                            2 => "storeh",
                            4 => "storew",
                            _ => "storel",
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
                    Instr::FloatBin(n, op, a, b, ty) => {
                        let suffix = if matches!(ty, Type::Float) { "s" } else { "d" };
                        let (result, op) = match op {
                            crate::ast::BinOp::Add => (suffix, "add"),
                            crate::ast::BinOp::Sub => (suffix, "sub"),
                            crate::ast::BinOp::Mul => (suffix, "mul"),
                            crate::ast::BinOp::Div => (suffix, "div"),
                            crate::ast::BinOp::Eq => {
                                ("w", if suffix == "s" { "ceqs" } else { "ceqd" })
                            }
                            crate::ast::BinOp::Ne => {
                                ("w", if suffix == "s" { "cnes" } else { "cned" })
                            }
                            crate::ast::BinOp::Lt => {
                                ("w", if suffix == "s" { "clts" } else { "cltd" })
                            }
                            crate::ast::BinOp::Le => {
                                ("w", if suffix == "s" { "cles" } else { "cled" })
                            }
                            crate::ast::BinOp::Gt => {
                                ("w", if suffix == "s" { "cgts" } else { "cgtd" })
                            }
                            crate::ast::BinOp::Ge => {
                                ("w", if suffix == "s" { "cges" } else { "cged" })
                            }
                            _ => ("w", "copy"),
                        };
                        o += &format!("{} ={} {} {}, {}\n", n, result, op, a, b);
                    }
                    Instr::Convert(dst, src, from, to) => {
                        let from_ty = Self::ty(from);
                        let to_ty = Self::ty(to);
                        if matches!(from_ty, "s" | "d") && matches!(to_ty, "s" | "d") {
                            if from_ty == to_ty {
                                o += &format!("{} ={} copy {}\n", dst, to_ty, src);
                            } else if to_ty == "d" {
                                o += &format!("{} =d exts {}\n", dst, src);
                            } else {
                                o += &format!("{} =s truncd {}\n", dst, src);
                            }
                        } else if matches!(from_ty, "s" | "d") {
                            let op = if from_ty == "s" { "stosi" } else { "dtosi" };
                            if to_ty == "l" {
                                let tmp = format!("{}_w", dst);
                                o += &format!("{} =w {} {}\n", tmp, op, src);
                                o += &format!("{} =l extsw {}\n", dst, tmp);
                            } else {
                                o += &format!("{} =w {} {}\n", dst, op, src);
                            }
                        } else {
                            let op = if from_ty == "l" { "sltof" } else { "swtof" };
                            let tmp = format!("{}_d", dst);
                            o += &format!("{} =d {} {}\n", tmp, op, src);
                            if to_ty == "s" {
                                o += &format!("{} =s truncd {}\n", dst, tmp);
                            } else {
                                o += &format!("{} =d copy {}\n", dst, tmp);
                            }
                        }
                    }
                    Instr::Call(dst, name, args, fixed, ret) => {
                        let rendered = args
                            .iter()
                            .map(|(x, t)| format!("{} {}", Self::abi_ty(t), x))
                            .collect::<Vec<_>>();
                        let args = if let Some(fixed) = fixed {
                            let mut parts = rendered[..*fixed].to_vec();
                            parts.push("...".to_string());
                            parts.extend_from_slice(&rendered[*fixed..]);
                            parts.join(", ")
                        } else {
                            rendered.join(", ")
                        };
                        if let Some(dst) = dst {
                            o += &format!(
                                "{} ={} call ${}({})\n",
                                dst,
                                Self::abi_ty(ret),
                                name,
                                args
                            );
                        } else {
                            o += &format!("call ${}({})\n", name, args);
                        }
                    }
                    Instr::VaStart(ap) => {
                        o += &format!("vastart {}\n", ap);
                    }
                    Instr::VaArg(dst, ap, ty) => {
                        o += &format!("{} ={} vaarg {}\n", dst, Self::abi_ty(ty), ap);
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

    fn data_items(data: &DataObject) -> String {
        let mut relocations = data.relocations.clone();
        relocations.sort_by_key(|(offset, _)| *offset);
        let mut items = Vec::new();
        let mut offset = 0usize;
        let mut next_relocation = 0usize;
        while offset < data.bytes.len() {
            if next_relocation < relocations.len() && relocations[next_relocation].0 == offset {
                items.push(format!("l ${}", relocations[next_relocation].1));
                offset += 8;
                next_relocation += 1;
                continue;
            }
            let end = relocations
                .get(next_relocation)
                .map(|(offset, _)| (*offset).min(data.bytes.len()))
                .unwrap_or(data.bytes.len());
            if end <= offset {
                offset = end;
                continue;
            }
            Self::push_bytes(&mut items, &data.bytes[offset..end]);
            offset = end;
        }
        if items.is_empty() {
            "z 0".to_string()
        } else {
            items.join(", ")
        }
    }

    fn push_bytes(items: &mut Vec<String>, bytes: &[u8]) {
        let mut index = 0usize;
        while index < bytes.len() {
            if bytes[index] == 0 {
                let start = index;
                while index < bytes.len() && bytes[index] == 0 {
                    index += 1;
                }
                items.push(format!("z {}", index - start));
            } else {
                items.push(format!("b {}", bytes[index]));
                index += 1;
            }
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
            DataValue::Float(value, ty) => {
                format!("{}_{}", directive, Self::float_literal(*value, ty))
            }
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
