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
    let source = r#"
#define VALUE 4
#define ADD(a, b) ((a) + (b))
#define STR(x) #x
#define CAT(a, b) a ## b
#define JOIN(first, ...) first __VA_ARGS__
#define LEFT RIGHT
#define RIGHT LEFT
ADD(VALUE, 3)
STR(VALUE)
CAT(ma, in)
JOIN(1, 2, 3)
LEFT
#if defined(VALUE) && (2 + 3 * 4 == 14)
selected
#else
not_selected
#endif
#undef VALUE
#ifdef VALUE
wrong
#else
undefined
#endif
"#;
    let output = ai_cc::preprocess(source, "macros.c").unwrap();
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

    let output =
        ai_cc::preprocess("#line 100 \"logical.c\"\n__LINE__ __FILE__\n", "physical.c").unwrap();
    assert!(
        nonempty_lines(&output).contains("100 \"logical.c\""),
        "{output}"
    );
}

#[test]
fn macro_redefinition() {
    let errors = ai_cc::preprocess("#define VALUE 1\n#define VALUE 2\n", "macros.c").unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.message.contains("incompatible redefinition")));
}

#[test]
fn translation_phases() {
    let trigraph = "??=define ANSWER 7\nANSWER\n";
    let output = ai_cc::preprocess(trigraph, "trigraph.c").unwrap();
    assert_eq!(nonempty_lines(&output), "7");

    let spliced = "#define ANSWER 1\\\n2\nint main(void){return ANSWER;}\n";
    ai_cc::compile(spliced, "spliced.c").unwrap();

    let comments = "int/* comment\n */main(void){return 0;}\n";
    ai_cc::compile(comments, "comments.c").unwrap();

    let ucn = r"int main(void){ int \u0061 = 3; return \u0061; }";
    ai_cc::compile(ucn, "ucn.c").unwrap();

    let tokens = ai_cc::pp::tokenize_preprocessing("int \\u0061 -> 1\n").unwrap();
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

    assert_eq!(
        ai_cc::pp::normalize_source("a\r\nb\rc").unwrap(),
        "a\nb\nc\n"
    );
}

#[test]
fn unterminated_comment() {
    let errors = ai_cc::compile("int main(void){return 0;} /*", "comment.c").unwrap_err();
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
    for source in [
        "int main(void) { return (int[2]){ [2] = 1 }[0]; }",
        "struct S { int value; }; int main(void) { return (struct S){ .missing = 1 }.value; }",
        "int main(void) { return (int){ 1, 2 }; }",
        "union U { int value; char byte; }; int main(void) { return (union U){ 1, 2 }.value; }",
    ] {
        assert!(
            ai_cc::compile(source, "compound-literal-invalid.c").is_err(),
            "accepted: {source}"
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
    let errors = ai_cc::compile("int source; int copy = source;", "global-init.c").unwrap_err();
    assert!(errors.iter().any(|error| error
        .message
        .contains("not a supported constant expression")));
    assert!(errors.iter().all(|error| error.span.is_some()));

    let ir = ai_cc::compile("int values[3];", "global-array.c").unwrap();
    assert!(
        ir.contains("export data $values = align 4 { z 12 }"),
        "{ir}"
    );

    let errors = ai_cc::compile("auto int value;", "global-auto.c").unwrap_err();
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
