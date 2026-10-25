use ai_cc::{
    compile_preprocessed,
    diag::{Diagnostic, DiagnosticFormatter, SourceMap},
    preprocess_with_hosted,
};
use clap::{ArgAction, Parser};
use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{self, Command},
};

#[derive(Parser, Debug)]
#[command(name = "ai-cc", version, about = "Toy C99 to QBE compiler")]
struct Cli {
    #[arg(short = 'E', action = ArgAction::SetTrue, help = "Preprocess source only and write C output", conflicts_with_all = ["emit_qbe", "compile_only"])]
    preprocess_only: bool,
    #[arg(short = 'S', action = ArgAction::SetTrue, help = "Compile to QBE IR and write it to stdout or -o FILE", conflicts_with = "compile_only")]
    emit_qbe: bool,
    #[arg(short = 'c', action = ArgAction::SetTrue, help = "Compile to object file without linking")]
    compile_only: bool,
    #[arg(short = 'v', action = ArgAction::SetTrue, help = "Print external tool commands")]
    verbose: bool,
    #[arg(long, action = ArgAction::SetTrue, help = "Link without glibc or host startup files")]
    freestanding: bool,
    #[arg(short = 'o', value_name = "FILE", help = "Write output to FILE")]
    output: Option<PathBuf>,
    #[arg(
        value_name = "INPUT",
        required = true,
        help = "C source file(s), or - for stdin"
    )]
    inputs: Vec<PathBuf>,
}

struct Unit {
    name: String,
    ir: String,
}

struct WorkDir(PathBuf);
impl WorkDir {
    fn new() -> Result<Self, String> {
        let path = env::temp_dir().join(format!("ai-cc-{}", process::id()));
        fs::create_dir_all(&path).map_err(|e| format!("create temporary directory: {}", e))?;
        Ok(Self(path))
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn report(name: &str, source: &str, errors: Vec<Diagnostic>) {
    let map = SourceMap::new(name, source);
    for error in errors {
        eprintln!("{}", DiagnosticFormatter::format(&error, &map));
    }
}

fn read_input(path: &Path) -> Result<String, String> {
    if path == Path::new("-") {
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .map_err(|e| format!("read stdin: {}", e))?;
        Ok(source)
    } else {
        fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))
    }
}

fn output(path: Option<&Path>, text: &str) -> Result<(), String> {
    match path {
        None => {
            print!("{}", text);
            Ok(())
        }
        Some(path) if path == Path::new("-") => {
            print!("{}", text);
            Ok(())
        }
        Some(path) => fs::write(path, text).map_err(|e| format!("{}: {}", path.display(), e)),
    }
}

fn stem(path: &Path) -> String {
    if path == Path::new("-") {
        "stdin".into()
    } else {
        path.file_stem()
            .and_then(|x| x.to_str())
            .unwrap_or("output")
            .into()
    }
}

fn qbe_path() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("AI_CC_QBE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "AI_CC_QBE is not an executable file: {}",
            path.display()
        ));
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(bin) = exe.parent() {
            candidates.push(bin.join("qbe"));
            if let Some(prefix) = bin.parent() {
                candidates.push(prefix.join("libexec/ai-cc/qbe"));
            }
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/qbe"));
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            candidates.push(dir.join("qbe"));
        }
    }
    candidates
        .into_iter()
        .find(|x| x.is_file())
        .ok_or_else(|| "QBE not found; set AI_CC_QBE or install qbe".into())
}

fn tool_path(name: &str, variable: &str) -> Result<PathBuf, String> {
    if let Ok(path) = env::var(variable) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("{} is not a file: {}", variable, path.display()));
    }
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            let path = dir.join(name);
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    Err(format!("{} not found in PATH; set {}", name, variable))
}

fn run_qbe(qbe: &Path, ir: &Path, asm: &Path, verbose: bool) -> Result<(), String> {
    let mut command = Command::new(qbe);
    command.arg("-o").arg(asm).arg(ir);
    if verbose {
        eprintln!("ai-cc: {:?}", command);
    }
    let status = Command::new(qbe)
        .arg("-o")
        .arg(asm)
        .arg(ir)
        .status()
        .map_err(|e| format!("run QBE: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("QBE failed with status {}", status))
    }
}

fn compiler_crt(name: &str) -> Result<PathBuf, String> {
    let mut paths = Vec::new();
    if let Ok(dir) = env::var("AI_CC_CRT_DIR") {
        paths.push(PathBuf::from(dir).join(name));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(bin) = exe.parent() {
            if let Some(prefix) = bin.parent() {
                paths.push(prefix.join("libexec/ai-cc/crt").join(name));
            }
        }
    }
    paths.push(Path::new(env!("AI_CC_BUILD_CRT_DIR")).join(name));
    paths
        .into_iter()
        .find(|x| x.is_file())
        .ok_or_else(|| format!("compiler CRT not found: {}", name))
}

fn host_file(name: &str, verbose: bool) -> Result<PathBuf, String> {
    let mut command = Command::new("cc");
    command.arg(format!("-print-file-name={}", name));
    if verbose {
        eprintln!("ai-cc: {:?}", command);
    }
    let output = command
        .output()
        .map_err(|e| format!("find {}: {}", name, e))?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path = PathBuf::from(path);
    if output.status.success() && path.is_file() {
        Ok(path)
    } else {
        Err(format!("host file not found: {}", name))
    }
}

fn dynamic_linker(verbose: bool) -> Result<PathBuf, String> {
    if let Ok(path) = env::var("AI_CC_DYNAMIC_LINKER") {
        return Ok(PathBuf::from(path));
    }
    for name in [
        "ld-linux-x86-64.so.2",
        "ld-linux-aarch64.so.1",
        "ld-linux.so.2",
    ] {
        if let Ok(path) = host_file(name, verbose) {
            return Ok(path);
        }
    }
    for dir in ["/lib64", "/lib", "/usr/lib64", "/usr/lib"] {
        if let Ok(entries) = fs::read_dir(dir) {
            // BUG: first ld-linux match is accepted without checking ELF class or architecture.
            if let Some(path) = entries.flatten().map(|x| x.path()).find(|x| {
                x.file_name()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x.starts_with("ld-linux") && x.is_ascii())
            }) {
                return Ok(path);
            }
        }
    }
    Err("dynamic linker not found; set AI_CC_DYNAMIC_LINKER".into())
}

