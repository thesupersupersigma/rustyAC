//! Which of the car's systems a recorded force call belongs to, from where in the game's code
//! the call was made. The function table is generated from acs.pdb (`gen_functions.py`); the
//! per-call-site labels inside `Suspension::step` and `Tyre::step` come from reading their
//! disassembly (see docs/oracle/car_oracle.md).

const FUNCTIONS: &str = include_str!("physics_functions.tsv");
const IMAGE_BASE: u64 = 0x1_4000_0000;

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

/// Call sites that need more than the function name: `(return address, system)`.
/// Addresses are Ghidra addresses of the instruction after the call.
const SITES: &[(u64, &str)] = &[
    // filled in from the disassembly; see `describe`
];

/// The system a call site belongs to.
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
        "Suspension" | "SuspensionStrut" | "SuspensionAxle" | "SuspensionML" => "spring",
        "HeaveSpring" => "heave",
        "AntirollBar" => "arb",
        "Wing" | "AeroMap" | "DRS" => "aero",
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
