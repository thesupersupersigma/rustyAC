# tyres.ini keys → code → struct fields

Every key below is read by **one function: `Tyre::initCompounds` @ `0x140280800`**
(called once per wheel from `Tyre::init` @ `0x140280650`, which is called from `Car::Car`).
The file is `<car data path>/tyres.ini`, opened through `INIReader`.

How the values travel:

1. `initCompounds` fills a local `TyreCompoundDef` (size 0x3f0) per compound and pushes it into
   `Tyre::compoundDefs` (`Tyre+0x500`).
2. `Tyre::setCompound(index)` @ `0x1402834e0` copies the chosen def into the live wheel:
   `def.modelData → Tyre::modelData`, `def.data → Tyre::data`,
   `def.slipProvider → Tyre::slipProvider`, `def.thermalPatchData → Tyre::thermalModel.patchData`,
   `def.thermalPerformanceCurve → Tyre::thermalModel.performanceCurve`,
   `def.pressureStatic → Tyre::status.pressureStatic / pressureDynamic`, and then mirrors the
   values the V10 model needs into `Tyre::scTM` (the `SCTM` object).

Offsets in the "def field" column are offsets inside `TyreCompoundDef`
(layouts: `re/types/TyreCompoundDef.txt`, `re/types/TyreModelData.txt`, `re/types/TyreData.txt`, rebuilt by `tools/pdb_types.py`).
They were derived from the stack slot each value is stored in (`WIDTH` → `local_4d0` pins the local
def at stack `0x7a8`), cross-checked against the PDB layouts. Regenerate with `tools/ini_map.py`.

`ver` = the compound's `[HEADER] VERSION` (stored in `modelData.version`).

## File-level keys (not per compound)

| Section | Key | Stored in | Notes |
|---|---|---|---|
| `HEADER` | `VERSION` | local only (first read), then `def.modelData.version` (+0x48) per compound | First read only decides `generateCompoundNames()` when `< 4`. The per-compound copy is what selects the force path (see below). |
| `EXPLOSION` | `TEMPERATURE` | `Tyre::explosionTemperature` (Tyre+0x848) | Only if the section exists. Used by `Tyre::stepPuncture`. |
| `VIRTUALKM` | `USE_LOAD` | `Tyre::useLoadForVKM` (Tyre+0x5a0) | Only if the section exists. Used in `addTyreForcesV10` wear distance. |
| `ADDITIONAL1` | `BLANKETS_TEMP` | `Tyre::blanketTemperature` (Tyre+0x84c) | Used by `Tyre::stepTyreBlankets`. |
| `ADDITIONAL1` | `PRESSURE_TEMPERATURE_GAIN` | `Tyre::pressureTemperatureGain` (Tyre+0x850) | `pressureDynamic = (coreTemp - 26) * gain + pressureStatic` in `Tyre::step`. |
| `ADDITIONAL1` | `CAMBER_TEMP_SPREAD_K` | `Tyre::thermalModel.camberSpreadK` (TyreThermalModel+0xcc) | Only applied when non-zero. |

## Per-compound keys — section `FRONT` / `REAR` (first compound), `FRONT_1` / `REAR_1`, … (others)

Section name = `FRONT` or `REAR` (picked by wheel index) plus `_<n>` for compound n > 0.
The loop stops at the first missing section.

