use std::{env, fs, path::PathBuf, process::Command};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn case(file: &str) -> (String, PathBuf) {
    let path = root().join("tests/cases").join(file);
    (fs::read_to_string(&path).unwrap(), path)
}
fn compile_ok(file: &str) {
    let (source, path) = case(file);
    ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
}
fn compile_errors(file: &str) -> Vec<ai_cc::diag::Diagnostic> {
    let (source, path) = case(file);
    ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap_err()
}
fn qbe() -> PathBuf {
    env::var_os("QBE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("tools/qbe"))
}
fn run_glibc_case(file: &str, expected: &[u8]) {
    let dir = env::temp_dir().join(format!(
        "ai-cc-rust-tests-{}-{}",
        std::process::id(),
        file.replace('.', "-")
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = root().join("tests/cases").join(file);
    let source = fs::read_to_string(&path).unwrap();
    let ir = ai_cc::compile(&source, path.to_str().unwrap()).unwrap();
    let ir_path = dir.join("program.ssa");
    let asm_path = dir.join("program.s");
    let exe_path = dir.join("program");
    fs::write(&ir_path, ir).unwrap();
    let s = Command::new(qbe())
        .arg("-o")
        .arg(&asm_path)
        .arg(&ir_path)
        .status()
        .unwrap();
    assert!(s.success(), "QBE failed for {}", file);
    let s = Command::new("cc")
        .arg(&asm_path)
        .arg("-o")
        .arg(&exe_path)
        .status()
        .unwrap();
    assert!(s.success(), "link failed for {}", file);
    let output = Command::new("bwrap")
        .args([
            "--die-with-parent",
            "--unshare-user",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--unshare-cgroup",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/tmp",
            "--bind",
        ])
        .arg(&dir)
        .arg(&dir)
        .arg("--chdir")
        .arg(&dir)
        .arg(&exe_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "runtime failed for {}: {}",
        file,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected, "stdout mismatch for {}", file);
}
#[test]
fn glibc_output_interop() {
    run_glibc_case("glibc_output.c", b"hello from glibc\nvalue=42\n");
}
#[test]
fn unknown_identifier() {
    let file = "unknown_identifier.c";
    let path = root().join("tests/cases").join(file);
    let source = fs::read_to_string(&path).unwrap();
    let errors = ai_cc::compile(&source, path.to_str().unwrap()).unwrap_err();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|x| x.message.contains("unknown")));
}
#[test]
fn bad_syntax() {
    let file = "bad_syntax.c";
    let path = root().join("tests/cases").join(file);
    let source = fs::read_to_string(&path).unwrap();
    let errors = ai_cc::compile(&source, path.to_str().unwrap()).unwrap_err();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|x| x.message.contains("expected")));
}

