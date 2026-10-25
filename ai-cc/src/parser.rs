use crate::{
    ast::*,
    diag::{Diagnostic, Span},
    lex::{Token, TokenKind},
};
use std::collections::HashMap;

#[derive(Clone)]
enum DeclaratorOp {
    Pointer(Qualifiers),
    Array(ArrayBound, Qualifiers, bool),
    Function(Vec<(String, Type)>, bool, bool),
}

#[derive(Clone)]
enum ArrayBound {
    Fixed(usize),
    Unspecified,
    Vla(Box<Expr>),
}

enum ExternalDecl {
    Functions(Vec<Function>),
    Object,
}

struct DeclaratorDetails {
    name: String,
    ty: Type,
    params: Vec<(String, Type)>,
    is_variadic: bool,
    has_prototype: bool,
    is_function_declarator: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TagKind {
    Struct,
    Union,
    Enum,
}

impl TagKind {
    fn name(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Union => "union",
            Self::Enum => "enum",
        }
    }
}

#[derive(Clone)]
struct TagEntry {
    internal: String,
    complete: bool,
}

#[derive(Clone)]
struct ParseSnapshot {
    aliases: Vec<HashMap<String, Option<Type>>>,
    constants: Vec<HashMap<String, i64>>,
    tags: Vec<HashMap<(TagKind, String), TagEntry>>,
    definitions: usize,
    tag_counter: usize,
    anonymous_tags: usize,
}

