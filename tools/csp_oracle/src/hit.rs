// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5i-c: break kind `f` on CSP's car collision callback (+0x587cc0). At its entry the
//! oracle reads the callback's inputs and the car fields it writes; the call's return site and
//! the car's four `Suspension::setDamage` (vtable +0x78) get breaks of kind `f` too. At the return
//! the port's `csp_hit` runs on the same inputs and fields, and its fields, engine blow-up and
//! `setDamage` calls are compared with CSP's bit for bit.

use crate::read;
use rustyac_physics::car::chassis::{csp_hit, CspHitFields, CspHitInput, CspHitOut};
use rustyac_physics::vecmath::Vec3f;
use std::cell::RefCell;
use std::sync::Mutex;

/// One open call: what the port gets, the car, its engine life before and the setDamage calls.
struct Open {
    car: usize,
    rsp: usize,
    input: CspHitInput,
    before: CspHitFields,
    life: f64,
    damage_fns: [usize; 4],
    suspensions: [usize; 4],
    damage: [Option<f32>; 4],
}

thread_local!(static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) });

#[derive(Default)]
struct Tally {
    calls: usize,
    zero: usize,
    ground: usize,
    damaged: usize,
    set_damage: usize,
    blown: usize,
    wrong: usize,
    wrong_scale: usize,
    lines: Vec<String>,
}

static TALLY: Mutex<Tally> = Mutex::new(Tally { calls: 0, zero: 0, ground: 0, damaged: 0, set_damage: 0, blown: 0, wrong: 0, wrong_scale: 0, lines: Vec::new() });

unsafe fn fields(car: usize) -> CspHitFields {
    CspHitFields {
        sleeping_frames: read(car + 0x3d94),
        last_collision_time: read(car + 0x3508),
        last_collision_with_car_time: read(car + 0x3c30),
        damage_zone_level: read(car + 0x3de0),
    }
}

unsafe fn v3(p: usize) -> Vec3f {
    Vec3f::new(read(p), read(p + 4), read(p + 8))
}

/// A break of kind `f` at `at`: the callback's entry, a setDamage or the callback's return.
/// Returns the return site to arm, at the entry.
pub unsafe fn hit(at: usize, dll: usize, context: usize) -> Option<usize> {
    let rsp = read::<usize>(context + 0x98);
    let rcx = read::<usize>(context + 0x80);
    if at == dll + 0x58_7cc0 {
        let car = rcx;
        let arg = |k: usize| rsp + 0x28 + 8 * (k - 5);
        let (other, object, pos) = (read::<usize>(arg(5)), read::<usize>(arg(7)), read::<usize>(arg(9)));
        let clock = read::<usize>(car + 0x138);
        // the box the zones read (CSP's car config, slot +0x440 → +0x90/+0x9c)
        let slot = read::<usize>(read::<usize>(dll + 0x282_13d8) + 8 * read::<u32>(car + 0x3e9c) as usize);
        let config = read::<usize>(slot + 0x440);
        // worldToLocal of the car's body (vtable +0x58), as the callback calls it
        let body = read::<usize>(car + 0x118);
        let to_local: extern "C" fn(usize, *mut [f32; 3], usize) = std::mem::transmute(read::<usize>(read::<usize>(body) + 0x58));
        let mut local = [0f32; 3];
        to_local(body, &mut local, pos);
        // the contact maker's mass scale against the port's (on_csp_collision): the other body's
        // mass over this car's (the wrappers' +176), at most 1
        let mine = read::<f32>(read::<usize>(context + 0x88) + 176);
        let scale = if other != 0 { (read::<f32>(other + 176) / mine).min(1.0) } else { 1.0 };
        if scale.to_bits() != read::<f32>(arg(12)).to_bits() {
            TALLY.lock().unwrap().wrong_scale += 1;
        }
        let input = CspHitInput {
            speed: read(arg(11)),
            scale: read(arg(12)),
            group: if object != 0 { read(object + 144) } else { 0 },
            with_car: other != 0,
            local: Vec3f::new(local[0], local[1], local[2]),
            time: read(clock + 0x20),
            damage_rate: read(clock + 0x108),
            min: v3(config + 0x90),
            max: v3(config + 0x9c),
        };
        let table = read::<usize>(car + 0x2c88);
        let suspensions: [usize; 4] = std::array::from_fn(|i| read(table + 8 * i));
        let damage_fns = suspensions.map(|s| read::<usize>(read::<usize>(s) + 0x78));
        OPEN.with(|o| *o.borrow_mut() = Some(Open { car, rsp, input, before: fields(car), life: read(car + 0x710), damage_fns, suspensions, damage: [None; 4] }));
        return Some(read::<usize>(rsp));
    }
    OPEN.with(|o| {
        let mut o = o.borrow_mut();
        let Some(open) = o.as_mut() else { return };
        if let Some(i) = (0..4).find(|&i| open.damage_fns[i] == at && open.suspensions[i] == rcx) {
            open.damage[i] = Some(read::<f32>(context + 0x1b0));
            return;
        }
        if rsp != open.rsp + 8 {
            return;
        }
        let open = o.take().unwrap();
        leave(open);
    });
    None
}

/// The setDamage functions of the open call (to arm with the return site).
pub fn damage_fns() -> Vec<usize> {
    OPEN.with(|o| o.borrow().as_ref().map(|o| o.damage_fns.to_vec()).unwrap_or_default())
}

unsafe fn leave(open: Open) {
    let after = fields(open.car);
    let life = read::<f64>(open.car + 0x710);
    let mut port = open.before;
    let out = csp_hit(&mut port, &open.input);
    let bits = |f: &CspHitFields| (f.sleeping_frames, f.last_collision_time.to_bits(), f.last_collision_with_car_time.to_bits(), f.damage_zone_level.map(f32::to_bits));
    let damage = |d: [Option<f32>; 4]| d.map(|x| x.map(f32::to_bits));
    let port_life = if out.is_some_and(|o| o.blow_up) { -100.0 } else { open.life };
    let port_damage = out.map_or([None; 4], |o: CspHitOut| o.suspension_damage);
    let mut t = TALLY.lock().unwrap();
    t.calls += 1;
    match out {
        None => t.zero += 1,
        Some(o) if o.ground => t.ground += 1,
        Some(o) => {
            t.damaged += 1;
            t.blown += o.blow_up as usize;
        }
    }
    t.set_damage += open.damage.iter().flatten().count();
    if bits(&port) != bits(&after) || life.to_bits() != port_life.to_bits() || damage(port_damage) != damage(open.damage) {
        t.wrong += 1;
        if t.lines.len() < 20 {
            t.lines.push(format!("callback differs: {:?}\n  before {:?}\n  csp  {after:?} life {life} setDamage {:?}\n  port {port:?} life {port_life} setDamage {port_damage:?}", open.input, open.before, open.damage));
        }
    }
}

pub fn finish() -> String {
    let t = TALLY.lock().unwrap();
    if t.calls == 0 {
        return String::new();
    }
    let mut s = format!(
        "car collision callback (+0x587cc0): {} calls ({} zero speed, {} ground, {} damaging, {} setDamage, {} engine blow-ups), {} differ from the port's csp_hit, {} mass scales differ from the port's\n",
        t.calls, t.zero, t.ground, t.damaged, t.set_damage, t.blown, t.wrong, t.wrong_scale
    );
    t.lines.iter().for_each(|l| s.push_str(&format!("{l}\n")));
    s
}
