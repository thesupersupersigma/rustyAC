// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the setup editor's proofs: `rustyac_gui`'s setup screen on a Rust car
//! (`rustyac_physics::car::RollingChassis`) of the same data folder; the editor's jobs are
//! done at once, as the game side's stand-in for the physics thread does them.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use rustyac_gui::setup_car::{SetupCar, SetupItemInfo, SetupJob};
use rustyac_physics::car::replay::Ground;
use rustyac_physics::car::setup::SetupTarget;
use rustyac_physics::car::{ChassisEnvironment, RollingChassis};

thread_local! {
    static CAR: RefCell<Option<Rc<RefCell<RollingChassis>>>> = const { RefCell::new(None) };
}

pub struct PortCar {
    pub chassis: Rc<RefCell<RollingChassis>>,
    pub name: String,
    pub max_gear: i32,
}

impl SetupCar for PortCar {
    fn unix_name(&self) -> String {
        self.name.clone()
    }

    fn data_path(&self) -> PathBuf {
        self.chassis.borrow().data_path.clone()
    }

    fn items(&self) -> Vec<SetupItemInfo> {
        self.chassis.borrow().setup_manager.items.iter().map(|item| SetupItemInfo { name: item.name.clone(), units: item.units.clone(), label_multiplier: item.label_multiplier }).collect()
    }

    fn item_new_value(&self, index: usize) -> f32 {
        self.chassis.borrow().setup_manager.items[index].new_value
    }

    fn max_gear(&self) -> i32 {
        self.max_gear
    }

    fn job(&mut self, job: SetupJob) {
        self.chassis.borrow_mut().setup_manager.apply_job(&job);
    }
}

/// The script's car: the Rust car of `<root>/content/cars/<name>/data`.
pub fn make_car(root: &std::path::Path, name: &str, max_gear: i32) -> Result<PortCar, String> {
    let data = root.join("content/cars").join(name).join("data");
    let mut chassis = RollingChassis::new(&data, ChassisEnvironment::default(), Box::new(Ground::Flat), 1, 0.0)?;
    chassis.install_aero()?;
    chassis.install_brakes()?;
    chassis.install_drivetrain()?;
    chassis.install_aids()?;
    chassis.install_telemetry()?;
    let chassis = Rc::new(RefCell::new(chassis));
    CAR.with(|car| *car.borrow_mut() = Some(chassis.clone()));
    Ok(PortCar { chassis, name: name.to_string(), max_gear })
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
