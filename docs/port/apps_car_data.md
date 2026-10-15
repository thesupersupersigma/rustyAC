# Task 33b: the car-data apps and the dashboard panels

## Resume here

- Item 1 (digital panels, `RPM_GRAPH`, `DELTA_GRAPH`, `GEAR_TX`): **done.** It was already ported in
  Task 22; `DELTA_GRAPH` was the one item no proof had reached, and it is now proven (table 1).
- Next: item 2, FormTelemetry.
- Scratch: `re/scratch/task33b/` (git-ignored). Scene proofs: `sh re/scratch/task33b/scene.sh [filter]`,
  results in `re/scratch/task33b/scene_result.txt`.

## In plain English

(Filled in at the end.)

## Which cars have digital panels and the three items

Every installed car was looked at: the 138 already in `cardata/` and the others that ship a `data.acd` or
a `data` folder (extracted into `re/scratch/task33b/cardata/`, git-ignored). 70 Kunos DLC folders have no
car data at all (the DLC is not installed) and could not be read.

| What | Cars |
|---|---|
| `digital_panels.ini` | `vrc_formula_lithium_2023`, `vrc_formula_lithium_2023_csp` (and the test car `lithium_ers_front`) |
| `texture/display_panel/` | `rss_formula_hybrid_2020`, `vrc_formula_lithium_2023`, `vrc_formula_lithium_2023_csp` |
| `RPM_GRAPH` | `ferrari_599xxevo`, `formula_k`, `gt3_multilink`, `ks_ferrari_488_gt3`, `ks_mazda_787b`, `ktm_xbow_r`, `tatuusfa1`, `bdc_streetspec_aristo_v4`, `vrc_1988_mclaren_mp4-4_r02`, `_r04`, `_r09`, `_r10`, `vrc_formula_beta_2024` |
| `DELTA_GRAPH` | none |
| `GEAR_TX` | `ks_lamborghini_aventador_sv`, `ks_lamborghini_huracan_performante`, `ks_pagani_huayra_bc` |

The Formula Lithium's `digital_panels.ini` names a parent node its models do not have, so nothing of it is
drawn in either program (Task 22 found this); its proof uses a made-up file
(`re/scratch/task22/cars/lithium_panels/`). No car uses `DELTA_GRAPH`, so its proof uses the 488 GT3 with
a made-up `digital_instruments.ini` (`re/scratch/task33b/cars/488_delta/`): its first item turned into a
`DELTA_GRAPH` (`DIRECTION=0`, `DELTA_MAX=1.5`) and one more `DELTA_GRAPH` added (`DIRECTION=1`,
`DELTA_MAX=0.75`, orange).

## What is ported, with the game's addresses

### Item 1: digital panels and the three display items (Task 22)

- `DigitalPanels` (0xd8 bytes; constructor 0x1400820f0, `initPanels` 0x140082740, `update`
  0x140083f70) and `DisplayNode` (0x168; `render` 0x1400f74a0, `drawBase` 0x1400f6fb0,
  `drawBaseInverted` 0x1400f70e0, `drawTop` 0x1400f7210, `drawTopInverted` 0x1400f7360):
  `crates/rustyac-render/src/panels.rs`.
- `RPM_GRAPH`, `DELTA_GRAPH`, `GEAR_TX` in `DigitalInstruments`: `crates/rustyac-render/src/digital.rs`.
  `DELTA_GRAPH` reads the PerformanceMeter (`CarPhysicsState::performance_meter`, fed by Task 33's
  `performance_meter.rs`); `DIRECTION` picks the plain or the inverted drawing, `DELTA_MAX` the full
  scale, `DEBUG` above 0 draws it full.

## How it is proven

Scene sequences of the render oracle (`render_oracle compare`): the game's own car objects and the port's
draw the same car on WARP, the command logs and the pixels compared every frame.

## Results

### Table 1: digital panels and display items

| # | Sequence | Frames | Command log | Pixels |
|---|---|---|---|---|
| 1 | imola, 488 GT3 with two `DELTA_GRAPH` items; delta -2, -0.4, 0, 0.3, 1.2, 4 s | 35 | identical | identical |
| 2 | magione, Formula Lithium with a made-up `digital_panels.ini` (race position, push-to-pass, `P2P_DASH`, `POSITION_CAR`), pause at frame 30 | 39 | identical | identical |
| 3 | imola, 488 GT3, `RPM_GRAPH` through its range, TC 3, ABS 5 | 29 | identical | identical |
| 4 | magione, Aventador SV, `GEAR_TX` through R, N, 1, 4 (the neutral delay) | 49 | identical | identical |

## Disk space

- Start: 39 GB free.
- End: (filled in at the end).

## Not done

(Filled in at the end.)

## Open questions

(Filled in at the end.)
