// SPDX-License-Identifier: GPL-3.0-or-later

//! The player's own `python33.dll`, loaded at run time from `<AC>\system\x64` (acs.exe sets
//! that folder with `SetDllDirectoryW` and delay-loads the DLL from it). Only the 28 functions
//! acs.exe imports are used, with CPython 3.3's x64 signatures; the reference counting the
//! game does through CPython's macros is written out in [`incref`] / [`decref`].

use std::ffi::{c_char, c_int, c_long, c_void, CString, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A `PyObject*`.
pub type Obj = *mut c_void;
/// A `PyCFunction` (`METH_VARARGS`).
pub type CFunction = unsafe extern "C" fn(Obj, Obj) -> Obj;

/// `PyMethodDef`.
#[repr(C)]
pub struct MethodDef {
    pub name: *const c_char,
    pub meth: Option<CFunction>,
    pub flags: c_int,
    pub doc: *const c_char,
}

/// `PyModuleDef` (with `PyModuleDef_Base` written out).
#[repr(C)]
pub struct ModuleDef {
    pub refcnt: isize,
    pub ob_type: *mut c_void,
    pub m_init: *mut c_void,
    pub m_index: isize,
    pub m_copy: *mut c_void,
    pub name: *const c_char,
    pub doc: *const c_char,
    pub size: isize,
    pub methods: *mut MethodDef,
    pub reload: *mut c_void,
    pub traverse: *mut c_void,
    pub clear: *mut c_void,
    pub free: *mut c_void,
}

macro_rules! api {
    ($($field:ident = $name:literal : $ty:ty;)*) => {
        /// The functions acs.exe imports from `python33.dll`, by their exports' names.
        pub struct Api {
            $(pub $field: $ty,)*
            /// where the DLL was loaded from
            pub path: PathBuf,
        }

        impl Api {
            unsafe fn load(module: *mut c_void, path: PathBuf) -> Result<Api, String> {
                Ok(Api {
                    $($field: {
                        let p = GetProcAddress(module, concat!($name, "\0").as_ptr());
                        if p.is_null() {
                            return Err(format!("{} has no export {}", path.display(), $name));
                        }
                        std::mem::transmute::<*mut c_void, $ty>(p)
                    },)*
                    path,
                })
            }
        }
    };
}

api! {
    unicode_as_utf8_string = "PyUnicode_AsUTF8String": unsafe extern "C" fn(Obj) -> Obj;
    err_fetch = "PyErr_Fetch": unsafe extern "C" fn(*mut Obj, *mut Obj, *mut Obj);
    err_print = "PyErr_Print": unsafe extern "C" fn();
    object_call_function = "PyObject_CallFunction": unsafe extern "C" fn(Obj, *const c_char, ...) -> Obj;
    unicode_from_unicode = "PyUnicode_FromUnicode": unsafe extern "C" fn(*const u16, isize) -> Obj;
    unicode_as_unicode_and_size = "PyUnicode_AsUnicodeAndSize": unsafe extern "C" fn(Obj, *mut isize) -> *mut u16;
    long_from_long = "PyLong_FromLong": unsafe extern "C" fn(c_long) -> Obj;
    bool_from_long = "PyBool_FromLong": unsafe extern "C" fn(c_long) -> Obj;
    float_from_double = "PyFloat_FromDouble": unsafe extern "C" fn(f64) -> Obj;
    tuple_new = "PyTuple_New": unsafe extern "C" fn(isize) -> Obj;
    unicode_as_wide_char_string = "PyUnicode_AsWideCharString": unsafe extern "C" fn(Obj, *mut isize) -> *mut u16;
    list_new = "PyList_New": unsafe extern "C" fn(isize) -> Obj;
    arg_parse_tuple = "PyArg_ParseTuple": unsafe extern "C" fn(Obj, *const c_char, ...) -> c_int;
    run_simple_string_flags = "PyRun_SimpleStringFlags": unsafe extern "C" fn(*const c_char, *mut c_void) -> c_int;
    module_create2 = "PyModule_Create2": unsafe extern "C" fn(*mut ModuleDef, c_int) -> Obj;
    initialize = "Py_Initialize": unsafe extern "C" fn();
    finalize = "Py_Finalize": unsafe extern "C" fn();
    is_initialized = "Py_IsInitialized": unsafe extern "C" fn() -> c_int;
    import_append_inittab = "PyImport_AppendInittab": unsafe extern "C" fn(*const c_char, Option<unsafe extern "C" fn() -> Obj>) -> c_int;
    err_occurred = "PyErr_Occurred": unsafe extern "C" fn() -> Obj;
    err_clear = "PyErr_Clear": unsafe extern "C" fn();
    import_import = "PyImport_Import": unsafe extern "C" fn(Obj) -> Obj;
    unicode_from_wide_char = "PyUnicode_FromWideChar": unsafe extern "C" fn(*const u16, isize) -> Obj;
    bytes_as_string = "PyBytes_AsString": unsafe extern "C" fn(Obj) -> *mut c_char;
    mem_free = "PyMem_Free": unsafe extern "C" fn(*mut c_void);
    object_get_attr = "PyObject_GetAttr": unsafe extern "C" fn(Obj, Obj) -> Obj;
    tuple_set_item = "PyTuple_SetItem": unsafe extern "C" fn(Obj, isize, Obj) -> c_int;
    object_get_attr_string = "PyObject_GetAttrString": unsafe extern "C" fn(Obj, *const c_char) -> Obj;
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
}

const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;

static API: OnceLock<Result<Api, String>> = OnceLock::new();

/// Loads `<ac_root>\system\x64\python33.dll` (once per process; a later call returns the first
/// result). CPython finds its standard library from the DLL's place: `Python33.zip` beside it.
pub fn load(ac_root: &Path) -> Result<&'static Api, String> {
    API.get_or_init(|| {
        let path = ac_root.join("system").join("x64").join("python33.dll");
        if !path.is_file() {
            return Err(format!("{} not found", path.display()));
        }
        let wide: Vec<u16> = OsStr::new(&path).encode_wide().chain(Some(0)).collect();
        // SAFETY: a terminated path; the DLL's own start-up runs, as it does in the game.
        let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
        if module.is_null() {
            return Err(format!("{} could not be loaded (error {})", path.display(), unsafe { GetLastError() }));
        }
        let _ = MODULE.set(module as usize);
        // SAFETY: the types are CPython 3.3's own for these exports.
        unsafe { Api::load(module, path) }
    })
    .as_ref()
    .map_err(Clone::clone)
}

