use crate::{diag::Diagnostic, lower::*};
pub struct QbeCodegen;
impl QbeCodegen {
    pub fn new() -> Self {
        Self
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
                .map(|x| format!("w %{}", x))
                .collect::<Vec<_>>()
                .join(", ");
            o += &format!("export function w ${}({}) {{\n@start\n", f.name, params);
            for x in &f.body {
                match x {
                    Instr::Const(n, v) => o += &format!("{} =w copy {}\n", n, v),
                    Instr::Move(n, x) => o += &format!("{} =w copy {}\n", n, x),
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
                                crate::ast::BinOp::And => "and",
                                crate::ast::BinOp::Or => "or",
                                crate::ast::BinOp::BitOr => "or",
                                crate::ast::BinOp::BitXor => "xor",
                                crate::ast::BinOp::BitAnd => "and",
                                crate::ast::BinOp::Shl => "shl",
                                crate::ast::BinOp::Shr => "sar",
                                crate::ast::BinOp::Comma => "copy",
                            },
                            a,
                            b
                        )
                    }
                    Instr::Call(n, f, args) => {
                        let args = args
                            .iter()
                            .map(|x| {
                                format!("{} {}", if x.starts_with("$str") { "l" } else { "w" }, x)
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        o += &format!("{} =w call ${}({})\n", n, f, args)
                    }
                    Instr::Ret(Some(x)) => o += &format!("ret {}\n", x),
                    Instr::Ret(None) => o.push_str("ret\n"),
                    Instr::Label(x) => o += &format!("{}\n", x),
                    Instr::Jump(x) => o += &format!("jmp {}\n", x),
                    Instr::Branch(x, a, b) => o += &format!("jnz {}, {}, {}\n", x, a, b),
                }
            }
            o.push_str("}\n\n")
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
