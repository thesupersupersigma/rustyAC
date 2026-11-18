// SPDX-License-Identifier: GPL-3.0-or-later

//! CSP's air density (Task C4a, CSP 4305 `dwrite.dll`): the barometric law at the car's
//! altitude, with the temperature of the air at the car's height above the road blended between
//! road and ambient. CSP gives it to cars with its extended physics only (`[HEADER] VERSION =
//! extended-…`); every other car keeps Vanilla's `1.2922 - 0.0041 T` (docs/csp/port_c4.md).
//! The f32 operations are in CSP's order; the maths functions are CSP's own (they give
//! MSVCR120's bits on every input the law can reach, docs/csp/port_c4.md part C).

use crate::math::{atanf, expf, fdtest_inf_or_nan, logf, powf, sqrtf};
use crate::vecmath::Vec3f;

const EPS: f32 = 1.192_092_9e-7;
const KELVIN: f32 = 273.15;
const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);
// CSP's -0.04 and -0.005 sit one bit above the nearest f32 of the literal.
const MINUS_0_04: f32 = f32::from_bits(0xbd23_d70b);
const MINUS_0_005: f32 = f32::from_bits(0xbba3_d70b);

/// `((sqrt(1 - x * 16) + 1) * 0.5)^2`, the logarithm's argument of the stable side.
fn half_root_squared(one_minus: f32) -> f32 {
    let h = (sqrtf(one_minus) + 1.0) * 0.5;
    h * h
}

/// `dwrite+0xb0fcc0`: the temperature (deg C) of the air `height` m above a road at `road` deg C
/// under air at `ambient` deg C, with `wind` m/s; a surface-layer profile solved in four rounds.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn near_ground_temperature(road: f32, ambient: f32, wind: f32, height: f32) -> f32 {
    if !(height > 0.0) {
        return road;
    }
    let d = road - ambient;
    if !(d.abs() >= EPS) {
        return ambient;
    }
    let ta = ambient + KELVIN;
    let tr = road + KELVIN;
    let tm = (ta + tr) * 0.5;
    let u0 = if road > ambient { sqrtf(d) * 0.05 } else { 0.0 };
    let g = ((ambient - road) * 19.62) / tm;
    let u = sqrtf(u0 * u0 + wind * wind);
    let u = if u > EPS { u } else { EPS };
    let mut us = u * 0.052_625_33;
    let ri = g / (u * u);
    let mut l = 1.0e6f32;
    if !(ri >= 0.0) {
        l = 2.0 / ri;
    } else if ri > 0.0 && ri < 0.2 {
        let t = 1.0 - ri * 5.0;
        l = (t + t) / ri;
    }
    let c1 = u * 0.4;
    let dt = (ta - tr) * 0.4;
    let (mut z0, mut r, mut lnz) = (0.0f32, 0.0f32, 0.0f32);
    for _ in 0..4 {
        z0 = expf(sqrtf(us * 66.666_67) * MINUS_0_04) * 0.001;
        let zl = 1.0 / l;
        let zl2 = zl + zl;
        let x16 = zl2 * 16.0;
        let (pm2, c8);
        if !(zl2 >= 0.0) {
            let a = sqrtf(1.0 - x16);
            let b = sqrtf(a);
            let at = atanf(b);
            let hb = (b + 1.0) * 0.5;
            pm2 = (logf((hb * hb) * ((a + 1.0) * 0.5)) - (at + at)) + HALF_PI;
            c8 = zl2 * -5.0;
        } else {
            pm2 = zl2 * -5.0;
            c8 = pm2;
        }
        let pm0 = if !(zl * 0.001 >= 0.0) {
            let a = sqrtf(1.0 - zl * 0.016);
            let b = sqrtf(a);
            let at = atanf(b);
            let hb = (b + 1.0) * 0.5;
            (logf(((a + 1.0) * 0.5) * (hb * hb)) - (at + at)) + HALF_PI
        } else {
            zl * MINUS_0_005
        };
        r = 1.0 / z0;
        lnz = logf(r + r);
        let ph2 = if !(zl2 >= 0.0) { logf(half_root_squared(1.0 - x16)) } else { c8 };
        let t = zl * z0;
        let ph0 = if !(t >= 0.0) { logf(half_root_squared(1.0 - t * 16.0)) } else { t * -5.0 };
        let c = c1 / ((7.600_902_6 - pm2) + pm0);
        let k = ((((lnz - ph2) + ph0) * 0.127_420_99) / dt) * c;
        us = (c + us) * 0.5;
        l = k * (c * tm) + l * 0.5;
    }
    if !(height > z0) {
        return road;
    }
    let zl = 1.0 / l;
    let ph2 = if !(zl + zl >= 0.0) { logf(half_root_squared(1.0 - zl * 32.0)) } else { zl * -10.0 };
    let t = zl * z0;
    let p0 = if !(t >= 0.0) { logf(half_root_squared(1.0 - t * 16.0)) } else { t * -5.0 };
    let s = dt / ((lnz - ph2) + p0);
    let lq = logf(r * height);
    let t2 = zl * height;
    let phz = if !(t2 >= 0.0) { logf(half_root_squared(1.0 - t2 * 16.0)) } else { t2 * -5.0 };
    ((((lq - phz) + p0) * s) * 2.5 + tr) - KELVIN
}

