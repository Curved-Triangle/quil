use std::{path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    if std::env::consts::OS != "macos" {
        return;
    }

    // The prebuilt Core ML ONNX Runtime uses __isPlatformVersionAtLeast from
    // Clang's runtime. Rust links with -nodefaultlibs, so expose the matching
    // resource library explicitly rather than relying on a machine-specific path.
    let output = Command::new("clang")
        .arg("-print-file-name=libclang_rt.osx.a")
        .output()
        .expect("run clang to locate libclang_rt.osx.a");
    if !output.status.success() {
        panic!("clang failed to locate libclang_rt.osx.a");
    }
    let path = PathBuf::from(
        String::from_utf8(output.stdout)
            .expect("libclang_rt path is UTF-8")
            .trim(),
    );
    if !path.is_file() {
        panic!("libclang_rt.osx.a was not found at {}", path.display());
    }
    let parent = path.parent().expect("libclang_rt has a parent directory");
    println!("cargo:rustc-link-search=native={}", parent.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
