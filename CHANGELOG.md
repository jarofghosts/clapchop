# 1.4.0

- Added MIDI channel filtering for pad chop triggering
  - Configure a specific MIDI channel (1-16) to listen to for pad chops
  - Set to "All" (default) to listen to all MIDI channels (backward compatible)
- Added MIDI channel for pitch reference updates
  - Configure a specific MIDI channel (1-16) to listen to for reference pitch updates
  - When a note is received on this channel, the pitch parameter is updated based on the semitone difference from the starting note
  - Set to "Off" (default) to disable pitch reference updates
- MIDI channel settings are now stored as part of presets
- Preset version incremented to 5

# 1.3.0

- Added pitch offset control in semitones (-24 to +24, default 0)
- Pitch adjustment is relative to the root note and affects both pitch and playback speed
- Pitch semitones setting is now stored as part of presets
- Preset version incremented to 4

# 1.2.0

- Added trim silence option to automatically remove silent portions from the start and end of samples when loading
- Trim silence setting is now stored as part of presets
- Preset version incremented to 3

# 1.1.0

- Added playback speed control (10-300% in 1% increments, default 100%)
- Playback speed is now stored as part of presets
- Preset version incremented to 2

# 1.0.0

- Rearranged UI

# 0.3.0

- Fixed window sizing
- Added tooltips to gate and release options
- Updated BPM input to increment by integer values

# 0.2.0

- Added UI scaling
- Updated defaults for button holds

# 0.1.0

- Initial release
