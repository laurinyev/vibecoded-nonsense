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
fn parser_recovery() {
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
fn diagnostic_rendering() {
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
fn lexer_errors() {
    let (source, _) = case("lexical-errors.c");
    let errors = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap_err();
    assert!(errors.len() >= 2);
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn c99_lexing() {
    let (source, path) = case("lexical-basics.c");
    let ir = ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
    assert!(ir.contains("ret"));
}

#[test]
fn integer_specifier_order() {
    let source = "int main(void) { unsigned long a = 1; long unsigned b = 2; int long unsigned c = 3; signed short int d = 4; short signed e = 5; char unsigned f = 6; signed char g = 7; long double h; return a + b + c + d + e + f + g; }";
    ai_cc::compile_preprocessed(source, "declaration-specifiers.c").unwrap();
}

#[test]
fn local_declarators() {
    let source = "int main(void) { int a = 2, b = a + 3, *p, (*row)[3]; const int c = 4; int const d = 5; int * const cp = &b; p = &b; return *p + c + d; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    let ai_cc::ast::Stmt::Decls(declarations, _) = &ast.items[0].body.stmts[0] else {
        panic!("expected declaration group");
    };
    assert_eq!(declarations.len(), 4);
    assert_eq!(
        declarations[3].1,
        ai_cc::ast::Type::Pointer(Box::new(ai_cc::ast::Type::Array(
            Box::new(ai_cc::ast::Type::Int),
            Some(3),
            false,
        )))
    );
    let ai_cc::ast::Stmt::Decl(_, trailing_const, _, _) = &ast.items[0].body.stmts[2] else {
        panic!("expected trailing-qualified declaration");
    };
    assert_eq!(
        trailing_const,
        &ai_cc::ast::Type::Qualified(
            Box::new(ai_cc::ast::Type::Int),
            ai_cc::ast::Qualifiers {
                is_const: true,
                is_volatile: false,
                is_restrict: false,
                is_static: false,
            }
        )
    );
    let ai_cc::ast::Stmt::Decl(_, qualified_pointer, _, _) = &ast.items[0].body.stmts[3] else {
        panic!("expected qualified pointer declaration");
    };
    assert_eq!(
        qualified_pointer,
        &ai_cc::ast::Type::Qualified(
            Box::new(ai_cc::ast::Type::Pointer(Box::new(ai_cc::ast::Type::Int))),
            ai_cc::ast::Qualifiers {
                is_const: true,
                is_volatile: false,
                is_restrict: false,
                is_static: false,
            }
        )
    );
    ai_cc::compile_preprocessed(source, "multiple-declarators.c").unwrap();
}

#[test]
fn prototype_params() {
    let source = "int add(int left, int right); int main(void) { return add(2, 3); }";
    ai_cc::compile_preprocessed(source, "function-prototype.c").unwrap();
}

#[test]
fn abstract_params() {
    let source = "int consume(int *); int main(void) { return 0; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    assert!(parser.parse_translation_unit().is_ok());
}

#[test]
fn nested_declarators() {
    let source = "int (*factory(void))(int); int (*factory2(int, int))(int); int (*array_factory(void))[4]; int apply(int (*callback)(int), int value); int invoke(int callback(int)); int consume(int matrix[static const 2][3], int *, int (*)(int)); int (*table[3])(int); int main(void) { return 0; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert_eq!(ast.items[0].name, "factory");
    assert!(ast.items[0].external);
    assert!(matches!(
        ast.items[0].ret,
        ai_cc::ast::Type::Pointer(ref inner)
            if matches!(**inner, ai_cc::ast::Type::Function(_, _, _, _))
    ));
    assert!(matches!(
        ast.items[2].ret,
        ai_cc::ast::Type::Pointer(ref inner)
            if matches!(**inner, ai_cc::ast::Type::Array(_, Some(4), _))
    ));
    assert_eq!(ast.items[1].params.len(), 2);
    assert_eq!(ast.items[3].params.len(), 2);
    assert_eq!(ast.items[4].params.len(), 1);
    assert_eq!(ast.items[5].params.len(), 3);
    assert!(matches!(
        ast.declarations[0].ty,
        ai_cc::ast::Type::Array(_, Some(3), _)
    ));
}

#[test]
fn variadic_declarators() {
    let source = "int old(); int old_identifier(a, b); int fixed(void); int format(register int level, ...); int (*legacy_callback)(); int (*identifier_callback)(a, b); int (*var_callback)(int, ...); int main(void) { format(1, 2, 3); return 0; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(ast.items[0].params_unspecified);
    assert!(ast.items[1].params_unspecified);
    assert!(!ast.items[2].params_unspecified);
    assert!(ast.items[3].is_variadic);
    assert!(matches!(
        ast.declarations[0].ty,
        ai_cc::ast::Type::Pointer(ref inner)
            if matches!(**inner, ai_cc::ast::Type::Function(_, _, false, false))
    ));
    assert!(matches!(
        ast.declarations[1].ty,
        ai_cc::ast::Type::Pointer(ref inner)
            if matches!(**inner, ai_cc::ast::Type::Function(_, _, false, false))
    ));
    assert!(matches!(
        ast.declarations[2].ty,
        ai_cc::ast::Type::Pointer(ref inner)
            if matches!(**inner, ai_cc::ast::Type::Function(_, _, true, true))
    ));
    ai_cc::compile_preprocessed(source, "function-forms.c").unwrap();

    let errors =
        ai_cc::compile_preprocessed("int invalid(...);", "invalid-variadic.c").unwrap_err();
    assert!(!errors.is_empty());

    let errors = ai_cc::compile_preprocessed(
        "int format(int level, ...); int main(void) { return format(); }",
        "too-few-variadic-arguments.c",
    )
    .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("expects 1 argument(s), got 0")));

    let ir = ai_cc::compile_preprocessed(
        "int supported(int first, ...) { return first; }",
        "variadic-definition.c",
    )
    .unwrap();
    assert!(ir.contains("..."), "{ir}");
}

