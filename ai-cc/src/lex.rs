use crate::diag::{Diagnostic, Span};
use std::fmt;
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(i64),
    FloatConstant(String),
    String(String),
    Char(char),
    Int,
    Bool,
    CharKw,
    Void,
    Auto,
    Case,
    Default,
    Extern,
    Goto,
    Register,
    Switch,
    Inline,
    Complex,
    Imaginary,
    Return,
    If,
    Else,
    While,
    For,
    Do,
    Break,
    Continue,
    Struct,
    Union,
    Const,
    Volatile,
    Restrict,
    Signed,
    Unsigned,
    Short,
    Long,
    Float,
    Double,
    Enum,
    Typedef,
    Static,
    Sizeof,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitOr,
    Xor,
    BitAnd,
    Shl,
    Shr,
    Not,
    Tilde,
    PlusPlus,
    MinusMinus,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    RemAssign,
    ShlAssign,
    ShrAssign,
    AndAssign,
    XorAssign,
    OrAssign,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    Question,
    Dot,
    Arrow,
    Ellipsis,
    Hash,
    HashHash,
    End,
}
impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            TokenKind::Ident(_) => "identifier",
            TokenKind::Number(_) => "integer constant",
            TokenKind::FloatConstant(_) => "floating constant",
            TokenKind::String(_) => "string literal",
            TokenKind::Char(_) => "character constant",
            TokenKind::Int => "'int'",
            TokenKind::Bool => "'_Bool'",
            TokenKind::CharKw => "'char'",
            TokenKind::Void => "'void'",
            TokenKind::Struct => "'struct'",
            TokenKind::Union => "'union'",
            TokenKind::Enum => "'enum'",
            TokenKind::Static => "'static'",
            TokenKind::Return => "'return'",
            TokenKind::If => "'if'",
            TokenKind::Else => "'else'",
            TokenKind::While => "'while'",
            TokenKind::For => "'for'",
            TokenKind::Do => "'do'",
            TokenKind::Break => "'break'",
            TokenKind::Continue => "'continue'",
            TokenKind::Semicolon => "';'",
            TokenKind::Comma => "','",
            TokenKind::LParen => "'('",
            TokenKind::RParen => "')'",
            TokenKind::LBrace => "'{'",
            TokenKind::RBrace => "'}'",
            TokenKind::LBracket => "'['",
            TokenKind::RBracket => "']'",
            TokenKind::Colon => "':'",
            TokenKind::Question => "'?'",
            TokenKind::End => "end of file",
            TokenKind::Plus => "'+'",
            TokenKind::Minus => "'-'",
            TokenKind::Star => "'*'",
            TokenKind::Slash => "'/'",
            TokenKind::Percent => "'%'",
            TokenKind::Assign => "'='",
            TokenKind::Eq => "'=='",
            TokenKind::Ne => "'!='",
            TokenKind::Lt => "'<'",
            TokenKind::Le => "'<='",
            TokenKind::Gt => "'>'",
            TokenKind::Ge => "'>='",
            TokenKind::And => "'&&'",
            TokenKind::Or => "'||'",
            TokenKind::BitOr => "'|'",
            TokenKind::Xor => "'^'",
            TokenKind::BitAnd => "'&'",
            TokenKind::Shl => "'<<'",
            TokenKind::Shr => "'>>'",
            TokenKind::Not => "'!'",
            TokenKind::Tilde => "'~'",
            TokenKind::PlusPlus => "'++'",
            TokenKind::MinusMinus => "'--'",
            TokenKind::AddAssign => "'+='",
            TokenKind::SubAssign => "'-='",
            TokenKind::MulAssign => "'*='",
            TokenKind::DivAssign => "'/='",
            TokenKind::RemAssign => "'%='",
            TokenKind::ShlAssign => "'<<='",
            TokenKind::ShrAssign => "'>>='",
            TokenKind::AndAssign => "'&='",
            TokenKind::XorAssign => "'^='",
            TokenKind::OrAssign => "'|='",
            TokenKind::Dot => "'.'",
            TokenKind::Arrow => "'->'",
            TokenKind::Ellipsis => "'...'",
            _ => "type or keyword",
        };
        f.write_str(name)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
