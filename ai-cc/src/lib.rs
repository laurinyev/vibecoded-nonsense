pub mod ast;
pub mod codegen;
pub mod diag;
pub mod lex;
pub mod lower;
pub mod parser;
pub mod pp;
pub mod sema;
pub mod symbol;
use diag::Diagnostic;
use pp::{FileResolver, Preprocessor};
use std::{env, path::Path};
struct FsResolver;
impl FileResolver for FsResolver {
    fn resolve(&self, path: &str, current: &str) -> Result<String, std::io::Error> {
        let local = Path::new(current)
            .parent()
            .unwrap_or(Path::new("."))
            .join(path);
        if local.is_file() {
            return std::fs::read_to_string(local);
        }
        let mut includes = Vec::new();
        if let Ok(dir) = env::var("AI_CC_INCLUDE") {
            includes.push(Path::new(&dir).join(path));
        }
        includes.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("lib/include")
                .join(path),
        );
        if let Ok(dirs) = env::var("AI_CC_SYSTEM_INCLUDE") {
            for dir in env::split_paths(&dirs) {
                includes.push(dir.join(path));
            }
        }
        includes.push(Path::new("/usr/include").join(path));
        if let Ok(entries) = std::fs::read_dir("/usr/include") {
            for entry in entries.flatten() {
                let dir = entry.path();
                if dir.is_dir()
                    && dir
                        .file_name()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| x.contains('-'))
                {
                    includes.push(dir.join(path));
                }
            }
        }
        if let Ok(exe) = env::current_exe() {
            if let Some(bin) = exe.parent() {
                if let Some(prefix) = bin.parent() {
                    includes.push(prefix.join("lib/include").join(path));
                }
            }
        }
        for candidate in includes {
            if candidate.is_file() {
                return std::fs::read_to_string(candidate);
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("header not found: {}", path),
        ))
    }
}
pub fn compile(source: &str, file_name: &str) -> Result<String, Vec<Diagnostic>> {
    let source = preprocess(source, file_name)?;
    compile_preprocessed(&source, file_name)
}
pub fn preprocess(source: &str, file_name: &str) -> Result<String, Vec<Diagnostic>> {
    preprocess_with_hosted(source, file_name, true)
}
pub fn preprocess_with_hosted(
    source: &str,
    file_name: &str,
    hosted: bool,
) -> Result<String, Vec<Diagnostic>> {
    let mut pp = Preprocessor::new_with_hosted(FsResolver, hosted);
    pp.run(file_name, source)
}
pub fn compile_preprocessed(source: &str, _file_name: &str) -> Result<String, Vec<Diagnostic>> {
    let mut l = lex::Lexer::new(source);
    let tokens = l.tokenize_all()?;
    let mut p = parser::Parser::new(tokens);
    let mut ast = p.parse_translation_unit()?;
    let mut s = sema::Sema::new();
    s.analyze(&mut ast)?;
    let mut lower = lower::Lowerer::new();
    let program = lower.lower_translation_unit(ast)?;
    codegen::QbeCodegen::new().emit_program(&program)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compile_main() {
        let x = compile("int main(){return 0;}", "test.c").unwrap();
        assert!(x.contains("export function w $main"));
        assert!(x.contains("ret %t0"));
    }
    #[test]
    fn compile_vars() {
        let x = compile("int main(){int x=3;return x+2;}", "test.c").unwrap();
        assert!(x.contains("ret"));
    }
}
