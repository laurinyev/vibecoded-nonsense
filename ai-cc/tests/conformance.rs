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
fn accepted_fixtures_compile() {
    for path in fixtures("valid") {
        let source = fs::read_to_string(&path).unwrap();
        ai_cc::compile(&source, path.to_str().unwrap())
            .unwrap_or_else(|errors| panic!("{} rejected: {errors:?}", path.display()));
    }
}

#[test]
fn rejected_fixtures_fail() {
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
fn required_diagnostics_match_fixture_expectations() {
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
fn preprocessor_matches_reference_for_basic_fixture() {
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
fn runtime_fixture_has_expected_exit_status() {
    let path = root().join("tests/conformance/runtime/exit-code.c");
    let work = temp_dir("runtime");
    let executable = work.join("ai-program");
    compile_ai(&path, &executable);
    let output = run_sandboxed(&executable, &work);
    assert_eq!(output.status.code(), Some(23));
    let _ = fs::remove_dir_all(work);
}

#[test]
fn differential_integer_program_matches_host_compiler() {
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