fn compile_object(
    qbe: &Path,
    unit: &Unit,
    work: &WorkDir,
    output: &Path,
    index: usize,
    verbose: bool,
) -> Result<(), String> {
    let ir = work.path(&format!("unit-{}.ssa", index));
    let asm = work.path(&format!("unit-{}.s", index));
    fs::write(&ir, &unit.ir).map_err(|e| format!("{}: {}", ir.display(), e))?;
    run_qbe(qbe, &ir, &asm, verbose)?;
    let as_path = tool_path("as", "AI_CC_AS")?;
    let mut command = Command::new(&as_path);
    command.arg(&asm).arg("-o").arg(output);
    if verbose {
        eprintln!("ai-cc: {:?}", command);
    }
    let status = command.status().map_err(|e| format!("run as: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("as failed with status {}", status))
    }
}

fn run(cli: Cli) -> Result<(), String> {
    if cli.output.is_some()
        && cli.inputs.len() > 1
        && (cli.preprocess_only || cli.emit_qbe || cli.compile_only)
    {
        return Err("-o with multiple inputs requires separate output files".into());
    }
    let mut units = Vec::new();
    let mut preprocessed = String::new();
    for input in &cli.inputs {
        let name = input.display().to_string();
        let raw = read_input(input)?;
        let pre = match preprocess_with_hosted(&raw, &name, !cli.freestanding) {
            Ok(x) => x,
            Err(errors) => {
                report(&name, &raw, errors);
                return Err("preprocessing failed".into());
            }
        };
        preprocessed.push_str(&pre);
        if !cli.preprocess_only {
            let ir = match compile_preprocessed(&pre, &name) {
                Ok(x) => x,
                Err(errors) => {
                    report(&name, &pre, errors);
                    return Err("compilation failed".into());
                }
            };
            units.push(Unit { name, ir });
        }
    }
    if cli.preprocess_only {
        return output(cli.output.as_deref(), &preprocessed);
    }
    if cli.emit_qbe {
        let ir = units.iter().map(|x| x.ir.as_str()).collect::<String>();
        return output(cli.output.as_deref(), &ir);
    }
    let qbe = qbe_path()?;
    let work = WorkDir::new()?;
    if cli.compile_only {
        for (index, unit) in units.iter().enumerate() {
            let path = cli
                .output
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("{}.o", stem(Path::new(&unit.name)))));
            compile_object(&qbe, unit, &work, &path, index, cli.verbose)?;
        }
        return Ok(());
    }
    let ir = work.path("program.ssa");
    let asm = work.path("program.s");
    let object = work.path("program.o");
    fs::write(&ir, units.iter().map(|x| x.ir.as_str()).collect::<String>())
        .map_err(|e| format!("{}: {}", ir.display(), e))?;
    run_qbe(&qbe, &ir, &asm, cli.verbose)?;
    let as_path = tool_path("as", "AI_CC_AS")?;
    let mut assemble = Command::new(&as_path);
    assemble.arg(&asm).arg("-o").arg(&object);
    if cli.verbose {
        eprintln!("ai-cc: {:?}", assemble);
    }
    let status = assemble.status().map_err(|e| format!("run as: {}", e))?;
    if !status.success() {
        return Err(format!("as failed with status {}", status));
    }
    let target = cli.output.unwrap_or_else(|| PathBuf::from("a.out"));
    let linker = tool_path("ld", "AI_CC_LD")?;
    let mut command = Command::new(&linker);
    if cli.freestanding {
        let crtstart = compiler_crt("crtstart.o")?;
        let crtend = compiler_crt("crtend.o")?;
        command
            .arg("-e")
            .arg("_start")
            .arg(&crtstart)
            .arg(&object)
            .arg(&crtend);
    } else {
        let crt1 = host_file("crt1.o", cli.verbose)?;
        let crti = host_file("crti.o", cli.verbose)?;
        let crtn = host_file("crtn.o", cli.verbose)?;
        let libgcc = host_file("libgcc.a", cli.verbose)?;
        let crtbegin = compiler_crt("crtbegin.o")?;
        let crtend = compiler_crt("crtend.o")?;
        let dynamic = dynamic_linker(cli.verbose)?;
        command
            .arg("-dynamic-linker")
            .arg(&dynamic)
            .arg(&crt1)
            .arg(&crti)
            .arg(&crtbegin)
            .arg(&object)
            .arg(&crtend)
            .arg(&crtn)
            .arg("--start-group")
            .arg("-lc")
            .arg("-lgcc")
            .arg("--end-group")
            .arg("-L")
            .arg(libgcc.parent().unwrap_or(Path::new(".")));
    }
    command.arg("-o").arg(&target);
    if cli.verbose {
        eprintln!("ai-cc: {:?}", command);
    }
    let status = command.status().map_err(|e| format!("run ld: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ld failed with status {}", status))
    }
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("ai-cc: {}", error);
        process::exit(1);
    }
}
