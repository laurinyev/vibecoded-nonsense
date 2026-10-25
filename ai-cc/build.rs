use std::{env, path::PathBuf, process::Command};
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    for name in ["crtbegin", "crtend", "crtstart"] {
        let src = root.join("crt").join(format!("{}.S", name));
        let dst = out.join(format!("{}.o", name));
        let assembler = env::var_os("AS").unwrap_or_else(|| "as".into());
        let s = Command::new(&assembler)
            .arg(&src)
            .arg("-o")
            .arg(&dst)
            .status()
            .expect("run as for compiler CRT");
        if !s.success() {
            panic!("assembler failed for {}", name)
        }
    }
    println!("cargo:rustc-env=AI_CC_BUILD_CRT_DIR={}", out.display());
    println!("cargo:rerun-if-changed=crt/crtbegin.S");
    println!("cargo:rerun-if-changed=crt/crtend.S");
    println!("cargo:rerun-if-changed=crt/crtstart.S");
}
