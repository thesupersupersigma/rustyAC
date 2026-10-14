// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the setup editor's proofs: `rustyac_gui`'s setup screen on a Rust car
//! (`rustyac_physics::car::RollingChassis`) of the same data folder, seen through the same
//! snapshot rustyac.exe hands its editor; the editor's jobs are done at once, as the game
//! side's stand-in for the physics thread does them.

use std::cell::RefCell;
use std::rc::Rc;

use rustyac_gui::setup_car::{SetupJob, SetupSnapshot, SnapshotCar, SnapshotData};
use rustyac_physics::car::replay::Ground;
use rustyac_physics::car::setup::SetupTarget;
use rustyac_physics::car::{ChassisEnvironment, RollingChassis};

thread_local! {
    static CAR: RefCell<Option<Rc<RefCell<RollingChassis>>>> = const { RefCell::new(None) };
    static DATA: RefCell<Option<Rc<RefCell<SnapshotData>>>> = const { RefCell::new(None) };
}

/// The script's car: the Rust car of `<root>/content/cars/<name>/data`.
pub fn make_car(root: &std::path::Path, name: &str, max_gear: i32) -> Result<SnapshotCar, String> {
    let data = root.join("content/cars").join(name).join("data");
    let mut chassis = RollingChassis::new(&data, ChassisEnvironment::default(), Box::new(Ground::Flat), 1, 0.0)?;
    chassis.install_aero()?;
    chassis.install_brakes()?;
    chassis.install_drivetrain()?;
    chassis.install_aids()?;
    chassis.install_telemetry()?;
    let chassis = Rc::new(RefCell::new(chassis));
    CAR.with(|car| *car.borrow_mut() = Some(chassis.clone()));
    let snapshot = SetupSnapshot::of(&chassis.borrow(), name);
    if snapshot.max_gear != max_gear {
        return Err(format!("{name}: {} gears in the car, {max_gear} in its drivetrain.ini", snapshot.max_gear));
    }
    let unix_name = name.to_string();
    let apply = move |job: &SetupJob| {
        chassis.borrow_mut().apply_setup_job(job);
        let snapshot = SetupSnapshot::of(&chassis.borrow(), &unix_name);
        snapshot
    };
    let car = SnapshotCar { data: Rc::new(RefCell::new(SnapshotData { snapshot, apply: Some(Box::new(apply)), ..SnapshotData::default() })) };
    DATA.with(|data| *data.borrow_mut() = Some(car.data.clone()));
    Ok(car)
}

/// The car's state of the script, and its body where the HUD scripts have it.
pub fn set_state(body: &rustyac_physics::vecmath::Mat44f) {
    DATA.with(|data| {
        if let Some(data) = &*data.borrow() {
            let mut state = crate::gui_seq_setup::car_state();
            state.body_matrix = *body;
            data.borrow_mut().state = state;
        }
    });
}

/// Every setup item of the car, as the game's side writes them.
pub fn items(out: &mut String) {
    CAR.with(|car| {
        let Some(chassis) = &*car.borrow() else { return };
        let chassis = chassis.borrow();
        for item in &chassis.setup_manager.items {
            let value = match item.target {
                SetupTarget::GearSetting(index) => chassis.setup_manager.gear_settings[index],
                target => target.get(&chassis),
            };
            out.push_str(&crate::gui_seq_setup::item_line(&item.name, item.new_value, item.attached, value));
        }
    });
}
