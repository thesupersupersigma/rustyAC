// SPDX-License-Identifier: GPL-3.0-or-later

//! Says where a hard fault of the port's side happened (which module, which offset), so that a
//! crash inside FMOD's own threads can be told from one in the port.

use std::ffi::c_void;

#[repr(C)]
struct ExceptionPointers {
    record: *const ExceptionRecord,
    context: *const u8,
}

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *const ExceptionRecord,
    address: usize,
    parameter_count: u32,
    information: [usize; 15],
}

#[link(name = "kernel32")]
#[allow(clashing_extern_declarations)]
extern "system" {
    fn AddVectoredExceptionHandler(first: u32, handler: extern "system" fn(*mut ExceptionPointers) -> i32) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    fn GetCurrentThreadId() -> u32;
}

fn module_of(address: usize) -> String {
    const FROM_ADDRESS: u32 = 4;
    const UNCHANGED_REFCOUNT: u32 = 2;
    let mut module = std::ptr::null_mut();
    // SAFETY: plain Win32 queries about an address.
    unsafe {
        if GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, address as *const u16, &mut module) == 0 {
            return format!("{address:#x} (no module)");
        }
        let mut name = [0u16; 260];
        let n = GetModuleFileNameW(module, name.as_mut_ptr(), 260) as usize;
        let path = String::from_utf16_lossy(&name[..n]);
        let file = path.rsplit(['\\', '/']).next().unwrap_or("").to_string();
        format!("{file}+{:#x}", address - module as usize)
    }
}

extern "system" fn report(pointers: *mut ExceptionPointers) -> i32 {
    // SAFETY: the system hands over valid records.
    unsafe {
        let record = &*(*pointers).record;
        if record.code >> 28 != 0xc {
            return 0;
        }
        if ONLY_FMOD.load(std::sync::atomic::Ordering::Relaxed) && !module_of(record.address).contains("fmod") {
            return 0;
        }
        eprintln!("crash: exception {:#x} at {} on thread {} (main thread {})", record.code, module_of(record.address), GetCurrentThreadId(), MAIN_THREAD.load(std::sync::atomic::Ordering::Relaxed));
        if record.code == 0xc000_0005 {
            eprintln!("  access violation {} address {:#x}", if record.information[0] == 0 { "reading" } else { "writing" }, record.information[1]);
        }
        // the stack: return addresses that lie in a module
        let rsp = *((*pointers).context.add(0x98) as *const usize) as *const usize;
        let mut shown = 0;
        for i in 0..400 {
            let value = *rsp.add(i);
            if value > 0x10000 && value < 0x7fff_ffff_ffff {
                let name = module_of(value);
                if !name.contains("no module") {
                    eprintln!("  stack[{i}]: {name}");
                    shown += 1;
                    if shown == 24 {
                        break;
                    }
                }
            }
        }
        std::process::exit(3);
    }
}

static MAIN_THREAD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// With the game's code in the process: its own crash reports come from the loader's handler.
static ONLY_FMOD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn install_for_fmod_only() {
    ONLY_FMOD.store(true, std::sync::atomic::Ordering::Relaxed);
    install();
}

pub fn install() {
    // SAFETY: registering a handler.
    unsafe {
        MAIN_THREAD.store(GetCurrentThreadId(), std::sync::atomic::Ordering::Relaxed);
        AddVectoredExceptionHandler(1, report);
    }
}
