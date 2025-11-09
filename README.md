 # clapchop
ergonomic sample chopping in a [CLAP plugin](https://cleveraudio.org/)

![](./screenshot.png)

## usage

1. load a sample with the "Browse..." button.
2. choose a slice algorithm / BPM to set chop points.
3. hit the buttons

## options

- "Hold beyond slice"
  - continuing to hold the trigger button will continue playing sample past the chop point.
- "Gate on release"
  - depressing the trigger button will stop sample playback before the chop endpoint.

these options are both _enabled_ by default. that means that the triggering of a sample chop is entirely dependent on holding the trigger button.
 
## development

```bash
cargo xtask bundle clapchop --release
```

The bundler produces a `.clap` bundle at `target/bundled/clapchop.clap`.
 
- Copy or symlink the bundled plugin (`clapchop.clap`) to your CLAP plug-ins folder (e.g., `~/.clap/`).
 - Load in a CLAP-compatible host.
 - In the UI:
   - Enter a file path to a WAV and click "Load".
   - Set BPM and choose a slice algorithm, then click "Re-slice".
   - Set the starting MIDI note; pads map row-major across 16 notes.
   - Toggle "Hold continue beyond slice" to keep playing beyond the slice end while the note is held.
 
### license

MIT

