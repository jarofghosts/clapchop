 # clapchop
ergonomic sample chopping in a [CLAP plugin](https://cleveraudio.org/)
 
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

