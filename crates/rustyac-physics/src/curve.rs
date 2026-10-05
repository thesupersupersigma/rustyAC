//! 1:1 port of the parts of AC's `Curve` (a lookup table) that the tyre model evaluates:
//! `getCount`, `addValue`, `getValue` (linear) and `getCubicSplineValue`
//! (`CubicSpline<float,float>`).
//!
//! Operation order follows the disassembly, not the pseudo-C; all arithmetic is f32.
//! `Curve::load` (a `.lut` file), `scale` and `getPairAtIndex` are ported too; inline ini
//! tables are `INIReader::getCurve`, see [`crate::data::ini`].

// Comparisons are spelled the way the original branches so NaN takes the same path.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::implicit_saturating_sub)]

use std::path::Path;
use std::sync::OnceLock;

use crate::data::ini::{sibling_acd, text_mode};
use crate::math::wcstod;

/// `CubicSpline<float,float>::Element` (0x14 bytes): one cubic piece starting at `x`.
/// The PDB member names were not read; `a..d` are the usual spline coefficient names.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Element {
    x: f32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
}

/// `Curve`: `references` (x) and `values` (y), plus the lazily built `cSpline`.
#[derive(Clone, Debug, Default)]
pub struct Curve {
    references: Vec<f32>,
    values: Vec<f32>,
    /// `cSpline.mElements`; unset plays the role of `cubicSplineReady == false`.
    c_spline: OnceLock<Vec<Element>>,
}

impl Curve {
    pub fn new() -> Curve {
        Curve::default()
    }

    pub fn from_pairs(pairs: &[(f32, f32)]) -> Curve {
        let mut curve = Curve::new();
        for &(reference, value) in pairs {
            curve.add_value(reference, value);
        }
        curve
    }

    /// The text of a `.lut` file, read the way [`Curve::load`] reads it.
    pub fn from_lut_text(text: &str) -> Result<Curve, String> {
        let mut curve = Curve::new();
        curve.load_lines(text)?;
        Ok(curve)
    }

    /// `Curve::load` @ 0x140206a50: replaces the table with the contents of a `.lut` file.
    /// A file that cannot be opened leaves the curve empty ("ERROR: Lut file not found").
    ///
    /// Every line is cut at `|` (empty pieces dropped, like `wcstok`); a line with exactly
    /// two pieces is a point, anything else is skipped. Both numbers are parsed like C
    /// `wcstod`, so trailing text such as a comment is ignored; a piece that does not start
    /// with a number makes the game throw, which is the `Err` here.
    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        if let Some(acd) = sibling_acd(path).filter(|acd| acd.is_file()) {
            return Err(format!(
                "{} exists: the game would read {} from that archive, which is not ported",
                acd.display(),
                path.display()
            ));
        }
        self.references.clear();
        self.values.clear();
        self.c_spline = OnceLock::new();
        let Ok(bytes) = std::fs::read(path) else {
            return Ok(());
        };
        // a default-locale wide stream: every byte is one character
        let text: String = text_mode(&bytes).iter().map(|&b| b as char).collect();
        self.load_lines(&text)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    fn load_lines(&mut self, text: &str) -> Result<(), String> {
        for line in text.split('\n') {
            let pieces: Vec<&str> = line.split('|').filter(|p| !p.is_empty()).collect();
            let [reference, value] = pieces[..] else {
                continue;
            };
            // the value is parsed first, like the original
            let value = wcstod(value);
            let reference = wcstod(reference);
            if value.consumed == 0 || reference.consumed == 0 {
                return Err(format!("lut line {line:?}: invalid stof argument"));
            }
            if value.out_of_range || reference.out_of_range {
                return Err(format!("lut line {line:?}: stof argument out of range"));
            }
            self.add_value(reference.value as f32, value.value as f32);
        }
        Ok(())
    }

    /// `Curve::scale` @ 0x1402073e0: multiplies every value (not the references).
    pub fn scale(&mut self, scale: f32) {
        self.c_spline = OnceLock::new();
        for value in &mut self.values {
            *value *= scale;
        }
    }

    /// `Curve::getPairAtIndex` @ 0x140206940: `(reference, value)`, or `(0, 0)` when the
    /// index is outside the table.
    pub fn get_pair_at_index(&self, index: i32) -> (f32, f32) {
        match usize::try_from(index) {
            Ok(i) if i < self.values.len() && i < self.references.len() => {
                (self.references[i], self.values[i])
            }
            _ => (0.0, 0.0),
        }
    }

    /// `references`: the x column.
    pub fn references(&self) -> &[f32] {
        &self.references
    }

    /// `values`: the y column.
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// `Curve::addValue` @ 0x140205ae0
    pub fn add_value(&mut self, reference: f32, value: f32) {
        self.c_spline = OnceLock::new();
        self.references.push(reference);
        self.values.push(value);
    }

    /// `Curve::getCount` @ 0x1402068e0 (the length of `values`)
    pub fn get_count(&self) -> i32 {
        self.values.len() as i32
    }

