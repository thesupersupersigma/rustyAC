// SPDX-License-Identifier: MIT OR Apache-2.0
// Ported from MSVCR120.dll 12.0.21005.1 (the Microsoft Visual C++ 2013 runtime Assetto Corsa
// imports `qsort` from).

//! `qsort` (0x180077250) and its `shortsort` (0x180077550): a quicksort with the median of three,
//! a selection sort below nine elements and an explicit stack. It is not stable, so elements the
//! comparator calls equal come out in the DLL's order only if the same steps run: this copies
//! them, comparison for comparison and swap for swap.

/// Sorts `v` as the DLL's `qsort(v, len, size, comp)` does; `comp` returns the C comparator's
/// `int` (negative, zero, positive).
pub fn qsort<T>(v: &mut [T], mut comp: impl FnMut(&T, &T) -> i32) {
    if v.len() < 2 {
        return;
    }
    // (indices, signed: `higuy` and `loguy` step one past their bounds)
    let mut stack: Vec<(isize, isize)> = Vec::new();
    let mut lo: isize = 0;
    let mut hi = v.len() as isize - 1;
    let mut c = |v: &[T], a: isize, b: isize| comp(&v[a as usize], &v[b as usize]);
    loop {
        let size = hi - lo + 1;
        if size <= 8 {
            shortsort(v, lo, hi, &mut c);
        } else {
            let mut mid = lo + (size / 2);
            if c(v, lo, mid) > 0 {
                v.swap(lo as usize, mid as usize);
            }
            if c(v, lo, hi) > 0 {
                v.swap(lo as usize, hi as usize);
            }
            if c(v, mid, hi) > 0 {
                v.swap(mid as usize, hi as usize);
            }
            let mut loguy = lo;
            let mut higuy = hi;
            loop {
                if mid > loguy {
                    loop {
                        loguy += 1;
                        if !(loguy < mid && c(v, loguy, mid) <= 0) {
                            break;
                        }
                    }
                }
                if mid <= loguy {
                    loop {
                        loguy += 1;
                        if !(loguy <= hi && c(v, loguy, mid) <= 0) {
                            break;
                        }
                    }
                }
                loop {
                    higuy -= 1;
                    if !(higuy > mid && c(v, higuy, mid) > 0) {
                        break;
                    }
                }
                if higuy < loguy {
                    break;
                }
                v.swap(loguy as usize, higuy as usize);
                if mid == higuy {
                    mid = loguy;
                }
            }
            higuy += 1;
            if mid < higuy {
                loop {
                    higuy -= 1;
                    if !(higuy > mid && c(v, higuy, mid) == 0) {
                        break;
                    }
                }
            }
            if mid >= higuy {
                loop {
                    higuy -= 1;
                    if !(higuy > lo && c(v, higuy, mid) == 0) {
                        break;
                    }
                }
            }
            // the bigger half goes on the stack, the smaller one is sorted next
            if higuy - lo >= hi - loguy {
                if lo < higuy {
                    stack.push((lo, higuy));
                }
                if loguy < hi {
                    lo = loguy;
                    continue;
                }
            } else {
                if loguy < hi {
                    stack.push((loguy, hi));
                }
                if lo < higuy {
                    hi = higuy;
                    continue;
                }
            }
        }
        match stack.pop() {
            Some((l, h)) => (lo, hi) = (l, h),
            None => return,
        }
    }
}

/// The selection sort: the largest of `lo..=hi` (the last one found greater) swapped to the end.
fn shortsort<T>(v: &mut [T], lo: isize, mut hi: isize, c: &mut impl FnMut(&[T], isize, isize) -> i32) {
    while hi > lo {
        let mut max = lo;
        for p in lo + 1..=hi {
            if c(v, p, max) > 0 {
                max = p;
            }
        }
        v.swap(max as usize, hi as usize);
        hi -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::qsort;

    fn by_key(a: &(i32, i32), b: &(i32, i32)) -> i32 {
        a.0.cmp(&b.0) as i32
    }

    /// Ties: the order the DLL gives (`same_as_the_dll` checks it against the DLL), on any target.
    #[test]
    fn ties_in_the_dlls_order() {
        let keys = [3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 0, 3, 0, 1, 2];
        let mut v: Vec<(i32, i32)> = keys.iter().enumerate().map(|(i, &k)| (k, i as i32)).collect();
        qsort(&mut v, by_key);
        let order: Vec<i32> = v.iter().map(|e| e.1).collect();
        assert_eq!(order, TIES_ORDER);
    }

    const TIES_ORDER: [i32; 17] = [14, 12, 15, 4, 10, 7, 1, 16, 5, 2, 8, 11, 13, 6, 3, 0, 9];

    /// Many random arrays full of ties against the real `MSVCR120.dll`'s `qsort`.
    #[cfg(windows)]
    #[test]
    fn same_as_the_dll() {
        use std::ffi::c_void;
        #[link(name = "kernel32")]
        extern "system" {
            fn LoadLibraryA(name: *const u8) -> *mut c_void;
            fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        }
        type Comp = unsafe extern "C" fn(*const c_void, *const c_void) -> i32;
        type Qsort = unsafe extern "C" fn(*mut c_void, usize, usize, Comp);
        unsafe extern "C" fn comp(a: *const c_void, b: *const c_void) -> i32 {
            // SAFETY: the DLL hands back pointers into the `(i32, i32)` array it was given
            unsafe { super::tests::by_key(&*a.cast(), &*b.cast()) }
        }
        // SAFETY: plain Win32 calls; `qsort` is the documented C function
        let dll: Qsort = unsafe {
            let module = LoadLibraryA(c"msvcr120.dll".as_ptr().cast());
            if module.is_null() {
                eprintln!("msvcr120.dll not installed, skipping");
                return;
            }
            std::mem::transmute(GetProcAddress(module, c"qsort".as_ptr().cast()))
        };
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let keys = [3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 0, 3, 0, 1, 2];
        let mut first: Vec<(i32, i32)> = keys.iter().enumerate().map(|(i, &k)| (k, i as i32)).collect();
        // SAFETY: a live, correctly sized array of `(i32, i32)` and a comparator for it
        unsafe { dll(first.as_mut_ptr().cast(), first.len(), 8, comp) };
        assert_eq!(first.iter().map(|e| e.1).collect::<Vec<_>>(), super::tests::TIES_ORDER);
        for round in 0..20_000 {
            let len = (next() % 300) as usize;
            let spread = 1 + (next() % if round % 2 == 0 { 4 } else { 1000 }) as i32;
            let base: Vec<(i32, i32)> = (0..len).map(|i| ((next() % spread as u64) as i32, i as i32)).collect();
            let mut want = base.clone();
            // SAFETY: as above
            unsafe { dll(want.as_mut_ptr().cast(), want.len(), 8, comp) };
            let mut got = base;
            qsort(&mut got, by_key);
            assert_eq!(got, want, "round {round}, {len} elements, keys below {spread}");
        }
    }
}