#[test]
fn multi_declarators() {
    let source = "int first(int value), second(int value); int old(first, second), fixed(void); int main(void) { return 0; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert_eq!(ast.items[0].name, "first");
    assert_eq!(ast.items[1].name, "second");
    assert_eq!(ast.items[2].name, "old");
    assert!(ast.items[2].params_unspecified);
    assert_eq!(ast.items[3].name, "fixed");
    assert!(!ast.items[3].params_unspecified);
    ai_cc::compile_preprocessed(source, "multiple-function-declarators.c").unwrap();
}

#[test]
fn declaration_merge() {
    let source = "int add(int left, int right); int add(int left, int right) { return left + right; } int main(void) { return add(1, 2) - 3; }";
    ai_cc::compile_preprocessed(source, "function-declaration-definition.c").unwrap();
}

#[test]
fn compatible_redeclarations() {
    let source = "int value(void); int value(void); int value(void) { return 0; } int main(void) { return value(); }";
    ai_cc::compile_preprocessed(source, "repeated-function-declaration.c").unwrap();
}

#[test]
fn incompatible_redeclarations() {
    let errors = ai_cc::compile_preprocessed(
        "int value(void); long value(int); int main(void) { return 0; }",
        "incompatible-function-redeclaration.c",
    )
    .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("conflicting types for value")));
}

#[test]
fn duplicate_definitions() {
    let errors = ai_cc::compile_preprocessed(
        "int value(void) { return 1; } int value(void) { return 2; } int main(void) { return value(); }",
        "duplicate-function-definition.c",
    )
    .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("redefinition of value")));
}

#[test]
fn old_style_params() {
    let source = "int subtract(left, right) int left; char right; { return left - right; } int main(void) { return subtract(9, 4) - 5; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert_eq!(ast.items[0].params[0].0, "left");
    assert_eq!(ast.items[0].params[0].1, ai_cc::ast::Type::Int);
    assert_eq!(ast.items[0].params[1].0, "right");
    assert_eq!(ast.items[0].params[1].1, ai_cc::ast::Type::Char);
    ai_cc::compile_preprocessed(source, "old-style-definition.c").unwrap();
}

#[test]
fn old_style_missing_params() {
    let errors = ai_cc::compile_preprocessed(
        "int add(left, right) int left; { return left + right; }",
        "old-style-missing-parameter.c",
    )
    .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("parameter right has no declaration")));
}

#[test]
fn parameter_errors() {
    let errors = ai_cc::compile_preprocessed(
        "int missing_name(int) { return 0; }",
        "unnamed-definition-parameter.c",
    )
    .unwrap_err();
    assert!(errors.iter().any(|error| error
        .message
        .contains("function definition parameters must be named")));

    let errors =
        ai_cc::compile_preprocessed("int invalid(void value);", "void-parameter.c").unwrap_err();
    assert!(
        errors.iter().any(|error| error
            .message
            .contains("function parameter cannot have void type")),
        "{errors:?}"
    );
}

