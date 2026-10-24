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
    pub params_unspecified: bool,
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
    Decl(String, Type, Option<Expr>, Span),
    Expr(Expr, Span),
    Return(Option<Expr>, Span),
    Block(Block, Span),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>, Span),
    While(Expr, Box<Stmt>, Span),
    DoWhile(Box<Stmt>, Expr, Span),
    For(
        Option<Box<Stmt>>,
        Option<Expr>,
        Option<Expr>,
        Box<Stmt>,
        Span,
    ),
    Break(Span),
    Continue(Span),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Self::Decl(_, _, _, span)
            | Self::Expr(_, span)
            | Self::Return(_, span)
            | Self::Block(_, span)
            | Self::If(_, _, _, span)
            | Self::While(_, _, span)
            | Self::DoWhile(_, _, span)
            | Self::For(_, _, _, _, span)
            | Self::Break(span)
            | Self::Continue(span) => *span,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64),
    Char(char),
    String(String),
    Var(String),
    Assign(Box<Expr>, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Call(String, Vec<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String, bool),
    Subscript(Box<Expr>, Box<Expr>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Pos,
    BitNot,
    Address,
    Deref,
}
