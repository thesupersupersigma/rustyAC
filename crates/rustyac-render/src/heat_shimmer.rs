// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).
//! The heat shimmer's particles, Kunos' half (Task Y3d-3): `KGLPostProcessing`'s heat fields,
//! `KGLYebisParticleManager` and its sets, moved by `PostProcessEffectsUpdater::update`
//! 0x1400abc20 and `CameraForwardYebis::setPostProcessing` 0x140025080.

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::session::MsvcRand;
use rustyac_physics::vecmath::Mat44f;

/// `KGLVolume` (0x18 bytes).
#[derive(Clone, Copy, Debug, Default)]
pub struct Volume {
    pub center: [f32; 3],
    pub width: f32,
    pub depth: f32,
    pub height: f32,
}

/// `KGLYebisParticle` (0x2c bytes).
#[derive(Clone, Copy, Debug, Default)]
pub struct Particle {
    pub position: [f32; 3],
    pub radius: f32,
    pub intensity: f32,
    pub position_velocity: [f32; 3],
    pub radius_velocity: f32,
    pub intensity_velocity: f32,
    pub life: f32,
}

/// `KGLPostProcessing`'s heat fields (+0x40..+0x3dc) and the particle manager.
#[derive(Clone, Debug, Default)]
pub struct HeatShimmer {
    pub active: bool,
    pub radius: f32,
    pub shimmer: f32,
    pub coord: f32,
    pub fractal_octaves: f32,
    pub color: [f32; 3],
    pub max_number_per_set: i32,
    pub sets_number: usize,
    pub min_life: f32,
    pub life_mult: f32,
    pub intensity_mult: f32,
    pub min_intensity: f32,
    pub intensity_velocity_mult: f32,
    pub intensity_min_velocity: f32,
    pub position_velocity: [f32; 3],
    pub radius_velocity_mult: f32,
    /// `heatParticleSets[30]` (`KGLPostProcessingParticleSet`: active, volume)
    pub volumes: [(i32, Volume); 30],
    /// the manager's sets
    pub sets: Vec<Vec<Particle>>,
}

impl HeatShimmer {
    /// The ctor's memset and `heatParticleSetsNumber` (Sim::createCamera: race.ini [RACE] CARS).
    pub fn new(sets_number: usize) -> HeatShimmer {
        HeatShimmer { sets_number, ..Default::default() }
    }

    /// `readPPSetOptions` 0x140026150's [HEAT_SHIMMER] part (fields stay as they were without it).
    pub fn read(&mut self, filter: &IniReader) -> Result<(), String> {
        const S: &str = "HEAT_SHIMMER";
        if !filter.has_section(S) {
            return Ok(());
        }
        self.active = true;
        self.radius = filter.get_float(S, "HEAT_PARTICLE_RADIUS")?;
        self.shimmer = filter.get_float(S, "HEAT_PARTICLE_SHIMMER")?;
        self.coord = filter.get_float(S, "HEAT_PARTICLE_COORD")?;
        self.fractal_octaves = filter.get_float(S, "HEAT_PARTICLE_FRACTAL_OCTAVES")?;
        self.color = filter.get_float3(S, "HEAT_PARTICLE_COLOR")?;
        self.max_number_per_set = filter.get_int(S, "HEAT_PARTICLE_MAX_NUMBER")?;
        self.life_mult = filter.get_float(S, "HEAT_PARTICLE_LIFE_MULT")?;
        self.min_life = filter.get_float(S, "HEAT_PARTICLE_MIN_LIFE")?;
        self.intensity_mult = filter.get_float(S, "HEAT_PARTICLE_INTENSITY_MULT")?;
        self.min_intensity = filter.get_float(S, "HEAT_PARTICLE_MIN_INTENSITY")?;
        self.intensity_velocity_mult = filter.get_float(S, "HEAT_PARTICLE_VELOCITY_MULT")?;
        self.intensity_min_velocity = filter.get_float(S, "HEAT_PARTICLE_MIN_VELOCITY")?;
        self.radius_velocity_mult = filter.get_float(S, "HEAT_PARTICLE_RADIUS_VELOCITY_MULT")?;
        self.position_velocity = filter.get_float3(S, "HEAT_PARTICLE_POSITION_VELOCITY")?;
        Ok(())
    }

    /// `initPostProcessing` 0x140024ca0: a new `KGLYebisParticleManager` 0x140024a50, each set
    /// (`KGLYebisParticleSet` 0x1400247f0) initialising its particles from its volume.
    pub fn remake(&mut self, rand: &mut MsvcRand) {
        // shortcut: the game reads past heatParticleSets[30] with more cars, here they share the last volume
        self.sets = (0..self.sets_number).map(|i| (0..self.max_number_per_set).map(|_| self.initialize(&self.volumes[i.min(29)].1, rand)).collect()).collect();
    }

