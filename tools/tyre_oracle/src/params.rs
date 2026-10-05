//! Compares what the game's own `Tyre::init` loaded from a tyres.ini with what the Rust
//! loader (`data::tyres_ini::init_compounds`) produced, member by member, bit by bit.

use rustyac_physics::curve::Curve;
use rustyac_physics::tyre::VanillaTyre;

use crate::game::{GameTyre, T_COMPOUND_DEFS};

unsafe fn rd<T: Copy>(base: *const u8, offset: usize) -> T {
    base.add(offset).cast::<T>().read_unaligned()
}

/// A `std::vector<float>` at `base + offset`.
unsafe fn float_vector(base: *const u8, offset: usize) -> Vec<u32> {
    let begin: *const u32 = rd(base, offset);
    let end: *const u32 = rd(base, offset + 8);
    (0..end.offset_from(begin) as usize)
        .map(|i| begin.add(i).read_unaligned())
        .collect()
}

/// A `std::wstring` at `base + offset`.
unsafe fn wstring(base: *const u8, offset: usize) -> String {
    let size: usize = rd(base, offset + 0x10);
    let capacity: usize = rd(base, offset + 0x18);
    let data: *const u16 = if capacity >= 8 {
        rd(base, offset)
    } else {
        base.add(offset).cast()
    };
    String::from_utf16_lossy(std::slice::from_raw_parts(data, size))
}

pub struct Comparison {
    pub compared: usize,
    pub differences: Vec<String>,
}

impl Comparison {
    fn bits(&mut self, name: &str, game: u32, port: u32) {
        self.compared += 1;
        if game != port {
            self.differences.push(format!(
                "{name}: AC {:?} ({game:#010x}), port {:?} ({port:#010x})",
                f32::from_bits(game),
                f32::from_bits(port)
            ));
        }
    }

    fn float(&mut self, name: &str, base: *const u8, offset: usize, port: f32) {
        self.bits(name, unsafe { rd(base, offset) }, port.to_bits());
    }

    fn int(&mut self, name: &str, game: i64, port: i64) {
        self.compared += 1;
        if game != port {
            self.differences
                .push(format!("{name}: AC {game}, port {port}"));
        }
    }

    fn text(&mut self, name: &str, game: &str, port: &str) {
        self.compared += 1;
        if game != port {
            self.differences
                .push(format!("{name}: AC {game:?}, port {port:?}"));
        }
    }

    /// AC's `Curve` at `base + offset`: `references` at +0x8, `values` at +0x20.
    fn curve(&mut self, name: &str, base: *const u8, offset: usize, port: &Curve) {
        let (references, values) = unsafe {
            (
                float_vector(base, offset + 0x8),
                float_vector(base, offset + 0x20),
            )
        };
        let port_references: Vec<u32> = port.references().iter().map(|x| x.to_bits()).collect();
        let port_values: Vec<u32> = port.values().iter().map(|x| x.to_bits()).collect();
        self.compared += references.len() + values.len() + 1;
        if references != port_references || values != port_values {
            self.differences.push(format!(
                "{name}: AC has {} points, port {} (or their bits differ)",
                values.len(),
                port_values.len()
            ));
        }
    }
}

