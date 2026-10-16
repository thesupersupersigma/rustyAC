// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! `Array.Sort(array, comparer)` as .NET Framework 4.5+ does it (`SorterObjectArray`'s
//! introsort, which is not stable), with the launcher's `AlphaNumericStringSort` comparer.
//! Equal keys (every name whose number does not parse is 0) end up in introsort's order, so
//! the order matters as much as the comparer.

use std::cmp::Ordering;

/// `AlphaNumericStringSort(prefix, suffix)`: the number left after taking every `prefix` out
/// (the suffix is never used), 0 when it is not a number.
pub fn alphanumeric_key(s: &str, prefix: &str) -> i32 {
    let rest = if prefix.is_empty() { s.to_string() } else { s.replace(prefix, "") };
    // int.TryParse with NumberStyles.Integer
    let t = rest.trim();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    let v: i64 = match digits.trim_start_matches('0') {
        "" => 0,
        d if d.len() > 10 => return 0,
        d => d.parse().unwrap_or(i64::MAX),
    };
    let v = if neg { -v } else { v };
    i32::try_from(v).unwrap_or(0)
}

/// `Array.Sort(keys, comparer)`.
pub fn introsort<T>(keys: &mut [T], cmp: &dyn Fn(&T, &T) -> Ordering) {
    let n = keys.len();
    if n < 2 {
        return;
    }
    let mut floor_log2 = 0;
    let mut m = n;
    while m >= 1 {
        floor_log2 += 1;
        m /= 2;
    }
    intro(keys, cmp, 0, n as isize - 1, 2 * floor_log2);
}

fn swap_if_greater<T>(k: &mut [T], cmp: &dyn Fn(&T, &T) -> Ordering, a: isize, b: isize) {
    if a != b && cmp(&k[a as usize], &k[b as usize]) == Ordering::Greater {
        k.swap(a as usize, b as usize);
    }
}

fn intro<T>(k: &mut [T], cmp: &dyn Fn(&T, &T) -> Ordering, lo: isize, mut hi: isize, mut depth: i32) {
    while hi > lo {
        let size = hi - lo + 1;
        if size <= 16 {
            match size {
                1 => {}
                2 => swap_if_greater(k, cmp, lo, hi),
                3 => {
                    swap_if_greater(k, cmp, lo, hi - 1);
                    swap_if_greater(k, cmp, lo, hi);
                    swap_if_greater(k, cmp, hi - 1, hi);
                }
                _ => insertion(k, cmp, lo, hi),
            }
            return;
        }
        if depth == 0 {
            heapsort(k, cmp, lo, hi);
            return;
        }
        depth -= 1;
        let p = partition(k, cmp, lo, hi);
        intro(k, cmp, p + 1, hi, depth);
        hi = p - 1;
    }
}

fn partition<T>(k: &mut [T], cmp: &dyn Fn(&T, &T) -> Ordering, lo: isize, hi: isize) -> isize {
    let mid = lo + (hi - lo) / 2;
    swap_if_greater(k, cmp, lo, mid);
    swap_if_greater(k, cmp, lo, hi);
    swap_if_greater(k, cmp, mid, hi);
    k.swap(mid as usize, (hi - 1) as usize);
    let pivot = (hi - 1) as usize;
    let mut left = lo;
    let mut right = hi - 1;
    while left < right {
        left += 1;
        while cmp(&k[left as usize], &k[pivot]) == Ordering::Less {
            left += 1;
        }
        right -= 1;
        while cmp(&k[pivot], &k[right as usize]) == Ordering::Less {
            right -= 1;
        }
        if left >= right {
            break;
        }
        k.swap(left as usize, right as usize);
    }
    k.swap(left as usize, (hi - 1) as usize);
    left
}

fn insertion<T>(k: &mut [T], cmp: &dyn Fn(&T, &T) -> Ordering, lo: isize, hi: isize) {
    for i in lo..hi {
        let mut j = i;
        // t = keys[i + 1]; move the greater ones up
        while j >= lo && cmp(&k[(j + 1) as usize], &k[j as usize]) == Ordering::Less {
            k.swap((j + 1) as usize, j as usize);
            j -= 1;
        }
    }
}

fn heapsort<T>(k: &mut [T], cmp: &dyn Fn(&T, &T) -> Ordering, lo: isize, hi: isize) {
    let n = hi - lo + 1;
    let mut i = n / 2;
    while i >= 1 {
        down_heap(k, cmp, i, n, lo);
        i -= 1;
    }
    let mut i = n;
    while i > 1 {
        k.swap(lo as usize, (lo + i - 1) as usize);
        down_heap(k, cmp, 1, i - 1, lo);
        i -= 1;
    }
}

fn down_heap<T>(k: &mut [T], cmp: &dyn Fn(&T, &T) -> Ordering, mut i: isize, n: isize, lo: isize) {
    // d travels down by swaps, which leaves the same array as the launcher's moves
    while i <= n / 2 {
        let mut child = 2 * i;
        if child < n && cmp(&k[(lo + child - 1) as usize], &k[(lo + child) as usize]) == Ordering::Less {
            child += 1;
        }
        if cmp(&k[(lo + i - 1) as usize], &k[(lo + child - 1) as usize]) != Ordering::Less {
            break;
        }
        k.swap((lo + i - 1) as usize, (lo + child - 1) as usize);
        i = child;
    }
}
