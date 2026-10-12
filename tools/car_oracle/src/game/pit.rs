// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28b: the game's own `PitStop`, in-process, beside the referee of `race.rs`.
//!
//! `PitStop` and `QuickMenuPitstop` are main-thread objects whose constructors build the
//! menu's widgets, so both are made by hand: zeroed blocks with the game's vtables (the
//! pit stop finds the menu by a `dynamic_cast` over `Sim`'s children) and the members the
//! constructors would set. The menu's items and presets are plain data and are laid out
//! from the port's reading of the car's `setup.ini`.
//!
//! What then runs is the game's own code, by address: `PitStop::update` with
//! `isInPitPosition`, `isPitStopDone`, `commitQuickMenuChanges`, the menu's `getValue` /
//! `getMaxFuel` / `getWingPitstopTime` / `resetControls`, `CarAvatar::getPitstopTime` and
//! `Car::getPitstopTime` (with the referee's `rand`), the new-session lambda, the applying
//! lambda on the game's real car (through the queue of the physics thread, as every job of
//! the referee), `CarAvatar::updateInPitlaneState` and `RaceManager::checkMandatoryPit`.
//!
//! The mandatory pit window's opening, closing and black flag are a part of
//! `RaceManager::update` that only runs with a server (`Sim::client`); with one, the whole
//! of `update` takes its online path, which this harness cannot give it. That block is
//! written out here a second time from the listing ([`GamePit::window`]).

use rustyac_physics::race::pitstop::QuickMenu;

use super::race_ini::{rd, wr, write_wstring, zeroed};
use crate::acs::Acs;

const VA_PITSTOP_VTABLE: usize = 0x1_404b_7950;
const VA_QUICK_MENU_VTABLE: usize = 0x1_404b_8d38;
const VA_PITSTOP_UPDATE: usize = 0x1_400a_b190; // void PitStop::update(float)
const VA_PITSTOP_NEW_SESSION: usize = 0x1_400a_7ce0; // the handler of Sim::evOnNewSession
const VA_UPDATE_IN_PITLANE_STATE: usize = 0x1_400d_d6d0; // void CarAvatar::updateInPitlaneState(float)
const VA_SET_BLACK_FLAG: usize = 0x1_400d_9e70; // void CarAvatar::setBlackFlag(PenaltyDescription)

const SIM_GAME_OBJECTS: usize = 0x40;
const SIM_GAME_SCREEN: usize = 0x108;
const SIM_PAUSE_MENU: usize = 0x110;
const SIM_ESC_MENU: usize = 0x118;
const SIM_CLIENT: usize = 0x1c8;
const SIM_PIT_STOP: usize = 0x1d0;
const SIM_USE_MOUSE_PITSTOP: usize = 0x1e4;
const RM_PIT_WINDOW_START: usize = 0xb0;
const RM_PIT_WINDOW_END: usize = 0xb4;
const RM_CURRENT_SESSION: usize = 0xe8;
const CLIENT_CURRENT_SESSION: usize = 0x104f0;
const CA_BODY_MATRIX: usize = 0x224;
const CA_WORLD_MATRIX: usize = 0x26c;
const CA_MANDATORY_DONE: usize = 0x1295;
const CA_LAST_PITSTOP_TIME: usize = 0x1298;
const CA_IN_PITLANE: usize = 0x129c;
const CAR_IS_RETIRED: usize = 0xf0;
const CAR_AERO_WINGS: usize = 0xab8 + 0x38;
const CAR_TYRES: usize = 0xb28;
const CAR_MAX_FUEL: usize = 0x3dc8;
const TYRE_SIZE: usize = 0x858;
const TYRE_COMPOUND_DEFS: usize = 0x500;
const TYRE_CURRENT_COMPOUND: usize = 0x648;
const COMPOUND_SIZE: usize = 0x3f0;

