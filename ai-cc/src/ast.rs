use crate::diag::Span;
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Int,
    SignedInt,
    UnsignedInt,
    Short,
    UnsignedShort,
    Long,
    UnsignedLong,
    LongLong,
    UnsignedLongLong,
    Bool,
    Char,
    SignedChar,
    UnsignedChar,
    Float,
    Double,
    LongDouble,
    Void,
    Pointer(Box<Type>),
    Array(Box<Type>, Option<usize>),
    Function(Box<Type>, Vec<Type>),
    Struct(String, Vec<(String, Type)>),
    Union(String, Vec<(String, Type)>),
    Enum(String, Vec<(String, i64)>),
    Qualified(Box<Type>, Qualifiers),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Qualifiers {
    pub is_const: bool,
    pub is_volatile: bool,
    pub is_restrict: bool,
}
#[derive(Clone, Debug)]
pub struct TranslationUnit {
    pub items: Vec<Function>,
    pub types: Vec<Type>,
}
#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub ret: Type,
    pub params: Vec<(String, Type)>,
    pub body: Block,
    pub external: bool,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}
#[derive(Clone, Debug)]
pub enum Stmt {
    Decl(String, Type, Option<Expr>),
    Expr(Expr),
    Return(Option<Expr>),
    Block(Block),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    Break,
    Continue,
}
#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64),
    Char(char),
    String(String),
    Var(String),
    Assign(String, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Call(String, Vec<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String, bool),
}
#[derive(Clone, Copy, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitOr,
    BitXor,
    BitAnd,
    Shl,
    Shr,
    Comma,
}
#[derive(Clone, Copy, Debug)]
pub enum UnOp {
    Neg,
    Not,
    Pos,
    BitNot,
}