pub struct Lexer {
    src: Vec<char>,
    pos: usize,
}
impl Lexer {
    pub fn new(s: &str) -> Self {
        Self {
            src: s.chars().collect(),
            pos: 0,
        }
    }
    fn t(&self, k: TokenKind, s: usize) -> Token {
        Token {
            kind: k,
            span: Span {
                start: s,
                end: self.pos,
            },
        }
    }

    fn ucn_at(&self, start: usize) -> Result<(char, usize), Diagnostic> {
        let prefix = self.src.get(start + 1).copied();
        let digits = match prefix {
            Some('u') => 4,
            Some('U') => 8,
            _ => {
                return Err(Diagnostic::error(
                    "invalid universal character name",
                    Some(Span {
                        start,
                        end: (start + 2).min(self.src.len()),
                    }),
                ))
            }
        };
        let end = start.saturating_add(2).saturating_add(digits);
        if end > self.src.len() {
            return Err(Diagnostic::error(
                "incomplete universal character name",
                Some(Span {
                    start,
                    end: self.src.len(),
                }),
            ));
        }
        let mut value = 0u32;
        for c in &self.src[start + 2..end] {
            let Some(digit) = c.to_digit(16) else {
                return Err(Diagnostic::error(
                    "invalid universal character name",
                    Some(Span { start, end }),
                ));
            };
            value = value * 16 + digit;
        }
        let Some(c) = char::from_u32(value) else {
            return Err(Diagnostic::error(
                "universal character name is not a scalar value",
                Some(Span { start, end }),
            ));
        };
        Ok((c, end))
    }

