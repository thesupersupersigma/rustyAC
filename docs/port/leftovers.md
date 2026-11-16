# Task L1 — the leftovers before v1.0

## Resume here
- Done: item (a), commit b8297a8. A refused custom-filter key no longer gives black frames:
  `yebis_cb::refuse` logs it once a run and queues it for AC's banner (drained in
  `AcRenderer`'s frame, title "Post-processing filter"); the frame draws without the key's
  effect. Shafts / glare / DOF refusals turn that effect off; HDR=0, other tone map functions,
  CA off, the tone map viewport, feedback, lens distortion, auto exposure delay, vignette FOV
  dependence and colour temperature under 4000 K draw as the shipped filters do; a bloom
  gaussian of radius 2 draws with the 7-tap program. Proof: the unit test
  `refused_keys_draw_without_their_effect` (FEEDBACK ENABLED=1 + GLARE THRESHOLD=0 on
  default.ini). Not proven on the live exe yet.
- Next: step 1's table below, from `re/scratch/taskL1/collected.md` (519 lines, every
  "Not done" / open question since Task 43), then step 2's other fixes.
- Still refused per frame (not filter keys): glare resample sizes, a glare halving with
  several taps, the heat shimmer without chromatic aberration (VR); these still skip YEBIS
  for the frame.

## Table
(step 1, not written yet)
