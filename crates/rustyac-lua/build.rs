// SPDX-License-Identifier: MIT OR Apache-2.0

//! Feature `luajit` (Task C6a): LuaJIT 2.1 built as CSP built it, a static library (/MT, GC64,
//! JIT, FFI, LUAJIT_ENABLE_LUA52COMPAT). The source is commit `COMMIT` of the clone in
//! `LUAJIT_DIR`, else `ignored/luajit`. Needs MSVC (vswhere, vcvars64). The library goes to
//! `RUSTYAC_LUAJIT_LIB` (a folder several target folders share, CI), else `<target>/luajit-<commit>-mt-<TAG>`.
//! `PATCHES` (Task C6b) make it behave as CSP's fork where scripts can see it: `ffi.C` asks the
//! host's `lj_clib_resolver` (CSP resolves its `lj_*` functions from its own table, no Windows
//! symbols), tostring's hidden addresses, and CSP's extra `table.*` functions.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The last upstream v2.1 commit before CSP's fork commit (unix time 1791809196, the version
/// "2.1.1791809196" in build 4305): docs/csp/lua_host.md.
const COMMIT: &str = "c6ffc141a8762b41703f9287d63d93622a13dd8f";
/// Names the library folder; a new patch needs a new tag, or an old folder's library is linked.
const TAG: &str = "csp2";

