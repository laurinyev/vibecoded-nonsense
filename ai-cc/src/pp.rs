use crate::diag::{Diagnostic, Span};

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
    while i < chars.len() {
        let start = i;
        let c = chars[i];
        if c == '\n' {
            tokens.push(PreprocessingToken {
                kind: PreprocessingTokenKind::Newline,
                span: Span { start, end: i + 1 },
            });
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
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
            });
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
            });
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
                });
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
            });
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
pub struct Preprocessor {
    resolver: Box<dyn FileResolver>,
    macros: std::collections::HashMap<String, String>,
    function_macros: std::collections::HashMap<String, (Vec<String>, String)>,
    once: std::collections::HashSet<String>,
}
impl Preprocessor {
    pub fn new(resolver: impl FileResolver + 'static) -> Self {
        Self::new_with_hosted(resolver, true)
    }

    pub fn new_with_hosted(resolver: impl FileResolver + 'static, hosted: bool) -> Self {
        let mut macros = std::collections::HashMap::new();
        macros.insert(
            "__STDC_HOSTED__".into(),
            if hosted { "1".into() } else { "0".into() },
        );
        Self {
            resolver: Box::new(resolver),
            macros,
            function_macros: std::collections::HashMap::new(),
            once: std::collections::HashSet::new(),
        }
    }
    pub fn run(&mut self, file_name: &str, source: &str) -> Result<String, Vec<Diagnostic>> {
        if self.once.contains(file_name) {
            return Ok(String::new());
        }
        let source = normalize_source(source)?;
        preprocessing_tokenize_normalized(&source)?;
        let mut out = String::new();
        let mut errs = Vec::new();
        let mut cond: Vec<(bool, bool)> = Vec::new();
        let mut offset = 0;
        for raw_line in source.split_inclusive('\n') {
            let has_newline = raw_line.ends_with('\n');
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            let line_start = offset;
            offset += raw_line.len();
            let span = || {
                Some(Span {
                    start: line_start,
                    end: line_start + line.len(),
                })
            };
            let preserve_line = |out: &mut String| {
                if has_newline {
                    out.push('\n');
                }
            };
            let t = line.trim();
            let directive = t.strip_prefix('#').map(str::trim_start);
            let mut directive_parts = directive.unwrap_or("").splitn(2, char::is_whitespace);
            let directive_name = directive_parts.next().unwrap_or("");
            let directive_rest = directive_parts.next().unwrap_or("").trim();
            let parent = cond.iter().all(|(active, _)| *active);
            if directive_name == "if" {
                let value = self.if_value(directive_rest);
                cond.push((parent && value, value));
                preserve_line(&mut out);
                continue;
            }
            if directive_name == "ifdef" {
                let value = self.macros.contains_key(directive_rest)
                    || self.function_macros.contains_key(directive_rest);
                cond.push((parent && value, value));
                preserve_line(&mut out);
                continue;
            }
            if directive_name == "ifndef" {
                let value = !(self.macros.contains_key(directive_rest)
                    || self.function_macros.contains_key(directive_rest));
                cond.push((parent && value, value));
                preserve_line(&mut out);
                continue;
            }
            if directive_name == "else" {
                let outer = cond
                    .iter()
                    .take(cond.len().saturating_sub(1))
                    .all(|(active, _)| *active);
                if let Some((active, seen)) = cond.last_mut() {
                    *active = outer && !*seen;
                    *seen = true;
                } else {
                    errs.push(Diagnostic::error("#else without matching #if", span()));
                }
                preserve_line(&mut out);
                continue;
            }
            if directive_name == "elif" {
                let value = self.if_value(directive_rest);
                let outer = cond
                    .iter()
                    .take(cond.len().saturating_sub(1))
                    .all(|(active, _)| *active);
                if let Some((active, seen)) = cond.last_mut() {
                    *active = outer && !*seen && value;
                    *seen |= value;
                } else {
                    errs.push(Diagnostic::error("#elif without matching #if", span()));
                }
                preserve_line(&mut out);
                continue;
            }
            if directive_name == "endif" {
                if cond.pop().is_none() {
                    errs.push(Diagnostic::error("#endif without matching #if", span()));
                }
                preserve_line(&mut out);
                continue;
            }
            if !cond.iter().all(|(active, _)| *active) {
                preserve_line(&mut out);
                continue;
            }
            if directive_name == "define" {
                let def = directive_rest;
                if let Some(open) = def.find('(') {
                    if let Some(close) = def.find(')') {
                        let name = def[..open].trim().to_string();
                        let args = def[open + 1..close]
                            .split(',')
                            .map(|x| x.trim().to_string())
                            .filter(|x| !x.is_empty())
                            .collect();
                        self.function_macros
                            .insert(name, (args, def[close + 1..].trim().to_string()));
                    }
                } else {
                    let mut p = def.splitn(2, char::is_whitespace);
                    if let Some(k) = p.next() {
                        self.macros
                            .insert(k.to_string(), p.next().unwrap_or("").trim().to_string());
                    }
                }
                preserve_line(&mut out);
            } else if directive_name == "undef" {
                let name = directive_rest;
                self.macros.remove(name);
                self.function_macros.remove(name);
                preserve_line(&mut out);
            } else if directive_name == "include" {
                let q = directive_rest.trim_matches(['"', '<', '>']);
                match self.resolver.resolve(q, file_name) {
                    Ok(s) => match self.run(q, &s) {
                        Ok(x) => out.push_str(&x),
                        Err(mut e) => errs.append(&mut e),
                    },
                    Err(e) => errs.push(Diagnostic::error(format!("include {}: {}", q, e), span())),
                }
            } else if directive_name == "pragma" && directive_rest == "once" {
                self.once.insert(file_name.to_string());
                preserve_line(&mut out);
            } else if directive_name == "error" {
                errs.push(Diagnostic::error(
                    format!("preprocessor #error {}", directive_rest),
                    span(),
                ));
                preserve_line(&mut out);
            } else if directive.is_some() && !directive_name.is_empty() {
                errs.push(Diagnostic::error(
                    format!("unsupported preprocessor directive: {}", t),
                    span(),
                ));
                preserve_line(&mut out);
            } else {
                let mut x = line.to_string();
                for (name, (args, body)) in &self.function_macros {
                    while let Some(start) = x.find(&format!("{}(", name)) {
                        let open = start + name.len();
                        let Some(end) = x[open..].find(')') else {
                            break;
                        };
                        let end = open + end;
                        let values: Vec<_> = x[open + 1..end].split(',').map(str::trim).collect();
                        if values.len() != args.len() {
                            break;
                        }
                        let mut replacement = body.clone();
                        for (arg, value) in args.iter().zip(values) {
                            replacement = replacement.replace(arg, value);
                        }
                        x.replace_range(start..=end, &replacement);
                    }
                }
                for (k, v) in &self.macros {
                    x = x.replace(k, v);
                }
                out.push_str(&x);
                out.push('\n');
            }
        }
        if !cond.is_empty() {
            errs.push(Diagnostic::error(
                "unterminated conditional inclusion",
                Some(Span {
                    start: source.len(),
                    end: source.len(),
                }),
            ));
        }
        if errs.is_empty() {
            Ok(out)
        } else {
            Err(errs)
        }
    }
    fn if_value(&self, expr: &str) -> bool {
        let x = expr.trim();
        if let Some(n) = x.strip_prefix("defined") {
            let n = n.trim().trim_matches(['(', ')', ' ']);
            return self.macros.contains_key(n) || self.function_macros.contains_key(n);
        }
        x.parse::<i64>()
            .unwrap_or_else(|_| self.macros.get(x).and_then(|v| v.parse().ok()).unwrap_or(0))
            != 0
    }
}
