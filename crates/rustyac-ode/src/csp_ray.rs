// SPDX-License-Identifier: BSD-3-Clause

//! CSP's tyre rays (Task C5h): its `cphys::ode_impl_raycaster` on Embree 3.13.4, linked as CSP
//! links it (feature `embree`, native only; `build.rs` builds the library). The scene is
//! `+0x9223e0`'s, a ray is the Embree half of `+0x932900`; the ODE half is ODE's ray (part 2).

use std::ffi::{c_void, CStr};

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub org: [f32; 3],
    pub tnear: f32,
    pub dir: [f32; 3],
    pub time: f32,
    pub tfar: f32,
    pub mask: u32,
    pub id: u32,
    pub flags: u32,
    pub ng: [f32; 3],
    pub u: f32,
    pub v: f32,
    pub prim_id: u32,
    pub geom_id: u32,
    pub inst_id: u32,
}

#[repr(C)]
struct Context {
    flags: u32,
    filter: *const c_void,
    inst_id: u32,
}

type Handle = *mut c_void;

#[link(name = "embree3", kind = "static")]
extern "C" {
    fn rtcNewDevice(config: *const i8) -> Handle;
    fn rtcGetDeviceError(device: Handle) -> i32;
    fn rtcReleaseDevice(device: Handle);
    fn rtcNewScene(device: Handle) -> Handle;
    fn rtcSetSceneBuildQuality(scene: Handle, quality: i32);
    fn rtcSetSceneFlags(scene: Handle, flags: i32);
    fn rtcNewGeometry(device: Handle, kind: i32) -> Handle;
    fn rtcSetNewGeometryBuffer(geom: Handle, kind: i32, slot: u32, format: i32, stride: usize, count: usize) -> *mut c_void;
    fn rtcSetGeometryBuildQuality(geom: Handle, quality: i32);
    fn rtcCommitGeometry(geom: Handle);
    fn rtcAttachGeometryByID(scene: Handle, geom: Handle, id: u32);
    fn rtcReleaseGeometry(geom: Handle);
    fn rtcCommitScene(scene: Handle);
    fn rtcReleaseScene(scene: Handle);
    fn rtcIntersect1(scene: Handle, context: *mut Context, rayhit: *mut RayHit);
}

/// A track mesh as CSP's mesh object holds it (`+0x91d8b0`'s block): float vertices, u16
/// indices, and the flag (`+40`, vfunc +24) that turns a normal against the ray.
pub struct TraceMesh<'a> {
    pub vertices: &'a [[f32; 3]],
    pub indices: &'a [u16],
    pub two_sided: bool,
}

/// What `+0x932900`'s Embree half hands back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cast {
    /// out +32 = 0, +24 = null
    Miss,
    /// out +0 point, +12 normal, +24 the mesh at this index of CSP's list, +32 = 1
    Hit { point: [f32; 3], normal: [f32; 3], mesh: u32 },
    /// a downward ray on an ODE ray geom hit a face from below: ODE's ray decides
    Ode,
}

pub struct Scene {
    device: Handle,
    scene: Handle,
    flip: Vec<bool>,
}

// Embree's scene is read-only once committed; rtcIntersect1 is thread-safe
unsafe impl Send for Scene {}

impl Scene {
    /// `+0x9223e0`: a device ("threads=1", CSP's string; `max_isa=` added to pin the ISA), quality
    /// HIGH, ROBUST when CSP's byte says so, one triangle geometry per non-empty mesh with its
    /// list index as id (`+0x922800`).
    pub fn new(config: &CStr, robust: bool, meshes: &[Option<TraceMesh>]) -> Result<Scene, String> {
        unsafe {
            let device = rtcNewDevice(config.as_ptr());
            if device.is_null() {
                return Err(format!("rtcNewDevice({config:?}): error {}", rtcGetDeviceError(std::ptr::null_mut())));
            }
            let scene = rtcNewScene(device);
            rtcSetSceneBuildQuality(scene, 2);
            if robust {
                rtcSetSceneFlags(scene, 4);
            }
            let mut flip = vec![false; meshes.len()];
            for (i, m) in meshes.iter().enumerate() {
                let Some(m) = m else { continue };
                flip[i] = m.two_sided;
                if m.vertices.is_empty() || m.indices.len() < 3 {
                    continue;
                }
                let geom = rtcNewGeometry(device, 0);
                let vb = rtcSetNewGeometryBuffer(geom, 1, 0, 0x9003, 12, m.vertices.len()) as *mut [f32; 3];
                std::ptr::copy_nonoverlapping(m.vertices.as_ptr(), vb, m.vertices.len());
                let tris = m.indices.len() / 3;
                let ib = rtcSetNewGeometryBuffer(geom, 0, 0, 0x5003, 12, tris) as *mut u32;
                for k in 0..tris * 3 {
                    *ib.add(k) = u32::from(m.indices[k]);
                }
                rtcSetGeometryBuildQuality(geom, 2);
                rtcCommitGeometry(geom);
                rtcAttachGeometryByID(scene, geom, i as u32);
                rtcReleaseGeometry(geom);
            }
            rtcCommitScene(scene);
            match rtcGetDeviceError(device) {
                0 => Ok(Scene { device, scene, flip }),
                e => Err(format!("Embree error {e} building the scene")),
            }
        }
    }

    /// `rtcIntersect1` on the ray `+0x932900` fills: tnear and time 0, the context's flags 0.
    pub fn intersect(&self, org: [f32; 3], dir: [f32; 3], length: f32) -> RayHit {
        let mut rh = RayHit { org, tnear: 0.0, dir, time: 0.0, tfar: length, mask: u32::MAX, id: 0, flags: 0, ng: [0.0; 3], u: 0.0, v: 0.0, prim_id: 0, geom_id: u32::MAX, inst_id: u32::MAX };
        let mut context = Context { flags: 0, filter: std::ptr::null(), inst_id: u32::MAX };
        unsafe { rtcIntersect1(self.scene, &mut context, &mut rh) };
        rh
    }

    /// `+0x932900`'s Embree half. `ode_ray`: the call came with an ODE ray geom (the tyre's);
    /// `below`: CSP's byte `+0x2812c7f`, set lets a downward ray take a hit from below.
    pub fn cast(&self, org: [f32; 3], dir: [f32; 3], length: f32, ode_ray: bool, below: bool) -> Cast {
        let down = dir[1] == -1.0 && ode_ray && !below;
        let rh = self.intersect(org, dir, length);
        if rh.geom_id == u32::MAX {
            return Cast::Miss;
        }
        if down && !(rh.ng[1] >= 0.0) {
            return Cast::Ode;
        }
        let t = rh.tfar;
        let point = [org[0] + dir[0] * t, org[1] + dir[1] * t, org[2] + dir[2] * t];
        let [x, y, z] = rh.ng;
        let inv = 1.0 / ((x * x + y * y) + z * z).sqrt();
        let mut normal = [x * inv, inv * y, inv * z];
        // +0x922b80
        if self.flip[rh.geom_id as usize] && (normal[1] * dir[1] + normal[0] * dir[0]) + normal[2] * dir[2] > 0.0 {
            normal = [-normal[0], -normal[1], -normal[2]];
        }
        Cast::Hit { point, normal, mesh: rh.geom_id }
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        unsafe {
            rtcReleaseScene(self.scene);
            rtcReleaseDevice(self.device);
        }
    }
}