    /// `KGLYebisParticle::initialize` 0x140024600, in the disassembly's order.
    fn initialize(&self, v: &Volume, rand: &mut MsvcRand) -> Particle {
        let unit = |rand: &mut MsvcRand| rand.next() as f32 / 32767.0;
        let mut p = Particle { radius: (v.width * 0.5 * v.width * 0.5 + v.height * 0.5 * v.height * 0.5 + v.depth * 0.5 * v.depth * 0.5).sqrt(), ..Default::default() };
        p.radius_velocity = unit(rand) * self.radius_velocity_mult + 0.1;
        p.position[0] = (unit(rand) * v.width + v.center[0]) - v.width * 0.5;
        p.position[1] = (unit(rand) * v.height + v.center[1]) - v.height * 0.5;
        p.position[2] = (unit(rand) * v.depth - v.center[2]) - v.depth * 0.5;
        p.position_velocity = [self.position_velocity[0], self.position_velocity[2], self.position_velocity[1]];
        p.life = unit(rand) * self.life_mult;
        if self.min_life > p.life {
            p.life = self.min_life;
        }
        p.intensity = unit(rand) * self.intensity_mult;
        if self.min_intensity > p.intensity {
            p.intensity = self.min_intensity;
        }
        p.intensity_velocity = self.intensity_velocity_mult * p.intensity;
        if self.intensity_min_velocity > p.intensity_velocity {
            p.intensity_velocity = self.intensity_min_velocity;
        }
        p
    }

    /// `PostProcessEffectsUpdater::update` 0x1400abc20: off above 25 km/h of the focused car
    /// (`speed` in m/s), each car's volume 2 m behind its body. `bodies` are the cars' body world
    /// matrices in Sim's order (all connected: the game stops at the first one that is not).
    pub fn update_cars(&mut self, speed: f32, bodies: &[Mat44f]) {
        self.active = speed * f32::from_bits(0x4066_6666) <= 25.0;
        for (i, b) in bodies.iter().enumerate() {
            let m = &b.m;
            let x = ((m[1][0] * 0.0 + m[0][0] * 0.0) + m[2][0] * -2.0) + m[3][0];
            let y = ((m[1][1] * 0.0 + m[0][1] * 0.0) + m[2][1] * -2.0) + m[3][1];
            let z = ((m[1][2] * 0.0 + m[0][2] * 0.0) + m[2][2] * -2.0) + m[3][2];
            let inv = 1.0 / (((m[1][3] * 0.0 + m[0][3] * 0.0) + m[2][3] * -2.0) + m[3][3]);
            if self.active && i < 30 {
                self.volumes[i] = (1, Volume { center: [inv * x, inv * y + 0.5, inv * z], width: 1.5, depth: 1.5, height: 0.0 });
            }
        }
    }

    /// `setPostProcessing`'s particle part: `KGLYebisParticleSet::update` 0x140024950 for each
    /// active set, while the shimmer is active.
    pub fn update(&mut self, dt: f32, rand: &mut MsvcRand) {
        if !self.active {
            return;
        }
        let mut sets = std::mem::take(&mut self.sets);
        for (i, set) in sets.iter_mut().enumerate() {
            let (on, v) = self.volumes[i.min(29)];
            if on == 0 {
                continue;
            }
            for p in set.iter_mut() {
                let velocity = p.position_velocity;
                for (x, v) in p.position.iter_mut().zip(velocity) {
                    *x += dt * v;
                }
                p.radius += dt * p.radius_velocity;
                p.intensity += dt * p.intensity_velocity;
                p.life -= dt;
                if p.life <= 0.0 {
                    *p = self.initialize(&v, rand);
                }
            }
        }
        self.sets = sets;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_moves_and_reinitialises() {
        let mut h = HeatShimmer { max_number_per_set: 1, life_mult: 0.0, min_life: 0.02, ..HeatShimmer::new(1) };
        h.remake(&mut MsvcRand(22));
        assert_eq!(h.sets[0][0].position, [0.0; 3]);
        h.update_cars(0.0, &[Mat44f { m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [10.0, 0.0, 0.0, 1.0]] }]);
        let mut rand = MsvcRand(1);
        h.update(1.0 / 60.0, &mut rand);
        assert_eq!(rand, MsvcRand(1));
        // life 0.02 runs out on the second step: back inside the 1.5 m volume 2 m behind the car
        h.update(1.0 / 60.0, &mut rand);
        assert_ne!(rand, MsvcRand(1));
        let p = h.sets[0][0];
        assert!((9.25..=10.75).contains(&p.position[0]) && p.position[1] == 0.5 && (1.25..=2.75).contains(&p.position[2]), "{p:?}");
    }
}
