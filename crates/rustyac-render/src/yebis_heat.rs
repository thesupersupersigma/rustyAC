// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).
//! The heat shimmer's draws, YEBIS's half (Task Y3d-3 part 2): `CameraForwardYebis`'s particle
//! array, `CTextureUtil::SetupHeatShimmerParticles` 0x1403fd860 (cull and sort) and
//! `DrawIndicesGPU_HeatShimmerParticles` 0x1403fc950 (variables and vertices), in the
//! disassembly's order. Matrices are row-major `[f32; 16]` as in `yebis_cb`.

use crate::heat_shimmer::HeatShimmer;
use crate::yebis_cb::{inverse, mul, PostEffect};
use rustyac_math::{asinf, atan2f, cosf, sinf, sqrtf, tanf};

/// `PFX_PARTICLE_HEATSHIMMER` (0x38 bytes): position, radius, intensity per corner, texture
/// scale, texture offset, depth falloff.
pub type Element = [f32; 14];

const IDENTITY: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

/// `CPostEffect`'s shimmer fields as `setPostProcessing` 0x140025080 sets them.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub radius: f32,       // +0xa54
    pub depth_scale: f32,  // +0xa58
    pub shimmer: [f32; 4], // +0xa5c
    pub coord: [f32; 2],   // +0xa6c
    pub offset: [f32; 3],  // +0xa74
    pub octaves: i32,      // +0xa80
    pub fractal: [f32; 2], // +0xa84
    pub falloff: [f32; 2], // +0xaa4 (near), +0xaa0 (far)
    pub color: [f32; 4],   // +0xa90
}

impl Params {
    pub fn new(h: &HeatShimmer) -> Params {
        Params {
            radius: h.radius,
            depth_scale: 1.0,
            shimmer: [h.shimmer; 4],
            coord: [h.coord; 2],
            offset: [0.0; 3],
            octaves: h.fractal_octaves as i32,
            fractal: [2.7, 0.28],
            falloff: [0.0, 3.0],
            color: [h.color[0], h.color[1], h.color[2], 1.0],
        }
    }
}