/// (file, upstream text, replacement), each upstream text found exactly once.
const PATCHES: &[(&str, &str, &str)] = &[
    (
        "src/lj_clib.c",
        "static void *clib_getsym(CLibrary *cl, const char *name)
{
  void *p = NULL;
  if (cl->handle == CLIB_DEFHANDLE) {",
        "void *(*lj_clib_resolver)(const char *name) = NULL;

static void *clib_getsym(CLibrary *cl, const char *name)
{
  void *p = NULL;
  if (cl->handle == CLIB_DEFHANDLE && lj_clib_resolver) return lj_clib_resolver(name);
  if (cl->handle == CLIB_DEFHANDLE) {",
    ),
    (
        "src/lj_clib.c",
        "      if (!p)
	clib_error(L, \"cannot resolve symbol \" LUA_QS \": %s\", sym);",
        "      if (!p && cl->handle == CLIB_DEFHANDLE && lj_clib_resolver)
	lj_err_callermsg(L, lj_strfmt_pushf(L, \"cannot resolve symbol \" LUA_QS, sym));
      if (!p)
	clib_error(L, \"cannot resolve symbol \" LUA_QS \": %s\", sym);",
    ),
    // tostring hides addresses and builtin ids: "table: 0xff", "function: builtin#0", NULL stays.
    (
        "src/lj_strfmt.c",
        "    *p++ = 'N'; *p++ = 'U'; *p++ = 'L'; *p++ = 'L';
    return p;
  }",
        "    *p++ = 'N'; *p++ = 'U'; *p++ = 'L'; *p++ = 'L';
    return p;
  }
  *p++ = '0'; *p++ = 'x'; *p++ = 'f'; *p++ = 'f';
  return p;",
    ),
    ("src/lj_strfmt.c", "p = lj_strfmt_wint(p, funcV(o)->c.ffid);", "p = lj_strfmt_wint(p, 0);"),
    // CSP's message leaves the name out ("missing declaration for", probe of ffi.C.GetCurrentProcessId)
    ("src/lj_errmsg.h", "\"missing declaration for symbol \" LUA_QS", "\"missing declaration for\""),
    // CSP's table.clone/isarray/nkeys/isempty/issequence (dll 0x181770460..0x1817AD560), preloaded
    // in CSP's order; no JIT recorders, so traces stitch around them.
    (
        "src/lib_table.c",
        "static int luaopen_table_new(lua_State *L)",
        "#include <math.h>

LJLIB_NOREG LJLIB_CF(table_clone)
{
  GCtab *kt = lj_lib_checktab(L, 1);
  GCtab *t = lj_tab_dup(L, kt);
  uint32_t i;
  /* CSP's dup takes a flag that keeps table values (not a template's nil marker) */
  for (i = 0; kt->hmask && i <= kt->hmask; i++)
    noderef(t->node)[i].val = noderef(kt->node)[i].val;
  settabV(L, L->base, t);
  L->top = L->base+1;
  return 1;
}

LJLIB_NOREG LJLIB_CF(table_isarray)
{
  GCtab *t = lj_lib_checktab(L, 1);
  Node *node = noderef(t->node);
  int r = 1;
  uint32_t i;
  for (i = 0; i <= t->hmask; i++)
    if (!tvisnil(&node[i].val) &&
\t(!tvisnum(&node[i].key) || floor(numV(&node[i].key)) != numV(&node[i].key))) r = 0;
  setboolV(L->base, r);
  L->top = L->base+1;
  return 1;
}

LJLIB_NOREG LJLIB_CF(table_nkeys)
{
  GCtab *t = lj_lib_checktab(L, 1);
  TValue *a = tvref(t->array);
  Node *node = noderef(t->node);
  int32_t n = 0;
  uint32_t i;
  for (i = 0; i < t->asize; i++) n += !tvisnil(&a[i]);
  if (t->hmask) for (i = 0; i <= t->hmask; i++) n += !tvisnil(&node[i].val);
  setnumV(L->base, (lua_Number)n);
  L->top = L->base+1;
  return 1;
}

LJLIB_NOREG LJLIB_CF(table_isempty)
{
  GCtab *t = lj_lib_checktab(L, 1);
  TValue *a = tvref(t->array);
  Node *node = noderef(t->node);
  int r = 1;
  uint32_t i;
  for (i = 0; i < t->asize; i++) if (!tvisnil(&a[i])) r = 0;
  if (t->hmask) for (i = 0; i <= t->hmask; i++) if (!tvisnil(&node[i].val)) r = 0;
  setboolV(L->base, r);
  L->top = L->base+1;
  return 1;
}

/* 1..n in the array part with nothing after the first hole, then integer keys n+1.. in the hash part. */
static int tab_issequence(GCtab *t)
{
  TValue *a = tvref(t->array);
  Node *node = noderef(t->node);
  uint32_t asize = t->asize, n = 1, i;
  if (asize && !tvisnil(&a[0])) return 0;
  while (n < asize && !tvisnil(&a[n])) n++;
  for (i = n+1; i < asize; i++) if (!tvisnil(&a[i])) return 0;
  if (t->hmask) {
    int32_t cnt = asize ? (int32_t)n-1 : 0;
    double mx = (double)cnt, lim = (double)(int32_t)t->hmask + (double)cnt + 1.0;
    for (i = 0; i <= t->hmask; i++) {
      if (tvisnil(&node[i].val)) continue;
      if (n < asize || !tvisnum(&node[i].key)) return 0;
      {
\tdouble k = numV(&node[i].key);
\tif (floor(k) != k || k < 1.0 || k > lim) return 0;
\tcnt++;
\tmx = fmax(k, mx);
      }
    }
    if ((double)cnt != mx) return 0;
  }
  return 1;
}

LJLIB_NOREG LJLIB_CF(table_issequence)
{
  setboolV(L->base, tab_issequence(lj_lib_checktab(L, 1)));
  L->top = L->base+1;
  return 1;
}

static int luaopen_table_clone(lua_State *L)
{
  return lj_lib_postreg(L, lj_cf_table_clone, FF_table_clone, \"clone\");
}

static int luaopen_table_isarray(lua_State *L)
{
  return lj_lib_postreg(L, lj_cf_table_isarray, FF_table_isarray, \"isarray\");
}

static int luaopen_table_nkeys(lua_State *L)
{
  return lj_lib_postreg(L, lj_cf_table_nkeys, FF_table_nkeys, \"nkeys\");
}

static int luaopen_table_isempty(lua_State *L)
{
  return lj_lib_postreg(L, lj_cf_table_isempty, FF_table_isempty, \"isempty\");
}

static int luaopen_table_issequence(lua_State *L)
{
  return lj_lib_postreg(L, lj_cf_table_issequence, FF_table_issequence, \"issequence\");
}

static int luaopen_table_new(lua_State *L)",
    ),
    (
        "src/lib_table.c",
        "  lj_lib_prereg(L, LUA_TABLIBNAME \".new\", luaopen_table_new, tabV(L->top-1));
",
        "  lj_lib_prereg(L, LUA_TABLIBNAME \".new\", luaopen_table_new, tabV(L->top-1));
  lj_lib_prereg(L, LUA_TABLIBNAME \".clone\", luaopen_table_clone, tabV(L->top-1));
  lj_lib_prereg(L, LUA_TABLIBNAME \".isarray\", luaopen_table_isarray, tabV(L->top-1));
  lj_lib_prereg(L, LUA_TABLIBNAME \".nkeys\", luaopen_table_nkeys, tabV(L->top-1));
  lj_lib_prereg(L, LUA_TABLIBNAME \".isempty\", luaopen_table_isempty, tabV(L->top-1));
  lj_lib_prereg(L, LUA_TABLIBNAME \".issequence\", luaopen_table_issequence, tabV(L->top-1));
",
    ),
];

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
    let lib = std::env::var_os("RUSTYAC_LUAJIT_LIB").map(PathBuf::from).unwrap_or_else(|| out.ancestors().nth(4).unwrap_or(&out).join(format!("luajit-{}-mt-{TAG}", &COMMIT[..8])));
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
        for (file, from, to) in PATCHES {
            let path = src.join(file);
            let text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n"); // git archive may give CRLF
            assert_eq!(text.matches(from).count(), 1, "patch of {file} does not apply: {from}");
            std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
        }
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
