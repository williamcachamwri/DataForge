//! Build script embedding the TableMinus Win32 icon and Per-Monitor DPI v2 manifest.

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn emit_cargo(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
}

fn find_rc_tool() -> Option<PathBuf> {
    for candidate in ["rc.exe", "llvm-rc.exe"] {
        if Command::new(candidate).arg("/?").output().is_ok() {
            return Some(PathBuf::from(candidate));
        }
    }
    let llvm_default = Path::new(r"C:\Program Files\LLVM\bin\llvm-rc.exe");
    if llvm_default.exists() {
        return Some(llvm_default.to_path_buf());
    }
    None
}

fn main() {
    emit_cargo("cargo:rerun-if-changed=assets/tableminus.rc");
    emit_cargo("cargo:rerun-if-changed=assets/tableminus.ico");
    emit_cargo("cargo:rerun-if-changed=assets/tableminus.exe.manifest");

    let Ok(target_os) = env::var("CARGO_CFG_TARGET_OS") else {
        return;
    };
    if target_os != "windows" {
        return;
    }

    let Ok(out_dir) = env::var("OUT_DIR") else {
        return;
    };
    let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") else {
        return;
    };

    let rc_path = Path::new(&manifest_dir)
        .join("assets")
        .join("tableminus.rc");
    let res_path = Path::new(&out_dir).join("tableminus.res");

    if let Some(rc_tool) = find_rc_tool() {
        let mut cmd = Command::new(&rc_tool);
        cmd.arg("/FO").arg(&res_path).arg(&rc_path);
        if let Ok(status) = cmd.status()
            && status.success()
            && res_path.exists()
        {
            emit_cargo(&format!(
                "cargo:rustc-link-arg-bin=tableminus={}",
                res_path.display()
            ));
        }
    }
}