#[test]
fn array_param_declarators() {
    let source = "int consume(int values[const static 8]); int main(void) { int n = 3; int data[n]; return 0; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert_eq!(
        ast.items[0].params[0].1,
        ai_cc::ast::Type::Qualified(
            Box::new(ai_cc::ast::Type::Pointer(Box::new(ai_cc::ast::Type::Int))),
            ai_cc::ast::Qualifiers {
                is_const: true,
                is_volatile: false,
                is_restrict: false,
                is_static: true,
            }
        )
    );
    let ai_cc::ast::Stmt::Decl(_, ty, _, _) = &ast.items[1].body.stmts[1] else {
        panic!("expected VLA declaration");
    };
    assert!(matches!(
        ty,
        ai_cc::ast::Type::Vla(ref inner, _) if **inner == ai_cc::ast::Type::Int
    ));
}

#[test]
fn function_pointer_declarators() {
    let source = "int main(void) { int (*callback)(int); return 0; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    let ai_cc::ast::Stmt::Decl(_, ty, _, _) = &ast.items[0].body.stmts[0] else {
        panic!("expected function pointer declaration");
    };
    assert_eq!(
        ty,
        &ai_cc::ast::Type::Pointer(Box::new(ai_cc::ast::Type::Function(
            Box::new(ai_cc::ast::Type::Int),
            vec![ai_cc::ast::Type::Int],
            false,
            true
        )))
    );
    ai_cc::compile_preprocessed(source, "function-pointer.c").unwrap();
}

#[test]
fn float_literals() {
    use ai_cc::lex::TokenKind as K;
    let tokens = ai_cc::lex::Lexer::new("1.25 1e-3F 0x1.fp+2L")
        .tokenize_all()
        .unwrap();
    assert!(matches!(tokens[0].kind, K::FloatConstant(_)));
    assert!(matches!(tokens[1].kind, K::FloatConstant(_)));
    assert!(matches!(tokens[2].kind, K::FloatConstant(_)));
}

#[test]
fn block_typedef_scope() {
    let source = "int main(void) { typedef unsigned long word; word value = 3; return value; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(matches!(
        ast.items[0].body.stmts[0],
        ai_cc::ast::Stmt::Typedef(..)
    ));
    ai_cc::compile_preprocessed(source, "block-typedef.c").unwrap();
}

#[test]
fn local_typedefs() {
    let source =
        "int main(void) { typedef int *intptr, row[4]; intptr p; row values; p = 0; return 0; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(matches!(
        ast.items[0].body.stmts[0],
        ai_cc::ast::Stmt::Typedefs(..)
    ));
}

#[test]
fn typedef_shadowing() {
    let source =
        "typedef int T; int main(void) { { int T = 2; T = 3; } T value = 4; return value; }";
    ai_cc::compile_preprocessed(source, "typedef-shadow.c").unwrap();
}

