use crate::diag::{Diagnostic, Span};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

/// Apply translation phases 1–3 that are independent of macro expansion.
///
/// The input is normalized to LF, trigraphs are replaced, backslash-newline
/// pairs are spliced, and comments become whitespace while retaining their
/// newlines. The final newline makes a source file without one well-formed for
/// the line-oriented preprocessor.
pub fn normalize_source(source: &str) -> Result<String, Vec<Diagnostic>> {
    let mut newlines = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            newlines.push('\n');
        } else {
            newlines.push(c);
        }
    }

    let mut trigraphs = Vec::with_capacity(newlines.len());
    let mut i = 0;
    while i < newlines.len() {
        let replacement = if i + 2 < newlines.len() && newlines[i] == '?' && newlines[i + 1] == '?'
        {
            match newlines[i + 2] {
                '=' => Some('#'),
                '/' => Some('\\'),
                '\'' => Some('^'),
                '(' => Some('['),
                ')' => Some(']'),
                '!' => Some('|'),
                '<' => Some('{'),
                '>' => Some('}'),
                '-' => Some('~'),
                _ => None,
            }
        } else {
            None
        };
        if let Some(c) = replacement {
            trigraphs.push(c);
            i += 3;
        } else {
            trigraphs.push(newlines[i]);
            i += 1;
        }
    }

    let mut spliced = Vec::with_capacity(trigraphs.len());
    let mut i = 0;
    while i < trigraphs.len() {
        if trigraphs[i] == '\\' && trigraphs.get(i + 1) == Some(&'\n') {
            i += 2;
        } else {
            spliced.push(trigraphs[i]);
            i += 1;
        }
    }

    let mut out = String::new();
    let mut i = 0;
    let mut quote = None;
    let mut errors = Vec::new();
    while i < spliced.len() {
        let c = spliced[i];
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(next) = spliced.get(i + 1) {
                    out.push(*next);
                    i += 2;
                    continue;
                }
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            out.push(c);
            i += 1;
        } else if c == '/' && spliced.get(i + 1) == Some(&'/') {
            out.push(' ');
            i += 2;
            while i < spliced.len() && spliced[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && spliced.get(i + 1) == Some(&'*') {
            let start = i;
            out.push(' ');
            i += 2;
            let mut closed = false;
            while i < spliced.len() {
                if spliced[i] == '*' && spliced.get(i + 1) == Some(&'/') {
                    i += 2;
                    closed = true;
                    break;
                }
                if spliced[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            if !closed {
                errors.push(Diagnostic::error(
                    "unterminated comment",
                    Some(Span {
                        start,
                        end: spliced.len(),
                    }),
                ));
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    if quote.is_some() {
        errors.push(Diagnostic::error(
            "unterminated string or character constant",
            Some(Span {
                start: out.len().saturating_sub(1),
                end: out.len(),
            }),
        ));
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreprocessingTokenKind {
    Identifier(String),
    Number(String),
    StringLiteral(String),
    CharacterConstant(String),
    Punctuator(String),
    Newline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreprocessingToken {
    pub kind: PreprocessingTokenKind,
    pub span: Span,
    pub leading_space: bool,
}

fn consume_ucn(chars: &[char], start: usize) -> Result<usize, Diagnostic> {
    let digits = match chars.get(start + 1) {
        Some('u') => 4,
        Some('U') => 8,
        _ => {
            return Err(Diagnostic::error(
                "invalid universal character name",
                Some(Span {
                    start,
                    end: (start + 2).min(chars.len()),
                }),
            ))
        }
    };
    let end = start + 2 + digits;
    if end > chars.len() || chars[start + 2..end].iter().any(|c| !c.is_ascii_hexdigit()) {
        return Err(Diagnostic::error(
            "invalid universal character name",
            Some(Span {
                start,
                end: end.min(chars.len()),
            }),
        ));
    }
    Ok(end)
}

fn preprocessing_tokenize_normalized(
    source: &str,
) -> Result<Vec<PreprocessingToken>, Vec<Diagnostic>> {
    let chars: Vec<_> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut errors = Vec::new();
    let mut i = 0;
    let mut leading_space = false;
    while i < chars.len() {
        let start = i;
        let c = chars[i];
        if c == '\n' {
            tokens.push(PreprocessingToken {
                kind: PreprocessingTokenKind::Newline,
                span: Span { start, end: i + 1 },
                leading_space: false,
            });
            i += 1;
            leading_space = false;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            leading_space = true;
            continue;
        }
        if c.is_ascii_alphabetic()
            || c == '_'
            || (c == '\\' && chars.get(i + 1).is_some_and(|x| *x == 'u' || *x == 'U'))
        {
            let mut value = String::new();
            if c == '\\' {
                let end = consume_ucn(&chars, i).map_err(|error| vec![error])?;
                value.extend(chars[i..end].iter());
                i = end;
            } else {
                value.push(c);
                i += 1;
            }
            while i < chars.len() {
                if chars[i].is_ascii_alphanumeric() || chars[i] == '_' {
                    value.push(chars[i]);
                    i += 1;
                } else if chars[i] == '\\'
                    && chars.get(i + 1).is_some_and(|x| *x == 'u' || *x == 'U')
                {
                    let end = consume_ucn(&chars, i).map_err(|error| vec![error])?;
                    value.extend(chars[i..end].iter());
                    i = end;
                } else {
                    break;
                }
            }
            tokens.push(PreprocessingToken {
                kind: PreprocessingTokenKind::Identifier(value),
                span: Span { start, end: i },
                leading_space,
            });
            leading_space = false;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|x| x.is_ascii_digit()))
        {
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            tokens.push(PreprocessingToken {
                kind: PreprocessingTokenKind::Number(chars[start..i].iter().collect()),
                span: Span { start, end: i },
                leading_space,
            });
            leading_space = false;
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i += 2;
                } else if chars[i] == quote {
                    i += 1;
                    closed = true;
                    break;
                } else if chars[i] == '\n' {
                    break;
                } else {
                    i += 1;
                }
            }
            if !closed {
                errors.push(Diagnostic::error(
                    "unterminated string or character constant",
                    Some(Span { start, end: i }),
                ));
            } else {
                tokens.push(PreprocessingToken {
                    kind: if quote == '"' {
                        PreprocessingTokenKind::StringLiteral(chars[start..i].iter().collect())
                    } else {
                        PreprocessingTokenKind::CharacterConstant(chars[start..i].iter().collect())
                    },
                    span: Span { start, end: i },
                    leading_space,
                });
                leading_space = false;
            }
            continue;
        }
        let remainder: String = chars[start..].iter().collect();
        let punctuator = [
            "%:%:", "<<=", ">>=", "...", "##", "->", "++", "--", "<<", ">>", "<=", ">=", "==",
            "!=", "&&", "||", "*=", "/=", "%=", "+=", "-=", "&=", "^=", "|=", "<:", ":>", "<%",
            "%>", "%:", "[", "]", "(", ")", "{", "}", ".", "&", "*", "+", "-", "~", "!", "/", "%",
            "<", ">", "^", "|", "?", ":", ";", "=", ",", "#",
        ]
        .into_iter()
        .find(|candidate| remainder.starts_with(candidate));
        if let Some(punctuator) = punctuator {
            i += punctuator.chars().count();
            tokens.push(PreprocessingToken {
                kind: PreprocessingTokenKind::Punctuator(punctuator.into()),
                span: Span { start, end: i },
                leading_space,
            });
            leading_space = false;
            continue;
        }
        errors.push(Diagnostic::error(
            format!("invalid preprocessing character {:?}", c),
            Some(Span {
                start,
                end: start + 1,
            }),
        ));
        i += 1;
    }
    if errors.is_empty() {
        Ok(tokens)
    } else {
        Err(errors)
    }
}

pub fn tokenize_preprocessing(source: &str) -> Result<Vec<PreprocessingToken>, Vec<Diagnostic>> {
    let normalized = normalize_source(source)?;
    preprocessing_tokenize_normalized(&normalized)
}

pub trait FileResolver {
    fn resolve(&self, path: &str, current_file: &str) -> Result<String, std::io::Error>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MacroDef {
    params: Option<Vec<String>>,
    variadic: bool,
    replacement: Vec<PreprocessingToken>,
}

#[derive(Clone, Copy)]
struct Conditional {
    active: bool,
    taken: bool,
    else_seen: bool,
}

pub struct Preprocessor {
    resolver: Box<dyn FileResolver>,
    macros: HashMap<String, MacroDef>,
    once: HashSet<String>,
    include_stack: Vec<String>,
    expansion_errors: RefCell<Vec<Diagnostic>>,
    date: String,
    time: String,
}

impl Preprocessor {
    pub fn new(resolver: impl FileResolver + 'static) -> Self {
        Self::new_with_hosted(resolver, true)
    }

    pub fn new_with_hosted(resolver: impl FileResolver + 'static, hosted: bool) -> Self {
        let (date, time) = current_date_time();
        let mut preprocessor = Self {
            resolver: Box::new(resolver),
            macros: HashMap::new(),
            once: HashSet::new(),
            include_stack: Vec::new(),
            expansion_errors: RefCell::new(Vec::new()),
            date,
            time,
        };
        preprocessor.add_builtin("__STDC__", "1");
        preprocessor.add_builtin("__STDC_VERSION__", "199901L");
        preprocessor.add_builtin("__STDC_HOSTED__", if hosted { "1" } else { "0" });
        preprocessor.add_builtin("__DATE__", &preprocessor.date.clone());
        preprocessor.add_builtin("__TIME__", &preprocessor.time.clone());
        preprocessor.add_builtin("__FILE__", "");
        preprocessor.add_builtin("__LINE__", "");
        preprocessor
    }

    fn add_builtin(&mut self, name: &str, replacement: &str) {
        self.macros.insert(
            name.to_string(),
            MacroDef {
                params: None,
                variadic: false,
                replacement: literal_tokens(replacement),
            },
        );
    }

    pub fn run(&mut self, file_name: &str, source: &str) -> Result<String, Vec<Diagnostic>> {
        if self.once.contains(file_name) {
            return Ok(String::new());
        }
        if self.include_stack.iter().any(|file| file == file_name) {
            return Err(vec![Diagnostic::error(
                format!("recursive include of {}", file_name),
                None,
            )]);
        }
        if self.include_stack.len() >= 200 {
            return Err(vec![Diagnostic::error(
                "include nesting limit exceeded",
                None,
            )]);
        }
        self.include_stack.push(file_name.to_string());
        let result = self.run_inner(file_name, source);
        self.include_stack.pop();
        result
    }

    fn run_inner(&mut self, file_name: &str, source: &str) -> Result<String, Vec<Diagnostic>> {
        let source = normalize_source(source)?;
        let tokens = preprocessing_tokenize_normalized(&source)?;
        let mut lines = Vec::new();
        let mut line = Vec::new();
        for token in tokens {
            if matches!(token.kind, PreprocessingTokenKind::Newline) {
                lines.push(std::mem::take(&mut line));
            } else {
                line.push(token);
            }
        }

        let mut out = String::new();
        let mut errors = Vec::new();
        let mut conditionals = Vec::new();
        let mut logical_file = file_name.to_string();
        let mut logical_line = 1usize;
        for tokens in lines {
            let span = line_span(&tokens);
            let (directive, rest) = split_directive(&tokens);
            let parent_active = conditionals.iter().all(|x: &Conditional| x.active);
            match directive.as_deref() {
                Some("if") => {
                    let value = if parent_active {
                        match self.if_value(rest, &logical_file, logical_line) {
                            Ok(value) => value,
                            Err(error) => {
                                errors.push(error);
                                false
                            }
                        }
                    } else {
                        false
                    };
                    conditionals.push(Conditional {
                        active: parent_active && value,
                        taken: value,
                        else_seen: false,
                    });
                }
                Some("ifdef") | Some("ifndef") => {
                    let name = rest.first().and_then(identifier);
                    let defined = name.is_some_and(|name| self.macros.contains_key(name));
                    let value = if directive.as_deref() == Some("ifdef") {
                        defined
                    } else {
                        !defined
                    };
                    conditionals.push(Conditional {
                        active: parent_active && value,
                        taken: value,
                        else_seen: false,
                    });
                }
                Some("elif") => {
                    if !conditionals.is_empty() {
                        let last = conditionals.len() - 1;
                        let outer_active = conditionals[..last].iter().all(|x| x.active);
                        let already_taken = conditionals[last].taken;
                        if conditionals[last].else_seen {
                            errors.push(Diagnostic::error("#elif after #else", span));
                        }
                        let value = if outer_active && !already_taken {
                            match self.if_value(rest, &logical_file, logical_line) {
                                Ok(value) => value,
                                Err(error) => {
                                    errors.push(error);
                                    false
                                }
                            }
                        } else {
                            false
                        };
                        let current = &mut conditionals[last];
                        current.active = outer_active && !current.taken && value;
                        current.taken |= value;
                    } else {
                        errors.push(Diagnostic::error("#elif without matching #if", span));
                    }
                }
                Some("else") => {
                    if !conditionals.is_empty() {
                        let last = conditionals.len() - 1;
                        let outer_active = conditionals[..last].iter().all(|x| x.active);
                        if conditionals[last].else_seen {
                            errors.push(Diagnostic::error("repeated #else", span));
                        }
                        let current = &mut conditionals[last];
                        current.active = outer_active && !current.taken;
                        current.taken = true;
                        current.else_seen = true;
                    } else {
                        errors.push(Diagnostic::error("#else without matching #if", span));
                    }
                }
                Some("endif") => {
                    if conditionals.pop().is_none() {
                        errors.push(Diagnostic::error("#endif without matching #if", span));
                    }
                }
                _ if !parent_active => {}
                Some("define") => {
                    if let Err(error) = self.define_macro(rest, span) {
                        errors.push(error);
                    }
                }
                Some("undef") => {
                    if let Some(name) = rest.first().and_then(identifier) {
                        if matches!(name, "__STDC__" | "__STDC_VERSION__" | "__STDC_HOSTED__") {
                            errors.push(Diagnostic::error(
                                "cannot undefine a required predefined macro",
                                span,
                            ));
                        } else {
                            self.macros.remove(name);
                        }
                    } else {
                        errors.push(Diagnostic::error("#undef requires an identifier", span));
                    }
                }
                // Includes are guarded by run(); recursion is diagnosed before re-entry.
                Some("include") => match self.include_path(rest, &logical_file, logical_line) {
                    Ok(path) => match self.resolver.resolve(&path, file_name) {
                        Ok(included) => match self.run(&path, &included) {
                            Ok(expanded) => out.push_str(&expanded),
                            Err(mut include_errors) => errors.append(&mut include_errors),
                        },
                        Err(error) => errors.push(Diagnostic::error(
                            format!("include {}: {}", path, error),
                            span,
                        )),
                    },
                    Err(error) => errors.push(Diagnostic::error(error, span)),
                },
                Some("line") => {
                    if let Some(number) = integer_spelling(rest.first()) {
                        logical_line = (number as usize).saturating_sub(1);
                        if let Some(file) = rest.get(1).and_then(string_spelling) {
                            logical_file = file.to_string();
                        }
                    } else {
                        errors.push(Diagnostic::error("#line requires a line number", span));
                    }
                }
                Some("error") => errors.push(Diagnostic::error(
                    format!("preprocessor #error{}", tokens_to_text(rest)),
                    span,
                )),
                Some("pragma") => {
                    if rest.len() == 1 && token_spelling(&rest[0]) == "once" {
                        self.once.insert(file_name.to_string());
                    }
                }
                Some("") => {}
                Some(_) => errors.push(Diagnostic::error(
                    "unsupported preprocessor directive",
                    span,
                )),
                None => {
                    let mut disabled = HashSet::new();
                    let expanded =
                        self.expand_tokens(&tokens, &mut disabled, &logical_file, logical_line);
                    let expanded = remove_pragma_operators(expanded);
                    emit_tokens(&mut out, &expanded);
                }
            }
            logical_line = logical_line.saturating_add(1);
        }
        if !conditionals.is_empty() {
            errors.push(Diagnostic::error(
                "unterminated conditional inclusion",
                Some(Span {
                    start: source.len(),
                    end: source.len(),
                }),
            ));
        }
        errors.extend(self.expansion_errors.borrow_mut().drain(..));
        if errors.is_empty() {
            Ok(out)
        } else {
            Err(errors)
        }
    }

    fn define_macro(
        &mut self,
        tokens: &[PreprocessingToken],
        span: Option<Span>,
    ) -> Result<(), Diagnostic> {
        let Some(name) = tokens.first().and_then(identifier) else {
            return Err(Diagnostic::error("#define requires an identifier", span));
        };
        let (params, variadic, replacement_start) = if tokens
            .get(1)
            .is_some_and(|token| token_spelling(token) == "(" && !token.leading_space)
        {
            let mut params = Vec::new();
            let mut variadic = false;
            let mut i = 2;
            while i < tokens.len() && token_spelling(&tokens[i]) != ")" {
                if token_spelling(&tokens[i]) == "..." {
                    variadic = true;
                    i += 1;
                    break;
                }
                let Some(parameter) = identifier(&tokens[i]) else {
                    return Err(Diagnostic::error("invalid macro parameter list", span));
                };
                if params.iter().any(|x| x == parameter) {
                    return Err(Diagnostic::error("duplicate macro parameter", span));
                }
                params.push(parameter.to_string());
                i += 1;
                if token_spelling(tokens.get(i).unwrap_or(&tokens[0])) == "," {
                    i += 1;
                } else if token_spelling(tokens.get(i).unwrap_or(&tokens[0])) != ")" {
                    return Err(Diagnostic::error("invalid macro parameter list", span));
                }
            }
            if token_spelling(tokens.get(i).unwrap_or(&tokens[0])) != ")" {
                return Err(Diagnostic::error("unterminated macro parameter list", span));
            }
            (Some(params), variadic, i + 1)
        } else {
            (None, false, 1)
        };
        let definition = MacroDef {
            params,
            variadic,
            replacement: {
                let mut replacement = tokens[replacement_start..].to_vec();
                if let Some(first) = replacement.first_mut() {
                    first.leading_space = false;
                }
                replacement
            },
        };
        if let Some(previous) = self.macros.get(name) {
            if !macro_equivalent(previous, &definition) {
                return Err(Diagnostic::error(
                    format!("incompatible redefinition of macro {}", name),
                    span,
                ));
            }
        } else {
            self.macros.insert(name.to_string(), definition);
        }
        Ok(())
    }

    fn include_path(
        &self,
        tokens: &[PreprocessingToken],
        file: &str,
        line: usize,
    ) -> Result<String, String> {
        let mut disabled = HashSet::new();
        let expanded = self.expand_tokens(tokens, &mut disabled, file, line);
        if let Some(path) = expanded.first().and_then(string_spelling) {
            return Ok(path.to_string());
        }
        if expanded.first().is_some_and(|x| token_spelling(x) == "<")
            && expanded.last().is_some_and(|x| token_spelling(x) == ">")
        {
            return Ok(expanded[1..expanded.len() - 1]
                .iter()
                .map(token_spelling)
                .collect());
        }
        Err("#include requires a header name".into())
    }

    fn if_value(
        &self,
        tokens: &[PreprocessingToken],
        file: &str,
        line: usize,
    ) -> Result<bool, Diagnostic> {
        let mut defined = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            if identifier(&tokens[i]) == Some("defined") {
                let (name, next) = if tokens.get(i + 1).is_some_and(|x| token_spelling(x) == "(") {
                    let name = tokens.get(i + 2).and_then(identifier);
                    if tokens.get(i + 3).is_none_or(|x| token_spelling(x) != ")") {
                        return Err(Diagnostic::error(
                            "defined requires a closing parenthesis",
                            Some(tokens[i].span),
                        ));
                    }
                    (name, i + 4)
                } else {
                    (tokens.get(i + 1).and_then(identifier), i + 2)
                };
                let Some(name) = name else {
                    return Err(Diagnostic::error(
                        "defined requires an identifier",
                        Some(tokens[i].span),
                    ));
                };
                defined.push(PreprocessingToken {
                    kind: PreprocessingTokenKind::Number(
                        if self.macros.contains_key(name) {
                            "1"
                        } else {
                            "0"
                        }
                        .into(),
                    ),
                    span: tokens[i].span,
                    leading_space: tokens[i].leading_space,
                });
                i = next;
            } else {
                defined.push(tokens[i].clone());
                i += 1;
            }
        }
        let mut disabled = HashSet::new();
        let expanded = self.expand_tokens(&defined, &mut disabled, file, line);
        IfExpr::new(expanded)
            .parse()
            .map(|value| value != 0)
            .map_err(|message| Diagnostic::error(message, tokens.first().map(|x| x.span)))
    }

    fn expand_tokens(
        &self,
        tokens: &[PreprocessingToken],
        disabled: &mut HashSet<String>,
        file: &str,
        line: usize,
    ) -> Vec<PreprocessingToken> {
        let mut output = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let Some(name) = identifier(&tokens[i]).map(str::to_string) else {
                output.push(tokens[i].clone());
                i += 1;
                continue;
            };
            if name == "__FILE__" {
                let mut token = synthetic(PreprocessingTokenKind::StringLiteral(format!(
                    "\"{}\"",
                    file.replace('"', "\\\"")
                )));
                token.leading_space = tokens[i].leading_space;
                output.push(token);
                i += 1;
                continue;
            }
            if name == "__LINE__" {
                let mut token = synthetic(PreprocessingTokenKind::Number(line.to_string()));
                token.leading_space = tokens[i].leading_space;
                output.push(token);
                i += 1;
                continue;
            }
            let Some(definition) = self.macros.get(&name) else {
                output.push(tokens[i].clone());
                i += 1;
                continue;
            };
            if disabled.contains(&name) {
                output.push(tokens[i].clone());
                i += 1;
                continue;
            }
            if let Some(params) = &definition.params {
                let Some(open) = tokens.get(i + 1) else {
                    output.push(tokens[i].clone());
                    i += 1;
                    continue;
                };
                if token_spelling(open) != "(" {
                    output.push(tokens[i].clone());
                    i += 1;
                    continue;
                }
                let Some((mut args, end)) = parse_macro_arguments(tokens, i + 1) else {
                    output.push(tokens[i].clone());
                    i += 1;
                    continue;
                };
                if args.is_empty() && !params.is_empty() {
                    args.push(Vec::new());
                }
                if (!definition.variadic && args.len() != params.len())
                    || (definition.variadic && args.len() < params.len())
                {
                    self.expansion_errors.borrow_mut().push(Diagnostic::error(
                        "macro invocation has incorrect number of arguments",
                        Some(tokens[i].span),
                    ));
                    output.push(tokens[i].clone());
                    i += 1;
                    continue;
                }
                let mut invoking_disabled = disabled.clone();
                invoking_disabled.insert(name.clone());
                let replacement = substitute_macro(definition, &args, self, disabled, file, line);
                output.extend(self.expand_tokens(&replacement, &mut invoking_disabled, file, line));
                i = end + 1;
            } else {
                let mut nested_disabled = disabled.clone();
                nested_disabled.insert(name.clone());
                output.extend(self.expand_tokens(
                    &definition.replacement,
                    &mut nested_disabled,
                    file,
                    line,
                ));
                i += 1;
            }
        }
        output
    }
}

fn current_date_time() -> (String, String) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |x| x.as_secs());
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let date = format!("\"{} {:02} {:04}\"", months[month as usize - 1], day, year);
    let hour = day_seconds / 3_600;
    let minute = day_seconds / 60 % 60;
    let second = day_seconds % 60;
    let time = format!("\"{:02}:{:02}:{:02}\"", hour, minute, second);
    (date, time)
}

fn literal_tokens(text: &str) -> Vec<PreprocessingToken> {
    // BUG: malformed replacement tokenization is silently discarded.
    preprocessing_tokenize_normalized(&format!("{}\n", text))
        .unwrap_or_default()
        .into_iter()
        .filter(|x| !matches!(x.kind, PreprocessingTokenKind::Newline))
        .collect()
}

fn synthetic(kind: PreprocessingTokenKind) -> PreprocessingToken {
    PreprocessingToken {
        kind,
        span: Span::default(),
        leading_space: false,
    }
}

fn line_span(tokens: &[PreprocessingToken]) -> Option<Span> {
    Some(Span {
        start: tokens.first()?.span.start,
        end: tokens.last()?.span.end,
    })
}

fn identifier(token: &PreprocessingToken) -> Option<&str> {
    match &token.kind {
        PreprocessingTokenKind::Identifier(value) => Some(value),
        _ => None,
    }
}

fn string_spelling(token: &PreprocessingToken) -> Option<&str> {
    match &token.kind {
        PreprocessingTokenKind::StringLiteral(value) => {
            value.strip_prefix('"').and_then(|x| x.strip_suffix('"'))
        }
        _ => None,
    }
}

fn integer_spelling(token: Option<&PreprocessingToken>) -> Option<i64> {
    let value = token.map(token_spelling)?;
    let value = value.trim_end_matches(['u', 'U', 'l', 'L']);
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16).ok()
    } else if value.len() > 1 && value.starts_with('0') {
        i64::from_str_radix(&value[1..], 8).ok()
    } else {
        value.parse().ok()
    }
}