| Key | def field (offset in TyreCompoundDef) | Runtime field after `setCompound` | Gate / default / transform |
|---|---|---|---|
| `NAME` | name string (+0x08) | — | |
| `SHORT_NAME` | short name string (+0x28) | — | only `ver > 3`; name becomes `NAME (SHORT)` |
| `WIDTH` | `data.width` (+0x2d8) | `Tyre::data.width` | |
| `RADIUS` | `data.radius` (+0x2dc) | `Tyre::data.radius` | |
| `RIM_RADIUS` | `data.rimRadius` (+0x310) | `Tyre::data.rimRadius` | `ver < 3`: not read, fixed 0.13 |
| `FLEX` | `modelData.flexK` (+0x60) | `Tyre::modelData.flexK`; also `slipProvider.brushModel.data.CF1 = FLEX * -50000` | passed to `BrushSlipProvider` ctor |
| `FRICTION_LIMIT_ANGLE` | not stored raw | `slipProvider.brushModel.data.CF = 3*78.125 / tan(angle°)`; for `ver >= 5` also `maxSlip0 = tan(angle°)` → `scTM.maxSlip0` | 0 → 7.5 |
| `XMU` | `slipProvider.brushModel.data.xu` | `Tyre::slipProvider.brushModel.data.xu` | forced to 0 when `ver > 4` |
| `CX_MULT` | `modelData.cfXmult` (+0x244) | `Tyre::modelData.cfXmult` → `scTM.cfXmult` | only `ver > 9` |
| `RADIUS_ANGULAR_K` | `data.radiusRaiseK` (+0x31c) | `Tyre::data.radiusRaiseK` | only `ver > 9`; stored × 0.001 |
| `BRAKE_DX_MOD` | `modelData.brakeDXMod` (+0x248) | `Tyre::modelData.brakeDXMod` → `scTM.brakeDXMod` | only `ver > 9`, only if key present; stored as `1 + value` (0 → 1) |
| `COMBINED_FACTOR` | `modelData.combinedFactor` (+0x2d4) | `Tyre::modelData.combinedFactor` → `scTM.combinedFactor` | only `ver > 9`, only if key present |
| `DY0` | `modelData.Dy0` (+0x4c) | `Tyre::modelData.Dy0` | only `ver < 5` |
| `DY1` | `modelData.Dy1` (+0x50) | `Tyre::modelData.Dy1` | only `ver < 5` |
| `DX0` | `modelData.Dx0` (+0x54) | `Tyre::modelData.Dx0` | only `ver < 5`; 0 → `Dy0 * 1.2` |
| `DX1` | `modelData.Dx1` (+0x58) | `Tyre::modelData.Dx1` | only `ver < 5`; 0 → `Dy1 * 0.1` |
| `FZ0` | `slipProvider.brushModel.data.Fz0` | `Tyre::slipProvider.brushModel.data.Fz0` → `scTM.Fz0` | only `ver >= 5`. NB `modelData.Fz0` (+0x5c) is **not** written from this key in the code read so far (see open questions in the report). |
| `LS_EXPX` | `modelData.lsExpX` (+0x234) | `Tyre::modelData.lsExpX` → `scTM.lsExpX` | only `ver >= 5` |
| `LS_EXPY` | `modelData.lsExpY` (+0x22c) | `Tyre::modelData.lsExpY` → `scTM.lsExpY` | only `ver >= 5` |
| `DX_REF` | `modelData.Dx0` (+0x54) | also `modelData.lsMultX` (+0x230) `= DX_REF*FZ0 / FZ0^LS_EXPX` → `scTM.lsMultX` | only `ver >= 5`; via `calcLoadSensMult` |
| `DY_REF` | `modelData.Dy0` (+0x4c) | also `modelData.lsMultY` (+0x228) `= DY_REF*FZ0 / FZ0^LS_EXPY` → `scTM.lsMultY` | only `ver >= 5`; via `calcLoadSensMult` |
| `FLEX_GAIN` | not stored raw | `slipProvider.brushModel.data.maxSlip1 = tan((FLEX_GAIN+1) * angle°)` → `scTM.maxSlip1` | only `ver >= 5` |
| `DY_CURVE` | `modelData.dyLoadCurve` (+0x128) | `Tyre::modelData.dyLoadCurve` → `scTM.dyLoadCurve` | only `ver >= 5`, only if key present; replaces the exponent load-sensitivity |
| `DX_CURVE` | `modelData.dxLoadCurve` (+0x1a8) | `Tyre::modelData.dxLoadCurve` → `scTM.dxLoadCurve` | same |
| `FALLOFF_LEVEL` | `slipProvider.asy` (+0x344) | `Tyre::slipProvider.asy` → `modelData.asy` and `scTM.asy` | only `ver > 6`; default 0.85 (`ver < 5`) or 0.92 (`ver >= 5`) |
| `FALLOFF_SPEED` | `slipProvider.brushModel.data.falloffSpeed` (+0x340) | → `scTM.falloffSpeed` | only `ver > 6` |
| `SPEED_SENSITIVITY` | `modelData.speedSensitivity` (+0x64) | `Tyre::modelData.speedSensitivity` → `scTM.speedSensitivity` | |
| `RELAXATION_LENGTH` | `modelData.relaxationLength` (+0x68) | `Tyre::modelData.relaxationLength` | |
| `ROLLING_RESISTANCE_0` | `modelData.rr0` (+0x6c) | `Tyre::modelData.rr0` | |
| `ROLLING_RESISTANCE_1` | `modelData.rr1` (+0x70) | `Tyre::modelData.rr1` | |
| `ROLLING_RESISTANCE_SA` | `modelData.rr_sa` (+0x74) | `Tyre::modelData.rr_sa` | only `ver == 1` |
| `ROLLING_RESISTANCE_SR` | `modelData.rr_sr` (+0x78) | `Tyre::modelData.rr_sr` | only `ver == 1` |
| `ROLLING_RESISTANCE_SLIP` | `modelData.rr_slip` (+0x7c) | `Tyre::modelData.rr_slip` | only `ver != 1` |
| `CAMBER_GAIN` | `modelData.camberGain` (+0x80) | `Tyre::modelData.camberGain` → `scTM.camberGain` | |
| `DCAMBER_0` | `modelData.dcamber0` (+0x120) | `Tyre::modelData.dcamber0` → `scTM.dcamber0` | if either is 0: 0.1 / -0.8 |
| `DCAMBER_1` | `modelData.dcamber1` (+0x124) | `Tyre::modelData.dcamber1` → `scTM.dcamber1` | |
| `DCAMBER_LUT` | `modelData.dCamberCurve` (+0x250) | `Tyre::modelData.dCamberCurve` → `scTM.dCamberCurve` | only if key present |
| `DCAMBER_LUT_SMOOTH` | `modelData.useSmoothDCamberCurve` (+0x2d0) | → `scTM.useSmoothDCamberCurve` | read with `DCAMBER_LUT` |
| `ANGULAR_INERTIA` | `data.angularInertia` (+0x2e8) | `Tyre::data.angularInertia` | 0 → 1.2 |
| `DAMP` | `data.d` (+0x2e4) | `Tyre::data.d` | 0 → 400 |
| `RATE` | `data.k` (+0x2e0) | `Tyre::data.k` | 0 → 220000 |
| `PRESSURE_STATIC` | `pressureStatic` (+0x358); also `modelData.pressureRef` (+0x98) | `Tyre::status.pressureStatic`, `status.pressureDynamic`, `modelData.pressureRef` | 0 → 26 |
| `PRESSURE_SPRING_GAIN` | `modelData.pressureSpringGain` (+0x84) | `Tyre::modelData.pressureSpringGain` | 0 → 1000 |
| `PRESSURE_FLEX_GAIN` | `modelData.pressureFlexGain` (+0x88) | `Tyre::modelData.pressureFlexGain` → `scTM.pressureCfGain` | |
| `PRESSURE_RR_GAIN` | `modelData.pressureRRGain` (+0x8c) | `Tyre::modelData.pressureRRGain` | |
| `PRESSURE_D_GAIN` | `modelData.pressureGainD` (+0x90) | `Tyre::modelData.pressureGainD` | |
| `PRESSURE_IDEAL` | `modelData.idealPressure` (+0x94) | `Tyre::modelData.idealPressure` | 0 → 26 |
| `WEAR_CURVE` | `modelData.wearCurve` (+0xa0) | `Tyre::modelData.wearCurve` | file name; loaded with `Curve::load`, scaled × 0.01. Its minimum also fills `modelData.maxWearKM` (+0x238) / `maxWearMult` (+0x23c). |