thread_local! {
    /// the pit stop's block and the lambda's address, for the new-session handler
    static PIT: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// The handler of `Sim::evOnNewSession` that `PitStop::PitStop` registers: the game's own
/// lambda, with the capture it has (the pit stop).
extern "C" fn on_new_session(_function: *mut u8, event: *const u8) {
    let (pit, lambda) = PIT.with(|p| p.get());
    if pit != 0 {
        let closure = [pit];
        let call: extern "C" fn(*const usize, *const u8) = unsafe { std::mem::transmute(lambda) };
        call(closure.as_ptr(), event);
    }
}

/// What the menu's item functions read of the car, out of the game's own `Car`.
pub unsafe fn menu_car(car: *const u8) -> (rustyac_physics::race::pitstop::QuickMenuCar, Vec<String>) {
    let mut static_pressure = [0i32; 4];
    let mut names = Vec::new();
    let mut compounds = 0;
    for (wheel, slot) in static_pressure.iter_mut().enumerate() {
        let tyre = car.add(CAR_TYRES + TYRE_SIZE * wheel);
        let (begin, end): (*const u8, *const u8) = (rd(tyre, TYRE_COMPOUND_DEFS), rd(tyre, TYRE_COMPOUND_DEFS + 8));
        let count = if begin.is_null() { 0 } else { end.offset_from(begin) as usize / COMPOUND_SIZE };
        let current: i32 = rd(tyre, TYRE_CURRENT_COMPOUND);
        if current >= 0 && (current as usize) < count {
            *slot = rd::<f32>(begin.add(COMPOUND_SIZE * current as usize), 0x358) as i32;
        }
        if wheel == 0 {
            compounds = count;
            for n in 0..count {
                names.push(super::race_ini::text(begin.add(COMPOUND_SIZE * n + 0x8)));
            }
        }
    }
    let (begin, end): (*const u8, *const u8) = (rd(car, CAR_AERO_WINGS), rd(car, CAR_AERO_WINGS + 8));
    let wings = if begin.is_null() { 0 } else { end.offset_from(begin) as usize / 0x310 };
    let wing_angle = (0..wings)
        .map(|n| {
            let wing = begin.add(0x310 * n);
            if rd::<u8>(wing, 0x240) != 0 {
                rd::<f32>(wing, 0x260)
            } else {
                rd::<f32>(wing, 0x25c)
            }
        })
        .collect();
    (rustyac_physics::race::pitstop::QuickMenuCar { max_fuel: rd::<f64>(car, CAR_MAX_FUEL) as f32, compounds, static_pressure, wing_angle }, names)
}

/// The short name of the compound the first tyre has on.
pub unsafe fn current_compound(car: *const u8) -> Option<String> {
    let tyre = car.add(CAR_TYRES);
    let (begin, end): (*const u8, *const u8) = (rd(tyre, TYRE_COMPOUND_DEFS), rd(tyre, TYRE_COMPOUND_DEFS + 8));
    let count = if begin.is_null() { 0 } else { end.offset_from(begin) as usize / COMPOUND_SIZE };
    let current: i32 = rd(tyre, TYRE_CURRENT_COMPOUND);
    (current >= 0 && (current as usize) < count).then(|| super::race_ini::text(begin.add(COMPOUND_SIZE * current as usize + 0x28)))
}

/// The game's pit stop of the first car.
pub struct GamePit<'a> {
    acs: &'a Acs,
    pub pit: *mut u8,
    menu: *mut u8,
    sim: *mut u8,
    manager: *mut u8,
    avatar: *mut u8,
    car: *mut u8,
    client: *mut u8,
    screen: *mut u8,
    indicator: *mut u8,
    presets: Vec<*mut f32>,
    items: usize,
    /// the mandatory pit window (0, 0: none)
    pub window: (u16, u16),
}

