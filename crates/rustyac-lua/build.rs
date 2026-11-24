// SPDX-License-Identifier: MIT OR Apache-2.0

//! Feature `luajit` (Task C6a): LuaJIT 2.1 built as CSP built it, a static library (/MT, GC64,
//! JIT, FFI, LUAJIT_ENABLE_LUA52COMPAT). The source is commit `COMMIT` of the clone in
//! `LUAJIT_DIR`, else `ignored/luajit`. Needs MSVC (vswhere, vcvars64). The library goes to
//! `RUSTYAC_LUAJIT_LIB` (a folder several target folders share, CI), else `<target>/luajit-<commit>-mt`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The last upstream v2.1 commit before CSP's fork commit (unix time 1791809196, the version
/// "2.1.1791809196" in build 4305): docs/csp/lua_host.md.
const COMMIT: &str = "c6ffc141a8762b41703f9287d63d93622a13dd8f";

fn run(c: &mut Command) {
    let status = c.status().unwrap_or_else(|e| panic!("{c:?}: {e}"));
    assert!(status.success(), "{c:?}: {status}");
}

/// A build error that says what to install, instead of a panic's backtrace.
fn fail(why: &str) -> ! {
    eprintln!("error: rustyac-lua's feature `luajit` (the Windows rustyac.exe, tools/csp_oracle): {why}");
    std::process::exit(1);
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LUAJIT_DIR");
    println!("cargo:rerun-if-env-changed=RUSTYAC_LUAJIT_LIB");
    if std::env::var_os("CARGO_FEATURE_LUAJIT").is_none() {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") || std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64") {
        fail("x64 Windows only");
    }
    // a /MT LuaJIT in a dynamic-CRT exe links both runtimes; RUSTFLAGS replaces .cargo/config.toml's
    if !std::env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default().split(',').any(|f| f == "crt-static") {
        fail("the build has no +crt-static: RUSTFLAGS in the environment replaces .cargo/config.toml's; add `-C target-feature=+crt-static` to it");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // one build for every profile and check of this target folder (OUT_DIR is <target>/<profile>/build/<package>/out)
    let lib = std::env::var_os("RUSTYAC_LUAJIT_LIB").map(PathBuf::from).unwrap_or_else(|| out.ancestors().nth(4).unwrap_or(&out).join(format!("luajit-{}-mt", &COMMIT[..8])));
    if !lib.join("lua51.lib").exists() {
        let clone = std::env::var_os("LUAJIT_DIR").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ignored/luajit"));
        if !clone.join(".git").exists() {
            fail("no LuaJIT clone: clone it into ignored/luajit (git clone https://github.com/LuaJIT/LuaJIT.git ignored/luajit) or set LUAJIT_DIR to a clone");
        }
        // `git archive` writes .relver (export-subst), which the version string is made of
        let src = out.join("luajit-src");
        let tar = out.join("luajit.tar");
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(&src).unwrap();
        run(Command::new("git").arg("-C").arg(&clone).args(["archive", "-o"]).arg(&tar).arg(COMMIT));
        run(Command::new("tar").arg("-xf").arg(&tar).arg("-C").arg(&src));
        let vswhere = r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe";
        let vs = Command::new(vswhere).args(["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"]).output();
        let vs = vs.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        if vs.is_empty() || !Path::new(&vs).join(r"VC\Auxiliary\Build\vcvars64.bat").exists() {
            fail("no Visual Studio with its C++ tools was found (vswhere): install Visual Studio or its Build Tools with the \"Desktop development with C++\" workload");
        }
        // msvcbuild.bat's static build compiles without /MD, so with cl's default /MT; it runs
        // minilua.exe and buildvm.exe from the current folder, which this variable turns off
        let bat = out.join("luajit.bat");
        std::fs::write(
            &bat,
            format!(
                "@set NoDefaultCurrentDirectoryInExePath=\r\n@call \"{vs}\\VC\\Auxiliary\\Build\\vcvars64.bat\" >nul || exit /b 1\r\ncd /d \"{}\" || exit /b 1\r\ncall msvcbuild.bat lua52compat static || exit /b 1\r\n",
                src.join("src").display()
            ),
        )
        .unwrap();
        let status = Command::new("cmd").arg("/c").arg(&bat).status().unwrap_or_else(|e| fail(&format!("cmd: {e}")));
        if !status.success() || !src.join("src/lua51.lib").exists() {
            fail("msvcbuild.bat failed (above)");
        }
        // whole or not at all; a build of another profile that got there first is as good
        let staged = out.join("luajit-lib");
        let _ = std::fs::remove_dir_all(&staged);
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::copy(src.join("src/lua51.lib"), staged.join("lua51.lib")).unwrap();
        let _ = std::fs::create_dir_all(lib.parent().unwrap_or(&lib));
        if std::fs::rename(&staged, &lib).is_err() && !lib.join("lua51.lib").exists() {
            fail(&format!("could not move {} to {}", staged.display(), lib.display()));
        }
    }
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=lua51");
}
