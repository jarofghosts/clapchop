 ClapChop — CLAP slicing sampler
 
 Overview
 
 ClapChop is a CLAP plugin (built with nih-plug + egui) that slices a loaded WAV sample into musical "chops" and maps them to a pad grid (MIDI notes). It supports:
 
 - File input (mp3/WAV mono/stereo)
 - Slice algorithm: 1/4, 1/8, 1/16, Bars (4/4)
 - Sample BPM
 - Starting MIDI note for the 4x4 pad grid
 - Option to continue playing past slice end while the note is held
 - Optional gate mode to stop playback immediately on note release
 
Build

- Prerequisites: Rust (stable), Cargo
- Build release bundle:

```bash
cargo xtask bundle clap --release
```

The bundler produces a `.clap` bundle at `target/bundled/ClapChop.clap`. Copy the entire bundle directory to your CLAP plug-ins folder (e.g., `~/.clap/` on Linux).
 
 Usage
 
- Copy or symlink the bundled plugin (`ClapChop.clap`) to your CLAP plug-ins folder (e.g., `~/.clap/`).
 - Load in a CLAP-compatible host.
 - In the UI:
   - Enter a file path to a WAV and click "Load".
   - Set BPM and choose a slice algorithm, then click "Re-slice".
   - Set the starting MIDI note; pads map row-major across 16 notes.
   - Toggle "Hold continue beyond slice" to keep playing beyond the slice end while the note is held.
 
 Notes
 
 - Basic on-the-fly resampling is applied if the sample rate differs from the host.
 - Slices are beat-quantized; up to 16 slices are generated for a 4x4 grid.
 - UI audition via mouse is not implemented; trigger pads via MIDI notes starting at the configured note.
 
 License
 
 MIT or Apache-2.0
 

