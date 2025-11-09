use std::fs;
use std::path::{Path, PathBuf};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::slicing::SliceAlgorithm;
use crate::{ClapChopParams, SharedState};

const PRESET_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct PresetData {
    pub version: u32,
    pub sample_path: Option<PathBuf>,
    pub starting_note: i32,
    pub bpm: f32,
    pub slice_algo: SliceAlgorithm,
    pub hold_continue: bool,
    pub gate_on_release: bool,
    pub num_pads: i32,
}

impl PresetData {
    pub fn capture(params: &ClapChopParams, shared: &RwLock<SharedState>) -> Self {
        let shared_guard = shared.read();
        let sample_path = shared_guard.loaded_path.as_ref().map(PathBuf::from);
        let pad_count = shared_guard.slices.regions.len() as i32;
        drop(shared_guard);

        Self {
            version: PRESET_VERSION,
            sample_path,
            starting_note: params.starting_note.value(),
            bpm: params.bpm.value(),
            slice_algo: params.slice_algo.value(),
            hold_continue: params.hold_continue.value(),
            gate_on_release: params.gate_on_release.value(),
            num_pads: pad_count,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != PRESET_VERSION {
            return Err(format!(
                "Unsupported preset version {} (expected {PRESET_VERSION})",
                self.version
            ));
        }
        Ok(())
    }
}

pub fn save_preset(
    path: &Path,
    params: &ClapChopParams,
    shared: &RwLock<SharedState>,
) -> Result<(), String> {
    let preset = PresetData::capture(params, shared);
    let json = serde_json::to_string_pretty(&preset)
        .map_err(|e| format!("serialize preset failed: {e}"))?;
    fs::write(path, json).map_err(|e| format!("failed writing preset: {e}"))?;
    Ok(())
}

pub fn load_preset(path: &Path) -> Result<PresetData, String> {
    let data = fs::read_to_string(path).map_err(|e| format!("failed reading preset: {e}"))?;
    let preset: PresetData =
        serde_json::from_str(&data).map_err(|e| format!("failed parsing preset json: {e}"))?;
    preset.validate()?;
    Ok(preset)
}