/// `dwrite+0xb10410` (the physics' call, flag 1): kg/m^3 at `y` m above the track's origin,
/// `altitude` the origin's height above the sea (NaN: the sea), `height` m above the road.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn air_density(altitude: f32, y: f32, ambient: f32, road: f32, wind: f32, height: f32) -> f32 {
    let h = if altitude.is_nan() { 0.0 } else { altitude + y };
    let t = near_ground_temperature(road, ambient, wind.abs(), height);
    let inv = 1.0 / (t + KELVIN);
    let x = 1.0 - (h * 0.0065) * inv;
    let x = if x > 0.0 { x } else { 0.0 };
    let p = powf(x, 5.255_777) * 101.325;
    (if p > EPS { p } else { EPS } * 3.483_610_6) * inv
}

/// `dwrite+0x1150520`: the plane (normal, distance) through the four tyres' contact points
/// (front left, front right, rear left, rear right); flat at the first one's height when the
/// four make no plane.
pub fn ground_plane(p: [Vec3f; 4]) -> [f32; 4] {
    let ax = ((p[1].x - p[0].x) + p[3].x) - p[2].x;
    let ay = ((p[1].y - p[0].y) + p[3].y) - p[2].y;
    let az = ((p[1].z - p[0].z) + p[3].z) - p[2].z;
    let bx = ((p[0].x - p[2].x) + p[1].x) - p[3].x;
    let by = ((p[0].y - p[2].y) + p[1].y) - p[3].y;
    let bz = ((p[0].z - p[2].z) + p[1].z) - p[3].z;
    let nx = bz * ay - by * az;
    let ny = az * bx - ax * bz;
    let nz = ax * by - bx * ay;
    let inv = 1.0 / sqrtf((ny * ny + nx * nx) + nz * nz);
    let (nx, ny, nz) = (inv * nx, inv * ny, inv * nz);
    let cx = (((p[0].x + p[1].x) + p[2].x) + p[3].x) * 0.25;
    let cy = (((p[0].y + p[1].y) + p[2].y) + p[3].y) * 0.25;
    let cz = (((p[0].z + p[1].z) + p[2].z) + p[3].z) * 0.25;
    let d = (cx * nx + cy * ny) + cz * nz;
    if fdtest_inf_or_nan(ny) {
        return [0.0, 1.0, 0.0, p[0].y];
    }
    [nx, ny, nz, d]
}

/// `dwrite+0x1150ae0`, CSP's `RaceEngineer::getPointGroundHeight` for a car with its record:
/// the signed height of `p` over `plane`.
pub fn plane_height(plane: &[f32; 4], p: &Vec3f) -> f32 {
    ((p.y * plane[1] + p.x * plane[0]) + p.z * plane[2]) - plane[3]
}

/// What the law needs of one car between steps: the track's altitude, the ground plane the
/// car's last step left and the density of this step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarAir {
    pub altitude: f32,
    pub plane: [f32; 4],
    pub density: f32,
}

