use crate::diag::{Diagnostic, Span};
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
            let parent = cond.iter().all(|(active, _)| *active);
            if let Some(rest) = t.strip_prefix("#if ") {
                let value = self.if_value(rest.trim());
                cond.push((parent && value, value));
                preserve_line(&mut out);
                continue;
            }
            if let Some(rest) = t.strip_prefix("#ifdef ") {
                let value = self.macros.contains_key(rest.trim())
                    || self.function_macros.contains_key(rest.trim());
                cond.push((parent && value, value));
                preserve_line(&mut out);
                continue;
            }
            if let Some(rest) = t.strip_prefix("#ifndef ") {
                let value = !(self.macros.contains_key(rest.trim())
                    || self.function_macros.contains_key(rest.trim()));
                cond.push((parent && value, value));
                preserve_line(&mut out);
                continue;
            }
            if t == "#else" {
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
            if let Some(rest) = t.strip_prefix("#elif ") {
                let value = self.if_value(rest.trim());
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
            if t == "#endif" {
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
            if let Some(def) = t.strip_prefix("#define ") {
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
            } else if let Some(rest) = t.strip_prefix("#undef ") {
                let name = rest.trim();
                self.macros.remove(name);
                self.function_macros.remove(name);
                preserve_line(&mut out);
            } else if let Some(rest) = t.strip_prefix("#include ") {
                let q = rest.trim().trim_matches(['"', '<', '>']);
                match self.resolver.resolve(q, file_name) {
                    Ok(s) => match self.run(q, &s) {
                        Ok(x) => out.push_str(&x),
                        Err(mut e) => errs.append(&mut e),
                    },
                    Err(e) => errs.push(Diagnostic::error(format!("include {}: {}", q, e), span())),
                }
            } else if t == "#pragma once" {
                self.once.insert(file_name.to_string());
                preserve_line(&mut out);
            } else if let Some(rest) = t.strip_prefix("#error") {
                errs.push(Diagnostic::error(
                    format!("preprocessor #error{}", rest),
                    span(),
                ));
                preserve_line(&mut out);
            } else if t.starts_with('#') {
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
