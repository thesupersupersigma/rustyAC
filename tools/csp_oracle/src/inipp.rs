// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a part 2: CSP's own INIpp on an ini file. It builds a `utils::ini_file` the way CSP's
//! car and track config loaders do (dll+0x1588210: mode 0, Lua on, search folders from CSP_ORACLE_INIPP_DIRS), then
//! prints its flattened text (dll+0x149ceb0, the form `[__DEBUG] DUMP_FLATTEN_INI` writes).

/// MSVC std::string: 16 bytes of text or a pointer, then the size and the capacity.
#[repr(C)]
struct MsvcString {
    buf: [u8; 16],
    len: usize,
    cap: usize,
}

/// CSP's flattened `path` (sections sorted, values quoted where needed), or `(not loaded)`.
pub unsafe fn csp_flatten(dll: usize, path: &str) -> String {
    type New = extern "C" fn(usize) -> *mut u8;
    type Ctor = extern "C" fn(*mut u8, i32, *mut MsvcString, u8, *mut [usize; 3], *mut [usize; 7]) -> *mut u8;
    type Flatten = extern "C" fn(*mut MsvcString, *mut u8, *mut u8) -> *mut MsvcString;
    let new: New = std::mem::transmute::<usize, New>(dll + 0x181a674);
    let ctor: Ctor = std::mem::transmute::<usize, Ctor>(dll + 0x1588210);
    let flatten: Flatten = std::mem::transmute::<usize, Flatten>(dll + 0x149ceb0);
    // the ctor takes the path (UTF-8), the folder list and the pre/post lists by move: CSP's heap
    let bytes = path.as_bytes();
    let mut p = MsvcString { buf: [0; 16], len: bytes.len(), cap: 15 };
    if bytes.len() < 16 {
        p.buf[..bytes.len()].copy_from_slice(bytes);
    } else {
        assert!(bytes.len() < 4000, "{path}: path too long"); // CSP frees >= 4 KiB blocks differently
        let mem = new(bytes.len() + 1);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), mem, bytes.len());
        *mem.add(bytes.len()) = 0;
        *(p.buf.as_mut_ptr() as *mut *mut u8) = mem;
        p.cap = bytes.len();
    }
    // CSP_ORACLE_INIPP_DIRS=<dir>;<dir>: include search folders (ini +136), as the loaders pass
    // (C6a-2c): a std::vector of std::string (UTF-8)
    let mut dirs = [0usize; 3];
    if let Ok(list) = std::env::var("CSP_ORACLE_INIPP_DIRS") {
        let list: Vec<&str> = list.split(';').filter(|d| !d.is_empty()).collect();
        let vec = new(32 * list.len().max(1));
        for (i, d) in list.iter().enumerate() {
            let bytes = d.as_bytes();
            let e = vec.add(32 * i);
            std::ptr::write_bytes(e, 0, 32);
            if bytes.len() < 16 {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), e, bytes.len());
                *(e.add(24) as *mut usize) = 15;
            } else {
                let mem = new(bytes.len() + 1);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), mem, bytes.len());
                *mem.add(bytes.len()) = 0;
                *(e as *mut *mut u8) = mem;
                *(e.add(24) as *mut usize) = bytes.len();
            }
            *(e.add(16) as *mut usize) = bytes.len();
        }
        dirs = [vec as usize, vec as usize + 32 * list.len(), vec as usize + 32 * list.len().max(1)];
    }
    let mut lists = [0usize; 7];
    let ini = new(160);
    std::ptr::write_bytes(ini, 0, 160);
    // 4th argument 1 = Lua on ($" "/[FUNCTION]), as the track loader passes; 0 (CSP_ORACLE_INIPP_LUA=0,
    // the C6a-2a gold) leaves expressions empty
    let lua = u8::from(std::env::var("CSP_ORACLE_INIPP_LUA").as_deref() != Ok("0"));
    let ini = ctor(ini, 0, &mut p, lua, &mut dirs, &mut lists);
    if *ini.add(73) != 0 {
        return "(not loaded)".into();
    }
    // options: first word 1 = quote values that need it (as DUMP_FLATTEN_INI); 4 std::function slots empty
    let mut opts = [0u8; 0x140];
    opts[0] = 1;
    let mut out = MsvcString { buf: [0; 16], len: 0, cap: 0 };
    flatten(&mut out, ini, opts.as_mut_ptr());
    let text = if out.cap > 15 { *(out.buf.as_ptr() as *const *const u8) } else { out.buf.as_ptr() };
    // the ini_file and the string stay on CSP's heap: the process ends after the run
    String::from_utf8_lossy(std::slice::from_raw_parts(text, out.len)).into_owned()
}

/// `list`: one ini path a line; prints each file's flattened text after a `== <path>` line.
pub unsafe fn run(dll: usize, list: &str) {
    let text = std::fs::read_to_string(list).unwrap_or_else(|e| panic!("{list}: {e}"));
    for path in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        println!("== {path}");
        println!("{}", csp_flatten(dll, path));
    }
}