static MODULE: OnceLock<usize> = OnceLock::new();

/// The address of an export of the loaded DLL (0: none; the oracle fills the game's imports).
pub fn address(name: &std::ffi::CStr) -> usize {
    // SAFETY: a loaded module and a terminated name.
    MODULE.get().map_or(0, |m| unsafe { GetProcAddress(*m as *mut c_void, name.as_ptr().cast()) as usize })
}

/// The loaded API (after a successful [`load`]).
pub fn api() -> &'static Api {
    API.get().and_then(|r| r.as_ref().ok()).expect("python33.dll not loaded")
}

/// `Py_DECREF`: at zero, the type's `tp_dealloc` (`PyTypeObject+0x30`).
pub(crate) unsafe fn decref(o: Obj) {
    let count = o as *mut isize;
    *count -= 1;
    if *count == 0 {
        let ty = *(o as *const *const u8).add(1);
        let dealloc: unsafe extern "C" fn(Obj) = std::mem::transmute(*(ty.add(0x30) as *const usize));
        dealloc(o);
    }
}

/// `Py_XDECREF`.
pub(crate) unsafe fn xdecref(o: Obj) {
    if !o.is_null() {
        decref(o);
    }
}

/// A C string of `text` (no interior zero expected; cut there if one comes).
pub fn cstr(text: &str) -> CString {
    CString::new(text.split('\0').next().unwrap_or("")).unwrap()
}
