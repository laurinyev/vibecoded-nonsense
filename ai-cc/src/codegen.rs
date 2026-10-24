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
            o += &format!(
                "export function {}${}({}) {{\n@start\n",
                ret, f.name, params
            );
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
