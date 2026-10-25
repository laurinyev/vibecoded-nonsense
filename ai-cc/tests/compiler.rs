use std::{env, fs, path::PathBuf, process::Command};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn case(file: &str) -> (String, PathBuf) {
    let path = root().join("tests/compiler").join(file);
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
fn compile_ir(file: &str) -> String {
    let (source, path) = case(file);
    ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap()
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
    let path = root().join("tests/compiler").join(file);
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
    let path = root().join("tests/compiler").join(file);
    let source = fs::read_to_string(&path).unwrap();
    let errors = ai_cc::compile(&source, path.to_str().unwrap()).unwrap_err();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|x| x.message.contains("unknown")));
}
#[test]
fn bad_syntax() {
    let file = "bad_syntax.c";
    let path = root().join("tests/compiler").join(file);
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
    compile_ok("declaration-specifiers.c");
}

#[test]
fn local_declarators() {
    let (source, path) = case("multiple-declarators.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
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
    ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
}

#[test]
fn prototype_params() {
    compile_ok("function-prototype.c");
}

#[test]
fn abstract_params() {
    let (source, _) = case("function-prototype-abstract.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    assert!(parser.parse_translation_unit().is_ok());
}

#[test]
fn nested_declarators() {
    let (source, _) = case("nested-function-declarators.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
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
    let (source, path) = case("function-forms.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
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
    ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();

    let errors = compile_errors("invalid-variadic.c");
    assert!(!errors.is_empty());

    let errors = compile_errors("too-few-variadic-arguments.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("expects 1 argument(s), got 0")));

    let ir = compile_ir("variadic-definition.c");
    assert!(ir.contains("..."), "{ir}");
}

#[test]
fn multi_declarators() {
    let (source, path) = case("multiple-function-declarators.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert_eq!(ast.items[0].name, "first");
    assert_eq!(ast.items[1].name, "second");
    assert_eq!(ast.items[2].name, "old");
    assert!(ast.items[2].params_unspecified);
    assert_eq!(ast.items[3].name, "fixed");
    assert!(!ast.items[3].params_unspecified);
    ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
}

#[test]
fn declaration_merge() {
    compile_ok("function-declaration-definition.c");
}

#[test]
fn compatible_redeclarations() {
    compile_ok("repeated-function-declaration.c");
}

#[test]
fn incompatible_redeclarations() {
    let errors = compile_errors("incompatible-function-redeclaration.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("conflicting types for value")));
}

#[test]
fn object_declaration_compatibility() {
    run_glibc_case("compatible-object-redeclarations.c", b"");
    compile_ok("compatible-old-style-and-qualified.c");
    compile_ok("extern-incomplete-objects.c");

    for (file, expected) in [
        ("conflicting-qualifiers.c", "conflicting types"),
        ("conflicting-pointed-qualifiers.c", "conflicting types"),
        ("conflicting-integer-types.c", "conflicting types"),
        ("incomplete-file-object.c", "invalid file-scope object type"),
        (
            "sizeof-incomplete.c",
            "sizeof requires a complete object type",
        ),
    ] {
        let errors = compile_errors(file);
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "missing {expected:?} in {errors:?}"
        );
    }
}

#[test]
fn tentative_incomplete_array_completion() {
    run_glibc_case("tentative-incomplete-array.c", b"");
}

#[test]
fn duplicate_definitions() {
    let errors = compile_errors("duplicate-function-definition.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("redefinition of value")));
}

#[test]
fn old_style_params() {
    let (source, path) = case("old-style-definition.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert_eq!(ast.items[0].params[0].0, "left");
    assert_eq!(ast.items[0].params[0].1, ai_cc::ast::Type::Int);
    assert_eq!(ast.items[0].params[1].0, "right");
    assert_eq!(ast.items[0].params[1].1, ai_cc::ast::Type::Char);
    ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
}

#[test]
fn old_style_missing_params() {
    let errors = compile_errors("old-style-missing-parameter.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("parameter right has no declaration")));
}

#[test]
fn parameter_errors() {
    let errors = compile_errors("unnamed-definition-parameter.c");
    assert!(errors.iter().any(|error| error
        .message
        .contains("function definition parameters must be named")));

    let errors = compile_errors("void-parameter.c");
    assert!(
        errors.iter().any(|error| error
            .message
            .contains("function parameter cannot have void type")),
        "{errors:?}"
    );
}

#[test]
fn array_param_declarators() {
    let (source, _) = case("array-parameter-declarators.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
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
    let (source, path) = case("function-pointer.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
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
    ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
}

#[test]
fn float_literals() {
    use ai_cc::lex::TokenKind as K;
    let (source, _) = case("float-literals.txt");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    assert!(matches!(tokens[0].kind, K::FloatConstant(_)));
    assert!(matches!(tokens[1].kind, K::FloatConstant(_)));
    assert!(matches!(tokens[2].kind, K::FloatConstant(_)));
}

#[test]
fn block_typedef_scope() {
    let (source, path) = case("block-typedef.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(matches!(
        ast.items[0].body.stmts[0],
        ai_cc::ast::Stmt::Typedef(..)
    ));
    ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
}

#[test]
fn local_typedefs() {
    let (source, _) = case("local-typedefs.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(matches!(
        ast.items[0].body.stmts[0],
        ai_cc::ast::Stmt::Typedefs(..)
    ));
}

#[test]
fn typedef_shadowing() {
    compile_ok("typedef-shadow.c");
}

#[test]
fn c99_tokens() {
    use ai_cc::lex::TokenKind as K;
    let (source, _) = case("c99-tokens.txt");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
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
    let (source, _) = case("hosted-macro.txt");
    assert_eq!(ai_cc::preprocess(&source, "hosted.c").unwrap(), "1\n");
    assert_eq!(
        ai_cc::preprocess_with_hosted(&source, "freestanding.c", false).unwrap(),
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

#[test]
fn diagnostic_spans() {
    let cases = [
        ("diagnostic-modifiable-lvalue.c", "modifiable lvalue"),
        ("diagnostic-argument-count.c", "expects 1 argument"),
        ("diagnostic-dereference.c", "dereference operand"),
        ("diagnostic-return-type.c", "return value is incompatible"),
        ("diagnostic-missing-member.c", "no member named missing"),
    ];
    for (file, expected) in cases {
        let errors = compile_errors(file);
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
    let (source, _) = case("integer-overflow.txt");
    let errors = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap_err();
    assert!(errors
        .iter()
        .any(|e| e.message.contains("integer constant is out of range")));
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn malformed_eof() {
    let errors = compile_errors("malformed-eof.c");
    assert!(!errors.is_empty());
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn malformed_conditionals() {
    let (source, path) = case("malformed-conditionals.c");
    let errors = ai_cc::preprocess(&source, path.to_str().unwrap()).unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("#else without")));
    assert!(errors
        .iter()
        .any(|e| e.message.contains("unterminated conditional")));
    assert!(errors.iter().all(|e| e.span.is_some()));
}

#[test]
fn error_directive() {
    let (source, path) = case("error-directive.c");
    let errors = ai_cc::preprocess(&source, path.to_str().unwrap()).unwrap_err();
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
    compile_ok("cast-sizeof-type-name.c");
}

#[test]
fn static_locals() {
    run_glibc_case("static-local.c", b"");
}

#[test]
fn extern_local() {
    let ir = compile_ir("extern-local.c");
    assert!(
        ir.contains("loadw $shared"),
        "expected external data reference: {ir}"
    );
}

#[test]
fn register_address() {
    let errors = compile_errors("register-address.c");
    assert!(errors.iter().any(|e| e.message.contains("register object")));
}

#[test]
fn duplicate_storage() {
    let errors = compile_errors("duplicate-storage-class.c");
    assert!(errors
        .iter()
        .any(|e| e.message.contains("multiple storage-class")));
}

#[test]
fn inline_linkage() {
    let (source, path) = case("static-inline-function.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(ast.items[0].is_static && ast.items[0].is_inline);
    let ir = ai_cc::compile_preprocessed(&source, path.to_str().unwrap()).unwrap();
    assert!(ir.contains("function w $helper"));
    assert!(!ir.contains("export function w $helper"));
}

#[test]
fn function_constraints() {
    let errors = compile_errors("static-array.c");
    assert!(errors.iter().any(|error| error
        .message
        .contains("static array parameter requires a size")));

    compile_ok("static-array-parameter.c");

    let (source, _) = case("function-parameter-static-array.c");
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(matches!(
        &ast.items[0].params[1].1,
        ai_cc::ast::Type::Qualified(inner, qualifiers)
            if qualifiers.is_static
                && matches!(**inner, ai_cc::ast::Type::Pointer(_))
    ));

    let errors = compile_errors("restrict-object.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("incomplete or invalid type")));

    let errors = compile_errors("qualified-array-object.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("invalid object type")));

    let errors = compile_errors("incomplete-return.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("return type is incomplete")));

    compile_ok("qualified-array-typedef.c");
}

#[test]
fn tag_scopes_and_shadowing() {
    compile_ok("tag-shadow.c");

    let errors = compile_errors("tag-scope.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("invalid object type for value")));
}

#[test]
fn tags_in_declarations_and_duplicates() {
    compile_ok("tag-declarator.c");

    let errors = compile_errors("duplicate-tag.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("redefinition of struct S")));
}

#[test]
fn tag_and_ordinary_namespaces() {
    compile_ok("tag-ordinary-namespace.c");
}

#[test]
fn function_prototype_tag_scope() {
    let errors = compile_errors("prototype-tag-scope.c");
    assert!(errors
        .iter()
        .any(|error| error.message.contains("invalid object type for item")));
}

#[test]
fn enumerator_ordinary_namespace() {
    compile_ok("enumerator-constants.c");
    compile_ok("enumerator-shadow.c");
}

#[test]
fn inheritance_of_internal_linkage() {
    let ir = compile_ir("internal-linkage.c");
    assert!(ir.contains("function w $helper"), "{ir}");
    assert!(!ir.contains("export function w $helper"), "{ir}");

    compile_ok("internal-object-linkage.c");
}

#[test]
fn conflicting_linkage() {
    for (file, expected) in [
        (
            "conflicting-linkage-static-after-extern.c",
            "static declaration of value follows non-static declaration",
        ),
        (
            "conflicting-linkage-nonstatic-after-static.c",
            "non-static declaration of value follows static declaration",
        ),
        (
            "conflicting-linkage-function.c",
            "static declaration of helper follows non-static declaration",
        ),
    ] {
        let errors = compile_errors(file);
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "{errors:?}"
        );
    }
}