pub struct Parser {
    t: Vec<Token>,
    p: usize,
    errors: Vec<Diagnostic>,
    aliases: Vec<HashMap<String, Option<Type>>>,
    constants: Vec<HashMap<String, i64>>,
    tags: Vec<HashMap<(TagKind, String), TagEntry>>,
    type_definitions: Vec<Type>,
    pending_parameter_tags: Option<HashMap<(TagKind, String), TagEntry>>,
    tag_counter: usize,
    pending_functions: Vec<Function>,
    pending_declarations: Vec<GlobalDecl>,
    anonymous_tags: usize,
    eof: TokenKind,
}
impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            t: tokens,
            p: 0,
            errors: Vec::new(),
            aliases: vec![HashMap::new()],
            constants: vec![HashMap::new()],
            tags: vec![HashMap::new()],
            type_definitions: Vec::new(),
            pending_parameter_tags: None,
            tag_counter: 0,
            pending_functions: Vec::new(),
            pending_declarations: Vec::new(),
            anonymous_tags: 0,
            eof: TokenKind::End,
        }
    }
    fn cur(&self) -> &TokenKind {
        self.t.get(self.p).map(|x| &x.kind).unwrap_or(&self.eof)
    }
    fn cur_span(&self) -> Span {
        self.t
            .get(self.p)
            .map(|x| x.span)
            .or_else(|| self.t.last().map(|x| x.span))
            .unwrap_or_default()
    }
    fn span_from(&self, start: usize) -> Span {
        Span {
            start,
            end: self
                .t
                .get(self.p.saturating_sub(1))
                .map(|x| x.span.end)
                .unwrap_or(start),
        }
    }
    fn take(&mut self, k: &TokenKind) -> bool {
        if self.cur() == k {
            if self.p + 1 < self.t.len() {
                self.p += 1;
            }
            true
        } else {
            false
        }
    }
    fn expect(&mut self, k: &TokenKind) -> Result<(), Diagnostic> {
        if self.take(k) {
            Ok(())
        } else {
            Err(Diagnostic::error(
                format!("expected {}, found {}", k, self.cur()),
                Some(self.cur_span()),
            ))
        }
    }
    fn ident(&mut self) -> Result<String, Diagnostic> {
        if let TokenKind::Ident(x) = self.cur().clone() {
            self.p += 1;
            Ok(x)
        } else {
            Err(Diagnostic::error(
                "expected identifier",
                Some(self.cur_span()),
            ))
        }
    }
    fn typ(&mut self) -> Result<Type, Diagnostic> {
        if self.take(&TokenKind::Const)
            || self.take(&TokenKind::Volatile)
            || self.take(&TokenKind::Restrict)
        {
            let q = match &self.t[self.p - 1].kind {
                TokenKind::Const => Qualifiers {
                    is_const: true,
                    ..Default::default()
                },
                TokenKind::Volatile => Qualifiers {
                    is_volatile: true,
                    ..Default::default()
                },
                _ => Qualifiers {
                    is_restrict: true,
                    ..Default::default()
                },
            };
            return Ok(Self::qualified_type_specifier(self.typ()?, q));
        }
        if self.take(&TokenKind::Complex) {
            let base = if matches!(
                self.cur(),
                TokenKind::Float
                    | TokenKind::Double
                    | TokenKind::Long
                    | TokenKind::Const
                    | TokenKind::Volatile
            ) {
                self.typ()?
            } else {
                Type::Double
            };
            let base = match base {
                Type::Float | Type::Double | Type::LongDouble => base,
                Type::Qualified(inner, qualifiers)
                    if matches!(*inner, Type::Float | Type::Double | Type::LongDouble) =>
                {
                    return Ok(Type::Qualified(Box::new(Type::Complex(inner)), qualifiers))
                }
                _ => {
                    return Err(Diagnostic::error(
                        "_Complex requires a floating type",
                        Some(self.cur_span()),
                    ))
                }
            };
            return Ok(Type::Complex(Box::new(base)));
        }
        if self.take(&TokenKind::Imaginary) {
            return Err(Diagnostic::error(
                "_Imaginary types are not supported",
                Some(self.cur_span()),
            ));
        }
        if self.take(&TokenKind::Struct) || self.take(&TokenKind::Union) {
            let union = self.t[self.p - 1].kind == TokenKind::Union;
            let kind = if union {
                TagKind::Union
            } else {
                TagKind::Struct
            };
            let tag = if matches!(self.cur(), TokenKind::Ident(_)) {
                self.ident()?
            } else if *self.cur() == TokenKind::LBrace {
                self.anonymous_tags += 1;
                format!(
                    "__anonymous_{}_{}",
                    if union { "union" } else { "struct" },
                    self.anonymous_tags
                )
            } else {
                return Err(Diagnostic::error(
                    "expected struct/union tag or definition",
                    Some(self.cur_span()),
                ));
            };
            let mut fields: Vec<Field> = Vec::new();
            let has_body = self.take(&TokenKind::LBrace);
            if has_body {
                while *self.cur() != TokenKind::RBrace && *self.cur() != TokenKind::End {
                    let base = self.typ()?;
                    loop {
                        let (name, ft) = self.declarator_full(base.clone())?;
                        let bit_width = if self.take(&TokenKind::Colon) {
                            let width_expr = self.assign()?;
                            let width = Self::integer_constant(&width_expr).ok_or_else(|| {
                                Diagnostic::error(
                                    "bit-field width requires an integer constant expression",
                                    Some(self.cur_span()),
                                )
                            })?;
                            let width = u32::try_from(width).map_err(|_| {
                                Diagnostic::error(
                                    "bit-field width must be nonnegative",
                                    Some(self.cur_span()),
                                )
                            })?;
                            if width == 0 && !name.is_empty() {
                                return Err(Diagnostic::error(
                                    "named bit-field cannot have zero width",
                                    Some(self.cur_span()),
                                ));
                            }
                            if width > (crate::layout::size(&ft) * 8) as u32 {
                                return Err(Diagnostic::error(
                                    "bit-field width exceeds its declared type",
                                    Some(self.cur_span()),
                                ));
                            }
                            if !Self::bitfield_type(&ft) {
                                return Err(Diagnostic::error(
                                    "bit-field has an invalid base type",
                                    Some(self.cur_span()),
                                ));
                            }
                            Some(width)
                        } else {
                            None
                        };
                        if name.is_empty() && bit_width.is_none() {
                            return Err(Diagnostic::error(
                                "member declarator requires a name",
                                Some(self.cur_span()),
                            ));
                        }
                        if !name.is_empty()
                            && fields
                                .iter()
                                .any(|field| field.name.as_deref() == Some(name.as_str()))
                        {
                            return Err(Diagnostic::error(
                                format!("duplicate member {}", name),
                                Some(self.cur_span()),
                            ));
                        }
                        fields.push(match bit_width {
                            Some(width) => {
                                Field::bitfield((!name.is_empty()).then_some(name), ft, width)
                            }
                            None => Field::named(name, ft),
                        });
                        if !self.take(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::Semicolon)?;
                }
                self.expect(&TokenKind::RBrace)?;
            }
            let tag = if has_body {
                self.define_tag(kind, &tag)?
            } else {
                self.reference_tag(kind, &tag)
            };
            let ty = if union {
                Type::Union(tag, fields)
            } else {
                Type::Struct(tag, fields)
            };
            if has_body {
                self.type_definitions.push(ty.clone());
            }
            return Ok(ty);
        }
        if self.take(&TokenKind::Enum) {
            let tag = if matches!(self.cur(), TokenKind::Ident(_)) {
                self.ident()?
            } else if *self.cur() == TokenKind::LBrace {
                self.anonymous_tags += 1;
                format!("__anonymous_enum_{}", self.anonymous_tags)
            } else {
                return Err(Diagnostic::error(
                    "expected enum tag or definition",
                    Some(self.cur_span()),
                ));
            };
            let mut values = Vec::new();
            let mut next = 0;
            let has_body = self.take(&TokenKind::LBrace);
            if has_body {
                while *self.cur() != TokenKind::RBrace && *self.cur() != TokenKind::End {
                    let n = self.ident()?;
                    if self.take(&TokenKind::Assign) {
                        let value = self.assign()?;
                        next = Self::integer_constant(&value).ok_or_else(|| {
                            Diagnostic::error(
                                "enumerator requires an integer constant expression",
                                Some(self.cur_span()),
                            )
                        })?;
                    }
                    i32::try_from(next).map_err(|_| {
                        Diagnostic::error(
                            "enumerator value is not representable as int",
                            Some(self.cur_span()),
                        )
                    })?;
                    self.bind_constant(n.clone(), next);
                    values.push((n, next));
                    next = next.checked_add(1).ok_or_else(|| {
                        Diagnostic::error("enumerator value is out of range", Some(self.cur_span()))
                    })?;
                    if !self.take(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RBrace)?;
            }
            let tag = if has_body {
                self.define_tag(TagKind::Enum, &tag)?
            } else {
                self.reference_tag(TagKind::Enum, &tag)
            };
            let ty = Type::Enum(tag, values);
            if has_body {
                self.type_definitions.push(ty.clone());
            }
            return Ok(ty);
        }
        let x = match self.cur() {
            TokenKind::Signed
            | TokenKind::Unsigned
            | TokenKind::Short
            | TokenKind::Long
            | TokenKind::Int
            | TokenKind::CharKw => {
                let mut signed = false;
                let mut unsigned = false;
                let mut short = false;
                let mut longs = 0;
                let mut saw_int = false;
                let mut saw_char = false;
                let mut qualifiers = Qualifiers::default();
                loop {
                    match self.cur() {
                        TokenKind::Const => qualifiers.is_const = true,
                        TokenKind::Volatile => qualifiers.is_volatile = true,
                        TokenKind::Restrict => qualifiers.is_restrict = true,
                        TokenKind::Signed if !signed && !unsigned => signed = true,
                        TokenKind::Unsigned if !signed && !unsigned => unsigned = true,
                        TokenKind::Short if !short && longs == 0 => short = true,
                        TokenKind::Long if !short && longs < 2 => longs += 1,
                        TokenKind::Int if !saw_int => saw_int = true,
                        TokenKind::CharKw if !saw_char => saw_char = true,
                        _ => break,
                    }
                    self.p += 1;
                }
                if longs == 1
                    && !signed
                    && !unsigned
                    && !short
                    && !saw_int
                    && self.take(&TokenKind::Double)
                {
                    let mut ty = Type::LongDouble;
                    if self.take(&TokenKind::Complex) {
                        ty = Type::Complex(Box::new(ty));
                    }
                    return Ok(if qualifiers == Qualifiers::default() {
                        ty
                    } else {
                        Self::qualified(ty, qualifiers)
                    });
                }
                if saw_char {
                    let ty = match (signed, unsigned, short, longs, saw_int) {
                        (true, false, false, 0, false) => Type::SignedChar,
                        (false, true, false, 0, false) => Type::UnsignedChar,
                        (false, false, false, 0, false) => Type::Char,
                        _ => {
                            return Err(Diagnostic::error(
                                "invalid character type specifiers",
                                Some(self.cur_span()),
                            ))
                        }
                    };
                    return Ok(if qualifiers == Qualifiers::default() {
                        ty
                    } else {
                        Self::qualified(ty, qualifiers)
                    });
                }
                let ty = match (unsigned, short, longs) {
                    (true, true, 0) => Type::UnsignedShort,
                    (false, true, 0) => Type::Short,
                    (true, false, 1) => Type::UnsignedLong,
                    (false, false, 1) => Type::Long,
                    (true, false, 2) => Type::UnsignedLongLong,
                    (false, false, 2) => Type::LongLong,
                    (true, false, 0) => Type::UnsignedInt,
                    (false, false, 0) if signed => Type::SignedInt,
                    (false, false, 0) => Type::Int,
                    _ => {
                        return Err(Diagnostic::error(
                            "invalid integer type specifiers",
                            Some(self.cur_span()),
                        ))
                    }
                };
                if qualifiers == Qualifiers::default() {
                    ty
                } else {
                    Self::qualified(ty, qualifiers)
                }
            }
            TokenKind::Float => {
                self.p += 1;
                let mut ty = Type::Float;
                if self.take(&TokenKind::Complex) {
                    ty = Type::Complex(Box::new(ty));
                }
                while let Some(qualifier) = self.take_qualifier() {
                    ty = Self::qualified(ty, qualifier);
                }
                ty
            }
            TokenKind::Double => {
                self.p += 1;
                let mut ty = Type::Double;
                if self.take(&TokenKind::Complex) {
                    ty = Type::Complex(Box::new(ty));
                }
                while let Some(qualifier) = self.take_qualifier() {
                    ty = Self::qualified(ty, qualifier);
                }
                ty
            }
            TokenKind::Bool => {
                self.p += 1;
                Type::Bool
            }
            TokenKind::Ident(name) if self.alias_type(name).is_some() => {
                let ty = self.alias_type(name).unwrap();
                self.p += 1;
                return Ok(ty);
            }
            TokenKind::Void => {
                self.p += 1;
                Type::Void
            }
            _ => return Err(Diagnostic::error("expected type", Some(self.cur_span()))),
        };
        Ok(x)
    }
    fn alias_type(&self, name: &str) -> Option<Type> {
        self.aliases
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
            .flatten()
    }

    fn bind_ordinary_name(&mut self, name: String) {
        self.aliases.last_mut().unwrap().insert(name, None);
    }

    fn enter_scope(&mut self) {
        self.aliases.push(HashMap::new());
        self.constants.push(HashMap::new());
        self.tags.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        if self.aliases.len() > 1 {
            self.aliases.pop();
        }
        if self.constants.len() > 1 {
            self.constants.pop();
        }
        if self.tags.len() > 1 {
            self.tags.pop();
        }
    }

    fn capture_parameter_tags(&mut self) {
        self.pending_parameter_tags = self.tags.last().cloned();
    }

    fn bind_constant(&mut self, name: String, value: i64) {
        self.aliases.last_mut().unwrap().insert(name.clone(), None);
        self.constants.last_mut().unwrap().insert(name, value);
    }

    fn constant_value(&self, name: &str) -> Option<i64> {
        for (aliases, constants) in self.aliases.iter().zip(self.constants.iter()).rev() {
            if aliases.contains_key(name) {
                return constants.get(name).copied();
            }
        }
        None
    }

    fn snapshot(&self) -> ParseSnapshot {
        ParseSnapshot {
            aliases: self.aliases.clone(),
            constants: self.constants.clone(),
            tags: self.tags.clone(),
            definitions: self.type_definitions.len(),
            tag_counter: self.tag_counter,
            anonymous_tags: self.anonymous_tags,
        }
    }

    fn restore(&mut self, snapshot: ParseSnapshot, position: usize) {
        self.p = position;
        self.aliases = snapshot.aliases;
        self.constants = snapshot.constants;
        self.tags = snapshot.tags;
        self.type_definitions.truncate(snapshot.definitions);
        self.tag_counter = snapshot.tag_counter;
        self.anonymous_tags = snapshot.anonymous_tags;
    }

    fn visible_tag(&self, kind: TagKind, name: &str) -> Option<TagEntry> {
        self.tags
            .iter()
            .rev()
            .find_map(|scope| scope.get(&(kind, name.to_string())).cloned())
    }

    fn fresh_tag_name(&mut self, kind: TagKind, name: &str) -> String {
        if self.tags.len() == 1 {
            name.to_string()
        } else {
            self.tag_counter += 1;
            format!("__local_{}_{}_{}", kind.name(), self.tag_counter, name)
        }
    }

    fn reference_tag(&mut self, kind: TagKind, name: &str) -> String {
        if let Some(entry) = self.visible_tag(kind, name) {
            return entry.internal;
        }
        let internal = self.fresh_tag_name(kind, name);
        self.tags.last_mut().unwrap().insert(
            (kind, name.to_string()),
            TagEntry {
                internal: internal.clone(),
                complete: false,
            },
        );
        internal
    }

    fn define_tag(&mut self, kind: TagKind, name: &str) -> Result<String, Diagnostic> {
        if let Some(entry) = self
            .tags
            .last()
            .unwrap()
            .get(&(kind, name.to_string()))
            .cloned()
        {
            if entry.complete {
                return Err(Diagnostic::error(
                    format!("redefinition of {} {}", kind.name(), name),
                    Some(self.cur_span()),
                ));
            }
            self.tags
                .last_mut()
                .unwrap()
                .get_mut(&(kind, name.to_string()))
                .unwrap()
                .complete = true;
            return Ok(entry.internal);
        }
        let internal = self.fresh_tag_name(kind, name);
        self.tags.last_mut().unwrap().insert(
            (kind, name.to_string()),
            TagEntry {
                internal: internal.clone(),
                complete: true,
            },
        );
        Ok(internal)
    }
    pub fn parse_translation_unit(&mut self) -> Result<TranslationUnit, Vec<Diagnostic>> {
        let mut v = Vec::new();
        let mut declarations = Vec::new();
        let mut e = Vec::new();
        while *self.cur() != TokenKind::End {
            self.pending_parameter_tags = None;
            if self.take(&TokenKind::Typedef) {
                match self.global_typedef() {
                    Ok(()) => {
                        continue;
                    }
                    Err(x) => {
                        e.push(x);
                        self.synchronize(false);
                        continue;
                    }
                }
            }
            let parsed = if self.looks_like_function_declaration() {
                self.function().map(ExternalDecl::Functions)
            } else if self.looks_like_complex_function_definition() {
                self.complex_function_definition()
                    .map(|function| ExternalDecl::Functions(vec![function]))
            } else if self.looks_like_nested_function_declaration() {
                self.nested_function_declaration()
                    .map(|function| ExternalDecl::Functions(vec![function]))
            } else {
                self.global_declaration(&mut declarations)
                    .map(|()| ExternalDecl::Object)
            };
            match parsed {
                Ok(ExternalDecl::Functions(mut functions)) => {
                    functions.append(&mut self.pending_functions);
                    for function in functions {
                        self.aliases[0].insert(function.name.clone(), None);
                        v.push(function);
                    }
                    declarations.append(&mut self.pending_declarations);
                }
                Ok(ExternalDecl::Object) => {
                    for function in self.pending_functions.drain(..) {
                        self.aliases[0].insert(function.name.clone(), None);
                        v.push(function);
                    }
                    declarations.append(&mut self.pending_declarations);
                }
                Err(x) => {
                    e.push(x);
                    self.synchronize(false);
                }
            }
        }
        e.append(&mut self.errors);
        if e.is_empty() {
            Ok(TranslationUnit {
                items: v,
                types: std::mem::take(&mut self.type_definitions),
                declarations,
            })
        } else {
            Err(e)
        }
    }

    fn global_typedef(&mut self) -> Result<(), Diagnostic> {
        let ty = self.typ()?;
        loop {
            let (name, declared_ty) = self.declarator_full(ty.clone())?;
            if name.is_empty() {
                return Err(Diagnostic::error(
                    "typedef declarator requires a name",
                    Some(self.cur_span()),
                ));
            }
            self.aliases[0].insert(name, Some(declared_ty));
            if !self.take(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::Semicolon)
    }

    fn looks_like_function_declaration(&mut self) -> bool {
        let start = self.p;
        let snapshot = self.snapshot();
        while matches!(
            self.cur(),
            TokenKind::Static | TokenKind::Extern | TokenKind::Inline
        ) {
            self.p += 1;
        }
        let type_ok = self.typ().is_ok();
        let result = if type_ok {
            let mut i = self.p;
            while self.t.get(i).is_some_and(|t| t.kind == TokenKind::Star) {
                i += 1;
            }
            matches!(self.t.get(i).map(|t| &t.kind), Some(TokenKind::Ident(_)))
                && self
                    .t
                    .get(i + 1)
                    .is_some_and(|t| t.kind == TokenKind::LParen)
        } else {
            false
        };
        self.restore(snapshot, start);
        result
    }

    fn looks_like_nested_function_declaration(&mut self) -> bool {
        let start = self.p;
        let snapshot = self.snapshot();
        while matches!(
            self.cur(),
            TokenKind::Static | TokenKind::Extern | TokenKind::Inline
        ) {
            self.p += 1;
        }
        let result = self
            .typ()
            .ok()
            .and_then(|base| self.declarator_full(base).ok())
            .is_some_and(|(_, ty)| {
                matches!(ty, Type::Function(_, _, _, _)) && *self.cur() == TokenKind::Semicolon
            });
        self.restore(snapshot, start);
        result
    }

    fn looks_like_complex_function_definition(&mut self) -> bool {
        let start = self.p;
        let snapshot = self.snapshot();
        while matches!(
            self.cur(),
            TokenKind::Static | TokenKind::Extern | TokenKind::Inline
        ) {
            self.p += 1;
        }
        let result = self
            .typ()
            .ok()
            .and_then(|base| self.declarator_full_details(base).ok())
            .is_some_and(|details| {
                details.is_function_declarator
                    && matches!(details.ty, Type::Function(_, _, _, _))
                    && (*self.cur() == TokenKind::LBrace || self.is_type())
            });
        self.restore(snapshot, start);
        result
    }

    fn complex_function_definition(&mut self) -> Result<Function, Diagnostic> {
        let mut is_static = false;
        let mut is_extern = false;
        let mut is_inline = false;
        loop {
            if self.take(&TokenKind::Static) {
                is_static = true;
            } else if self.take(&TokenKind::Extern) {
                is_extern = true;
            } else if self.take(&TokenKind::Inline) {
                is_inline = true;
            } else {
                break;
            }
        }
        let base = self.typ()?;
        let DeclaratorDetails {
            name,
            ty,
            mut params,
            is_variadic,
            has_prototype,
            is_function_declarator,
        } = self.declarator_full_details(base)?;
        let Type::Function(ret, _, _, _) = ty else {
            return Err(Diagnostic::error(
                "function definition declarator must have function type",
                Some(self.cur_span()),
            ));
        };
        if !is_function_declarator {
            return Err(Diagnostic::error(
                "function definition requires a function declarator",
                Some(self.cur_span()),
            ));
        }
        if !has_prototype {
            self.parse_old_style_parameter_declarations(&mut params)?;
        }
        if params.iter().any(|(param, _)| param.is_empty()) {
            return Err(Diagnostic::error(
                "function definition parameters must be named",
                Some(self.cur_span()),
            ));
        }
        self.finish_function(
            name,
            *ret,
            params,
            !has_prototype,
            is_variadic,
            is_static || (is_inline && !is_extern),
            is_inline,
        )
    }

    fn parse_old_style_parameter_declarations(
        &mut self,
        params: &mut [(String, Type)],
    ) -> Result<(), Diagnostic> {
        let mut declared = HashMap::new();
        while self.is_type() {
            if self.take(&TokenKind::Typedef) {
                return Err(Diagnostic::error(
                    "typedef is not allowed in an old-style parameter declaration",
                    Some(self.cur_span()),
                ));
            }
            if self.take(&TokenKind::Auto)
                || self.take(&TokenKind::Static)
                || self.take(&TokenKind::Extern)
            {
                return Err(Diagnostic::error(
                    "invalid storage class in old-style parameter declaration",
                    Some(self.cur_span()),
                ));
            }
            self.take(&TokenKind::Register);
            let base = self.typ()?;
            loop {
                let (name, ty) = self.declarator_full(base.clone())?;
                if !params.iter().any(|(parameter, _)| parameter == &name) {
                    return Err(Diagnostic::error(
                        format!("{} is not a parameter", name),
                        Some(self.cur_span()),
                    ));
                }
                if declared.insert(name.clone(), ty).is_some() {
                    return Err(Diagnostic::error(
                        format!("duplicate declaration for parameter {}", name),
                        Some(self.cur_span()),
                    ));
                }
                if !self.take(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::Semicolon)?;
        }
        for (name, ty) in params {
            let Some(declared_ty) = declared.remove(name) else {
                return Err(Diagnostic::error(
                    format!("parameter {} has no declaration", name),
                    Some(self.cur_span()),
                ));
            };
            *ty = Self::adjust_parameter_type(declared_ty);
        }
        Ok(())
    }

    fn nested_function_declaration(&mut self) -> Result<Function, Diagnostic> {
        let start = self.cur_span().start;
        let mut is_static = false;
        let mut is_extern = false;
        let mut is_inline = false;
        loop {
            if self.take(&TokenKind::Static) {
                is_static = true;
            } else if self.take(&TokenKind::Extern) {
                is_extern = true;
            } else if self.take(&TokenKind::Inline) {
                is_inline = true;
            } else {
                break;
            }
        }
        let base = self.typ()?;
        let (name, ty) = self.declarator_full(base)?;
        let Type::Function(ret, params, is_variadic, has_prototype) = ty else {
            return Err(Diagnostic::error(
                "expected function declarator",
                Some(self.cur_span()),
            ));
        };
        self.expect(&TokenKind::Semicolon)?;
        Ok(Function {
            name,
            ret: *ret,
            params: params.into_iter().map(|ty| (String::new(), ty)).collect(),
            params_unspecified: !has_prototype,
            is_variadic,
            body: Block { stmts: Vec::new() },
            external: true,
            is_static: is_static || (is_inline && !is_extern),
            is_inline,
            span: self.span_from(start),
        })
    }

    fn global_declaration(&mut self, declarations: &mut Vec<GlobalDecl>) -> Result<(), Diagnostic> {
        let start = self.cur_span().start;
        let mut storage = None;
        let mut is_inline = false;
        loop {
            let class = match self.cur() {
                TokenKind::Static => Some(StorageClass::Static),
                TokenKind::Extern => Some(StorageClass::Extern),
                TokenKind::Auto => {
                    return Err(Diagnostic::error(
                        "auto is not allowed at file scope",
                        Some(self.cur_span()),
                    ))
                }
                TokenKind::Register => {
                    return Err(Diagnostic::error(
                        "register is not allowed at file scope",
                        Some(self.cur_span()),
                    ))
                }
                _ => None,
            };
            if let Some(class) = class {
                self.p += 1;
                if storage.replace(class).is_some() {
                    return Err(Diagnostic::error(
                        "multiple storage-class specifiers",
                        Some(self.cur_span()),
                    ));
                }
            } else if self.take(&TokenKind::Inline) {
                is_inline = true;
            } else {
                break;
            }
        }
        let base = self.typ()?;
        if self.take(&TokenKind::Semicolon) {
            if matches!(
                base,
                Type::Struct(_, _) | Type::Union(_, _) | Type::Enum(_, _)
            ) {
                return Ok(());
            }
            return Err(Diagnostic::error(
                "declaration requires a declarator",
                Some(self.cur_span()),
            ));
        }
        let mut declarations_here = Vec::new();
        loop {
            let (name, ty) = self.declarator_full(base.clone())?;
            if name.is_empty() {
                return Err(Diagnostic::error(
                    "file-scope declarator requires a name",
                    Some(self.cur_span()),
                ));
            }
            if let Type::Function(ret, params, is_variadic, has_prototype) = ty {
                if self.take(&TokenKind::Assign) {
                    return Err(Diagnostic::error(
                        "function declaration cannot have an initializer",
                        Some(self.cur_span()),
                    ));
                }
                self.aliases[0].insert(name.clone(), None);
                self.pending_functions.push(Function {
                    name,
                    ret: *ret,
                    params: params.into_iter().map(|ty| (String::new(), ty)).collect(),
                    params_unspecified: !has_prototype,
                    is_variadic,
                    body: Block { stmts: Vec::new() },
                    external: true,
                    is_static: storage == Some(StorageClass::Static)
                        || (is_inline && storage != Some(StorageClass::Extern)),
                    is_inline,
                    span: self.span_from(start),
                });
                if !self.take(&TokenKind::Comma) {
                    break;
                }
                continue;
            }
            let initializer = if self.take(&TokenKind::Assign) {
                Some(self.initializer()?)
            } else {
                None
            };
            if is_inline {
                return Err(Diagnostic::error(
                    "inline is only valid on a function declaration",
                    Some(self.cur_span()),
                ));
            }
            let tentative_definition =
                storage != Some(StorageClass::Extern) || initializer.is_some();
            self.aliases[0].insert(name.clone(), None);
            declarations_here.push(GlobalDecl {
                name,
                ty,
                storage,
                initializer,
                tentative_definition,
                is_inline,
                span: self.span_from(start),
            });
            if !self.take(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::Semicolon)?;
        declarations.extend(declarations_here);
        Ok(())
    }
    fn function(&mut self) -> Result<Vec<Function>, Diagnostic> {
        let mut storage = None;
        let mut is_inline = false;
        loop {
            let class = match self.cur() {
                TokenKind::Static => Some(StorageClass::Static),
                TokenKind::Extern => Some(StorageClass::Extern),
                TokenKind::Auto => Some(StorageClass::Auto),
                TokenKind::Register => Some(StorageClass::Register),
                _ => None,
            };
            if let Some(class) = class {
                self.p += 1;
                if storage.replace(class).is_some() {
                    return Err(Diagnostic::error(
                        "multiple storage-class specifiers",
                        Some(self.cur_span()),
                    ));
                }
            } else if self.take(&TokenKind::Inline) {
                is_inline = true;
            } else {
                break;
            }
        }
        if matches!(storage, Some(StorageClass::Auto | StorageClass::Register)) {
            return Err(Diagnostic::error(
                "function declaration cannot use auto or register",
                Some(self.cur_span()),
            ));
        }
        let ret = self.typ()?;
        self.function_after_type(
            ret,
            storage == Some(StorageClass::Static)
                || (is_inline && storage != Some(StorageClass::Extern)),
            storage == Some(StorageClass::Extern),
            is_inline,
        )
    }
    fn function_after_type(
        &mut self,
        ret: Type,
        is_static: bool,
        is_extern: bool,
        is_inline: bool,
    ) -> Result<Vec<Function>, Diagnostic> {
        let start = self.cur_span().start;
        let declaration_base = ret.clone();
        let (name, ret) = self.declarator(ret)?;
        if name.is_empty() {
            return Err(Diagnostic::error(
                "function declarator requires a name",
                Some(self.cur_span()),
            ));
        }
        self.expect(&TokenKind::LParen)?;
        self.enter_scope();
        let mut params = Vec::new();
        let mut params_unspecified = false;
        let mut is_variadic = false;
        if matches!(self.cur(), TokenKind::Ident(name) if self.alias_type(name).is_none()) {
            let first = self.ident()?;
            self.bind_ordinary_name(first.clone());
            let mut names = vec![first];
            while self.take(&TokenKind::Comma) {
                let name = self.ident()?;
                self.bind_ordinary_name(name.clone());
                names.push(name);
            }
            self.expect(&TokenKind::RParen)?;
            if *self.cur() == TokenKind::Semicolon || *self.cur() == TokenKind::Comma {
                let function = Function {
                    name,
                    ret,
                    params: Vec::new(),
                    params_unspecified: true,
                    is_variadic: false,
                    body: Block { stmts: Vec::new() },
                    external: true,
                    is_static,
                    is_inline,
                    span: self.span_from(start),
                };
                if *self.cur() == TokenKind::Semicolon || *self.cur() == TokenKind::Comma {
                    self.capture_parameter_tags();
                    self.exit_scope();
                    return self.finish_function_declaration_list(
                        function,
                        declaration_base,
                        is_extern,
                    );
                }
                self.capture_parameter_tags();
                self.exit_scope();
                return self
                    .finish_function(
                        function.name,
                        function.ret,
                        function.params,
                        function.params_unspecified,
                        function.is_variadic,
                        function.is_static,
                        function.is_inline,
                    )
                    .map(|function| vec![function]);
            }
            params = names.into_iter().map(|name| (name, Type::Int)).collect();
            let mut declared = HashMap::new();
            while self.is_type() {
                if self.take(&TokenKind::Typedef) {
                    return Err(Diagnostic::error(
                        "typedef is not allowed in an old-style parameter declaration list",
                        Some(self.cur_span()),
                    ));
                }
                if self.take(&TokenKind::Auto)
                    || self.take(&TokenKind::Static)
                    || self.take(&TokenKind::Extern)
                {
                    return Err(Diagnostic::error(
                        "invalid storage class in old-style parameter declaration",
                        Some(self.cur_span()),
                    ));
                }
                self.take(&TokenKind::Register);
                let param_ty = self.typ()?;
                loop {
                    let (param_name, param_ty) = self.declarator_full(param_ty.clone())?;
                    if !params.iter().any(|(name, _)| name == &param_name) {
                        return Err(Diagnostic::error(
                            format!("{} is not a parameter", param_name),
                            Some(self.cur_span()),
                        ));
                    }
                    if declared
                        .insert(param_name.clone(), param_ty.clone())
                        .is_some()
                    {
                        return Err(Diagnostic::error(
                            format!("duplicate declaration for parameter {}", param_name),
                            Some(self.cur_span()),
                        ));
                    }
                    if !self.take(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::Semicolon)?;
            }
            for (name, ty) in &mut params {
                let Some(declared_ty) = declared.remove(name) else {
                    return Err(Diagnostic::error(
                        format!("parameter {} has no declaration", name),
                        Some(self.cur_span()),
                    ));
                };
                *ty = Self::adjust_parameter_type(declared_ty);
            }
            if let Some(name) = declared.keys().next() {
                return Err(Diagnostic::error(
                    format!("parameter {} was declared more than once", name),
                    Some(self.cur_span()),
                ));
            }
            self.capture_parameter_tags();
            self.exit_scope();
            return self
                .finish_function(name, ret, params, false, false, is_static, is_inline)
                .map(|function| vec![function]);
        }
        if *self.cur() != TokenKind::RParen {
            if *self.cur() == TokenKind::Void
                && self
                    .t
                    .get(self.p + 1)
                    .is_some_and(|token| token.kind == TokenKind::RParen)
            {
                self.p += 1;
                self.expect(&TokenKind::RParen)?;
                let function = Function {
                    name,
                    ret,
                    params,
                    params_unspecified: false,
                    is_variadic: false,
                    body: Block { stmts: Vec::new() },
                    external: true,
                    is_static,
                    is_inline,
                    span: self.span_from(start),
                };
                if *self.cur() == TokenKind::Semicolon || *self.cur() == TokenKind::Comma {
                    self.capture_parameter_tags();
                    self.exit_scope();
                    return self.finish_function_declaration_list(
                        function,
                        declaration_base,
                        is_extern,
                    );
                }
                self.capture_parameter_tags();
                self.exit_scope();
                return self
                    .finish_function(
                        function.name,
                        function.ret,
                        function.params,
                        function.params_unspecified,
                        function.is_variadic,
                        function.is_static,
                        function.is_inline,
                    )
                    .map(|function| vec![function]);
            }
            loop {
                self.take(&TokenKind::Register);
                let ty = self.typ()?;
                let (n, ty) = self.declarator_full(ty)?;
                if !n.is_empty() {
                    self.bind_ordinary_name(n.clone());
                }
                params.push((n, Self::adjust_parameter_type(ty)));
                if !self.take(&TokenKind::Comma) {
                    break;
                }
                if self.take(&TokenKind::Ellipsis) {
                    is_variadic = true;
                    break;
                }
            }
        } else {
            params_unspecified = true;
        }
        self.expect(&TokenKind::RParen)?;
        if *self.cur() == TokenKind::Semicolon || *self.cur() == TokenKind::Comma {
            let function = Function {
                name,
                ret,
                params,
                params_unspecified,
                is_variadic,
                body: Block { stmts: Vec::new() },
                external: true,
                is_static,
                is_inline,
                span: self.span_from(start),
            };
            self.capture_parameter_tags();
            self.exit_scope();
            return self.finish_function_declaration_list(function, declaration_base, is_extern);
        }
        self.capture_parameter_tags();
        self.exit_scope();
        self.finish_function(
            name,
            ret,
            params,
            params_unspecified,
            is_variadic,
            is_static,
            is_inline,
        )
        .map(|function| vec![function])
    }

    fn finish_function_declaration_list(
        &mut self,
        first: Function,
        declaration_base: Type,
        is_extern: bool,
    ) -> Result<Vec<Function>, Diagnostic> {
        let is_static = first.is_static;
        let is_inline = first.is_inline;
        let mut functions = vec![first];
        loop {
            if self.take(&TokenKind::Semicolon) {
                return Ok(functions);
            }
            self.expect(&TokenKind::Comma)?;
            let DeclaratorDetails {
                name,
                ty,
                params,
                is_variadic,
                has_prototype,
                is_function_declarator,
            } = self.declarator_full_details(declaration_base.clone())?;
            if !matches!(ty, Type::Function(_, _, _, _)) {
                if is_inline {
                    return Err(Diagnostic::error(
                        "inline is only valid on a function declaration",
                        Some(self.cur_span()),
                    ));
                }
                let initializer = if self.take(&TokenKind::Assign) {
                    Some(self.initializer()?)
                } else {
                    None
                };
                let storage = if is_static {
                    Some(StorageClass::Static)
                } else if is_extern {
                    Some(StorageClass::Extern)
                } else {
                    None
                };
                self.aliases[0].insert(name.clone(), None);
                self.pending_declarations.push(GlobalDecl {
                    name,
                    ty,
                    storage,
                    tentative_definition: storage != Some(StorageClass::Extern)
                        || initializer.is_some(),
                    initializer,
                    is_inline: false,
                    span: self.span_from(self.cur_span().start),
                });
                continue;
            }
            let Type::Function(ret, _, _, _) = ty else {
                unreachable!()
            };
            if !is_function_declarator {
                return Err(Diagnostic::error(
                    "expected function declarator after comma",
                    Some(self.cur_span()),
                ));
            }
            functions.push(Function {
                name,
                ret: *ret,
                params: if has_prototype { params } else { Vec::new() },
                params_unspecified: !has_prototype,
                is_variadic,
                body: Block { stmts: Vec::new() },
                external: true,
                is_static,
                is_inline,
                span: self.span_from(self.cur_span().start),
            });
        }
    }
    fn finish_function(
        &mut self,
        name: String,
        ret: Type,
        params: Vec<(String, Type)>,
        params_unspecified: bool,
        is_variadic: bool,
        is_static: bool,
        is_inline: bool,
    ) -> Result<Function, Diagnostic> {
        let parameter_tags = self.pending_parameter_tags.take().unwrap_or_default();
        let start = self.cur_span().start;
        let external = self.take(&TokenKind::Semicolon);
        if !external && params.iter().any(|(name, _)| name.is_empty()) {
            return Err(Diagnostic::error(
                "function definition parameters must be named",
                Some(self.cur_span()),
            ));
        }
        let body = if external {
            Block { stmts: Vec::new() }
        } else {
            self.enter_scope();
            self.tags.last_mut().unwrap().extend(parameter_tags);
            for (param, _) in &params {
                self.bind_ordinary_name(param.clone());
            }
            self.block()?
        };
        if !external {
            self.exit_scope();
        }
        Ok(Function {
            name,
            ret,
            params,
            params_unspecified,
            is_variadic,
            body,
            external,
            is_static,
            is_inline,
            span: Span {
                start,
                end: self.t[self.p - 1].span.end,
            },
        })
    }
    fn block(&mut self) -> Result<Block, Diagnostic> {
        self.expect(&TokenKind::LBrace)?;
        self.enter_scope();
        let mut v = Vec::new();
        while *self.cur() != TokenKind::RBrace && *self.cur() != TokenKind::End {
            match self.stmt() {
                Ok(stmt) => v.push(stmt),
                Err(error) => {
                    self.errors.push(error);
                    self.synchronize(true);
                }
            }
        }
        // Keep the alias scope balanced even when the closing brace is missing.
        let closing = self.expect(&TokenKind::RBrace);
        self.exit_scope();
        closing?;
        Ok(Block { stmts: v })
    }
    fn synchronize(&mut self, in_block: bool) {
        while *self.cur() != TokenKind::End
            && (!in_block || *self.cur() != TokenKind::RBrace)
            && *self.cur() != TokenKind::Semicolon
        {
            self.p += 1;
        }
        self.take(&TokenKind::Semicolon);
    }
    fn stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.cur_span().start;
        if self.take(&TokenKind::LBrace) {
            self.p -= 1;
            let block = self.block()?;
            return Ok(Stmt::Block(block, self.span_from(start)));
        }
        if self.take(&TokenKind::Return) {
            let x = if *self.cur() == TokenKind::Semicolon {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt::Return(x, self.span_from(start)));
        }
        if self.take(&TokenKind::If) {
            self.expect(&TokenKind::LParen)?;
            let c = self.expr()?;
            self.expect(&TokenKind::RParen)?;
            let a = Box::new(self.stmt()?);
            let b = if self.take(&TokenKind::Else) {
                Some(Box::new(self.stmt()?))
            } else {
                None
            };
            return Ok(Stmt::If(c, a, b, self.span_from(start)));
        }
        if self.take(&TokenKind::While) {
            self.expect(&TokenKind::LParen)?;
            let c = self.expr()?;
            self.expect(&TokenKind::RParen)?;
            return Ok(Stmt::While(
                c,
                Box::new(self.stmt()?),
                self.span_from(start),
            ));
        }
        if self.take(&TokenKind::Do) {
            let body = Box::new(self.stmt()?);
            self.expect(&TokenKind::While)?;
            self.expect(&TokenKind::LParen)?;
            let c = self.expr()?;
            self.expect(&TokenKind::RParen)?;
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt::DoWhile(body, c, self.span_from(start)));
        }
        if self.take(&TokenKind::For) {
            self.expect(&TokenKind::LParen)?;
            self.enter_scope();
            let result = (|| {
                let init = if self.take(&TokenKind::Semicolon) {
                    None
                } else {
                    let s = if self.is_type() {
                        self.declaration()?
                    } else {
                        let expr_start = self.cur_span().start;
                        let e = self.expr()?;
                        self.expect(&TokenKind::Semicolon)?;
                        Stmt::Expr(e, self.span_from(expr_start))
                    };
                    Some(Box::new(s))
                };
                let cond = if self.take(&TokenKind::Semicolon) {
                    None
                } else {
                    let e = self.expr()?;
                    self.expect(&TokenKind::Semicolon)?;
                    Some(e)
                };
                let step = if self.take(&TokenKind::RParen) {
                    None
                } else {
                    let e = self.expr()?;
                    self.expect(&TokenKind::RParen)?;
                    Some(e)
                };
                Ok(Stmt::For(
                    init,
                    cond,
                    step,
                    Box::new(self.stmt()?),
                    self.span_from(start),
                ))
            })();
            self.exit_scope();
            return result;
        }
        if self.take(&TokenKind::Break) {
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt::Break(self.span_from(start)));
        }
        if self.take(&TokenKind::Continue) {
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt::Continue(self.span_from(start)));
        }
        if self.is_type() {
            return self.declaration();
        }
        if self.take(&TokenKind::Semicolon) {
            return Ok(Stmt::Block(
                Block { stmts: Vec::new() },
                self.span_from(start),
            ));
        }
        let x = self.expr()?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Expr(x, self.span_from(start)))
    }
    fn is_type(&self) -> bool {
        matches!(self.cur(), TokenKind::Ident(name) if self.alias_type(name).is_some())
            || matches!(
                self.cur(),
                TokenKind::Int
                    | TokenKind::Typedef
                    | TokenKind::Bool
                    | TokenKind::CharKw
                    | TokenKind::Void
                    | TokenKind::Struct
                    | TokenKind::Union
                    | TokenKind::Enum
                    | TokenKind::Const
                    | TokenKind::Volatile
                    | TokenKind::Restrict
                    | TokenKind::Signed
                    | TokenKind::Unsigned
                    | TokenKind::Short
                    | TokenKind::Long
                    | TokenKind::Float
                    | TokenKind::Double
                    | TokenKind::Complex
                    | TokenKind::Imaginary
                    | TokenKind::Auto
                    | TokenKind::Extern
                    | TokenKind::Register
                    | TokenKind::Static
                    | TokenKind::Inline
            )
    }
    fn declaration(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.cur_span().start;
        let is_typedef = self.take(&TokenKind::Typedef);
        let mut storage = None;
        let mut inline = false;
        loop {
            let next = match self.cur() {
                TokenKind::Auto => Some(StorageClass::Auto),
                TokenKind::Extern => Some(StorageClass::Extern),
                TokenKind::Register => Some(StorageClass::Register),
                TokenKind::Static => Some(StorageClass::Static),
                _ => None,
            };
            if let Some(class) = next {
                self.p += 1;
                if storage.replace(class).is_some() {
                    return Err(Diagnostic::error(
                        "multiple storage-class specifiers",
                        Some(self.cur_span()),
                    ));
                }
            } else if self.take(&TokenKind::Inline) {
                inline = true;
            } else {
                break;
            }
        }
        let ty = self.typ()?;
        loop {
            let next = match self.cur() {
                TokenKind::Auto => Some(StorageClass::Auto),
                TokenKind::Extern => Some(StorageClass::Extern),
                TokenKind::Register => Some(StorageClass::Register),
                TokenKind::Static => Some(StorageClass::Static),
                _ => None,
            };
            if let Some(class) = next {
                self.p += 1;
                if storage.replace(class).is_some() {
                    return Err(Diagnostic::error(
                        "multiple storage-class specifiers",
                        Some(self.cur_span()),
                    ));
                }
            } else if self.take(&TokenKind::Inline) {
                inline = true;
            } else {
                break;
            }
        }
        if inline {
            return Err(Diagnostic::error(
                "inline is only valid on a function declaration",
                Some(self.cur_span()),
            ));
        }
        if self.take(&TokenKind::Semicolon) {
            if matches!(
                ty,
                Type::Struct(_, _) | Type::Union(_, _) | Type::Enum(_, _)
            ) {
                return Ok(Stmt::Block(
                    Block { stmts: Vec::new() },
                    self.span_from(start),
                ));
            }
            return Err(Diagnostic::error(
                "declaration requires a declarator",
                Some(self.cur_span()),
            ));
        }
        if is_typedef && storage.is_some() {
            return Err(Diagnostic::error(
                "typedef cannot be combined with another storage-class specifier",
                Some(self.cur_span()),
            ));
        }
        if is_typedef {
            let mut aliases = Vec::new();
            loop {
                let (name, declared_ty) = self.declarator_full(ty.clone())?;
                if name.is_empty() {
                    return Err(Diagnostic::error(
                        "typedef declarator requires a name",
                        Some(self.cur_span()),
                    ));
                }
                self.aliases
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), Some(declared_ty.clone()));
                aliases.push((name, declared_ty));
                if !self.take(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::Semicolon)?;
            return if aliases.len() == 1 {
                let (name, ty) = aliases.pop().unwrap();
                Ok(Stmt::Typedef(name, ty, self.span_from(start)))
            } else {
                Ok(Stmt::Typedefs(aliases, self.span_from(start)))
            };
        }
        let mut declarations = Vec::new();
        loop {
            let (n, declared_ty) = self.declarator_full(ty.clone())?;
            if n.is_empty() {
                return Err(Diagnostic::error(
                    "declaration requires a name",
                    Some(self.cur_span()),
                ));
            }
            let init = if self.take(&TokenKind::Assign) {
                Some(self.initializer()?)
            } else {
                None
            };
            declarations.push((n, declared_ty, init));
            self.bind_ordinary_name(declarations.last().unwrap().0.clone());
            if !self.take(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::Semicolon)?;
        if let Some(storage) = storage {
            Ok(Stmt::StorageDecl(
                storage,
                declarations,
                self.span_from(start),
            ))
        } else if declarations.len() == 1 {
            let (name, ty, init) = declarations.pop().unwrap();
            Ok(Stmt::Decl(name, ty, init, self.span_from(start)))
        } else {
            Ok(Stmt::Decls(declarations, self.span_from(start)))
        }
    }
    fn declarator(&mut self, mut ty: Type) -> Result<(String, Type), Diagnostic> {
        self.declarator_inner(&mut ty, false)
    }

    fn declarator_full(&mut self, mut ty: Type) -> Result<(String, Type), Diagnostic> {
        self.declarator_inner(&mut ty, true)
    }

    fn declarator_full_details(&mut self, mut ty: Type) -> Result<DeclaratorDetails, Diagnostic> {
        while let Some(qualifier) = self.take_qualifier() {
            ty = Self::qualified(ty, qualifier);
        }
        let (name, ops) = self.declarator_ops(true)?;
        let (params, variadic, has_prototype, is_function_declarator) = match ops.first() {
            Some(DeclaratorOp::Function(params, variadic, has_prototype)) => {
                (params.clone(), *variadic, *has_prototype, true)
            }
            _ => (Vec::new(), false, false, false),
        };
        ty = Self::apply_declarator_ops(ty, ops);
        Ok(DeclaratorDetails {
            name,
            ty,
            params,
            is_variadic: variadic,
            has_prototype,
            is_function_declarator,
        })
    }

    fn declarator_inner(
        &mut self,
        ty: &mut Type,
        allow_function: bool,
    ) -> Result<(String, Type), Diagnostic> {
        while let Some(qualifier) = self.take_qualifier() {
            *ty = Self::qualified(ty.clone(), qualifier);
        }
        let (name, ops) = self.declarator_ops(allow_function)?;
        *ty = Self::apply_declarator_ops(ty.clone(), ops);
        Ok((name, ty.clone()))
    }

    fn apply_declarator_ops(mut ty: Type, ops: Vec<DeclaratorOp>) -> Type {
        for op in ops.into_iter().rev() {
            ty = match op {
                DeclaratorOp::Pointer(q) => {
                    let pointer = Type::Pointer(Box::new(ty.clone()));
                    if q == Qualifiers::default() {
                        pointer
                    } else {
                        Type::Qualified(Box::new(pointer), q)
                    }
                }
                DeclaratorOp::Array(bound, mut qualifiers, is_static) => {
                    let array = match bound {
                        ArrayBound::Fixed(size) => {
                            Type::Array(Box::new(ty.clone()), Some(size), is_static)
                        }
                        ArrayBound::Unspecified => {
                            Type::Array(Box::new(ty.clone()), None, is_static)
                        }
                        ArrayBound::Vla(bound) => {
                            qualifiers.is_static |= is_static;
                            Type::Vla(Box::new(ty.clone()), bound)
                        }
                    };
                    if qualifiers == Qualifiers::default() {
                        array
                    } else {
                        Self::qualified(array, qualifiers)
                    }
                }
                DeclaratorOp::Function(params, variadic, has_prototype) => Type::Function(
                    Box::new(ty.clone()),
                    if has_prototype {
                        params.into_iter().map(|(_, ty)| ty).collect()
                    } else {
                        Vec::new()
                    },
                    variadic,
                    has_prototype,
                ),
            };
        }
        ty
    }

    fn take_qualifier(&mut self) -> Option<Qualifiers> {
        let qualifier = match self.cur() {
            TokenKind::Const => Qualifiers {
                is_const: true,
                ..Default::default()
            },
            TokenKind::Volatile => Qualifiers {
                is_volatile: true,
                ..Default::default()
            },
            TokenKind::Restrict => Qualifiers {
                is_restrict: true,
                ..Default::default()
            },
            _ => return None,
        };
        self.p += 1;
        Some(qualifier)
    }

    fn qualified_type_specifier(ty: Type, q: Qualifiers) -> Type {
        match ty {
            Type::Array(element, count, is_static) => {
                Type::Array(Box::new(Self::qualified(*element, q)), count, is_static)
            }
            Type::Vla(element, bound) => Type::Vla(Box::new(Self::qualified(*element, q)), bound),
            ty => Self::qualified(ty, q),
        }
    }

    fn qualified(ty: Type, q: Qualifiers) -> Type {
        match ty {
            Type::Qualified(inner, mut existing) => {
                existing.is_const |= q.is_const;
                existing.is_volatile |= q.is_volatile;
                existing.is_restrict |= q.is_restrict;
                existing.is_static |= q.is_static;
                Type::Qualified(inner, existing)
            }
            ty => Type::Qualified(Box::new(ty), q),
        }
    }

    fn adjust_parameter_type(ty: Type) -> Type {
        match ty {
            Type::Array(element, _, is_static) => Self::qualified(
                Type::Pointer(element),
                Qualifiers {
                    is_static,
                    ..Default::default()
                },
            ),
            Type::Vla(element, _) => Type::Pointer(element),
            Type::Function(_, _, _, _) => Type::Pointer(Box::new(ty)),
            Type::Qualified(inner, q) => match *inner {
                Type::Array(element, _, is_static) => Self::qualified(
                    Type::Pointer(element),
                    Qualifiers {
                        is_static: q.is_static || is_static,
                        ..q
                    },
                ),
                Type::Vla(element, _) => Self::qualified(Type::Pointer(element), q),
                Type::Function(_, _, _, _) => Self::qualified(Type::Pointer(inner), q),
                other => Self::qualified(other, q),
            },
            other => other,
        }
    }

    fn declarator_ops(
        &mut self,
        allow_function: bool,
    ) -> Result<(String, Vec<DeclaratorOp>), Diagnostic> {
        let mut pointers = Vec::new();
        while self.take(&TokenKind::Star) {
            let mut qualifiers = Qualifiers::default();
            while let Some(q) = self.take_qualifier() {
                qualifiers.is_const |= q.is_const;
                qualifiers.is_volatile |= q.is_volatile;
                qualifiers.is_restrict |= q.is_restrict;
            }
            pointers.push(qualifiers);
        }
        let (name, mut ops) = if self.take(&TokenKind::LParen) {
            let inner = self.declarator_ops(allow_function)?;
            self.expect(&TokenKind::RParen)?;
            inner
        } else if matches!(self.cur(), TokenKind::Ident(_)) {
            (self.ident()?, Vec::new())
        } else {
            (String::new(), Vec::new())
        };
        loop {
            if self.take(&TokenKind::LBracket) {
                let mut is_static = self.take(&TokenKind::Static);
                let mut qualifiers = Qualifiers::default();
                while let Some(q) = self.take_qualifier() {
                    qualifiers.is_const |= q.is_const;
                    qualifiers.is_volatile |= q.is_volatile;
                    qualifiers.is_restrict |= q.is_restrict;
                }
                is_static |= self.take(&TokenKind::Static);
                let bound = if matches!(self.cur(), TokenKind::Number(_)) {
                    if let TokenKind::Number(n) = self.cur().clone() {
                        self.p += 1;
                        let bound = usize::try_from(n).map_err(|_| {
                            Diagnostic::error(
                                "array bound must be a nonnegative integer",
                                Some(self.cur_span()),
                            )
                        })?;
                        ArrayBound::Fixed(bound)
                    } else {
                        unreachable!()
                    }
                } else if self.take(&TokenKind::Star) {
                    ArrayBound::Unspecified
                } else if *self.cur() != TokenKind::RBracket {
                    let expression = self.assign()?;
                    if let Some(value) = Self::integer_constant(&expression) {
                        let size = usize::try_from(value).map_err(|_| {
                            Diagnostic::error(
                                "array bound must be a positive integer constant expression",
                                Some(self.cur_span()),
                            )
                        })?;
                        if size == 0 {
                            return Err(Diagnostic::error(
                                "array bound must be a positive integer constant expression",
                                Some(self.cur_span()),
                            ));
                        }
                        ArrayBound::Fixed(size)
                    } else {
                        ArrayBound::Vla(Box::new(expression))
                    }
                } else {
                    ArrayBound::Unspecified
                };
                if is_static && matches!(bound, ArrayBound::Unspecified) {
                    return Err(Diagnostic::error(
                        "a static array parameter requires a size",
                        Some(self.cur_span()),
                    ));
                }
                self.expect(&TokenKind::RBracket)?;
                ops.push(DeclaratorOp::Array(bound, qualifiers, is_static));
                continue;
            }
            if allow_function && self.take(&TokenKind::LParen) {
                self.enter_scope();
                let parsed = (|| -> Result<(Vec<(String, Type)>, bool, bool), Diagnostic> {
                    let mut params = Vec::new();
                    let mut variadic = false;
                    let mut has_prototype = !self.take(&TokenKind::RParen);
                    if has_prototype {
                        if matches!(self.cur(), TokenKind::Ident(name) if self.alias_type(name).is_none())
                        {
                            let name = self.ident()?;
                            self.bind_ordinary_name(name.clone());
                            params.push((name, Type::Int));
                            while self.take(&TokenKind::Comma) {
                                let name = self.ident()?;
                                self.bind_ordinary_name(name.clone());
                                params.push((name, Type::Int));
                            }
                            self.expect(&TokenKind::RParen)?;
                            has_prototype = false;
                        } else if *self.cur() == TokenKind::Void
                            && self
                                .t
                                .get(self.p + 1)
                                .is_some_and(|token| token.kind == TokenKind::RParen)
                        {
                            self.p += 2;
                            // `void` denotes an empty parameter list.
                        } else {
                            loop {
                                self.take(&TokenKind::Register);
                                let param_ty = self.typ()?;
                                let (param_name, param_ty) = if matches!(
                                    self.cur(),
                                    TokenKind::Ident(_) | TokenKind::Star | TokenKind::LParen
                                ) {
                                    self.declarator_full(param_ty)?
                                } else {
                                    (String::new(), param_ty)
                                };
                                if !param_name.is_empty() {
                                    self.bind_ordinary_name(param_name.clone());
                                }
                                params.push((param_name, Self::adjust_parameter_type(param_ty)));
                                if !self.take(&TokenKind::Comma) {
                                    break;
                                }
                                if self.take(&TokenKind::Ellipsis) {
                                    variadic = true;
                                    break;
                                }
                            }
                            self.expect(&TokenKind::RParen)?;
                        }
                    }
                    Ok((params, variadic, has_prototype))
                })();
                self.capture_parameter_tags();
                self.exit_scope();
                let (params, variadic, has_prototype) = parsed?;
                ops.push(DeclaratorOp::Function(params, variadic, has_prototype));
                continue;
            }
            break;
        }
        ops.extend(pointers.into_iter().map(DeclaratorOp::Pointer));
        Ok((name, ops))
    }
    fn expr(&mut self) -> Result<Expr, Diagnostic> {
        let mut x = self.assign()?;
        while self.take(&TokenKind::Comma) {
            x = Expr::Binary(BinOp::Comma, Box::new(x), Box::new(self.assign()?));
        }
        Ok(x)
    }
    fn assign(&mut self) -> Result<Expr, Diagnostic> {
        let x = self.binary(1)?;
        if self.take(&TokenKind::Question) {
            let a = self.expr()?;
            self.expect(&TokenKind::Colon)?;
            let b = self.assign()?;
            return Ok(Expr::Conditional(Box::new(x), Box::new(a), Box::new(b)));
        }
        let op = match self.cur() {
            TokenKind::Assign => Some(None),
            TokenKind::AddAssign => Some(Some(BinOp::Add)),
            TokenKind::SubAssign => Some(Some(BinOp::Sub)),
            TokenKind::MulAssign => Some(Some(BinOp::Mul)),
            TokenKind::DivAssign => Some(Some(BinOp::Div)),
            TokenKind::RemAssign => Some(Some(BinOp::Rem)),
            TokenKind::ShlAssign => Some(Some(BinOp::Shl)),
            TokenKind::ShrAssign => Some(Some(BinOp::Shr)),
            TokenKind::AndAssign => Some(Some(BinOp::BitAnd)),
            TokenKind::XorAssign => Some(Some(BinOp::BitXor)),
            TokenKind::OrAssign => Some(Some(BinOp::BitOr)),
            _ => None,
        };
        if let Some(op) = op {
            self.p += 1;
            if Self::is_lvalue(&x) {
                let rhs = self.assign()?;
                Ok(match op {
                    Some(op) => Expr::CompoundAssign(Box::new(x), op, Box::new(rhs)),
                    None => Expr::Assign(Box::new(x), Box::new(rhs)),
                })
            } else {
                Err(Diagnostic::error(
                    "assignment target required",
                    Some(self.cur_span()),
                ))
            }
        } else {
            Ok(x)
        }
    }
    fn is_lvalue(x: &Expr) -> bool {
        matches!(
            x,
            Expr::Var(_)
                | Expr::Member(_, _, _)
                | Expr::Subscript(_, _)
                | Expr::Unary(UnOp::Deref, _)
        )
    }
    fn prec(k: &TokenKind) -> Option<(u8, BinOp)> {
        Some(match k {
            TokenKind::Or => (1, BinOp::Or),
            TokenKind::And => (2, BinOp::And),
            TokenKind::BitOr => (3, BinOp::BitOr),
            TokenKind::Xor => (4, BinOp::BitXor),
            TokenKind::BitAnd => (5, BinOp::BitAnd),
            TokenKind::Eq => (6, BinOp::Eq),
            TokenKind::Ne => (6, BinOp::Ne),
            TokenKind::Lt => (7, BinOp::Lt),
            TokenKind::Le => (7, BinOp::Le),
            TokenKind::Gt => (7, BinOp::Gt),
            TokenKind::Ge => (7, BinOp::Ge),
            TokenKind::Shl => (8, BinOp::Shl),
            TokenKind::Shr => (8, BinOp::Shr),
            TokenKind::Plus => (9, BinOp::Add),
            TokenKind::Minus => (9, BinOp::Sub),
            TokenKind::Star => (10, BinOp::Mul),
            TokenKind::Slash => (10, BinOp::Div),
            TokenKind::Percent => (10, BinOp::Rem),
            _ => return None,
        })
    }
    fn binary(&mut self, min: u8) -> Result<Expr, Diagnostic> {
        let mut x = self.unary()?;
        while let Some((p, op)) = Self::prec(self.cur()) {
            if p < min {
                break;
            }
            self.p += 1;
            let y = self.binary(p + 1)?;
            x = Expr::Binary(op, Box::new(x), Box::new(y));
        }
        Ok(x)
    }
    fn unary(&mut self) -> Result<Expr, Diagnostic> {
        if self.take(&TokenKind::Sizeof) {
            if *self.cur() == TokenKind::LParen && self.lookahead_starts_type_name() {
                self.p += 1;
                let ty = self.type_name()?;
                self.expect(&TokenKind::RParen)?;
                return Ok(Expr::SizeofType(ty));
            }
            return Ok(Expr::Unary(UnOp::Sizeof, Box::new(self.unary()?)));
        }
        if *self.cur() == TokenKind::LParen
            && self
                .t
                .get(self.p + 1)
                .is_some_and(|_| self.lookahead_starts_type_name())
        {
            self.p += 1;
            let ty = self.type_name()?;
            self.expect(&TokenKind::RParen)?;
            if self.take(&TokenKind::LBrace) {
                return Ok(Expr::CompoundLiteral(
                    ty,
                    self.initializer_list_after_lbrace()?,
                ));
            }
            return Ok(Expr::Cast(ty, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::PlusPlus) || self.take(&TokenKind::MinusMinus) {
            let increment = self.t[self.p - 1].kind == TokenKind::PlusPlus;
            return Ok(Expr::IncDec(Box::new(self.unary()?), increment, true));
        }
        if self.take(&TokenKind::Minus) {
            return Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::Plus) {
            return Ok(Expr::Unary(UnOp::Pos, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::Tilde) {
            return Ok(Expr::Unary(UnOp::BitNot, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::Not) {
            return Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::BitAnd) {
            return Ok(Expr::Unary(UnOp::Address, Box::new(self.unary()?)));
        }
        if self.take(&TokenKind::Star) {
            return Ok(Expr::Unary(UnOp::Deref, Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn bitfield_type(ty: &Type) -> bool {
        match ty {
            Type::Qualified(inner, _) => Self::bitfield_type(inner),
            Type::Bool
            | Type::Char
            | Type::SignedChar
            | Type::UnsignedChar
            | Type::Short
            | Type::UnsignedShort
            | Type::Int
            | Type::SignedInt
            | Type::UnsignedInt
            | Type::Enum(_, _) => true,
            _ => false,
        }
    }

    fn integer_constant(expr: &Expr) -> Option<i64> {
        crate::consteval::integer_constant(expr)
    }

    fn initializer(&mut self) -> Result<Initializer, Diagnostic> {
        if self.take(&TokenKind::LBrace) {
            self.initializer_list_after_lbrace()
        } else {
            Ok(Initializer::Expr(Box::new(self.assign()?)))
        }
    }

    fn initializer_list_after_lbrace(&mut self) -> Result<Initializer, Diagnostic> {
        let mut items = Vec::new();
        if *self.cur() != TokenKind::RBrace {
            loop {
                let mut designators = Vec::new();
                while self.take(&TokenKind::LBracket) || self.take(&TokenKind::Dot) {
                    if matches!(self.t[self.p - 1].kind, TokenKind::LBracket) {
                        let index = self.assign()?;
                        self.expect(&TokenKind::RBracket)?;
                        designators.push(Designator::Index(index));
                    } else {
                        designators.push(Designator::Member(self.ident()?));
                    }
                }
                if !designators.is_empty() {
                    self.expect(&TokenKind::Assign)?;
                }
                items.push(InitializerItem {
                    designators,
                    value: self.initializer()?,
                });
                if !self.take(&TokenKind::Comma) || *self.cur() == TokenKind::RBrace {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Initializer::List(items))
    }

    fn parse_float_constant(spelling: &str) -> Option<Expr> {
        let imaginary = matches!(spelling.chars().last(), Some('i' | 'j'));
        let spelling = if imaginary {
            &spelling[..spelling.len() - 1]
        } else {
            spelling
        };
        let (body, ty) = match spelling.chars().last() {
            Some('f' | 'F') => (&spelling[..spelling.len() - 1], Type::Float),
            Some('l' | 'L') => (&spelling[..spelling.len() - 1], Type::LongDouble),
            _ => (spelling, Type::Double),
        };
        let value = if body.starts_with("0x") || body.starts_with("0X") {
            Self::parse_hex_float(body)?
        } else {
            body.parse::<f64>().ok()?
        };
        Some(if imaginary {
            Expr::Complex(0.0, value, ty)
        } else {
            Expr::Float(value, ty)
        })
    }

    fn parse_hex_float(body: &str) -> Option<f64> {
        let body = body
            .strip_prefix("0x")
            .or_else(|| body.strip_prefix("0X"))?;
        let (mantissa, exponent) = body.split_once(['p', 'P'])?;
        let exponent = exponent.parse::<i32>().ok()?;
        let (whole, fraction) = match mantissa.split_once('.') {
            Some((whole, fraction)) => (whole, fraction),
            None => (mantissa, ""),
        };
        let mut value = 0.0f64;
        for digit in whole.chars() {
            value = value * 16.0 + digit.to_digit(16)? as f64;
        }
        let mut place = 1.0 / 16.0;
        for digit in fraction.chars() {
            value += digit.to_digit(16)? as f64 * place;
            place /= 16.0;
        }
        Some(value * 2f64.powi(exponent))
    }

    fn lookahead_starts_type_name(&self) -> bool {
        match self.t.get(self.p + 1).map(|t| &t.kind) {
            Some(TokenKind::Ident(name)) => self.alias_type(name).is_some(),
            Some(k) => matches!(
                k,
                TokenKind::Void
                    | TokenKind::Bool
                    | TokenKind::CharKw
                    | TokenKind::Int
                    | TokenKind::Signed
                    | TokenKind::Unsigned
                    | TokenKind::Short
                    | TokenKind::Long
                    | TokenKind::Float
                    | TokenKind::Double
                    | TokenKind::Complex
                    | TokenKind::Imaginary
                    | TokenKind::Struct
                    | TokenKind::Union
                    | TokenKind::Enum
                    | TokenKind::Const
                    | TokenKind::Volatile
                    | TokenKind::Restrict
            ),
            None => false,
        }
    }

    fn type_name(&mut self) -> Result<Type, Diagnostic> {
        let mut ty = self.typ()?;
        if matches!(
            self.cur(),
            TokenKind::Star | TokenKind::LParen | TokenKind::LBracket
        ) {
            ty = self.declarator_full(ty)?.1;
        }
        Ok(ty)
    }
    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let x = match self.cur().clone() {
            TokenKind::Number(n) => {
                self.p += 1;
                Expr::Int(n)
            }
            TokenKind::FloatConstant(spelling) => {
                self.p += 1;
                Self::parse_float_constant(&spelling).ok_or_else(|| {
                    Diagnostic::error("invalid floating constant", Some(self.cur_span()))
                })?
            }
            TokenKind::Char(c) => {
                self.p += 1;
                Expr::Char(c)
            }
            TokenKind::String(s) => {
                self.p += 1;
                let mut value = s;
                while let TokenKind::String(next) = self.cur().clone() {
                    self.p += 1;
                    value.push_str(&next);
                }
                Expr::String(value)
            }
            TokenKind::Ident(n) => {
                self.p += 1;
                if self.take(&TokenKind::LParen) {
                    if matches!(n.as_str(), "va_arg" | "__builtin_va_arg") {
                        let ap = self.assign()?;
                        self.expect(&TokenKind::Comma)?;
                        let ty = self.type_name()?;
                        self.expect(&TokenKind::RParen)?;
                        Expr::VaArg(Box::new(ap), ty)
                    } else if matches!(n.as_str(), "va_start" | "__builtin_va_start") {
                        let ap = self.assign()?;
                        self.expect(&TokenKind::Comma)?;
                        let _last = self.assign()?;
                        self.expect(&TokenKind::RParen)?;
                        Expr::VaStart(Box::new(ap))
                    } else if matches!(n.as_str(), "va_end" | "__builtin_va_end") {
                        let ap = self.assign()?;
                        self.expect(&TokenKind::RParen)?;
                        Expr::VaEnd(Box::new(ap))
                    } else if matches!(n.as_str(), "va_copy" | "__builtin_va_copy") {
                        let destination = self.assign()?;
                        self.expect(&TokenKind::Comma)?;
                        let source = self.assign()?;
                        self.expect(&TokenKind::RParen)?;
                        Expr::VaCopy(Box::new(destination), Box::new(source))
                    } else {
                        let mut a = Vec::new();
                        if *self.cur() != TokenKind::RParen {
                            loop {
                                a.push(self.assign()?);
                                if !self.take(&TokenKind::Comma) {
                                    break;
                                }
                            }
                        }
                        self.expect(&TokenKind::RParen)?;
                        Expr::Call(n, a)
                    }
                } else if let Some(value) = self.constant_value(&n) {
                    Expr::Int(value)
                } else {
                    Expr::Var(n)
                }
            }
            TokenKind::LParen => {
                self.p += 1;
                let x = self.expr()?;
                self.expect(&TokenKind::RParen)?;
                x
            }
            _ => {
                return Err(Diagnostic::error(
                    "expected expression",
                    Some(self.cur_span()),
                ))
            }
        };
        let mut x = x;
        loop {
            if self.take(&TokenKind::LBracket) {
                let index = self.expr()?;
                self.expect(&TokenKind::RBracket)?;
                x = Expr::Subscript(Box::new(x), Box::new(index));
                continue;
            }
            if self.take(&TokenKind::LParen) {
                let mut arguments = Vec::new();
                if *self.cur() != TokenKind::RParen {
                    loop {
                        arguments.push(self.assign()?);
                        if !self.take(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RParen)?;
                x = Expr::IndirectCall(Box::new(x), arguments);
                continue;
            }
            if self.take(&TokenKind::PlusPlus) {
                x = Expr::IncDec(Box::new(x), true, false);
                continue;
            }
            if self.take(&TokenKind::MinusMinus) {
                x = Expr::IncDec(Box::new(x), false, false);
                continue;
            }
            let arrow = self.take(&TokenKind::Arrow);
            let dot = arrow || self.take(&TokenKind::Dot);
            if !dot {
                break;
            }
            let name = self.ident()?;
            x = Expr::Member(Box::new(x), name, arrow);
        }
        Ok(x)
    }
}
