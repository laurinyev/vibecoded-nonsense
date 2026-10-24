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
fn run_case(file: &str, expected: &[u8]) {
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
macro_rules! compiler_case {
    ($name:ident,$file:literal,$expected:expr) => {
        #[test]
        fn $name() {
            run_case($file, $expected);
        }
    };
}
compiler_case!(grouping, "grouping.c", b"G\n");
compiler_case!(macro_expansion, "macro.c", b")\n");
compiler_case!(control_flow, "control_flow.c", b"C\n");
compiler_case!(function_call, "function_call.c", b"5\n");
compiler_case!(
    glibc_output,
    "glibc_output.c",
    b"hello from glibc\nvalue=42\n"
);
compiler_case!(include_and_macro, "include.c", b"H\n");
compiler_case!(operator_precedence, "operator_precedence.c", b"G\n");
compiler_case!(all_scalar_operators, "operator_all.c", b"A\n");
compiler_case!(unary, "unary.c", b"B\n");
compiler_case!(header_stdbool, "header_stdbool.c", b"B\n");
compiler_case!(header_stdint, "header_stdint.c", b"I\n");
compiler_case!(header_stddef, "header_stddef.c", b"D\n");
compiler_case!(header_limits, "header_limits.c", b"8\n");
compiler_case!(header_float, "header_float.c", b"2\n");
compiler_case!(header_iso646, "header_iso646.c", b"O\n");
compiler_case!(header_stdarg, "header_stdarg.c", b"A\n");
compiler_case!(header_repeat, "header_repeat.c", b"R\n");
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
fn string_literal_concatenation() {
    let (source, path) = case("string-concat.c");
    let ir = ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
    assert!(ir.contains("aabb") == false);
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
    run_case("aggregate-lowering.c", b"");
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
    for file in ["unsupported-sizeof.c"] {
        assert!(
            compile_errors(file).iter().any(|_| true),
            "accepted unsupported operator: {file}"
        );
    }
}
