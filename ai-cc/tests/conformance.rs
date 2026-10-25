use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures(kind: &str) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(root().join("tests/conformance").join(kind))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|x| x.to_str()) == Some("c"))
        .collect();
    paths.sort();
    paths
}

fn compiler_name() -> String {
    env::var("CC").unwrap_or_else(|_| "cc".into())
}

fn qbe() -> PathBuf {
    env::var_os("QBE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("tools/qbe"))
}

fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!(
        "ai-cc-conformance-{}-{}-{}",
        label,
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn compile_ai(path: &Path, output: &Path) {
    let source = fs::read_to_string(path).unwrap();
    let ir = ai_cc::compile(&source, path.to_str().unwrap()).unwrap();
    let ir_path = output.with_extension("ssa");
    let asm_path = output.with_extension("s");
    fs::write(&ir_path, ir).unwrap();
    let status = Command::new(qbe())
        .args(["-o"])
        .arg(&asm_path)
        .arg(&ir_path)
        .status()
        .unwrap();
    assert!(status.success(), "QBE failed for {}", path.display());
    let status = Command::new(compiler_name())
        .arg(&asm_path)
        .arg("-o")
        .arg(output)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "host linker failed for {}",
        path.display()
    );
}

fn run_sandboxed(executable: &Path, work: &Path) -> Output {
    Command::new("bwrap")
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
        .arg(work)
        .arg(work)
        .arg("--chdir")
        .arg(work)
        .arg(executable)
        .output()
        .unwrap()
}

fn nonempty_lines(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn case(file: &str) -> (String, PathBuf) {
    let path = root().join("tests/conformance/cases").join(file);
    (fs::read_to_string(&path).unwrap(), path)
}

#[test]
fn accepted_fixtures() {
    for path in fixtures("valid") {
        let source = fs::read_to_string(&path).unwrap();
        ai_cc::compile(&source, path.to_str().unwrap())
            .unwrap_or_else(|errors| panic!("{} rejected: {errors:?}", path.display()));
    }
}

#[test]
fn rejected_fixtures() {
    for path in fixtures("invalid") {
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            ai_cc::compile(&source, path.to_str().unwrap()).is_err(),
            "{} was accepted",
            path.display()
        );
    }
}

#[test]
fn diagnostic_fixtures() {
    for path in fixtures("diagnostics") {
        let source = fs::read_to_string(&path).unwrap();
        let expected = source
            .lines()
            .find_map(|line| line.strip_prefix("/* EXPECT-DIAGNOSTIC: "))
            .and_then(|line| line.strip_suffix(" */"))
            .unwrap_or_else(|| panic!("{} has no diagnostic marker", path.display()));
        let errors = ai_cc::compile(&source, path.to_str().unwrap()).unwrap_err();
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "{} did not emit {:?}: {errors:?}",
            path.display(),
            expected
        );
        assert!(errors.iter().all(|error| error.span.is_some()));
    }
}

#[test]
fn preprocessor_reference() {
    let path = root().join("tests/conformance/preprocess/basic.c");
    let source = fs::read_to_string(&path).unwrap();
    let actual = ai_cc::preprocess(&source, path.to_str().unwrap()).unwrap();
    let reference = Command::new(compiler_name())
        .args(["-E", "-P"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(reference.status.success());
    assert_eq!(
        nonempty_lines(&actual),
        nonempty_lines(&String::from_utf8_lossy(&reference.stdout))
    );
}

#[test]
fn macro_expansion() {
    let (source, path) = case("macro-expansion.c");
    let output = ai_cc::preprocess(&source, path.to_str().unwrap()).unwrap();

    let lines = nonempty_lines(&output);
    assert!(lines.contains("((4) + (3))"), "{lines}");
    assert!(lines.contains("\"VALUE\""), "{lines}");
    assert!(lines.contains("main"), "{lines}");
    assert!(lines.contains("1 2, 3"), "{lines}");
    assert!(lines.contains("LEFT"), "{lines}");
    assert!(lines.contains("selected"), "{lines}");
    assert!(lines.contains("undefined"), "{lines}");
    assert!(!lines.contains("not_selected"), "{lines}");
}

#[test]
fn include_and_line() {
    let path = root().join("tests/conformance/preprocess/include_macro.c");
    let source = fs::read_to_string(&path).unwrap();
    let output = ai_cc::preprocess(&source, path.to_str().unwrap()).unwrap();
    assert!(nonempty_lines(&output).contains("9"));

    let (line_source, line_path) = case("line-directive.c");
    let output = ai_cc::preprocess(&line_source, line_path.to_str().unwrap()).unwrap();
    assert!(
        nonempty_lines(&output).contains("100 \"logical.c\""),
        "{output}"
    );
}

#[test]
fn macro_redefinition() {
    let (source, path) = case("macro-redefinition.c");
    let errors = ai_cc::preprocess(&source, path.to_str().unwrap()).unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("incompatible redefinition")));
}