    pub fn next_token(&mut self) -> Result<Token, Diagnostic> {
        loop {
            while self.pos < self.src.len() && self.src[self.pos].is_whitespace() {
                self.pos += 1;
            }
            if self.pos + 1 < self.src.len() && self.src[self.pos..self.pos + 2] == ['/', '/'] {
                self.pos += 2;
                while self.pos < self.src.len() && self.src[self.pos] != '\n' {
                    self.pos += 1;
                }
                continue;
            }
            if self.pos + 1 < self.src.len() && self.src[self.pos..self.pos + 2] == ['/', '*'] {
                self.pos += 2;
                while self.pos + 1 < self.src.len()
                    && self.src[self.pos..self.pos + 2] != ['*', '/']
                {
                    self.pos += 1;
                }
                if self.pos + 1 == self.src.len() {
                    self.pos = self.src.len();
                    return Err(Diagnostic::error(
                        "unterminated comment",
                        Some(Span {
                            start: self.pos,
                            end: self.pos,
                        }),
                    ));
                }
                self.pos += 2;
                continue;
            }
            break;
        }
        let s = self.pos;
        if s == self.src.len() {
            return Ok(self.t(TokenKind::End, s));
        }
        for (spelling, kind) in [
            ("<:", TokenKind::LBracket),
            (":>", TokenKind::RBracket),
            ("<%", TokenKind::LBrace),
            ("%>", TokenKind::RBrace),
        ] {
            if self.src[s..].starts_with(&spelling.chars().collect::<Vec<_>>()) {
                self.pos += 2;
                return Ok(self.t(kind, s));
            }
        }
        let c = self.src[self.pos];
        if c.is_ascii_digit() {
            // Consume the pp-number-shaped spelling first, then validate it as
            // either an integer or a C99 decimal/hexadecimal floating constant.
            while self.pos < self.src.len()
                && (self.src[self.pos].is_ascii_alphanumeric()
                    || self.src[self.pos] == '_'
                    || self.src[self.pos] == '.'
                    || self.src[self.pos] == '+'
                    || self.src[self.pos] == '-')
            {
                if matches!(self.src[self.pos], '+' | '-')
                    && self.pos > s
                    && matches!(self.src[self.pos - 1], 'e' | 'E' | 'p' | 'P')
                {
                    self.pos += 1;
                    continue;
                }
                if !matches!(self.src[self.pos], '+' | '-') {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let spelling: String = self.src[s..self.pos].iter().collect();
            let imaginary = matches!(spelling.chars().last(), Some('i' | 'j'));
            let numeric = if imaginary {
                &spelling[..spelling.len() - 1]
            } else {
                spelling.as_str()
            };
            let is_hex = numeric.starts_with("0x") || numeric.starts_with("0X");
            let is_float = imaginary
                || numeric.contains('.')
                || if is_hex {
                    numeric.contains('p') || numeric.contains('P')
                } else {
                    numeric.contains('e') || numeric.contains('E')
                }
                || matches!(numeric.chars().last(), Some('f' | 'F'));
            if is_float {
                let suffix = numeric
                    .chars()
                    .last()
                    .filter(|c| matches!(c, 'f' | 'F' | 'l' | 'L'));
                let body = suffix.map_or(numeric, |_| &numeric[..numeric.len() - 1]);
                let valid = if is_hex {
                    (body.starts_with("0x") || body.starts_with("0X"))
                        && (body.contains('p') || body.contains('P'))
                        && body.chars().all(|c| {
                            c.is_ascii_hexdigit()
                                || matches!(c, 'x' | 'X' | 'p' | 'P' | '+' | '-' | '.')
                        })
                } else {
                    body.parse::<f64>().is_ok()
                };
                if !valid {
                    return Err(Diagnostic::error(
                        "invalid floating constant",
                        Some(self.t(TokenKind::End, s).span),
                    ));
                }
                return Ok(self.t(TokenKind::FloatConstant(spelling), s));
            }
            self.pos = s;
            let base = if c == '0' && matches!(self.src.get(self.pos + 1), Some('x' | 'X')) {
                self.pos += 2;
                16
            } else if c == '0' {
                8
            } else {
                10
            };
            let digits_start = self.pos;
            while self.pos < self.src.len() && self.src[self.pos].is_digit(base) {
                self.pos += 1;
            }
            if self.pos == digits_start {
                return Err(Diagnostic::error(
                    if base == 16 {
                        "expected hexadecimal digit"
                    } else {
                        "invalid integer constant"
                    },
                    Some(self.t(TokenKind::End, s).span),
                ));
            }
            let digits: String = self.src[digits_start..self.pos].iter().collect();
            let suffix_start = self.pos;
            while self.pos < self.src.len() && matches!(self.src[self.pos], 'u' | 'U' | 'l' | 'L') {
                self.pos += 1;
            }
            let suffix: String = self.src[suffix_start..self.pos].iter().collect();
            let u_count = suffix.chars().filter(|c| matches!(c, 'u' | 'U')).count();
            let l_count = suffix.chars().filter(|c| matches!(c, 'l' | 'L')).count();
            let long_suffix: String = suffix.chars().filter(|c| matches!(c, 'l' | 'L')).collect();
            if u_count > 1
                || l_count > 2
                || suffix.len() != u_count + l_count
                || self
                    .src
                    .get(self.pos)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                || (l_count == 2 && long_suffix != "ll" && long_suffix != "LL")
            {
                return Err(Diagnostic::error(
                    "invalid integer constant suffix",
                    Some(self.t(TokenKind::End, s).span),
                ));
            }
            let value = i64::from_str_radix(&digits, base).ok();
            let Some(n) = value else {
                return Err(Diagnostic::error(
                    "integer constant is out of range",
                    Some(self.t(TokenKind::End, s).span),
                ));
            };
            return Ok(self.t(TokenKind::Number(n), s));
        }
        if c.is_ascii_alphabetic()
            || c == '_'
            || (c == '\\' && self.src.get(s + 1).is_some_and(|x| *x == 'u' || *x == 'U'))
        {
            let mut x = String::new();
            if c == '\\' {
                let (decoded, end) = self.ucn_at(self.pos)?;
                x.push(decoded);
                self.pos = end;
            } else {
                x.push(c);
                self.pos += 1;
            }
            loop {
                if self.pos < self.src.len()
                    && (self.src[self.pos].is_ascii_alphanumeric() || self.src[self.pos] == '_')
                {
                    x.push(self.src[self.pos]);
                    self.pos += 1;
                } else if self.src.get(self.pos) == Some(&'\\')
                    && self
                        .src
                        .get(self.pos + 1)
                        .is_some_and(|x| *x == 'u' || *x == 'U')
                {
                    let (decoded, end) = self.ucn_at(self.pos)?;
                    x.push(decoded);
                    self.pos = end;
                } else {
                    break;
                }
            }
            let k = match x.as_str() {
                "int" => TokenKind::Int,
                "_Bool" => TokenKind::Bool,
                "char" => TokenKind::CharKw,
                "void" => TokenKind::Void,
                "auto" => TokenKind::Auto,
                "case" => TokenKind::Case,
                "default" => TokenKind::Default,
                "extern" => TokenKind::Extern,
                "goto" => TokenKind::Goto,
                "register" => TokenKind::Register,
                "switch" => TokenKind::Switch,
                "inline" => TokenKind::Inline,
                "_Complex" => TokenKind::Complex,
                "_Imaginary" => TokenKind::Imaginary,
                "return" => TokenKind::Return,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "while" => TokenKind::While,
                "for" => TokenKind::For,
                "do" => TokenKind::Do,
                "break" => TokenKind::Break,
                "continue" => TokenKind::Continue,
                "struct" => TokenKind::Struct,
                "union" => TokenKind::Union,
                "const" => TokenKind::Const,
                "volatile" => TokenKind::Volatile,
                "restrict" => TokenKind::Restrict,
                "signed" => TokenKind::Signed,
                "unsigned" => TokenKind::Unsigned,
                "short" => TokenKind::Short,
                "long" => TokenKind::Long,
                "float" => TokenKind::Float,
                "double" => TokenKind::Double,
                "enum" => TokenKind::Enum,
                "typedef" => TokenKind::Typedef,
                "static" => TokenKind::Static,
                "sizeof" => TokenKind::Sizeof,
                _ => TokenKind::Ident(x),
            };
            return Ok(self.t(k, s));
        }
        if c == '"' || c == '\'' {
            let quote = c;
            self.pos += 1;
            let mut x = String::new();
            while self.pos < self.src.len() && self.src[self.pos] != quote {
                if self.src[self.pos] == '\\' {
                    let escape_start = self.pos;
                    if self
                        .src
                        .get(self.pos + 1)
                        .is_some_and(|x| *x == 'u' || *x == 'U')
                    {
                        let (decoded, end) = self.ucn_at(escape_start)?;
                        x.push(decoded);
                        self.pos = end;
                        continue;
                    }
                    self.pos += 1;
                    let Some(e) = self.src.get(self.pos).copied() else {
                        break;
                    };
                    x.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        '\\' => '\\',
                        '\'' => '\'',
                        '"' => '"',
                        _ => e,
                    });
                } else {
                    x.push(self.src[self.pos]);
                }
                self.pos += 1;
            }
            if self.pos == self.src.len() || self.src[self.pos] != quote {
                return Err(Diagnostic::error(
                    "unterminated string",
                    Some(Span {
                        start: s,
                        end: self.pos,
                    }),
                ));
            }
            self.pos += 1;
            return if quote == '"' {
                Ok(self.t(TokenKind::String(x), s))
            } else if x.chars().count() == 1 {
                Ok(self.t(TokenKind::Char(x.chars().next().unwrap()), s))
            } else {
                Err(Diagnostic::error(
                    "character constant must contain one character",
                    Some(self.t(TokenKind::End, s).span),
                ))
            };
        }
        self.pos += 1;
        let k = match c {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            '+' => {
                if self.src.get(self.pos) == Some(&'+') {
                    self.pos += 1;
                    TokenKind::PlusPlus
                } else if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::AddAssign
                } else {
                    TokenKind::Plus
                }
            }
            '-' => {
                if self.src.get(self.pos) == Some(&'>') {
                    self.pos += 1;
                    TokenKind::Arrow
                } else if self.src.get(self.pos) == Some(&'-') {
                    self.pos += 1;
                    TokenKind::MinusMinus
                } else if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::SubAssign
                } else {
                    TokenKind::Minus
                }
            }
            '*' => {
                if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::MulAssign
                } else {
                    TokenKind::Star
                }
            }
            '/' => {
                if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::DivAssign
                } else {
                    TokenKind::Slash
                }
            }
            '%' => {
                if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::RemAssign
                } else {
                    TokenKind::Percent
                }
            }
            ';' => TokenKind::Semicolon,
            ',' => TokenKind::Comma,
            ':' => TokenKind::Colon,
            '?' => TokenKind::Question,
            '.' => {
                if self.src.get(self.pos) == Some(&'.') && self.src.get(self.pos + 1) == Some(&'.')
                {
                    self.pos += 2;
                    TokenKind::Ellipsis
                } else {
                    TokenKind::Dot
                }
            }
            '#' => {
                if self.src.get(self.pos) == Some(&'#') {
                    self.pos += 1;
                    TokenKind::HashHash
                } else {
                    TokenKind::Hash
                }
            }
            '~' => TokenKind::Tilde,
            '^' => {
                if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::XorAssign
                } else {
                    TokenKind::Xor
                }
            }
            '=' => {
                if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::Eq
                } else {
                    TokenKind::Assign
                }
            }
            '!' => {
                if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::Ne
                } else {
                    TokenKind::Not
                }
            }
            '<' => {
                if self.src.get(self.pos) == Some(&'<') {
                    self.pos += 1;
                    if self.src.get(self.pos) == Some(&'=') {
                        self.pos += 1;
                        TokenKind::ShlAssign
                    } else {
                        TokenKind::Shl
                    }
                } else if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::Le
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if self.src.get(self.pos) == Some(&'>') {
                    self.pos += 1;
                    if self.src.get(self.pos) == Some(&'=') {
                        self.pos += 1;
                        TokenKind::ShrAssign
                    } else {
                        TokenKind::Shr
                    }
                } else if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::Ge
                } else {
                    TokenKind::Gt
                }
            }
            '&' => {
                if self.src.get(self.pos) == Some(&'&') {
                    self.pos += 1;
                    TokenKind::And
                } else if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::AndAssign
                } else {
                    TokenKind::BitAnd
                }
            }
            '|' => {
                if self.src.get(self.pos) == Some(&'|') {
                    self.pos += 1;
                    TokenKind::Or
                } else if self.src.get(self.pos) == Some(&'=') {
                    self.pos += 1;
                    TokenKind::OrAssign
                } else {
                    TokenKind::BitOr
                }
            }
            _ => {
                return Err(Diagnostic::error(
                    format!("unexpected character {}", c),
                    Some(Span {
                        start: s,
                        end: self.pos,
                    }),
                ))
            }
        };
        Ok(self.t(k, s))
    }
    pub fn tokenize_all(&mut self) -> Result<Vec<Token>, Vec<Diagnostic>> {
        let mut v = Vec::new();
        let mut errors = Vec::new();
        loop {
            match self.next_token() {
                Ok(t) => {
                    let end = t.kind == TokenKind::End;
                    v.push(t);
                    if end {
                        return if errors.is_empty() {
                            Ok(v)
                        } else {
                            Err(errors)
                        };
                    }
                }
                Err(error) => errors.push(error),
            }
        }
    }
}