impl<'a> GamePit<'a> {
    /// The blocks as the two constructors leave them, for the pit box at `pit_position`.
    /// `event`: `Sim::evOnNewSession`; `add_handler` registers one more handler there.
    pub unsafe fn build(acs: &'a Acs, sim: *mut u8, game: *mut u8, manager: *mut u8, avatar: *mut u8, car: *mut u8, pit_position: [f32; 3], port: &QuickMenu, window: (u16, u16)) -> GamePit<'a> {
        // QuickMenuPitstop: the items and every preset's values
        let menu = zeroed(0x190);
        wr(menu, 0, acs.va(VA_QUICK_MENU_VTABLE));
        wr(menu, 8, game);
        let count = port.items.len();
        let items = zeroed(0x78 * count.max(1));
        for (n, item) in port.items.iter().enumerate() {
            let at = items.add(0x78 * n);
            wr(at, 0, item.kind);
            wr(at, 4, item.index);
            wr(at, 0x38, item.min_value);
            wr(at, 0x3c, item.max_value);
            write_wstring(at.add(0x40), &item.setup_name);
            wr(at, 0x60, item.component_index);
            wr(at, 0x64, item.is_fixed as u8);
            wr(at, 0x68, item.pitstop_time);
        }
        wr(menu, 0x58, items);
        wr(menu, 0x60, items.add(0x78 * count));
        wr(menu, 0x68, items.add(0x78 * count));
        wr(menu, 0x70, port.stay_in_car as u8);
        let outer = zeroed(0x18 * port.presets.len().max(1));
        let mut presets = Vec::new();
        for (n, preset) in port.presets.iter().enumerate() {
            let values = Box::leak(preset.clone().into_boxed_slice()).as_mut_ptr();
            wr(outer, 0x18 * n, values);
            wr(outer, 0x18 * n + 8, values.add(preset.len()));
            wr(outer, 0x18 * n + 16, values.add(preset.len()));
            presets.push(values);
        }
        wr(menu, 0x78, outer);
        wr(menu, 0x80, outer.add(0x18 * port.presets.len()));
        wr(menu, 0x88, outer.add(0x18 * port.presets.len()));
        wr(menu, 0x90, sim);
        wr(menu, 0xe8, port.fuel_index);
        wr(menu, 0xec, port.current_index);
        wr(menu, 0xf0, port.current_preset);
        wr(menu, 0xf8, port.presets_count);

        // Sim: its children (the menu), the two menus that hide the stop, the screen
        let children = zeroed(8);
        wr(children, 0, menu);
        wr(sim, SIM_GAME_OBJECTS, children);
        wr(sim, SIM_GAME_OBJECTS + 8, children.add(8));
        wr(sim, SIM_GAME_OBJECTS + 16, children.add(8));
        let screen = zeroed(0x200);
        wr(screen, 0x178, 1u8);
        wr(sim, SIM_GAME_SCREEN, screen);
        wr(sim, SIM_PAUSE_MENU, zeroed(0x200));
        wr(sim, SIM_ESC_MENU, zeroed(0x200));
        wr(sim, SIM_USE_MOUSE_PITSTOP, 0u8);

        // PitStop::PitStop
        let pit = zeroed(0x240);
        wr(pit, 0, acs.va(VA_PITSTOP_VTABLE));
        wr(pit, 8, game);
        wr(pit, 0xac, 10.0f32);
        wr(pit, 0xc0, 1u8);
        wr(pit, 0xc8, avatar);
        wr(pit, 0xd0, sim);
        let indicator = zeroed(0x200);
        wr(indicator, 0xd8, 1u8);
        wr(pit, 0xd8, indicator);
        wr(pit, 0xe0, pit_position);
        write_wstring(pit.add(0xf0), "");
        wr(pit, 0x120, 4i32);
        wr(pit, 0x200, -1i32);
        wr(sim, SIM_PIT_STOP, pit);
        // (the first session has begun: the handler of its event and the player's teleport)
        wr(pit, 0x120, 5i32);

        // a server's session, for RaceManager::checkMandatoryPit only (see `before_manager`)
        let client = zeroed(0x10818);
        write_wstring(client.add(CLIENT_CURRENT_SESSION), "");
        PIT.with(|p| p.set((pit as usize, acs.va(VA_PITSTOP_NEW_SESSION))));
        GamePit { acs, pit, menu, sim, manager, avatar, car, client, screen, indicator, presets, items: count, window }
    }

    /// The handler `PitStop::PitStop` adds to `Sim::evOnNewSession`.
    pub fn new_session_handler() -> usize {
        on_new_session as *const () as usize
    }

    /// `CarAvatar::update` of the player's car as far as the stop reads it: the body's
    /// matrix, and `updateInPitlaneState` (which asks `RaceManager::checkMandatoryPit` when
    /// the car leaves the pit lane).
    pub unsafe fn before_manager(&self, dt: f32) {
        let matrix: [f32; 16] = rd(self.avatar, CA_WORLD_MATRIX);
        wr(self.avatar, CA_BODY_MATRIX, matrix);
        // checkMandatoryPit reads the session's start from the server's session: for this
        // one call there is a client, with the manager's own session start
        wr(self.client, CLIENT_CURRENT_SESSION + 0x30, rd::<f64>(self.manager, RM_CURRENT_SESSION + 0x58));
        wr(self.sim, SIM_CLIENT, self.client);
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_UPDATE_IN_PITLANE_STATE));
        update(self.avatar, dt);
        wr(self.sim, SIM_CLIENT, 0usize);
    }

    /// The part of `RaceManager::update` @ 0x140141550 that keeps the mandatory pit
    /// window, written out from the listing (see the head of this file), then
    /// `PitStop::update`.
    pub unsafe fn update(&self, dt: f32, now: f64) {
        if self.window.1 != 0 && rd::<i32>(self.manager, RM_CURRENT_SESSION) == 3 {
            let elapsed = now - rd::<f64>(self.manager, RM_CURRENT_SESSION + 0x58);
            if rd::<f32>(self.manager, RM_PIT_WINDOW_END) == 0.0 {
                let closed;
                if rd::<u8>(self.manager, RM_CURRENT_SESSION + 4) == 0 {
                    let leader: u32 = rd(self.manager, RM_CURRENT_SESSION + 0x68);
                    if rd::<f32>(self.manager, RM_PIT_WINDOW_START) == 0.0 && self.window.0 as u32 <= leader {
                        wr(self.manager, RM_PIT_WINDOW_START, elapsed as f32);
                    }
                    closed = leader >= self.window.1 as u32 - 1;
                } else {
                    if rd::<f32>(self.manager, RM_PIT_WINDOW_START) == 0.0 && (self.window.0 as i32 * 60000) as f64 <= elapsed {
                        wr(self.manager, RM_PIT_WINDOW_START, elapsed as f32);
                    }
                    closed = elapsed >= (self.window.1 as i32 * 60000) as f64;
                }
                if closed {
                    wr(self.manager, RM_PIT_WINDOW_END, elapsed as f32);
                }
            }
            if 0.0 < elapsed && 0.0 < rd::<f32>(self.manager, RM_PIT_WINDOW_END) && rd::<u8>(self.avatar, CA_MANDATORY_DONE) == 0 && rd::<u8>(self.avatar, CA_IN_PITLANE) == 0 {
                let set_black_flag: extern "C" fn(*mut u8, i32) = std::mem::transmute(self.acs.va(VA_SET_BLACK_FLAG));
                set_black_flag(self.avatar, 3);
                wr(self.car, CAR_IS_RETIRED, 1u8);
            }
        }
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_PITSTOP_UPDATE));
        update(self.pit, dt);
    }

    /// The three lines the port's `Referee::dump` has for the stop, out of the game's blocks.
    pub unsafe fn dump(&self) -> Vec<(String, String)> {
        let pit = self.pit;
        let (wings_begin, wings_end): (*const u8, *const u8) = (rd(pit, 0x208), rd(pit, 0x210));
        let mut wings = Vec::new();
        let mut at = wings_begin;
        while !at.is_null() && at < wings_end {
            wings.push(format!("{}:{:08x}", rd::<i32>(at, 0), rd::<u32>(at, 4)));
            at = at.add(8);
        }
        let (psi_begin, psi_end): (*const u8, *const u8) = (rd(pit, 0x220), rd(pit, 0x228));
        let mut psi = Vec::new();
        let mut at = psi_begin;
        while !at.is_null() && at < psi_end {
            psi.push(format!("{:08x}", rd::<u32>(at, 0)));
            at = at.add(4);
        }
        let text = format!(
            "state {} menu {} timer {:08x} max {:08x} fuel {:08x} repair {:08x} tyre {:08x} indicator {} compound {} wings [{}] psi [{}] getFuel {:08x} repairs {}{}{} apps {}",
            rd::<i32>(pit, 0x120),
            rd::<u8>(pit, 0xa0),
            rd::<u32>(pit, 0xa4),
            rd::<u32>(pit, 0xac),
            rd::<u32>(pit, 0xb4),
            rd::<u32>(pit, 0xb8),
            rd::<u32>(pit, 0xbc),
            rd::<u8>(self.indicator, 0xd8),
            rd::<i32>(pit, 0x200),
            wings.join(" "),
            psi.join(" "),
            rd::<u32>(pit, 0x238),
            rd::<u8>(pit, 0x23c),
            rd::<u8>(pit, 0x23d),
            rd::<u8>(pit, 0x23e),
            rd::<u8>(self.screen, 0x178)
        );
        let presets: Vec<String> = self.presets.iter().map(|values| (0..self.items).map(|i| format!("{:08x}", (*values.add(i)).to_bits())).collect::<Vec<_>>().join(",")).collect();
        let menu = format!("{};{}", rd::<i32>(self.menu, 0xf0), presets.join("/"));
        let a = self.avatar;
        let avatar = format!("done {} last {:08x} in {}{} entry {:08x} exit {:08x}", rd::<u8>(a, CA_MANDATORY_DONE), rd::<u32>(a, CA_LAST_PITSTOP_TIME), rd::<u8>(a, CA_IN_PITLANE), rd::<u8>(a, 0x129d), rd::<u32>(a, 0x12a0), rd::<u32>(a, 0x12a4));
        vec![("pit".to_string(), text), ("pitMenu".to_string(), menu), ("pitAvatar".to_string(), avatar)]
    }
}
