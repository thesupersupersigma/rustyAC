// SPDX-License-Identifier: MIT OR Apache-2.0

//! `rustyac_math::pure` against `MSVCR120.dll` on a sample, and against a recorded hash.
//!
//! The full proof (every one of the 2^32 inputs per function) is `tools/math_proof`; this is
//! the part of it small enough for every `cargo test`. The recorded hashes were taken on a
//! build whose sample matched the DLL, and the hash test needs no DLL: it is what a
//! `wasm32-wasip1` build runs to show it computes the same bits as the desktop.

use rustyac_math::pure;

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn float(&mut self) -> f32 {
        f32::from_bits(self.next() as u32)
    }

    /// Floats of ordinary size, either sign.
    fn tame(&mut self) -> f32 {
        let r = self.next();
        let magnitude = (r >> 40) as f32 / (1u64 << 24) as f32;
        let scale = [0.01f32, 1.0, 4.0, 300.0][(r & 3) as usize];
        if r & 4 == 0 {
            magnitude * scale
        } else {
            -magnitude * scale
        }
    }
}

type F1 = fn(f32) -> f32;
type F2 = fn(f32, f32) -> f32;

const ONE: [(&str, F1, F1); 7] = [
    ("sinf", pure::sinf, rustyac_math::sinf),
    ("cosf", pure::cosf, rustyac_math::cosf),
    ("tanf", pure::tanf, rustyac_math::tanf),
    ("expf", pure::expf, rustyac_math::expf),
    ("asinf", pure::asinf, rustyac_math::asinf),
    ("acosf", pure::acosf, rustyac_math::acosf),
    ("atanf", pure::atanf, rustyac_math::atanf),
];

const TWO: [(&str, F2, F2); 2] = [
    ("atan2f", pure::atan2f, rustyac_math::atan2f),
    ("powf", pure::powf, rustyac_math::powf),
];

/// Every 65537th float.
fn floats() -> impl Iterator<Item = f32> {
    (0..(1u64 << 32))
        .step_by(65537)
        .map(|b| f32::from_bits(b as u32))
}

fn pairs() -> impl Iterator<Item = (f32, f32)> {
    let mut rng = Rng(20);
    (0..150_000).map(move |i| match i % 3 {
        0 => (rng.float(), rng.float()),
        1 => (rng.tame(), rng.tame()),
        _ => (rng.tame().abs(), (rng.next() % 9) as f32 - 4.0),
    })
}

fn doubles() -> impl Iterator<Item = f64> {
    let mut rng = Rng(21);
    (0..150_000).map(move |i| match i % 3 {
        0 => f64::from_bits(rng.next()),
        1 => rng.tame() as f64 * 1000.0,
        _ => rng.float() as f64,
    })
}

fn fnv(hash: &mut u64, bits: u64) {
    for byte in bits.to_le_bytes() {
        *hash ^= byte as u64;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// With the real runtime loaded, the pure functions give its bits on the whole sample.
#[test]
fn sample_matches_msvcr120() {
    if rustyac_math::backend() != rustyac_math::Backend::Msvcr120 {
        println!(
            "NOT TESTED: msvcr120.dll is not in use ({:?})",
            rustyac_math::backend()
        );
        return;
    }
    for (name, pure, dll) in ONE {
        for x in floats() {
            assert_eq!(
                pure(x).to_bits(),
                dll(x).to_bits(),
                "{name}({:08x})",
                x.to_bits()
            );
        }
    }
    for (name, pure, dll) in TWO {
        for (a, b) in pairs() {
            assert_eq!(
                pure(a, b).to_bits(),
                dll(a, b).to_bits(),
                "{name}({:08x}, {:08x})",
                a.to_bits(),
                b.to_bits()
            );
        }
    }
    for x in doubles() {
        assert_eq!(
            pure::sin(x).to_bits(),
            rustyac_math::sin(x).to_bits(),
            "sin({:016x})",
            x.to_bits()
        );
    }
}

/// The same sample as one hash per function: equal on every target, or that target's
/// compiler changed the arithmetic.
#[test]
fn sample_hash_is_the_recorded_one() {
    const RECORDED: [(&str, u64); 10] = [
        ("sinf", 0xcc67_7367_6e72_e1df),
        ("cosf", 0x9524_c1c5_fb65_e38b),
        ("tanf", 0x0394_8066_e80b_286e),
        ("expf", 0x8c38_3d0f_d59b_05df),
        ("asinf", 0x0707_57f8_113b_0348),
        ("acosf", 0x5a19_20a7_0d36_b2ca),
        ("atanf", 0x0475_68ff_f4b2_5271),
        ("atan2f", 0x820e_facc_4341_9819),
        ("powf", 0xd6a8_6ef4_2bf0_2ac3),
        ("sin", 0x360f_1bc7_39ce_f946),
    ];
    let mut found = Vec::new();
    for (name, pure, _) in ONE {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for x in floats() {
            fnv(&mut hash, pure(x).to_bits() as u64);
        }
        found.push((name, hash));
    }
    for (name, pure, _) in TWO {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for (a, b) in pairs() {
            fnv(&mut hash, pure(a, b).to_bits() as u64);
        }
        found.push((name, hash));
    }
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for x in doubles() {
        fnv(&mut hash, pure::sin(x).to_bits());
    }
    found.push(("sin", hash));
    for (name, hash) in &found {
        println!("        (\"{name}\", 0x{hash:016x}),");
    }
    assert_eq!(found, RECORDED);
}