    /// `Curve::getValue` @ 0x140206990: linear interpolation, clamped at both ends.
    pub fn get_value(&self, r#ref: f32) -> f32 {
        let references = &self.references;
        if references.is_empty() {
            // AC prints "EMPTY CURVE, RETURNING ZERO (%S)" here
            return 0.0;
        }
        // `comiss ref, x; ja`: a NaN `ref` is "not above" and returns the first value
        if !(r#ref > references[0]) {
            return self.values[0];
        }
        let mut i = 1;
        loop {
            if i == references.len() {
                return self.values[self.values.len() - 1];
            }
            if !(r#ref > references[i]) {
                break;
            }
            i += 1;
        }
        let values = &self.values;
        (values[i] - values[i - 1]) * (r#ref - references[i - 1])
            / (references[i] - references[i - 1])
            + values[i - 1]
    }

    /// `Curve::getCubicSplineValue` @ 0x1402068f0
    pub fn get_cubic_spline_value(&self, r#ref: f32) -> f32 {
        let elements = self
            .c_spline
            .get_or_init(|| compute_coefficients(&self.references, &self.values));
        value_at(elements, r#ref)
    }
}

/// `CubicSpline<float,float>::computeCoefficients` @ 0x140205c20 (natural cubic spline).
/// Locals `h, l, u, z, a, c, b, d` carry their PDB names.
fn compute_coefficients(x: &[f32], y: &[f32]) -> Vec<Element> {
    if x.len() != y.len() {
        // "CubicSpline :: [ERROR] X and Y must be the same size"
        return Vec::new();
    }
    if x.len() < 3 {
        // "CubicSpline :: [ERROR] Must have at least three points for interpolation"
        return Vec::new();
    }
    let size = y.len();
    let n = size - 1;
    let mut b = vec![0.0f32; n];
    let mut d = vec![0.0f32; n];
    let mut a = vec![0.0f32; n];
    let mut c = vec![0.0f32; size];
    let mut l = vec![0.0f32; size];
    let mut u = vec![0.0f32; size];
    let mut z = vec![0.0f32; size];
    let mut h = vec![0.0f32; size];

    l[0] = 1.0;
    u[0] = 0.0;
    z[0] = 0.0;
    h[0] = x[1] - x[0];
    for i in 1..n {
        h[i] = x[i + 1] - x[i];
        l[i] = (x[i + 1] - x[i - 1]) * 2.0 - h[i - 1] * u[i - 1];
        u[i] = h[i] / l[i];
        a[i] = (y[i + 1] - y[i]) * (3.0 / h[i]) - (y[i] - y[i - 1]) * (3.0 / h[i - 1]);
        z[i] = (a[i] - h[i - 1] * z[i - 1]) / l[i];
    }
    l[n] = 1.0;
    c[n] = 0.0;
    z[n] = 0.0;
    for j in (0..n).rev() {
        c[j] = z[j] - u[j] * c[j + 1];
        // 0.33333334 is the f32 literal 0x3eaaaaab, multiplied last
        b[j] = (y[j + 1] - y[j]) / h[j] - ((c[j] * 2.0 + c[j + 1]) * h[j]) * 0.333_333_34;
        d[j] = (c[j + 1] - c[j]) / (h[j] * 3.0);
    }
    (0..n)
        .map(|i| Element {
            x: x[i],
            a: y[i],
            b: b[i],
            c: c[i],
            d: d[i],
        })
        .collect()
}

/// `CubicSpline<float,float>::valueAt` @ 0x140207410
fn value_at(elements: &[Element], x: f32) -> f32 {
    if elements.is_empty() {
        return 0.0;
    }
    // std::lower_bound: the first element whose x is not below `x` (`comiss x, e.x; jbe`)
    let mut first = 0usize;
    let mut count = elements.len();
    while count > 0 {
        let half = count / 2;
        if x > elements[first + half].x {
            first += half + 1;
            count -= half + 1;
        } else {
            count = half;
        }
    }
    // ... then one element back, unless already at the start
    if first != 0 {
        first -= 1;
    }
    let e = &elements[first];
    let xdiff = x - e.x;
    let sq = xdiff * xdiff;
    ((xdiff * e.b + e.a) + sq * e.c) + (sq * xdiff) * e.d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_lookup_clamps_and_interpolates() {
        let curve = Curve::from_pairs(&[(0.0, 1.0), (10.0, 3.0), (20.0, 2.0)]);
        assert_eq!(curve.get_count(), 3);
        assert_eq!(curve.get_value(-5.0), 1.0);
        assert_eq!(curve.get_value(5.0), 2.0);
        assert_eq!(curve.get_value(10.0), 3.0);
        assert_eq!(curve.get_value(99.0), 2.0);
        assert_eq!(Curve::new().get_value(1.0), 0.0);
    }

    #[test]
    fn spline_passes_through_its_points() {
        let curve = Curve::from_pairs(&[(0.0, 1.0), (10.0, 3.0), (20.0, 2.0), (40.0, 5.0)]);
        // only the first point is hit exactly: every other knot is evaluated from the end
        // of the piece before it
        assert_eq!(curve.get_cubic_spline_value(0.0), 1.0);
        for (x, y) in [(10.0, 3.0), (20.0, 2.0), (40.0, 5.0)] {
            assert!((curve.get_cubic_spline_value(x) - y).abs() < 1e-4, "at {x}");
        }
        // fewer than three points: AC leaves the spline empty and returns 0
        assert_eq!(
            Curve::from_pairs(&[(0.0, 1.0), (1.0, 2.0)]).get_cubic_spline_value(0.5),
            0.0
        );
    }
}