#[test]
fn translation_phases() {
    let (trigraph, trigraph_path) = case("trigraph.c");
    let output = ai_cc::preprocess(&trigraph, trigraph_path.to_str().unwrap()).unwrap();
    assert_eq!(nonempty_lines(&output), "7");

    let (spliced, spliced_path) = case("spliced.c");
    ai_cc::compile(&spliced, spliced_path.to_str().unwrap()).unwrap();

    let (comments, comments_path) = case("comments.c");
    ai_cc::compile(&comments, comments_path.to_str().unwrap()).unwrap();

    let (ucn, ucn_path) = case("ucn.c");
    ai_cc::compile(&ucn, ucn_path.to_str().unwrap()).unwrap();

    let (tokens_source, _) = case("preprocessing-tokens.txt");
    let tokens = ai_cc::pp::tokenize_preprocessing(&tokens_source).unwrap();
    assert!(matches!(
        tokens[0].kind,
        ai_cc::pp::PreprocessingTokenKind::Identifier(ref value) if value == "int"
    ));
    assert!(matches!(
        tokens[1].kind,
        ai_cc::pp::PreprocessingTokenKind::Identifier(ref value) if value == "\\u0061"
    ));
    assert!(matches!(
        tokens[2].kind,
        ai_cc::pp::PreprocessingTokenKind::Punctuator(ref value) if value == "->"
    ));

    let (normalize_source, _) = case("normalize-source.txt");
    assert_eq!(
        ai_cc::pp::normalize_source(&normalize_source).unwrap(),
        "a\nb\nc\n"
    );
}

#[test]
fn unterminated_comment() {
    let (source, path) = case("unterminated-comment.c");
    let errors = ai_cc::compile(&source, path.to_str().unwrap()).unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("unterminated comment")));
    assert!(errors.iter().all(|error| error.span.is_some()));
}

