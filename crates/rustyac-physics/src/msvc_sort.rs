// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Visual Studio 2013's `std::sort` as it is compiled into acs.exe (`std::_Sort` with
//! `_Unguarded_partition`, `_Median`, `_Insertion_sort1`), for any element and comparator.
//! The sort is not stable, so where equal elements end up decides results: it is this
//! algorithm, move for move. (The same code as `track::spline`'s sort of points by
//! distance, which stays where it is.)

fn med3<T: Copy, F: Fn(&T, &T) -> bool>(a: &mut [T], lt: &F, i: usize, j: usize, k: usize) {
    if lt(&a[j], &a[i]) {
        a.swap(j, i);
    }
    if lt(&a[k], &a[j]) {
        a.swap(k, j);
        if lt(&a[j], &a[i]) {
            a.swap(j, i);
        }
    }
}

/// `last` is the last element itself here.
fn median<T: Copy, F: Fn(&T, &T) -> bool>(a: &mut [T], lt: &F, first: usize, mid: usize, last: usize) {
    if 40 < last - first {
        let step = (last - first + 1) / 8;
        med3(a, lt, first, first + step, first + 2 * step);
        med3(a, lt, mid - step, mid, mid + step);
        med3(a, lt, last - 2 * step, last - step, last);
        med3(a, lt, first + step, mid, last - step);
    } else {
        med3(a, lt, first, mid, last);
    }
}

fn partition<T: Copy, F: Fn(&T, &T) -> bool>(a: &mut [T], lt: &F, first: usize, last: usize) -> (usize, usize) {
    let mid = first + (last - first) / 2;
    median(a, lt, first, mid, last - 1);
    let mut pfirst = mid;
    let mut plast = pfirst + 1;
    while first < pfirst && !lt(&a[pfirst - 1], &a[pfirst]) && !lt(&a[pfirst], &a[pfirst - 1]) {
        pfirst -= 1;
    }
    while plast < last && !lt(&a[plast], &a[pfirst]) && !lt(&a[pfirst], &a[plast]) {
        plast += 1;
    }
    let mut gfirst = plast;
    let mut glast = pfirst;
    loop {
        while gfirst < last {
            if lt(&a[pfirst], &a[gfirst]) {
            } else if lt(&a[gfirst], &a[pfirst]) {
                break;
            } else {
                if plast != gfirst {
                    a.swap(plast, gfirst);
                }
                plast += 1;
            }
            gfirst += 1;
        }
        while first < glast {
            if lt(&a[glast - 1], &a[pfirst]) {
            } else if lt(&a[pfirst], &a[glast - 1]) {
                break;
            } else {
                pfirst -= 1;
                if pfirst != glast - 1 {
                    a.swap(pfirst, glast - 1);
                }
            }
            glast -= 1;
        }
        if glast == first && gfirst == last {
            return (pfirst, plast);
        }
        if glast == first {
            if plast != gfirst {
                a.swap(pfirst, plast);
            }
            plast += 1;
            a.swap(pfirst, gfirst);
            pfirst += 1;
            gfirst += 1;
        } else if gfirst == last {
            glast -= 1;
            pfirst -= 1;
            if glast != pfirst {
                a.swap(glast, pfirst);
            }
            plast -= 1;
            a.swap(pfirst, plast);
        } else {
            glast -= 1;
            a.swap(gfirst, glast);
            gfirst += 1;
        }
    }
}

fn insertion<T: Copy, F: Fn(&T, &T) -> bool>(a: &mut [T], lt: &F, first: usize, last: usize) {
    for next in first + 1..last {
        let val = a[next];
        if lt(&val, &a[first]) {
            a.copy_within(first..next, first + 1);
            a[first] = val;
        } else {
            let mut j = next;
            while lt(&val, &a[j - 1]) {
                a[j] = a[j - 1];
                j -= 1;
            }
            a[j] = val;
        }
    }
}

// The heap sort the algorithm falls back on when its quick sort degenerates. Not read in
// the binary (no installed line reaches it): the same compiler's library header.
fn push_heap<T: Copy, F: Fn(&T, &T) -> bool>(a: &mut [T], lt: &F, first: usize, mut hole: usize, top: usize, val: T) {
    while top < hole {
        let idx = (hole - 1) / 2;
        if !lt(&a[first + idx], &val) {
            break;
        }
        a[first + hole] = a[first + idx];
        hole = idx;
    }
    a[first + hole] = val;
}

fn adjust_heap<T: Copy, F: Fn(&T, &T) -> bool>(a: &mut [T], lt: &F, first: usize, mut hole: usize, bottom: usize, val: T) {
    let top = hole;
    let mut idx = 2 * hole + 2;
    while idx < bottom {
        if lt(&a[first + idx], &a[first + idx - 1]) {
            idx -= 1;
        }
        a[first + hole] = a[first + idx];
        hole = idx;
        idx = 2 * idx + 2;
    }
    if idx == bottom {
        a[first + hole] = a[first + bottom - 1];
        hole = bottom - 1;
    }
    push_heap(a, lt, first, hole, top, val);
}

fn heap_sort<T: Copy, F: Fn(&T, &T) -> bool>(a: &mut [T], lt: &F, first: usize, mut last: usize) {
    let count = last - first;
    let mut hole = count / 2;
    while 0 < hole {
        hole -= 1;
        let val = a[first + hole];
        adjust_heap(a, lt, first, hole, count, val);
    }
    while 1 < last - first {
        let val = a[last - 1];
        a[last - 1] = a[first];
        adjust_heap(a, lt, first, 0, last - 1 - first, val);
        last -= 1;
    }
}

fn sort_range<T: Copy, F: Fn(&T, &T) -> bool>(a: &mut [T], lt: &F, mut first: usize, mut last: usize, mut ideal: isize) {
    let mut count;
    loop {
        count = last - first;
        if !(32 < count && 0 < ideal) {
            break;
        }
        let (pf, pl) = partition(a, lt, first, last);
        ideal /= 2;
        ideal += ideal / 2;
        if pf - first < last - pl {
            sort_range(a, lt, first, pf, ideal);
            first = pl;
        } else {
            sort_range(a, lt, pl, last, ideal);
            last = pf;
        }
    }
    if 32 < count {
        heap_sort(a, lt, first, last);
    } else if 1 < count {
        insertion(a, lt, first, last);
    }
}

pub fn sort<T: Copy, F: Fn(&T, &T) -> bool>(a: &mut [T], lt: &F) {
    let n = a.len();
    sort_range(a, lt, 0, n, n as isize);
}
