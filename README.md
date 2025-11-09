# clapchop

ergonomic sample chopping in a [CLAP plugin](https://cleveraudio.org/)

![Screenshot of clapchop plugin running in Bitwig Studio](./screenshot.png)

## installation

- grab the latest `.clap` file release from [GitHub](https://github.com/jarofghosts/clapchop/releases)
- copy or symlink the plugin to your CLAP plug-ins folder (e.g., `~/.clap/`).

## usage

1. load a sample with the "Browse..." button.
2. choose a slice algorithm / BPM to set chop points.
3. hit the buttons to make the sounds

## options

- "Hold beyond slice"
  - continuing to hold the trigger button will continue playing sample past the chop point.
- "Gate on release"
  - depressing the trigger button will stop sample playback before the chop endpoint.

these options are both enabled by default, which makes sample playback naturally follow button presses.

## development

```bash
cargo run --package xtask -- bundle clapchop --release
```

- the bundler produces a `.clap` bundle at `target/bundled/clapchop.clap`.
- copy or symlink the plugin to your CLAP plug-ins folder (e.g., `~/.clap/`).

### license

MIT
