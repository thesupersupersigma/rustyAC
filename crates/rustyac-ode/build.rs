// SPDX-License-Identifier: BSD-3-Clause

//! Feature `embree` (Task C5h): Embree 3.13.4 built as CSP built it, a static library. The
//! source is `EMBREE_DIR`, else tag v3.13.4 of the clone in `ignored/embree`. Needs CMake,
//! Ninja and MSVC (found with vswhere, set up with vcvars64). The library goes to
//! `RUSTYAC_EMBREE_LIB` (a folder several target folders share, CI), else `<target>/embree-3.13.4`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(c: &mut Command) {
    let status = c.status().unwrap_or_else(|e| panic!("{c:?}: {e}"));
    assert!(status.success(), "{c:?}: {status}");
}

/// A build error that says what to install, instead of a panic's backtrace.
fn fail(why: &str) -> ! {
    eprintln!("error: rustyac-ode's feature `embree` (the Windows rustyac.exe, tools/csp_oracle): {why}");
    std::process::exit(1);
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=EMBREE_DIR");
    println!("cargo:rerun-if-env-changed=RUSTYAC_EMBREE_LIB");
    if std::env::var_os("CARGO_FEATURE_EMBREE").is_none() {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // one build for every profile and check of this target folder (OUT_DIR is
    // <target>/<profile>/build/<package>/out): clippy and the tests do not build it again
    let lib = std::env::var_os("RUSTYAC_EMBREE_LIB").map(PathBuf::from).unwrap_or_else(|| out.ancestors().nth(4).unwrap_or(&out).join("embree-3.13.4"));
    if !lib.join("embree3.lib").exists() {
        let build = out.join("embree-build");
        let src = match std::env::var_os("EMBREE_DIR") {
            Some(d) => PathBuf::from(d),
            None => {
                let clone = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ignored/embree");
                if !clone.join(".git").exists() {
                    fail("no Embree source: set EMBREE_DIR to Embree 3.13.4's source, or clone it into ignored/embree (git clone --branch v3.13.4 https://github.com/embree/embree.git ignored/embree)");
                }
                let src = out.join("embree-src");
                let tar = out.join("embree.tar");
                std::fs::create_dir_all(&src).unwrap();
                run(Command::new("git").arg("-C").arg(&clone).args(["archive", "-o"]).arg(&tar).arg("v3.13.4"));
                run(Command::new("tar").arg("-xf").arg(&tar).arg("-C").arg(&src));
                src
            }
        };
        let vswhere = r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe";
        // only an install with the C++ compiler (the "Desktop development with C++" workload)
        let vs = Command::new(vswhere).args(["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"]).output();
        let vs = vs.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        if vs.is_empty() || !Path::new(&vs).join(r"VC\Auxiliary\Build\vcvars64.bat").exists() {
            fail("no Visual Studio with its C++ tools was found (vswhere): install Visual Studio or its Build Tools with the \"Desktop development with C++\" workload (MSVC, CMake and Ninja)");
        }
        let jobs = std::env::var("NUM_JOBS").unwrap_or("4".into());
        // CSP's options (docs/csp/core.md "Embree"): ISAs SSE2 / AVX / AVX2, internal tasking,
        // no filter functions, ray masks or back-face culling; CMake 4 needs the policy floor
        let bat = out.join("embree.bat");
        std::fs::write(
            &bat,
            format!(
                "@call \"{vs}\\VC\\Auxiliary\\Build\\vcvars64.bat\" >nul || exit /b 1\r\n\
                 cmake -G Ninja -S \"{}\" -B \"{}\" -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
                 -DEMBREE_STATIC_LIB=ON -DEMBREE_TASKING_SYSTEM=INTERNAL -DEMBREE_ISPC_SUPPORT=OFF -DEMBREE_TUTORIALS=OFF \
                 -DEMBREE_FILTER_FUNCTION=OFF -DEMBREE_ISA_SSE2=ON -DEMBREE_ISA_SSE42=OFF -DEMBREE_ISA_AVX=ON \
                 -DEMBREE_ISA_AVX2=ON -DEMBREE_ISA_AVX512=OFF || exit /b 1\r\n\
                 cmake --build \"{}\" -j {jobs} || exit /b 1\r\n",
                src.display(),
                build.display(),
                build.display()
            ),
        )
        .unwrap();
        let status = Command::new("cmd").arg("/c").arg(&bat).status().unwrap_or_else(|e| fail(&format!("cmd: {e}")));
        if !status.success() {
            fail("CMake or Ninja failed (above): both come with Visual Studio's \"C++ CMake tools for Windows\", or put them on PATH");
        }
        // whole or not at all; a build of another profile that got there first is as good
        let _ = std::fs::create_dir_all(lib.parent().unwrap_or(&lib));
        if std::fs::rename(&build, &lib).is_err() && !lib.join("embree3.lib").exists() {
            fail(&format!("could not move {} to {}", build.display(), lib.display()));
        }
    }
    println!("cargo:rustc-link-search=native={}", lib.display());
    for f in std::fs::read_dir(&lib).unwrap() {
        let name = f.unwrap().file_name().into_string().unwrap();
        if let Some(stem) = name.strip_suffix(".lib") {
            if stem != "embree3" {
                println!("cargo:rustc-link-lib=static={stem}");
            }
        }
    }
}