pub fn compare(game: &GameTyre, port: &VanillaTyre) -> Comparison {
    let mut c = Comparison {
        compared: 0,
        differences: Vec::new(),
    };
    let tyre = game.ptr as *const u8;
    c.float(
        "explosionTemperature",
        tyre,
        0x848,
        port.explosion_temperature,
    );
    c.float("blanketTemperature", tyre, 0x84c, port.blanket_temperature);
    c.float(
        "pressureTemperatureGain",
        tyre,
        0x850,
        port.pressure_temperature_gain,
    );
    c.int(
        "useLoadForVKM",
        unsafe { rd::<u8>(tyre, 0x5a0) } as i64,
        port.use_load_for_vkm as i64,
    );
    c.float(
        "thermalModel.camberSpreadK",
        tyre,
        0x420 + 0xcc,
        port.thermal_model.camber_spread_k,
    );
    c.int(
        "compoundDefs.size()",
        game.compound_count() as i64,
        port.compound_defs.len() as i64,
    );

    let defs: *const u8 = unsafe { rd(tyre, T_COMPOUND_DEFS) };
    for (k, def) in port
        .compound_defs
        .iter()
        .enumerate()
        .take(game.compound_count())
    {
        let base = unsafe { defs.add(k * 0x3f0) };
        let n = |member: &str| format!("def{k}.{member}");
        c.int(
            &n("index"),
            unsafe { rd::<u32>(base, 0) } as i64,
            def.index as i64,
        );
        c.text(&n("name"), &unsafe { wstring(base, 0x8) }, &def.name);
        c.text(
            &n("shortName"),
            &unsafe { wstring(base, 0x28) },
            &def.short_name,
        );

        let m = &def.model_data;
        let model = unsafe { base.add(0x48) };
        c.int(
            &n("modelData.version"),
            unsafe { rd::<i32>(model, 0) } as i64,
            m.version as i64,
        );
        for (member, offset, value) in [
            ("Dy0", 0x4, m.dy0),
            ("Dy1", 0x8, m.dy1),
            ("Dx0", 0xc, m.dx0),
            ("Dx1", 0x10, m.dx1),
            ("Fz0", 0x14, m.fz0),
            ("flexK", 0x18, m.flex_k),
            ("speedSensitivity", 0x1c, m.speed_sensitivity),
            ("relaxationLength", 0x20, m.relaxation_length),
            ("rr0", 0x24, m.rr0),
            ("rr1", 0x28, m.rr1),
            ("rr_sa", 0x2c, m.rr_sa),
            ("rr_sr", 0x30, m.rr_sr),
            ("rr_slip", 0x34, m.rr_slip),
            ("camberGain", 0x38, m.camber_gain),
            ("pressureSpringGain", 0x3c, m.pressure_spring_gain),
            ("pressureFlexGain", 0x40, m.pressure_flex_gain),
            ("pressureRRGain", 0x44, m.pressure_rr_gain),
            ("pressureGainD", 0x48, m.pressure_gain_d),
            ("idealPressure", 0x4c, m.ideal_pressure),
            ("pressureRef", 0x50, m.pressure_ref),
            ("dcamber0", 0xd8, m.dcamber0),
            ("dcamber1", 0xdc, m.dcamber1),
            ("lsMultY", 0x1e0, m.ls_mult_y),
            ("lsExpY", 0x1e4, m.ls_exp_y),
            ("lsMultX", 0x1e8, m.ls_mult_x),
            ("lsExpX", 0x1ec, m.ls_exp_x),
            ("maxWearKM", 0x1f0, m.max_wear_km),
            ("maxWearMult", 0x1f4, m.max_wear_mult),
            ("asy", 0x1f8, m.asy),
            ("cfXmult", 0x1fc, m.cf_x_mult),
            ("brakeDXMod", 0x200, m.brake_dx_mod),
            ("combinedFactor", 0x28c, m.combined_factor),
        ] {
            c.float(&n(&format!("modelData.{member}")), model, offset, value);
        }
        c.int(
            &n("modelData.useSmoothDCamberCurve"),
            unsafe { rd::<u8>(model, 0x288) } as i64,
            m.use_smooth_d_camber_curve as i64,
        );
        c.curve(&n("modelData.wearCurve"), model, 0x58, &m.wear_curve);
        c.curve(&n("modelData.dyLoadCurve"), model, 0xe0, &m.dy_load_curve);
        c.curve(&n("modelData.dxLoadCurve"), model, 0x160, &m.dx_load_curve);
        c.curve(
            &n("modelData.dCamberCurve"),
            model,
            0x208,
            &m.d_camber_curve,
        );

        let d = &def.data;
        let data = unsafe { base.add(0x2d8) };
        for (i, (member, value)) in [
            ("width", d.width),
            ("radius", d.radius),
            ("k", d.k),
            ("d", d.d),
            ("angularInertia", d.angular_inertia),
            ("thermalFrictionK", d.thermal_friction_k),
            ("thermalRollingK", d.thermal_rolling_k),
            ("thermalRollingSurfaceK", d.thermal_rolling_surface_k),
            ("grainThreshold", d.grain_threshold),
            ("blisterThreshold", d.blister_threshold),
            ("grainGamma", d.grain_gamma),
            ("blisterGamma", d.blister_gamma),
            ("grainGain", d.grain_gain),
            ("blisterGain", d.blister_gain),
            ("rimRadius", d.rim_radius),
            ("optimumTemp", d.optimum_temp),
            ("softnessIndex", d.softness_index),
            ("radiusRaiseK", d.radius_raise_k),
        ]
        .into_iter()
        .enumerate()
        {
            c.float(&n(&format!("data.{member}")), data, i * 4, value);
        }

        let s = &def.slip_provider;
        let slip = unsafe { base.add(0x320) };
        c.float(&n("slipProvider.brushModel.data.Fz0"), slip, 0x14, s.fz0);
        c.float(
            &n("slipProvider.brushModel.data.maxSlip0"),
            slip,
            0x18,
            s.max_slip0,
        );
        c.float(
            &n("slipProvider.brushModel.data.maxSlip1"),
            slip,
            0x1c,
            s.max_slip1,
        );
        c.float(
            &n("slipProvider.brushModel.data.falloffSpeed"),
            slip,
            0x20,
            s.falloff_speed,
        );
        c.float(&n("slipProvider.asy"), slip, 0x24, s.asy);
        c.int(
            &n("slipProvider.version"),
            unsafe { rd::<i32>(slip, 0x28) } as i64,
            s.version as i64,
        );

        c.float(&n("pressureStatic"), base, 0x358, def.pressure_static);
        let t = &def.thermal_patch_data;
        for (i, (member, value)) in [
            ("surfaceTransfer", t.surface_transfer),
            ("patchTransfer", t.patch_transfer),
            ("patchCoreTransfer", t.patch_core_transfer),
            ("internalCoreTransfer", t.internal_core_transfer),
            ("coolFactorGain", t.cool_factor_gain),
        ]
        .into_iter()
        .enumerate()
        {
            c.float(
                &n(&format!("thermalPatchData.{member}")),
                base,
                0x35c + i * 4,
                value,
            );
        }
        c.curve(
            &n("thermalPerformanceCurve"),
            base,
            0x370,
            &def.thermal_performance_curve,
        );
    }

    // the thermal patches' neighbour lists, in order
    let patches: *const u8 = unsafe { rd(tyre, 0x420 + 0x8) };
    let patches_end: *const u8 = unsafe { rd(tyre, 0x420 + 0x10) };
    let count = unsafe { patches_end.offset_from(patches) } as usize / 0x28;
    c.int(
        "thermalModel.patches.size()",
        count as i64,
        port.thermal_model.patches.len() as i64,
    );
    for (i, patch) in port.thermal_model.patches.iter().enumerate().take(count) {
        let base = unsafe { patches.add(i * 0x28) };
        let begin: *const *const u8 = unsafe { rd(base, 0) };
        let end: *const *const u8 = unsafe { rd(base, 8) };
        let connections: Vec<usize> = (0..unsafe { end.offset_from(begin) } as usize)
            .map(|j| unsafe { begin.add(j).read().offset_from(patches) } as usize / 0x28)
            .collect();
        c.compared += 1;
        if connections != patch.connections {
            c.differences.push(format!(
                "thermalModel.patches[{i}].connections: AC {connections:?}, port {:?}",
                patch.connections
            ));
        }
    }
    c
}
