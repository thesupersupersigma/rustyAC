// SPDX-License-Identifier: GPL-3.0-or-later

//! Which of the car's systems a recorded force call belongs to, from where in the game's code
//! the call was made. The function table is generated from acs.pdb (`gen_functions.py`); the
//! per-call-site labels inside `Suspension::step` and `HeaveSpring::step` come from reading
//! their disassembly against docs/map/suspension.md 5.2 and 5.6.

const FUNCTIONS: &str = include_str!("physics_functions.tsv");
const IMAGE_BASE: u64 = 0x1_4000_0000;

/// Every label [`system_of`] can give, in the order of the CSV's `sum.` columns.
pub const SYSTEMS: [&str; 19] = [
    "tyre",
    "surface",
    "spring",
    "damper",
    "bumpstop",
    "heave_spring",
    "heave_damper",
    "heave_bumpstop",
    "arb",
    "aero_drag",
    "aero_lift",
    "drivetrain",
    "brake",
    "steering",
    "stability",
    "sleep",
    "teleport",
    "other",
    "unknown",
];

struct Function {
    start: u32,
    size: u32,
    name: &'static str,
}

fn functions() -> &'static [Function] {
    static TABLE: std::sync::OnceLock<Vec<Function>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        FUNCTIONS
            .lines()
            .filter_map(|line| {
                let mut cells = line.split('\t');
                let va = u64::from_str_radix(cells.next()?, 16).ok()?;
                let size = cells.next()?.parse().ok()?;
                Some(Function { start: (va - IMAGE_BASE) as u32, size, name: cells.next()? })
            })
            .collect()
    })
}

/// The function a code address (offset into acs.exe) lies in, and how far into it.
pub fn function_of(site: u32) -> Option<(&'static str, u32)> {
    let table = functions();
    let i = table.partition_point(|f| f.start <= site);
    let f = table.get(i.checked_sub(1)?)?;
    // a return address may be the first byte after the function (a call as its last instruction)
    (site <= f.start + f.size).then_some((f.name, site - f.start))
}

/// Call sites that need more than the function name: `(address of the instruction after the
/// call, system)`.
const SITES: &[(u64, &str)] = &[
    // Suspension::step (double wishbone) 0x1402c3390. Each force is two calls: on the hub
    // (through the suspension's own addForceAtPos), then the opposite on the car body.
    (0x1_402c_3523, "spring"), // -F*up on the hub; F = spring (linear + progressive) plus the packer
    (0x1_402c_354a, "spring"), // (0, F, 0) body-local at the body's reference point
    (0x1_402c_3604, "damper"), // f*up on the hub, f = Damper::getForce(relative speed)
    (0x1_402c_364d, "damper"), // -f*up on the body
    (0x1_402c_36bf, "spring"), // active-actuator branch (replaces spring and damper; no car uses it)
    (0x1_402c_3711, "spring"),
    (0x1_402c_37b8, "bumpstop"), // upper bump stop: hub
    (0x1_402c_37e5, "bumpstop"), // upper bump stop: body
    (0x1_402c_3865, "bumpstop"), // lower bump stop: hub
    (0x1_402c_3892, "bumpstop"), // lower bump stop: body
    // HeaveSpring::step 0x1402b3960: left hub, right hub, body twice, for each force
    (0x1_402b_3b70, "heave_spring"), // F = spring plus packer
    (0x1_402b_3bce, "heave_spring"),
    (0x1_402b_3c02, "heave_spring"),
    (0x1_402b_3c36, "heave_spring"),
    (0x1_402b_3cd0, "heave_bumpstop"), // upper
    (0x1_402b_3d2e, "heave_bumpstop"),
    (0x1_402b_3d62, "heave_bumpstop"),
    (0x1_402b_3d96, "heave_bumpstop"),
    (0x1_402b_3e26, "heave_bumpstop"), // lower
    (0x1_402b_3e84, "heave_bumpstop"),
    (0x1_402b_3eb9, "heave_bumpstop"),
    (0x1_402b_3eee, "heave_bumpstop"),
    (0x1_402b_403e, "heave_damper"),
    (0x1_402b_4072, "heave_damper"),
    (0x1_402b_40c8, "heave_damper"),
    (0x1_402b_4116, "heave_damper"),
    // Tyre::step 0x140283800: drag on the car body from a surface with damping (sand, gravel)
    (0x1_4028_4059, "surface"),
    // Car::step 0x140275da0: the sleeping rule stops the body and the fuel tank
    (0x1_4027_638e, "sleep"),
    (0x1_4027_639e, "sleep"),
];

/// The system a call site belongs to. For a call made inside one of the suspension's force
/// entries (`Suspension::addForceAtPos` …) ask about the outer site instead: that is where
/// the tyre, the heave spring or the anti-roll bar called in.
///
/// Limits: only the sites a double-wishbone car with wings reaches were read one by one. A
/// call made by a tail jump (`Suspension::stop`, the axle suspension's force entries) shows
/// the return address of the caller one level up.
pub fn system_of(site: u32) -> &'static str {
    let va = IMAGE_BASE + site as u64;
    if let Some(&(_, system)) = SITES.iter().find(|(address, _)| *address == va) {
        return system;
    }
    let Some((name, _)) = function_of(site) else {
        return "unknown";
    };
    let class = name.split("::").next().unwrap_or(name);
    match class {
        "Tyre" => "tyre",
        // a force entry of the suspension itself: only meaningful together with its caller
        "Suspension" | "SuspensionStrut" | "SuspensionAxle" | "SuspensionML" => "other",
        "HeaveSpring" => "heave_spring",
        "AntirollBar" => "arb",
        "Wing" if name == "Wing::addDrag" => "aero_drag",
        "Wing" if name == "Wing::addLift" => "aero_lift",
        // cars without wings: the old one-body aero
        "AeroMap" if name == "AeroMap::addLift" => "aero_lift",
        "Wing" | "AeroMap" | "DRS" => "aero_drag",
        "Drivetrain" | "Engine" | "Kers" | "ERS" => "drivetrain",
        "BrakeSystem" => "brake",
        "SteeringSystem" => "steering",
        "StabilityControl" => "stability",
        "Car" => match name {
            "Car::forcePosition" | "Car::forceRotation" | "Car::reset" => "teleport",
            _ => "other",
        },
        _ => "other",
    }
}

/// The trailer line of one call site.
pub fn describe(site: u32) -> String {
    let location = match function_of(site) {
        Some((name, offset)) => format!("{name}+{offset:#x}"),
        None => "?".to_string(),
    };
    format!("callsite {site:x} {} {location}", system_of(site))
}