#[test]
fn runtime_exit_status() {
    let path = root().join("tests/conformance/runtime/exit-code.c");
    let work = temp_dir("runtime");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(23));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn compound_literals() {
    let path = root().join("tests/conformance/valid/compound-literals.c");
    let work = temp_dir("compound-literals");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn function_pointers() {
    let path = root().join("tests/conformance/valid/function-pointers.c");
    let work = temp_dir("function-pointers");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn pointer_conversions() {
    run_valid_fixture("pointer-conversions.c");
}

#[test]
fn integer_constant_expressions() {
    run_valid_fixture("integer-constant-expressions.c");
}

#[test]
fn reversed_subscript() {
    run_valid_fixture("reversed-subscript.c");
}

#[test]
fn pointer_difference() {
    run_valid_fixture("pointer-difference.c");
}

#[test]
fn increment_decrement() {
    run_valid_fixture("increment-decrement.c");
}

#[test]
fn default_argument_promotions() {
    run_valid_fixture("default-argument-promotions.c");
}

#[test]
fn compound_assignment() {
    run_valid_fixture("compound-assignment.c");
}

#[test]
fn string_arrays() {
    let path = root().join("tests/conformance/valid/string-array-initialization.c");
    let work = temp_dir("string-array-initialization");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn compound_literal_errors() {
    for file in [
        "compound-literal-array-bound.c",
        "compound-literal-missing-member.c",
        "compound-literal-excess-list.c",
        "compound-literal-union-list.c",
    ] {
        let (source, path) = case(file);
        assert!(
            ai_cc::compile(&source, path.to_str().unwrap()).is_err(),
            "accepted: {file}"
        );
    }
}

#[test]
fn file_scope_objects() {
    let path = root().join("tests/conformance/valid/file-scope-objects.c");
    let source = fs::read_to_string(&path).unwrap();
    let tokens = ai_cc::lex::Lexer::new(&source).tokenize_all().unwrap();
    let mut parser = ai_cc::parser::Parser::new(tokens);
    let ast = parser.parse_translation_unit().unwrap();
    assert!(ast.declarations.iter().any(|declaration| {
        declaration.name == "external_tentative"
            && declaration.storage == Some(ai_cc::ast::StorageClass::Extern)
            && !declaration.tentative_definition
    }));
    assert!(ast.declarations.iter().any(|declaration| {
        declaration.name == "initialized" && declaration.initializer.is_some()
    }));

    let ir = ai_cc::compile(&source, path.to_str().unwrap()).unwrap();
    assert!(ir.contains("export data $initialized"), "{ir}");
    assert!(ir.contains("data $internal_tentative"), "{ir}");
    assert!(ir.contains("export data $external_tentative"), "{ir}");
    assert!(ir.contains("export data $callback"), "{ir}");

    let work = temp_dir("file-scope-objects");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn file_scope_errors() {
    let (source, path) = case("global-init.c");
    let errors = ai_cc::compile(&source, path.to_str().unwrap()).unwrap_err();
    assert!(errors.iter().any(|error| error
        .message
        .contains("not a supported constant expression")));
    assert!(errors.iter().all(|error| error.span.is_some()));

    let (source, path) = case("global-array.c");
    let ir = ai_cc::compile(&source, path.to_str().unwrap()).unwrap();
    assert!(
        ir.contains("export data $values = align 4 { z 12 }"),
        "{ir}"
    );

    let (source, path) = case("global-auto.c");
    let errors = ai_cc::compile(&source, path.to_str().unwrap()).unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("auto is not allowed at file scope")));
}

#[test]
fn old_style_definition() {
    let path = root().join("tests/conformance/valid/old-style-function-definition.c");
    let work = temp_dir("old-style-function");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn nested_definitions() {
    let path = root().join("tests/conformance/valid/nested-function-definitions.c");
    let work = temp_dir("nested-function-definition");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn differential_integer() {
    if env::var_os("AI_CC_RUN_DIFFERENTIAL").is_none() {
        eprintln!("skipping differential test; set AI_CC_RUN_DIFFERENTIAL=1 to enable");
        return;
    }
    let path = root().join("tests/conformance/runtime/exit-code.c");
    let work = temp_dir("differential");
    let ai_executable = work.join("ai-program");
    let host_executable = work.join("host-program");
    compile_ai(&path, &ai_executable);
    let status = Command::new(compiler_name())
        .arg(&path)
        .arg("-o")
        .arg(&host_executable)
        .status()
        .unwrap();
    assert!(status.success());
    let ai_output = run_sandboxed(&ai_executable, &work);
    let host_output = run_sandboxed(&host_executable, &work);
    assert_eq!(ai_output.status.code(), host_output.status.code());
    assert_eq!(ai_output.stdout, host_output.stdout);
    let _ = fs::remove_dir_all(work);
}

#[test]
fn brace_elision() {
    let path = root().join("tests/conformance/valid/brace-elision-and-layout.c");
    let work = temp_dir("brace-elision-and-layout");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn bit_fields() {
    let path = root().join("tests/conformance/valid/bit-fields.c");
    let work = temp_dir("bit-fields");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn floating_point() {
    let path = root().join("tests/conformance/valid/floating-point.c");
    let work = temp_dir("floating-point");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(&work);
}

#[test]
fn complex() {
    let path = root().join("tests/conformance/valid/complex.c");
    let work = temp_dir("complex");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(&work);
}

fn run_valid_fixture(name: &str) {
    let path = root().join("tests/conformance/valid").join(name);
    let work = temp_dir(name);
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(0));
    let _ = fs::remove_dir_all(&work);
}

#[test]
fn variadic_functions() {
    run_valid_fixture("variadic-functions.c");
}

#[test]
fn vla() {
    run_valid_fixture("variable-length-arrays.c");
}

#[test]
fn mixed_declarators() {
    run_valid_fixture("mixed-declarators-and-inline.c");
}

#[test]
fn block_typedefs() {
    run_valid_fixture("block-scope-typedefs.c");
}

#[test]
fn array_parameters() {
    run_valid_fixture("array-parameter-qualifiers.c");
}
