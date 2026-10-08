# Task 19: sound

## Resume here

State after the last commit (update this section with every commit):

- **Done:** Step 0 (grip result in `grip_investigation.md`); the specs (`re/scratch/task19/spec_*.md`,
  git-ignored); `crates/rustyac-audio` (own FFI to the player's FMOD DLLs with a call log; `AudioEngine`,
  `AudioEvent`, `AudioReverb`, `AudioOccluder`, the two DSP plug-ins, `CarAudioFMOD`, `TrackAudio`, the
  simulation's part of the sound); `car_oracle run --audio-tape`; `tools/audio_oracle run | compare`
  (the game's own sound code against the port: call logs identical on the first drive, F2004 launch at
  Spa, three cameras).
- **Running / next:** record the other drives (`re/scratch/task19/record_tapes.sh`) and compare them all;
  `audio_oracle dsp` and `survey`; sound in `rustyac.exe` (flags, listener from the cameras, missing
  inputs); golden test; the two listening WAVs; this report; version 0.19.0 and the tag.
- **How to check what is there:** `cd tools/audio_oracle && cargo build --release &&
  ./target/release/audio_oracle.exe compare --tape ../../oracle/audio/<car>/<scenario>.audiotape --camera cockpit`
