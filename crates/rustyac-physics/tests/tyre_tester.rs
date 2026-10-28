// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The Tyre tester's sweeps on a car of the game's folder (packed `data.acd`).

use std::path::PathBuf;

use rustyac_physics::tyre::tester::TyreTester;

#[test]
fn the_tyre_tester_sweeps_a_packed_car_s_tyre() {
    let root = rustyac_content::install::ac_root().unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"));
    let data = root.join("content/cars/ks_ferrari_f2004/data");
    if !root.join("content/cars/ks_ferrari_f2004").is_dir() {
        eprintln!("NOT TESTED: Assetto Corsa's folder is missing (set AC_ROOT): the tester reads the F2004's tyres");
        return;
    }
    let mut t = TyreTester::new(&data, 0).expect("the F2004's front left tyre");
    eprintln!("compounds: {}", t.tyre.compound_defs.len());
    t.velocity = 40.0;
    let sa = t.slip_angle_sweep(-20.0, 20.0, 3000.0, 200);
    let sr = t.slip_ratio_sweep(-1.0, 1.0, 3000.0, 200);
    let both = t.sr_sa_sweep(9.0, -1.0, 1.0, 3000.0, 200);
    assert_eq!((sa.len(), sr.len(), both.len()), (200, 200, 200));
    // grip both ways and a peak lateral force near the load times the tyre's grip
    let peak = sa.iter().map(|f| f.fy).fold(0.0f32, f32::max) / 3000.0;
    assert!(sa.iter().all(|f| f.fy.is_finite()) && peak > 0.8 && peak < 3.0, "peak {peak}");
    assert!(sa[0].fy * sa[199].fy < 0.0);
    assert!(sr.iter().map(|f| f.fx).fold(0.0f32, f32::max) > 0.0);
}