Derived (no key): `data.softnessIndex` (+0x318) `= max(0, D(3000 N) - 1)` using `loadSensLinearD`
(`ver < 5`) or `loadSensExpD` (`ver >= 5`). `slipProvider.maximum` / `maxSlip` come from
`BrushSlipProvider::recomputeMaximum`.

## Per-compound thermal keys — section `THERMAL_FRONT` / `THERMAL_REAR` (+ `_<n>`)

Only read when the section exists.

| Key | def field (offset in TyreCompoundDef) | Runtime field after `setCompound` | Gate / transform |
|---|---|---|---|
| `SURFACE_TRANSFER` | `thermalPatchData.surfaceTransfer` (+0x35c) | `Tyre::thermalModel.patchData.surfaceTransfer` | |
| `PATCH_TRANSFER` | `thermalPatchData.patchTransfer` (+0x360) | `…patchData.patchTransfer` | |
| `CORE_TRANSFER` | `thermalPatchData.patchCoreTransfer` (+0x364) | `…patchData.patchCoreTransfer` | |
| `INTERNAL_CORE_TRANSFER` | `thermalPatchData.internalCoreTransfer` (+0x368) | `…patchData.internalCoreTransfer` | only `ver > 4` |
| `COOL_FACTOR` | `thermalPatchData.coolFactorGain` (+0x36c) | `…patchData.coolFactorGain` | only `ver > 4`, only if present; stored as `(value - 1) * 0.000324` |
| `FRICTION_K` | `data.thermalFrictionK` (+0x2ec) | `Tyre::data.thermalFrictionK` | |
| `ROLLING_K` | `data.thermalRollingK` (+0x2f0) | `Tyre::data.thermalRollingK` | |
| `SURFACE_ROLLING_K` | `data.thermalRollingSurfaceK` (+0x2f4) | `Tyre::data.thermalRollingSurfaceK` | only `ver > 5` |
| `PERFORMANCE_CURVE` | `thermalPerformanceCurve` (+0x370) | `Tyre::thermalModel.performanceCurve` | file name → `Curve::load`. First temp where value ≥ 1 → `data.grainThreshold` (+0x2f8); last such temp → `data.blisterThreshold` (+0x2fc) and `data.optimumTemp` (+0x314). |
| `BLISTER_GAMMA` | `data.blisterGamma` (+0x304) | `Tyre::data.blisterGamma` | only `ver > 2` |
| `BLISTER_GAIN` | `data.blisterGain` (+0x30c) | `Tyre::data.blisterGain` | only `ver > 2` |
| `GRAIN_GAMMA` | `data.grainGamma` (+0x300) | `Tyre::data.grainGamma` | only `ver > 2` |
| `GRAIN_GAIN` | `data.grainGain` (+0x308) | `Tyre::data.grainGain` | only `ver > 2` |

## Notes

- The MCP string search found none of these keys because they are UTF-16 (`L"…"`) literals; they
  were located by reading the decompiled `initCompounds` instead.
- The four `DX0/DX1/DY0/DY1` and the `DX_REF/DY_REF` keys share the `Dx0`/`Dy0` slots: old
  (`ver < 5`) cars use the linear load model, newer ones the exponent (or curve) model.
- A compound that `PhysicsEngine::isTyreLegal` rejects is parked in a side list; if every compound
  is rejected they are all re-enabled.