#[test]
fn c99_tokens() {
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
fn string_concat() {
    let (source, path) = case("string-concat.c");
    let ir = ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
    assert!(!ir.contains("aabb"));
    assert!(ir.contains("ab"));
}

#[test]
fn loop_lowering() {
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
fn hosted_macros() {
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
fn qualified_types() {
    compile_ok("aggregate-types.c");
}

#[test]
fn member_access_compiles() {
    compile_ok("member-access.c");
}

#[test]
fn invalid_objects() {
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
fn diagnostic_spans() {
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
fn integer_overflow() {
    let errors = ai_cc::lex::Lexer::new("999999999999999999999999")
        .tokenize_all()
        .unwrap_err();
    assert!(errors
        .iter()
        .any(|e| e.message.contains("integer constant is out of range")));
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn malformed_eof() {
    let errors = inline_errors("int main(");
    assert!(!errors.is_empty());
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn malformed_conditionals() {
    let errors = ai_cc::preprocess("#else\n#if 1\nint x;\n", "directives.c").unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("#else without")));
    assert!(errors
        .iter()
        .any(|e| e.message.contains("unterminated conditional")));
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn error_directive() {
    let errors = ai_cc::preprocess("#error stop here\n", "directives.c").unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("#error")));
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn integer_spellings() {
    compile_ok("integer-types.c");
}

#[test]
fn float_spellings() {
    compile_ok("floating-types.c");
}

#[test]
fn tag_completion() {
    compile_ok("forward-struct.c");
}

#[test]
fn aggregate_members() {
    compile_ok("union-enum-types.c");
}

#[test]
fn pointer_declarators() {
    compile_ok("pointer-array-declarators.c");
}

#[test]
fn duplicate_members() {
    let e = compile_errors("duplicate-members.c");
    assert!(e
        .iter()
        .any(|x| x.message.contains("duplicate member value")));
}

#[test]
fn invalid_object_forms() {
    let e = compile_errors("invalid-object-forms.c");
    assert!(
        e.iter()
            .filter(|x| x.message.contains("invalid object type"))
            .count()
            >= 3
    );
}

#[test]
fn member_access() {
    compile_ok("member-access.c");
}

#[test]
fn aggregate_lowering() {
    compile_ok("aggregate-lowering.c");
}

#[test]
fn aggregate_qualifiers() {
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
fn sizeof_type() {
    let (source, path) = case("unsupported-sizeof.c");
    let ir = ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
    assert!(ir.contains("copy 4"), "int must have target size 4: {ir}");
}

#[test]
fn casts_and_sizeof() {
    let source =
        "int main(void) { long value = (long)3; return (int)(sizeof(long) + sizeof value); }";
    ai_cc::compile_preprocessed(source, "cast-sizeof-type-name.c").unwrap();
}

#[test]
fn static_locals() {
    run_glibc_case("static-local.c", b"");
}

#[test]
fn extern_local() {
    let ir = ai_cc::compile_preprocessed(
        "int main(void) { extern int shared; return shared; }",
        "extern-local.c",
    )
    .unwrap();
    assert!(
        ir.contains("loadw $shared"),
        "expected external data reference: {ir}"
    );
}

#[test]
fn register_address() {
    let errors = ai_cc::compile_preprocessed(
        "int main(void) { register int value = 1; return (int)&value; }",
        "register-address.c",
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("register object")));
}

#[test]
fn duplicate_storage() {
    let errors = ai_cc::compile_preprocessed(
        "int main(void) { static extern int value; return value; }",
        "duplicate-storage-class.c",
    )
    .unwrap_err();
    assert!(errors
        .iter()
        .any(|e| e.message.contains("multiple storage-class")));
}

#[test]
fn inline_linkage() {
    let source =
        "static inline int helper(void) { return 4; } int main(void) { return helper() - 4; }";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(ast.items[0].is_static && ast.items[0].is_inline);
    let ir = ai_cc::compile_preprocessed(source, "static-inline-function.c").unwrap();
    assert!(ir.contains("function w $helper"));
    assert!(!ir.contains("export function w $helper"));
}

#[test]
fn function_constraints() {
    let errors =
        ai_cc::compile_preprocessed("int f(int values[static]);", "static-array.c").unwrap_err();
    assert!(errors.iter().any(|error| error
        .message
        .contains("static array parameter requires a size")));

    ai_cc::compile_preprocessed(
        "int consume(int values[const static 4]); int main(void) { return 0; }",
        "static-array-parameter.c",
    )
    .unwrap();

    let source = "int scan(int count, int values[static count]);";
    let tokens = ai_cc::lex::Lexer::new(source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(matches!(
        &ast.items[0].params[1].1,
        ai_cc::ast::Type::Qualified(inner, qualifiers)
            if qualifiers.is_static
                && matches!(**inner, ai_cc::ast::Type::Pointer(_))
    ));

    let errors =
        ai_cc::compile_preprocessed("int f(int restrict value);", "restrict-object.c").unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("incomplete or invalid type")));

    let errors = ai_cc::compile_preprocessed(
        "int main(void) { int values[const 3]; return 0; }",
        "qualified-array-object.c",
    )
    .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("invalid object type")));

    let errors = ai_cc::compile_preprocessed(
        "struct S; struct S f(void) { return; }",
        "incomplete-return.c",
    )
    .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("return type is incomplete")));

    ai_cc::compile_preprocessed(
        "typedef int A[3]; const A values = { 1, 2, 3 }; int main(void) { return values[0] - 1; }",
        "qualified-array-typedef.c",
    )
    .unwrap();
}