fn token_spelling(token: &PreprocessingToken) -> String {
    match &token.kind {
        PreprocessingTokenKind::Identifier(value)
        | PreprocessingTokenKind::Number(value)
        | PreprocessingTokenKind::StringLiteral(value)
        | PreprocessingTokenKind::CharacterConstant(value)
        | PreprocessingTokenKind::Punctuator(value) => value.clone(),
        PreprocessingTokenKind::Newline => String::new(),
    }
}

fn tokens_to_text(tokens: &[PreprocessingToken]) -> String {
    tokens
        .iter()
        .map(token_spelling)
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_directive(tokens: &[PreprocessingToken]) -> (Option<String>, &[PreprocessingToken]) {
    if tokens.first().is_some_and(|x| token_spelling(x) == "#") {
        if let Some(name) = tokens.get(1).and_then(identifier) {
            return (Some(name.to_string()), &tokens[2..]);
        }
        return (Some(String::new()), &tokens[1..]);
    }
    (None, tokens)
}

fn macro_equivalent(a: &MacroDef, b: &MacroDef) -> bool {
    a.params == b.params
        && a.variadic == b.variadic
        && a.replacement
            .iter()
            .map(token_spelling)
            .eq(b.replacement.iter().map(token_spelling))
}

fn parse_macro_arguments(
    tokens: &[PreprocessingToken],
    open: usize,
) -> Option<(Vec<Vec<PreprocessingToken>>, usize)> {
    let mut args = Vec::new();
    let mut current = Vec::new();
    let mut depth = 0;
    let mut i = open + 1;
    if tokens.get(i).is_some_and(|x| token_spelling(x) == ")") {
        return Some((args, i));
    }
    while let Some(token) = tokens.get(i) {
        match token_spelling(token).as_str() {
            "(" => {
                depth += 1;
                current.push(token.clone());
            }
            ")" if depth == 0 => {
                args.push(current);
                return Some((args, i));
            }
            ")" => {
                depth -= 1;
                current.push(token.clone());
            }
            "," if depth == 0 => {
                args.push(std::mem::take(&mut current));
            }
            _ => current.push(token.clone()),
        }
        i += 1;
    }
    None
}

fn argument_tokens(
    name: &str,
    params: &[String],
    variadic: bool,
    raw: &[Vec<PreprocessingToken>],
    expanded: &[Vec<PreprocessingToken>],
    use_expanded: bool,
) -> Option<Vec<PreprocessingToken>> {
    let source = if use_expanded { expanded } else { raw };
    if name == "__VA_ARGS__" && variadic {
        let fixed = params.len();
        let mut result = Vec::new();
        for (index, arg) in source.iter().skip(fixed).enumerate() {
            if index > 0 {
                result.push(synthetic(PreprocessingTokenKind::Punctuator(",".into())));
            }
            result.extend(arg.clone());
        }
        if let Some(first) = result.first_mut() {
            first.leading_space = false;
        }
        return Some(result);
    }
    params
        .iter()
        .position(|param| param == name)
        .and_then(|index| source.get(index).cloned())
        .map(|mut result| {
            if let Some(first) = result.first_mut() {
                first.leading_space = false;
            }
            result
        })
}

fn stringify_argument(tokens: &[PreprocessingToken]) -> PreprocessingToken {
    let mut value = String::new();
    for token in tokens {
        let spelling = token_spelling(token);
        if token.leading_space && !value.is_empty() {
            value.push(' ');
        }
        value.push_str(&spelling);
    }
    let value = value.replace('\\', "\\\\").replace('"', "\\\"");
    synthetic(PreprocessingTokenKind::StringLiteral(format!(
        "\"{}\"",
        value
    )))
}

fn substitute_macro(
    definition: &MacroDef,
    args: &[Vec<PreprocessingToken>],
    preprocessor: &Preprocessor,
    disabled: &mut HashSet<String>,
    file: &str,
    line: usize,
) -> Vec<PreprocessingToken> {
    let params = definition.params.as_deref().unwrap_or(&[]);
    let expanded: Vec<_> = args
        .iter()
        .map(|arg| preprocessor.expand_tokens(arg, disabled, file, line))
        .collect();
    let mut output = Vec::new();
    let mut i = 0;
    while i < definition.replacement.len() {
        let token = &definition.replacement[i];
        if token_spelling(token) == "#"
            && definition
                .replacement
                .get(i + 1)
                .and_then(identifier)
                .and_then(|name| {
                    argument_tokens(name, params, definition.variadic, args, &expanded, false)
                })
                .is_some()
        {
            let name = identifier(&definition.replacement[i + 1]).unwrap();
            let argument =
                argument_tokens(name, params, definition.variadic, args, &expanded, false).unwrap();
            output.push(stringify_argument(&argument));
            i += 2;
        } else if token_spelling(token) == "##" && !output.is_empty() {
            if let Some(next) = definition.replacement.get(i + 1) {
                let rhs = if let Some(name) = identifier(next) {
                    argument_tokens(name, params, definition.variadic, args, &expanded, false)
                        .unwrap_or_else(|| vec![next.clone()])
                } else {
                    vec![next.clone()]
                };
                if let Some(lhs) = output.pop() {
                    let lhs = token_spelling(&lhs);
                    let pasted = match rhs.first() {
                        Some(rhs) => format!("{}{}", lhs, token_spelling(rhs)),
                        None => lhs.to_string(),
                    };
                    if let Some(token) = literal_tokens(&pasted).into_iter().next() {
                        output.push(token);
                    }
                }
                i += 2;
            } else {
                i += 1;
            }
        } else if let Some(name) = identifier(token) {
            let adjacent_paste = definition
                .replacement
                .get(i + 1)
                .is_some_and(|next| token_spelling(next) == "##");
            if let Some(argument) = argument_tokens(
                name,
                params,
                definition.variadic,
                args,
                &expanded,
                !adjacent_paste,
            ) {
                output.extend(argument);
            } else {
                output.push(token.clone());
            }
            i += 1;
        } else {
            output.push(token.clone());
            i += 1;
        }
    }
    output
}

fn remove_pragma_operators(tokens: Vec<PreprocessingToken>) -> Vec<PreprocessingToken> {
    let mut output = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if identifier(&tokens[i]) == Some("_Pragma")
            && tokens.get(i + 1).is_some_and(|x| token_spelling(x) == "(")
        {
            let mut depth = 0;
            i += 1;
            while i < tokens.len() {
                let spelling = token_spelling(&tokens[i]);
                i += 1;
                if spelling == "(" {
                    depth += 1;
                } else if spelling == ")" {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
            }
        } else {
            output.push(tokens[i].clone());
            i += 1;
        }
    }
    output
}

fn emit_tokens(output: &mut String, tokens: &[PreprocessingToken]) {
    let mut previous: Option<&PreprocessingToken> = None;
    for token in tokens {
        let spelling = token_spelling(token);
        if token.leading_space
            || previous
                .is_some_and(|previous| tokens_would_fuse(&token_spelling(previous), &spelling))
        {
            if !matches!(output.as_bytes().last(), Some(b' ' | b'\n')) {
                output.push(' ');
            }
        }
        output.push_str(&spelling);
        previous = Some(token);
    }
    output.push('\n');
}

fn tokens_would_fuse(previous: &str, next: &str) -> bool {
    let last = previous.chars().last();
    let first = next.chars().next();
    if let (Some(last), Some(first)) = (last, first) {
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        if (word(last) && word(first))
            || (last.is_ascii_digit() && first == '.')
            || (last == '.' && first.is_ascii_digit())
        {
            return true;
        }
    }
    if previous.ends_with('/') && (next.starts_with('/') || next.starts_with('*')) {
        return true;
    }
    if previous.ends_with('.') && next.starts_with('.') {
        return true;
    }
    const PUNCTUATORS: [&str; 27] = [
        "++", "--", "->", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "*=", "/=", "%=", "+=",
        "-=", "&=", "^=", "|=", "##", "%:", "<:", ":>", "<%", "%>", "...", "<<=",
    ];
    let joined = format!("{}{}", previous, next);
    PUNCTUATORS.contains(&joined.as_str()) || joined == ">>="
}

struct IfExpr {
    tokens: Vec<String>,
    position: usize,
}

impl IfExpr {
    fn new(tokens: Vec<PreprocessingToken>) -> Self {
        Self {
            tokens: tokens.iter().map(token_spelling).collect(),
            position: 0,
        }
    }

    fn parse(mut self) -> Result<i64, String> {
        let value = self.expression(0, true)?;
        if self.position != self.tokens.len() {
            return Err("invalid #if expression".into());
        }
        Ok(value)
    }

    fn expression(&mut self, minimum: u8, evaluate: bool) -> Result<i64, String> {
        let mut left = self.unary(evaluate)?;
        while let Some((precedence, operator)) = self
            .tokens
            .get(self.position)
            .and_then(|x| if_precedence(x))
            .map(|(precedence, operator)| (precedence, operator.to_string()))
        {
            if precedence < minimum {
                break;
            }
            self.position += 1;
            let short_circuit =
                evaluate && ((operator == "&&" && left == 0) || (operator == "||" && left != 0));
            let right = self.expression(precedence + 1, evaluate && !short_circuit)?;
            if !evaluate {
                left = 0;
            } else if short_circuit {
                left = i64::from(operator == "||");
            } else {
                left = apply_if_operator(&operator, left, right)?;
            }
        }
        Ok(left)
    }

    fn unary(&mut self, evaluate: bool) -> Result<i64, String> {
        if let Some(operator) = self.tokens.get(self.position).cloned() {
            if matches!(operator.as_str(), "+" | "-" | "!" | "~") {
                self.position += 1;
                let value = self.unary(evaluate)?;
                if !evaluate {
                    return Ok(0);
                }
                return Ok(match operator.as_str() {
                    "+" => value,
                    "-" => value.wrapping_neg(),
                    "!" => i64::from(value == 0),
                    "~" => !value,
                    _ => unreachable!(),
                });
            }
        }
        if self.tokens.get(self.position).map(String::as_str) == Some("(") {
            self.position += 1;
            let value = self.expression(0, evaluate)?;
            if self.tokens.get(self.position).map(String::as_str) != Some(")") {
                return Err("missing ')' in #if expression".into());
            }
            self.position += 1;
            return Ok(value);
        }
        let token = self
            .tokens
            .get(self.position)
            .ok_or_else(|| "expected expression in #if".to_string())?
            .clone();
        self.position += 1;
        let value = integer_text(&token)
            .or_else(|| is_identifier_text(&token).then_some(0))
            .ok_or_else(|| "invalid #if expression operand".to_string())?;
        Ok(if evaluate { value } else { 0 })
    }
}

fn if_precedence(operator: &str) -> Option<(u8, &str)> {
    Some(match operator {
        "||" => (1, operator),
        "&&" => (2, operator),
        "|" => (3, operator),
        "^" => (4, operator),
        "&" => (5, operator),
        "==" | "!=" => (6, operator),
        "<" | "<=" | ">" | ">=" => (7, operator),
        "<<" | ">>" => (8, operator),
        "+" | "-" => (9, operator),
        "*" | "/" | "%" => (10, operator),
        _ => return None,
    })
}

fn apply_if_operator(operator: &str, left: i64, right: i64) -> Result<i64, String> {
    Ok(match operator {
        "||" => i64::from(left != 0 || right != 0),
        "&&" => i64::from(left != 0 && right != 0),
        "|" => left | right,
        "^" => left ^ right,
        "&" => left & right,
        "==" => i64::from(left == right),
        "!=" => i64::from(left != right),
        "<" => i64::from(left < right),
        "<=" => i64::from(left <= right),
        ">" => i64::from(left > right),
        ">=" => i64::from(left >= right),
        "<<" => left.wrapping_shl(right as u32),
        ">>" => left.wrapping_shr(right as u32),
        "+" => left.wrapping_add(right),
        "-" => left.wrapping_sub(right),
        "*" => left.wrapping_mul(right),
        "/" => {
            if right == 0 {
                return Err("division by zero in #if expression".into());
            }
            left / right
        }
        "%" => {
            if right == 0 {
                return Err("remainder by zero in #if expression".into());
            }
            left % right
        }
        _ => return Err("invalid #if expression".into()),
    })
}

fn integer_text(text: &str) -> Option<i64> {
    let text = text.trim_end_matches(['u', 'U', 'l', 'L']);
    if let Some(value) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        i64::from_str_radix(value, 16).ok()
    } else if text.len() > 1 && text.starts_with('0') {
        i64::from_str_radix(&text[1..], 8).ok()
    } else {
        text.parse().ok()
    }
}

fn is_identifier_text(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
