// SPDX-License-Identifier: BSD-3-Clause

//! Feature `embree` (Task C5h): Embree 3.13.4 built as CSP built it, a static library. The
//! source is `EMBREE_DIR`, else tag v3.13.4 of the clone in `ignored/embree`. Needs CMake,
//! Ninja and MSVC (found with vswhere, set up with vcvars64).

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(c: &mut Command) {
    let status = c.status().unwrap_or_else(|e| panic!("{c:?}: {e}"));
    assert!(status.success(), "{c:?}: {status}");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=EMBREE_DIR");
    if std::env::var_os("CARGO_FEATURE_EMBREE").is_none() {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let lib = out.join("embree-build");
    if !lib.join("embree3.lib").exists() {
        let src = match std::env::var_os("EMBREE_DIR") {
            Some(d) => PathBuf::from(d),
            None => {
                let clone = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ignored/embree");
                let src = out.join("embree-src");
                let tar = out.join("embree.tar");
                std::fs::create_dir_all(&src).unwrap();
                run(Command::new("git").arg("-C").arg(&clone).args(["archive", "-o"]).arg(&tar).arg("v3.13.4"));
                run(Command::new("tar").arg("-xf").arg(&tar).arg("-C").arg(&src));
                src
            }
        };
        let vswhere = r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe";
        let vs = Command::new(vswhere).args(["-latest", "-products", "*", "-property", "installationPath"]).output().expect("vswhere");
        let vs = String::from_utf8(vs.stdout).unwrap().trim().to_string();
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
                lib.display(),
                lib.display()
            ),
        )
        .unwrap();
        run(Command::new("cmd").arg("/c").arg(&bat));
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