/// `setupPfxParticles` 0x140024bb0: every set's particles in order.
pub fn elements(h: &HeatShimmer) -> Vec<Element> {
    h.sets
        .iter()
        .flatten()
        .map(|p| {
            let i = p.intensity;
            [p.position[0], p.position[1], p.position[2], p.radius, i, i, i, i, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        })
        .collect()
}

/// `CPostEffect`+0x59c as `SetRenderSceneViewMatrix` 0x1403c2190 leaves it (after
/// `SetRenderSceneCameraMatrix`): the view matrix inverted, its handedness changed, inverted again.
pub fn view(view: &[f32; 16]) -> [f32; 16] {
    let mut m = inverse(view);
    for k in [2, 6, 8, 9, 11, 14] {
        m[k] = -m[k];
    }
    inverse(&m)
}

/// `GPUMath_MatrixFrustum` (left-handed, zero to one depth) over `SetRenderScenePerspective`'s
/// fields, as `UpdateParameters_EffectiveHeatShimmerParticleEnable` 0x1403ba2e0 builds it.
pub fn frustum(pe: &PostEffect) -> [f32; 16] {
    let t = tanf(pe.fov * 0.5);
    let (n, f) = (pe.near, pe.far);
    let fx = (t * pe.aspect) * n;
    let fy = n * t;
    let (l, r) = (0.0 * fx + -fx, 0.0 * fx + fx);
    let (b, top) = (-fy + 0.0 * fy, 0.0 * fy + fy);
    let mut m = [0.0; 16];
    m[0] = (n * 2.0) / (r - l);
    m[5] = (n * 2.0) / (top - b);
    m[8] = ((l + r) / (l - r)) * 1.0;
    m[9] = (b + top) / (b - top);
    m[10] = f / (f - n);
    m[11] = 1.0;
    m[14] = -((n * f) / (f - n));
    m
}

/// `Vec3Normalize` 0x1403aed20 on rows 0-2.
fn normalize_rows(m: &[f32; 16]) -> [f32; 16] {
    let mut o = *m;
    for r in 0..3 {
        let (x, y, z) = (m[r * 4], m[r * 4 + 1], m[r * 4 + 2]);
        let l2 = (x * x + y * y) + z * z;
        if l2 != 0.0 && l2 != 1.0 {
            let s = 1.0 / sqrtf(l2);
            o[r * 4] = x * s;
            o[r * 4 + 1] = s * y;
            o[r * 4 + 2] = s * z;
        }
    }
    o
}

/// `MatrixToFrustumParameters` 0x1403a2b30: (near, far).
fn near_far(m: &[f32; 16]) -> (f32, f32) {
    let f8 = if m[11] > 0.0 { m[10] } else { -m[10] };
    (-(m[14] / f8), -(m[14] / (f8 - 1.0)))
}

/// `SetupHeatShimmerParticles` 0x1403fd860: the particles in front of the camera, in view
/// space, farthest first; at most `max`.
pub fn setup(elements: &[Element], view: &[f32; 16], frustum: &[f32; 16], p: &Params, max: usize) -> Vec<Element> {
    if elements.is_empty() || p.shimmer.iter().all(|&s| s == 0.0) {
        return Vec::new();
    }
    let v = normalize_rows(view);
    let (near, far) = near_far(frustum);
    let lim = if p.falloff[0] <= near { near } else { p.falloff[0] };
    let mut out: Vec<Element> = Vec::new();
    for e in elements {
        // the array is made with the right-handed flag set: z is not negated
        let (x, y, z) = (e[0], e[1], e[2]);
        let vx = ((y * v[4] + x * v[0]) + z * v[8]) + v[12];
        let vy = ((x * v[1] + y * v[5]) + z * v[9]) + v[13];
        let vz = ((x * v[2] + y * v[6]) + z * v[10]) + v[14];
        if (e[13] > 0.0 && lim >= vz) || near >= vz || vz >= far {
            continue;
        }
        let r = p.radius * e[3];
        let mut a = vz * frustum[8] + frustum[0] * vx;
        if !(a > 0.0) {
            a = -a;
        }
        a -= frustum[0] * r;
        if a > vz {
            let mut b = vz * frustum[9] + frustum[5] * vy;
            if !(b > 0.0) {
                b = -b;
            }
            b -= frustum[5] * r;
            if b > vz {
                continue;
            }
        }
        let mut k = *e;
        k[..3].copy_from_slice(&[vx, vy, vz]);
        out.push(k);
        if out.len() >= max {
            break;
        }
    }
    // Callback_QSort_CompareParticleZ_S 0x1403fe1e0: farther first
    // shortcut: msvcr120's qsort is not stable, only particles at the same depth could come out in another order
    out.sort_by(|a, b| b[2].partial_cmp(&a[2]).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// What the frame hands YEBIS for the shimmer: `setupPfxParticles`' array, the camera's view
/// matrix (`pp.viewMatrix`, `Camera::getViewMatrix`, row-major) and the fields.
#[derive(Clone, Debug)]
pub struct Input {
    pub elements: Vec<Element>,
    pub view: [f32; 16],
    pub params: Params,
}

/// One frame's shimmer, both rounds: the kept particles and the matrices they were kept with.
#[derive(Clone, Debug)]
pub struct Shimmer {
    pub particles: Vec<Element>,
    pub view: [f32; 16],
    pub frustum: [f32; 16],
    pub params: Params,
}

impl Input {
    /// `UpdateParameters_EffectiveHeatShimmerParticleEnable` 0x1403ba2e0: none when no particle is kept.
    pub fn shimmer(&self, pe: &PostEffect, max: usize) -> Option<Shimmer> {
        let view = view(&self.view);
        let frustum = frustum(pe);
        let particles = setup(&self.elements, &view, &frustum, &self.params, max);
        (!particles.is_empty()).then_some(Shimmer { particles, view, frustum, params: self.params })
    }
}

/// `MatrixRotationAxis` 0x1403aeb90 about a unit axis.
fn rot_axis([x, y, z]: [f32; 3], a: f32) -> [f32; 16] {
    let (c, s) = (cosf(a), sinf(a));
    let t = 1.0 - c;
    let mut m = [0.0; 16];
    m[0] = (1.0 - x * x) * c + x * x;
    m[5] = (1.0 - y * y) * c + y * y;
    m[10] = (1.0 - z * z) * c + z * z;
    let xz = (x * z) * t;
    let yx = (y * x) * t;
    let yz = (y * z) * t;
    m[1] = z * s + yx;
    m[4] = yx - z * s;
    m[2] = xz - y * s;
    m[8] = y * s + xz;
    m[9] = yz - x * s;
    m[6] = x * s + yz;
    m[15] = 1.0;
    m
}

/// `MatrixToYawPitchRoll` 0x1403a2580: (yaw, pitch, roll).
fn yaw_pitch_roll(m: &[f32; 16]) -> (f32, f32, f32) {
    use std::f32::consts::PI;
    let mut roll = atan2f(m[1], m[5]);
    let pitch = asinf(-m[9]);
    let mut yaw = atan2f(m[8], m[10]);
    if cosf(pitch).abs() < 1e-6 {
        roll += if m[1] <= 0.0 { -PI } else { PI };
        yaw += if m[8] <= 0.0 { -PI } else { PI };
    }
    (yaw, pitch, roll)
}

/// One shimmer draw: its technique, variables and vertices (11 floats each, 4 per particle).
#[derive(Clone, Debug)]
pub struct Draw {
    pub technique: usize,
    pub transform: [[f32; 16]; 2], // am44_TransformMatrix
    pub offset: [f32; 4],          // afUV_TexCoordOffsetP32
    pub matrix: [f32; 16],         // m44_ColorTransformMatrix
    pub modulate: [f32; 8],        // afRGBA_Modulate
    pub vertices: Vec<f32>,
}

/// `DrawIndicesGPU_HeatShimmerParticles` 0x1403fc950 as `HeatShimmerParticles` 0x1403fdc70
/// calls it: `size` is the destination's, `uv` the source's used corner, `depth` the
/// `GetDepthConversionFactor` pair, `view` the raw +0x59c (normalized twice: by the caller and here).
pub fn draw(particles: &[Element], view: &[f32; 16], proj: &[f32; 16], depth: [f32; 2], size: [f32; 2], uv: [f32; 2], p: &Params) -> Draw {
    let (p5, p6, p7, p8) = (0.0f32, 0.0f32, uv[0], uv[1]);
    let camera = inverse(&normalize_rows(&normalize_rows(view)));
    let lx = (p7 - p5) * 0.5;
    let ox = p5 - lx * -1.0;
    let ly = (p8 - p6) * -0.5;
    let oy = p6 - ly;
    let mut st = IDENTITY;
    st[0] = lx;
    st[5] = ly;
    st[12] = ox;
    st[13] = oy;
    st[14] = 0.0;
    let transform = [*proj, mul(proj, &st)];
    let (n, f) = near_far(proj);
    let aspect = proj[5] * (1.0 / proj[0]);
    let lim = if p.falloff[0] <= n { n } else { p.falloff[0] };
    let fa = (((lim - n) * f) / (f - n)) / lim;
    let fb = if p.falloff[1] > 0.0 { (((p.falloff[1] - n) * f) / (f - n)) / p.falloff[1] } else { f / (f - n) };
    let l440 = 1.0 / (fb - fa);
    let l428 = 0.0 - l440 * fa;
    let offset = [depth[0], depth[1], p.fractal[0], p.fractal[1]];
    let (_, pitch, roll) = yaw_pitch_roll(&camera);
    let rot = {
        let y = rot_axis([0.0, 1.0, 0.0], 0.0);
        let x = rot_axis([1.0, 0.0, 0.0], pitch);
        let z = rot_axis([0.0, 0.0, 1.0], roll);
        mul(&mul(&z, &x), &y)
    };
    let mut s = pitch / 1.5707964;
    if !(s > 0.0) {
        s = -s;
    }
    s = s.clamp(0.0, 1.0);
    let s = 1.0 - s;
    let d4 = |a: f32, b: f32| {
        let mut m = IDENTITY;
        m[0] = a;
        m[5] = b;
        m
    };
    let (tx, ty) = (1.0 / proj[0], 1.0 / proj[5]);
    let mut matrix = mul(&IDENTITY, &IDENTITY);
    matrix = mul(&matrix, &rot);
    matrix = mul(&matrix, &d4(s, s));
    matrix = mul(&matrix, &d4(1.0 / tx, 1.0 / ty));
    matrix = mul(&matrix, &d4((lx * 2.0) * aspect, ly * 2.0));
    // the blend program (+0xa8c) is never set by AC
    let sh = (((p8 - p6).abs() / size[1]) * 0.5) * p.color[3];
    let modulate = [0.01, sh, p.fractal[0], p.fractal[1], 1.0 * p.color[0], 1.0 * p.color[1], 1.0 * p.color[2], 1.0];

    let (sn, cs) = (sinf(-roll), cosf(-roll));
    let r = p.radius;
    let corners = [(-r, r, 0.0, 1.0), (r, r, 1.0, 1.0), (-r, -r, 0.0, 0.0), (r, -r, 1.0, 0.0)];
    let depth01 = |z: f32| {
        let a = (((z - n) * f) / (f - n)) / z;
        a.clamp(0.0, 1.0)
    };
    let mut vertices = Vec::with_capacity(particles.len() * 44);
    for e in particles {
        let a = depth01(e[2]);
        let b = depth01(e[2] - (1.0 * p.depth_scale) * e[3]);
        let mut c = l440 * b + l428;
        c = c.clamp(0.0, 1.0);
        let d = -(a - b);
        let f25 = 1.0 / (0.0 - d);
        let v10 = 0.0 - d * f25;
        let om = 1.0 - e[13];
        let k = (1.0 - om) * c + om;
        let ma = d4(e[8] * p.coord[0], e[9] * p.coord[1]);
        let mut mb = IDENTITY;
        mb[12..15].copy_from_slice(&[e[10] + p.offset[0], e[11] + p.offset[1], e[12] + p.offset[2]]);
        let m = mul(&mul(&ma, &mb), &IDENTITY);
        for (j, &(cx, cy, u, v)) in corners.iter().enumerate() {
            let x = (cx * cs - cy * sn) * e[3] + e[0];
            let y = (cy * cs + cx * sn) * e[3] + e[1];
            let tu = (u * m[0] + v * m[4]) + m[12];
            let tv = (v * m[5] + u * m[1]) + m[13];
            let w = (e[4 + j] * p.shimmer[j]) * k;
            vertices.extend_from_slice(&[x, y, e[2], u, v, tu, tv, w, 1.0, f25, v10]);
        }
    }
    Draw { technique: 12 + p.octaves.clamp(1, 4) as usize, transform, offset, matrix, modulate, vertices }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn farthest_first_and_behind_culled() {
        let p = Params { radius: 0.75, depth_scale: 1.0, shimmer: [0.004; 4], coord: [1.5; 2], offset: [0.0; 3], octaves: 3, fractal: [2.7, 0.28], falloff: [0.0, 3.0], color: [1.0; 4] };
        let mut proj = IDENTITY;
        proj[0] = 0.9742984;
        proj[5] = 1.7320861;
        proj[10] = 1.001001;
        proj[11] = 1.0;
        proj[14] = -1.001001;
        proj[15] = 0.0;
        let at = |z: f32| [0.0, 0.0, z, 0.5, 0.2, 0.2, 0.2, 0.2, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let kept = setup(&[at(5.0), at(-5.0), at(9.0)], &IDENTITY, &proj, &p, 10);
        assert_eq!(kept.iter().map(|e| e[2]).collect::<Vec<_>>(), vec![9.0, 5.0]);
        let d = draw(&kept, &IDENTITY, &proj, [0.0; 2], [1280.0, 720.0], [1.0, 1.0], &p);
        assert_eq!((d.technique, d.vertices.len()), (15, 2 * 44));
        assert_eq!(d.modulate[1], 0.00069444446);
    }
}
