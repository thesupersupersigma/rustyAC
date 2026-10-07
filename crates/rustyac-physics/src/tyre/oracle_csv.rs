// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Reader for the CSV files `tools/sctm_oracle` writes (AC's own `SCTM::solve` inputs and
//! outputs), plus the bit-level comparison used by `sctm_compare` and the golden test.
//!
//! Columns are found by header name, so extra columns (e.g. the golden file's `case`) are fine.
//! The oracle prints every float as the shortest text that reads back to the same f32.

use super::{TyreModelInput, TyreModelOutput};

/// Output fields in `TyreModelOutput` order, as named in the CSV header.
pub const OUTPUT_FIELDS: [&str; 7] = ["Fy", "Fx", "Mz", "trail", "ndSlip", "Dy", "Dx"];

#[derive(Clone, Debug)]
pub struct OracleRow {
    /// 1-based line number in the file (the header is line 1).
    pub line: usize,
    /// The golden file's `case` column; empty for plain oracle files.
    pub case: String,
    /// The `axle` column.
    pub axle: String,
    pub input: TyreModelInput,
    /// What AC returned.
    pub expected: TyreModelOutput,
}

pub fn parse(text: &str) -> Result<Vec<OracleRow>, String> {
    let mut lines = text.lines().enumerate();
    let (_, header) = lines.next().ok_or("empty csv")?;
    let names: Vec<&str> = header.trim().split(',').collect();
    let col = |name: &str| {
        names
            .iter()
            .position(|n| *n == name)
            .ok_or_else(|| format!("csv has no {name} column"))
    };
    let case_col = names.iter().position(|n| *n == "case");
    let axle_col = col("axle")?;
    let input_cols = [
        col("load")?,
        col("slip_angle_rad")?,
        col("slip_ratio")?,
        col("camber_rad")?,
        col("speed")?,
        col("u")?,
        col("cp_length")?,
        col("grain")?,
        col("blister")?,
        col("pressure_ratio")?,
    ];
    let tyre_index_col = col("tyre_index")?;
    let simple_col = col("use_simple_model")?;
    let mut output_cols = [0usize; 7];
    for (slot, name) in output_cols.iter_mut().zip(OUTPUT_FIELDS) {
        *slot = col(name)?;
    }

    let mut rows = Vec::new();
    for (index, raw) in lines {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let line = index + 1;
        let cells: Vec<&str> = raw.split(',').collect();
        if cells.len() != names.len() {
            return Err(format!(
                "line {line}: {} cells, header has {}",
                cells.len(),
                names.len()
            ));
        }
        let float = |c: usize| {
            cells[c]
                .parse::<f32>()
                .map_err(|_| format!("line {line}: {:?} is not a number ({})", cells[c], names[c]))
        };
        let i = input_cols;
        let o = output_cols;
        rows.push(OracleRow {
            line,
            case: case_col.map(|c| cells[c].to_string()).unwrap_or_default(),
            axle: cells[axle_col].to_string(),
            input: TyreModelInput {
                load: float(i[0])?,
                slip_angle_rad: float(i[1])?,
                slip_ratio: float(i[2])?,
                camber_rad: float(i[3])?,
                speed: float(i[4])?,
                u: float(i[5])?,
                tyre_index: cells[tyre_index_col]
                    .parse()
                    .map_err(|_| format!("line {line}: bad tyre_index"))?,
                cp_length: float(i[6])?,
                grain: float(i[7])?,
                blister: float(i[8])?,
                pressure_ratio: float(i[9])?,
                use_simple_model: cells[simple_col] != "0",
            },
            expected: TyreModelOutput {
                fy: float(o[0])?,
                fx: float(o[1])?,
                mz: float(o[2])?,
                trail: float(o[3])?,
                nd_slip: float(o[4])?,
                dy: float(o[5])?,
                dx: float(o[6])?,
            },
        });
    }
    Ok(rows)
}

/// The seven outputs in [`OUTPUT_FIELDS`] order.
pub fn fields(out: &TyreModelOutput) -> [f32; 7] {
    [
        out.fy,
        out.fx,
        out.mz,
        out.trail,
        out.nd_slip,
        out.dy,
        out.dx,
    ]
}

/// Number of representable f32 values between `a` and `b` (0 = identical bits, +0 and -0
/// are 1 apart). `u32::MAX` if either is NaN and the bits differ.
pub fn ulp_distance(a: f32, b: f32) -> u32 {
    if a.to_bits() == b.to_bits() {
        return 0;
    }
    if a.is_nan() || b.is_nan() {
        return u32::MAX;
    }
    // sign-magnitude -> a number line where neighbouring floats are 1 apart
    let ordered = |x: f32| {
        let bits = x.to_bits();
        let magnitude = (bits & 0x7fff_ffff) as i64;
        if bits >> 31 != 0 {
            -magnitude - 1
        } else {
            magnitude
        }
    };
    (ordered(a) - ordered(b))
        .unsigned_abs()
        .min(u32::MAX as u64 - 1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulp_distance_counts_steps() {
        assert_eq!(ulp_distance(1.0, 1.0), 0);
        assert_eq!(ulp_distance(1.0, f32::from_bits(1.0f32.to_bits() + 3)), 3);
        assert_eq!(ulp_distance(0.0, -0.0), 1);
        assert_eq!(ulp_distance(f32::from_bits(1), -f32::from_bits(1)), 3);
        assert_eq!(ulp_distance(f32::NAN, 1.0), u32::MAX);
    }
}