#[test]
fn parser_panic_mode_reports_multiple_syntax_errors() {
    let errors = compile_errors("multiple-errors.c");
    assert!(
        errors.len() >= 2,
        "expected multiple diagnostics, got {errors:?}"
    );
    assert!(errors
        .iter()
        .all(|e| e.message.contains("expected expression")));
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn diagnostics_render_human_readable_source_context() {
    let (source, path) = case("human-readable.c");
    let errors = ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap_err();
    let rendered = ai_cc::diag::DiagnosticFormatter::format(
        &errors[0],
        &ai_cc::diag::SourceMap::new(path.to_str().unwrap(), source),
    );
    assert!(rendered.contains("human-readable.c:2:"));
    assert!(rendered.contains("int value = ;"));
    assert!(rendered.contains("^"));
    assert!(rendered.contains("expected expression"));
}

#[test]
fn lexer_collects_multiple_errors() {
    let (source, _) = case("lexical-errors.c");
    let errors = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap_err();
    assert!(errors.len() >= 2);
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn lexical_c99_basics() {
    let (source, path) = case("lexical-basics.c");
    let ir = ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
    assert!(ir.contains("ret"));
}

#[test]
fn integer_declaration_specifiers_allow_c99_ordering() {
    let source = "int main(void) { unsigned long a = 1; long unsigned b = 2; int long unsigned c = 3; signed short int d = 4; short signed e = 5; char unsigned f = 6; signed char g = 7; long double h; return a + b + c + d + e + f + g; }";
    ai_cc::compile_preprocessed(source, "declaration-specifiers.c").unwrap();
}

#[test]
fn lexer_recognizes_c99_keywords_punctuator_and_integer_radices() {
    use ai_cc::lex::TokenKind as K;
    let tokens = ai_cc::lex::Lexer::new(
        "auto case default extern goto register switch inline _Complex _Imaginary ... 077 0xffU 42LL size_t int32_t",
    )
    .tokenize_all()
    .unwrap();
    let kinds: Vec<_> = tokens.iter().map(|token| &token.kind).collect();
    assert!(matches!(kinds[0], K::Auto));
    assert!(matches!(kinds[1], K::Case));
    assert!(matches!(kinds[2], K::Default));
    assert!(matches!(kinds[3], K::Extern));
    assert!(matches!(kinds[4], K::Goto));
    assert!(matches!(kinds[5], K::Register));
    assert!(matches!(kinds[6], K::Switch));
    assert!(matches!(kinds[7], K::Inline));
    assert!(matches!(kinds[8], K::Complex));
    assert!(matches!(kinds[9], K::Imaginary));
    assert!(matches!(kinds[10], K::Ellipsis));
    assert!(matches!(kinds[11], K::Number(63)));
    assert!(matches!(kinds[12], K::Number(255)));
    assert!(matches!(kinds[13], K::Number(42)));
    assert!(matches!(kinds[14], K::Ident(name) if name == "size_t"));
    assert!(matches!(kinds[15], K::Ident(name) if name == "int32_t"));
}

#[test]
fn string_literal_concatenation() {
    let (source, path) = case("string-concat.c");
    let ir = ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
    assert!(!ir.contains("aabb"));
    assert!(ir.contains("ab"));
}

#[test]
fn loop_control_parses_and_lowers() {
    let (source, path) = case("loop-lowering.c");
    let ir = ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
    assert!(ir.contains("@for") && ir.contains("@do"));
}

#[test]
fn loop_control_diagnostic() {
    let errors = compile_errors("bad-loop.c");
    assert!(errors.iter().any(|e| e.message.contains("outside loop")));
}

#[test]
fn conditional_inclusion() {
    let (source, path) = case("conditional.c");
    assert!(ai_cc::compile(&source, path.to_str().unwrap()).is_ok());
}

#[test]
fn hosted_predefined_macro_matches_translation_mode() {
    assert_eq!(
        ai_cc::preprocess("__STDC_HOSTED__", "hosted.c").unwrap(),
        "1\n"
    );
    assert_eq!(
        ai_cc::preprocess_with_hosted("__STDC_HOSTED__", "freestanding.c", false).unwrap(),
        "0\n"
    );
}

#[test]
fn aggregate_and_qualified_types() {
    compile_ok("aggregate-types.c");
}

#[test]
fn member_access_compiles() {
    compile_ok("member-access.c");
}

#[test]
fn incomplete_and_void_objects_are_rejected() {
    let errors = compile_errors("invalid-types.c");
    assert!(
        errors
            .iter()
            .filter(|e| e.message.contains("invalid object type"))
            .count()
            >= 2
    );
}

fn inline_errors(source: &str) -> Vec<ai_cc::diag::Diagnostic> {
    ai_cc::compile_preprocessed(source, "inline.c").unwrap_err()
}

#[test]
fn semantic_constraint_diagnostics_have_spans() {
    let cases = [
        (
            "int main(void){ const int x = 1; x = 2; return 0; }",
            "modifiable lvalue",
        ),
        (
            "int f(int x){ return x; } int main(void){ return f(); }",
            "expects 1 argument",
        ),
        ("int main(void){ int x; return *x; }", "dereference operand"),
        (
            "int main(void){ int *x; return x; }",
            "return value is incompatible",
        ),
        (
            "struct Pair { int value; }; int main(void){ struct Pair p; return p.missing; }",
            "no member named missing",
        ),
    ];
    for (source, expected) in cases {
        let errors = inline_errors(source);
        assert!(
            errors.iter().any(|e| e.message.contains(expected)),
            "missing {expected:?} in {errors:?}"
        );
        assert!(
            errors.iter().all(|e| e.span.is_some()),
            "unspanned: {errors:?}"
        );
    }
}

#[test]
fn lexer_rejects_integer_overflow_without_panicking() {
    let errors = ai_cc::lex::Lexer::new("999999999999999999999999")
        .tokenize_all()
        .unwrap_err();
    assert!(errors
        .iter()
        .any(|e| e.message.contains("integer constant is out of range")));
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn malformed_eof_returns_diagnostic_without_panicking() {
    let errors = inline_errors("int main(");
    assert!(!errors.is_empty());
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn malformed_preprocessor_conditionals_are_diagnosed() {
    let errors = ai_cc::preprocess("#else\n#if 1\nint x;\n", "directives.c").unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("#else without")));
    assert!(errors
        .iter()
        .any(|e| e.message.contains("unterminated conditional")));
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn preprocessor_error_directive_is_diagnosed() {
    let errors = ai_cc::preprocess("#error stop here\n", "directives.c").unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("#error")));
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn all_standard_integer_spellings_parse() {
    compile_ok("integer-types.c");
}

#[test]
fn floating_type_spellings_parse() {
    compile_ok("floating-types.c");
}

#[test]
fn tagged_aggregate_forward_reference_and_completion() {
    compile_ok("forward-struct.c");
}

#[test]
fn union_enum_and_qualified_members_parse() {
    compile_ok("union-enum-types.c");
}

#[test]
fn pointer_and_array_declarators_parse() {
    compile_ok("pointer-array-declarators.c");
}

#[test]
fn duplicate_members_are_rejected() {
    let e = compile_errors("duplicate-members.c");
    assert!(e
        .iter()
        .any(|x| x.message.contains("duplicate member value")));
}

#[test]
fn incomplete_and_void_objects_are_rejected_in_all_forms() {
    let e = compile_errors("invalid-object-forms.c");
    assert!(
        e.iter()
            .filter(|x| x.message.contains("invalid object type"))
            .count()
            >= 3
    );
}

#[test]
fn member_access_is_accepted() {
    compile_ok("member-access.c");
}

#[test]
fn aggregate_lowering_runs() {
    compile_ok("aggregate-lowering.c");
}

#[test]
fn aggregate_qualifier_keywords_lex() {
    let (source, _) = case("type-keywords.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    assert!(matches!(tokens[0].kind, ai_cc::lex::TokenKind::Struct));
    assert!(matches!(tokens[1].kind, ai_cc::lex::TokenKind::Union));
    assert!(matches!(tokens[2].kind, ai_cc::lex::TokenKind::Enum));
    assert!(matches!(tokens[3].kind, ai_cc::lex::TokenKind::Const));
    assert!(matches!(tokens[4].kind, ai_cc::lex::TokenKind::Volatile));
    assert!(matches!(tokens[5].kind, ai_cc::lex::TokenKind::Restrict));
}

#[test]
fn unsupported_pointer_sizeof_and_subscript_operators_are_diagnosed() {
    let file = "unsupported-sizeof.c";
    assert!(
        compile_errors(file).iter().any(|_| true),
        "accepted unsupported operator: {file}"
    );
}