/// `dwrite+0x1237c50`: the altitude CSP knows for a track (folder name, layout), m; unknown:
/// [`UNKNOWN_ALTITUDE`].
pub fn known_altitude(id: &str, layout: &str) -> f32 {
    match id {
        "ks_barcelona" => 118.0,
        "ks_black_cat_county" => 1250.0,
        "ks_brands_hatch" => 130.0,
        "ks_drag" | "drift" => 100.0,
        "ks_highlands" => 240.0,
        "imola" => 37.0,
        "ks_laguna_seca" => 230.0,
        "magione" => 235.0,
        "monza" | "ks_monza66" => 182.0,
        "mugello" => 259.0,
        "ks_nordschleife" if layout == "touristenfahrten" => 574.0,
        "ks_nordschleife" => 625.0,
        "ks_nurburgring" => 619.0,
        "ks_red_bull_ring" => 678.0,
        "ks_silverstone" | "ks_silverstone1967" => 153.0,
        "spa" => 418.0,
        "trento-bondone" => 380.0,
        "ks_vallelunga" => 195.0,
        "ks_zandvoort" => 5.0,
        "india" => 193.0,
        "grandsambuc" => 583.0,
        "forliring" => 28.0,
        "greatwall" => 440.0,
        "korea" => 2.0,
        "la_canyons" => 1341.0,
        "auto24ring" => 4.0,
        "monaco_1966" => 3.0,
        _ => guessed_altitude(id),
    }
}

/// CSP's mark for a track it knows nothing of (`FLT_MIN`).
pub const UNKNOWN_ALTITUDE: f32 = f32::MIN_POSITIVE;

/// `dwrite+0x1236980`'s words: (word, second word or "", m); the "poznanl" is CSP's spelling.
pub const GUESSED_WORDS: [(&str, &str, f32); 106] = [
    ("barcelona", "", 118.0), ("adelaide", "", 10.0), ("alastaro", "", 110.0), ("bathurst", "", 711.0),
    ("bikernieki", "", 5.0), ("bridgehampton", "", 68.0), ("cadwell", "", 109.0), ("daytona", "", 6.0),
    ("donington", "", 89.0), ("hungaroring", "", 235.0), ("interlagos", "", 785.0), ("monaco", "", 8.0),
    ("norisring", "", 316.0), ("oesterreichring", "", 678.0), ("oschersleben", "", 83.0),
    ("atlanta", "motorsports", 421.0), ("autopolis", "oita", 819.0), ("brands", "hatch", 130.0),
    ("gilles", "villeneuve", 11.0), ("glava", "zete", 67.0), ("laguna", "seca", 230.0),
    ("paul", "ricard", 425.0), ("pikes", "peak", 2863.0), ("red", "bull", 678.0), ("trento", "bondone", 310.0),
    ("willow", "springs", 736.0), ("yas", "marina", 5.0), ("brno", "", 435.0), ("buddh", "", 193.0),
    ("goodwood", "", 29.0), ("moscow", "", 244.0), ("newbury", "", 80.0), ("salzburgring", "", 667.0),
    ("silverstone", "", 153.0), ("sochi", "", 5.0), ("suzuka", "", 30.0), ("topgear", "", 52.0),
    ("tsukuba", "", 27.0), ("valencia", "", 4.0), ("watkins", "", 481.0), ("ahvenisto", "", 100.0),
    ("algarve", "", 106.0), ("assen", "", 12.0), ("barbagallo", "", 73.0), ("baskerville", "", 47.0),
    ("bilsterberg", "", 278.0), ("feldberg", "", 640.0), ("fuji", "", 583.0), ("hockenheim", "", 98.0),
    ("highlands", "", 240.0), ("imola", "", 37.0), ("indi", "", 222.0), ("karelia", "", 164.0),
    ("kicevo", "", 625.0), ("knutstorp", "", 80.0), ("lemans", "", 62.0), ("luzia", "", 6.0),
    ("macau", "", 4.0), ("magione", "", 235.0), ("modena", "", 51.0), ("monza", "", 182.0),
    ("mugello", "", 259.0), ("nurburgring", "", 619.0), ("oulton", "", 70.0), ("peklo", "", 297.0),
    ("spa", "", 418.0), ("tajo", "", 1287.0), ("vallelunga", "", 195.0), ("varano", "", 178.0),
    ("vrc_alabama", "", 199.0), ("zandvoort", "", 5.0), ("chang", "", 180.0), ("charade", "", 800.0),
    ("chayka", "", 158.0), ("pau", "", 179.0), ("croft", "", 58.0), ("grobnik", "", 302.0),
    ("knockhill", "", 265.0), ("krajiskazmija", "", 202.0), ("kyalami", "", 1516.0),
    ("lapedrerasanluis", "", 527.0), ("longford", "", 163.0), ("lucca", "ring", 18.0), ("misano", "", 25.0),
    ("miseluk", "", 134.0), ("mount", "akina", 1171.0), ("nemuno", "ziedas", 48.0),
    ("njmp", "lightning", 17.0), ("okayama", "", 264.0), ("laodaohe", "", 29.0), ("potrero", "", 961.0),
    ("riverside", "", 489.0), ("road", "america", 330.0), ("road", "atlanta", 280.0),
    ("slovakia", "ring", 114.0), ("snetterton", "", 43.0), ("sonoma", "", 6.0), ("sveg", "", 352.0),
    ("targa", "florio", 29.0), ("thomson", "", 23.0), ("tor", "poznanl", 91.0), ("transfagarasan", "", 622.0),
    ("usce", "", 77.0), ("vir", "", 112.0), ("wakefield", "park", 650.0), ("sebring", "", 20.0),
];

