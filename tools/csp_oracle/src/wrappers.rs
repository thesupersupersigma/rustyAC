// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5g: the car's calls into CSP's cphys:: wrappers. `CSP_ORACLE_BREAK=<rva>p,...` (every
//! wrapper function) with `CSP_ORACLE_ISLANDS=<file>`: each outermost call into a wrapper is
//! recorded (record 7) and its return site gets a break of its own (kind 'o', record 8); calls a
//! wrapper makes into another wrapper (a deeper or the same stack pointer on the same thread) are
//! not. Every value is a little-endian u64 word:
//! record 7: 7, step, rva, thread, rcx, rdx, r8, r9, xmm0..xmm3 (low 8 bytes), 6 stack arguments
//! (rsp+0x28..), then 8 words at each of rdx, r8, r9 and the first 4 stack arguments (zeros where
//! not readable); record 8: 8, step, rva, thread, rax, xmm0, 8 words at each of rax, the call's
//! rdx and r8.

use std::cell::RefCell;
use std::ffi::c_void;

extern "system" {
    fn VirtualQuery(address: *const c_void, buffer: *mut [usize; 6], length: usize) -> usize;
}

thread_local!(static OPEN: RefCell<Vec<(usize, usize, usize, usize)>> = const { RefCell::new(Vec::new()) });

unsafe fn readable(p: usize) -> bool {
    (0x10000..0x7fff_ffff_0000).contains(&p) && [p, p + 63].iter().all(|&a| {
        let mut info = [0usize; 6];
        if VirtualQuery(a as *const c_void, &mut info, 48) == 0 {
            return false;
        }
        let (state, protect) = (info[4] as u32, (info[4] >> 32) as u32);
        state == 0x1000 && protect & 0xee != 0 && protect & 0x101 == 0
    })
}

unsafe fn words(out: &mut Vec<u64>, p: usize) {
    if readable(p) {
        (0..8).for_each(|i| out.push(std::ptr::read_unaligned((p + 8 * i) as *const u64)));
    } else {
        out.extend([0; 8]);
    }
}

/// A wrapper's entry: record 7 and the return site's break, unless a wrapper call is open below.
pub(crate) unsafe fn enter(context: usize, rva: usize, step: usize, thread: u32) {
    let r = |o: usize| std::ptr::read_unaligned((context + o) as *const usize);
    let rsp = r(0x98);
    let nested = OPEN.with(|open| {
        let mut open = open.borrow_mut();
        while open.last().is_some_and(|o| o.0 < rsp) {
            open.pop();
        }
        !open.is_empty()
    });
    if nested {
        return;
    }
    let [rcx, rdx, r8, r9] = [0x80, 0x88, 0xb8, 0xc0].map(r);
    let stack: Vec<usize> = (0..6).map(|i| std::ptr::read_unaligned((rsp + 0x28 + 8 * i) as *const usize)).collect();
    let mut out = vec![7, step as u64, rva as u64, thread as u64, rcx as u64, rdx as u64, r8 as u64, r9 as u64];
    (0..4).for_each(|i| out.push(r(0x1a0 + 0x10 * i) as u64));
    out.extend(stack.iter().map(|&s| s as u64));
    for p in [rdx, r8, r9].into_iter().chain(stack[..4].iter().copied()) {
        words(&mut out, p);
    }
    crate::island::push_words(&out);
    OPEN.with(|open| open.borrow_mut().push((rsp, rva, rdx, r8)));
    crate::arm_return(std::ptr::read_unaligned(rsp as *const usize), 'o');
}

/// A return site: record 8 when it returns the open outermost call.
pub(crate) unsafe fn leave(context: usize, step: usize, thread: u32) {
    let r = |o: usize| std::ptr::read_unaligned((context + o) as *const usize);
    let rsp = r(0x98);
    let call = OPEN.with(|open| {
        let mut open = open.borrow_mut();
        while open.last().is_some_and(|o| o.0 < rsp - 8) {
            open.pop();
        }
        if open.last().is_some_and(|o| o.0 == rsp - 8) {
            open.pop()
        } else {
            None
        }
    });
    let Some((_, rva, rdx, r8)) = call else { return };
    let rax = r(0x78);
    let mut out = vec![8, step as u64, rva as u64, thread as u64, rax as u64, r(0x1a0) as u64];
    for p in [rax, rdx, r8] {
        words(&mut out, p);
    }
    crate::island::push_words(&out);
}
