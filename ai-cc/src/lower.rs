use crate::{ast::*, diag::Diagnostic};
#[derive(Clone, Debug)]
pub struct LoweredProgram {
    pub functions: Vec<LoweredFunction>,
    pub strings: Vec<(String, String)>,
}
#[derive(Clone, Debug)]
pub struct LoweredFunction {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Instr>,
}
#[derive(Clone, Debug)]
pub enum Instr {
    Const(String, i64),
    Move(String, String),
    Bin(String, BinOp, String, String),
    Call(String, String, Vec<String>),
    Ret(Option<String>),
    Label(String),
    Jump(String),
    Branch(String, String, String),
}
pub struct Lowerer {
    n: usize,
    strings: Vec<(String, String)>,
    loops: Vec<(String, String)>,
}
impl Lowerer {
    pub fn new() -> Self {
        Self {
            n: 0,
            strings: Vec::new(),
            loops: Vec::new(),
        }
    }
    fn tmp(&mut self) -> String {
        let x = format!("%t{}", self.n);
        self.n += 1;
        x
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
            Expr::Var(n) => format!("%{}", n),
            Expr::Assign(n, e) => {
                let x = self.expr(e, v);
                v.push(Instr::Move(format!("%{}", n), x.clone()));
                x
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
                let z = self.tmp();
                if matches!(op, BinOp::Comma) {
                    y
                } else {
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
                v.push(Instr::Jump(e));
                v.push(Instr::Label(format!("@conde{}", i)));
                z
            }
            Expr::Call(n, a) => {
                let x = self.tmp();
                let xs = a.iter().map(|e| self.expr(e, v)).collect();
                v.push(Instr::Call(x.clone(), n.clone(), xs));
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
            Expr::Member(_, _, _) => "0".into(),
        }
    }
    fn stmt(&mut self, s: &Stmt, v: &mut Vec<Instr>) {
        match s {
            Stmt::Decl(n, _, x) => {
                if let Some(e) = x {
                    let y = self.expr(e, v);
                    v.push(Instr::Move(format!("%{}", n), y));
                }
            }
            Stmt::Expr(e) => {
                self.expr(e, v);
            }
            Stmt::Return(e) => {
                let x = e.as_ref().map(|z| self.expr(z, v));
                v.push(Instr::Ret(x));
            }
            Stmt::Block(b) => {
                for x in &b.stmts {
                    self.stmt(x, v)
                }
            }
            Stmt::If(c, a, b) => {
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
            Stmt::While(c, b) => {
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
            Stmt::DoWhile(b, c) => {
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
            Stmt::For(init, c, step, b) => {
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
            Stmt::Break => {
                if let Some((x, _)) = self.loops.last() {
                    v.push(Instr::Jump(x.clone()));
                }
            }
            Stmt::Continue => {
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
        let mut fs = Vec::new();
        for f in ast.items {
            if f.external {
                continue;
            }
            let mut b = Vec::new();
            for s in &f.body.stmts {
                self.stmt(s, &mut b)
            }
            if !matches!(b.last(), Some(Instr::Ret(_))) {
                b.push(Instr::Ret(Some("0".into())))
            }
            fs.push(LoweredFunction {
                name: f.name,
                params: f.params.into_iter().map(|(n, _)| n).collect(),
                body: b,
            })
        }
        Ok(LoweredProgram {
            functions: fs,
            strings: self.strings.clone(),
        })
    }
}