/// `dwrite+0x1236980`: [`GUESSED_WORDS`] in the folder name (any case), in this order; a pair
/// needs its second word after the first's first place.
fn guessed_altitude(id: &str) -> f32 {
    let id = id.to_ascii_lowercase();
    let at = |word: &str| id.find(word);
    for (a, b, metres) in GUESSED_WORDS {
        let found = match (at(a), b) {
            (a, "") => a.is_some(),
            (Some(a), b) => at(b).is_some_and(|b| b > a),
            (None, _) => false,
        };
        if found {
            return metres;
        }
    }
    // shortcut: CSP also gives FLT_MIN to "hong" when its track config object (qword
    // dwrite+0x2821078, +22) is set with a config version below 0xAC5; rustyAC has no CSP track
    // config, so that test is taken as false. Upgrade when CSP track configs are ported.
    if at("hong").is_some() { 950.0 } else { UNKNOWN_ALTITUDE }
}

/// `dwrite+0x118e6c0`, the physics' half (`dwrite+0x27fa4e0`): the altitude of the track's
/// origin. `key` is the text of `[_EXTENSION] PIT_ALTITUDE`, else `[ALTITUDE] BASE`, of the
/// track's `data/surfaces.ini` (None: neither is there); `pit_y` the world Y of the node
/// `AC_PIT_0` (None: missing, 0).
pub fn track_altitude(key: Option<&str>, id: &str, layout: &str, pit_y: Option<f32>) -> f32 {
    let pit = match key {
        Some(text) => super::parse_float(text, 0.0) as f32,
        None => known_altitude(id, layout),
    };
    if fdtest_inf_or_nan(pit) {
        return 0.0;
    }
    let y = pit_y.unwrap_or(0.0);
    // (the unknown track's 294 m, 0 with an old CSP track config: see guessed_altitude)
    if pit == UNKNOWN_ALTITUDE { 294.0 - y } else { pit - y }
}

#[cfg(test)]
mod tests {
    use super::*;

    // values CSP 0.3.2 (4305) gave in csp_oracle's grid, the two that once differed by the -0.04 / -0.005 bits
    #[test]
    fn near_ground_temperature_as_csp() {
        assert_eq!(near_ground_temperature(-15.0, -7.5, 15.0, 40.0).to_bits(), 0xc05f_9400);
        assert_eq!(near_ground_temperature(-15.0, 17.5, 20.0, 0.1).to_bits(), 0x409b_f640);
        assert_eq!(near_ground_temperature(20.0, 14.0, 3.0, 0.0), 20.0);
        assert_eq!(near_ground_temperature(20.0, 20.0, 3.0, 1.0), 20.0);
    }

    #[test]
    fn altitudes_as_csp() {
        assert_eq!(track_altitude(None, "spa", "", None), 418.0);
        assert_eq!(track_altitude(None, "ks_nordschleife", "touristenfahrten", Some(10.0)), 564.0);
        assert_eq!(track_altitude(Some("100"), "spa", "", Some(22.0)), 78.0);
        assert_eq!(track_altitude(None, "nowhere", "", Some(4.0)), 290.0);
    }
}
