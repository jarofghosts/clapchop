use nih_plug::prelude::*;
use serde::{Deserialize, Serialize};

use crate::sample::LoadedSample;

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum SliceAlgorithm {
    #[name = "1/4"]
    Quarter,
    #[name = "1/8"]
    Eighth,
    #[name = "1/16"]
    Sixteenth,
    #[name = "Bars"]
    Bars,
}

#[derive(Clone)]
pub struct Slices {
    // start,end in source frames
    pub regions: Vec<(usize, usize)>,
}

impl Slices {
    pub fn empty() -> Self {
        Self {
            regions: Vec::new(),
        }
    }
    pub fn get_slice_bounds(&self, idx: usize) -> Option<(usize, usize)> {
        self.regions.get(idx).cloned()
    }
}

impl Default for Slices {
    fn default() -> Self {
        Self::empty()
    }
}

impl SliceAlgorithm {
    pub fn label(self) -> &'static str {
        match self {
            SliceAlgorithm::Quarter => "1/4",
            SliceAlgorithm::Eighth => "1/8",
            SliceAlgorithm::Sixteenth => "1/16",
            SliceAlgorithm::Bars => "Bars",
        }
    }
}

pub fn compute_slices(
    sample: &LoadedSample,
    bpm: f32,
    algo: SliceAlgorithm,
    max_regions: usize,
    playback_speed_percent: f32,
) -> Slices {
    if max_regions == 0 {
        return Slices::empty();
    }

    if bpm <= 0.0 || sample.num_frames == 0 {
        return Slices::empty();
    }

    let sr = sample.sample_rate;
    let seconds_per_beat = 60.0 / bpm;
    let beats_per_bar = 4.0; // assume 4/4

    // Adjust slice duration based on playback speed
    // At 200% speed, slices should be double the duration in source frames (sample plays 2x faster, so need 2x frames for same real-time duration)
    // At 50% speed, slices should be half the duration in source frames (sample plays 0.5x faster, so need 0.5x frames for same real-time duration)
    let speed_multiplier = playback_speed_percent / 100.0;
    let frames_per_region = match algo {
        SliceAlgorithm::Quarter => seconds_per_beat,
        SliceAlgorithm::Eighth => seconds_per_beat * 0.5,
        SliceAlgorithm::Sixteenth => seconds_per_beat * 0.25,
        SliceAlgorithm::Bars => seconds_per_beat * beats_per_bar,
    } * sr * speed_multiplier;

    let frames_per_region = frames_per_region.max(1.0) as usize;
    let mut regions = Vec::new();

    let mut start = 0;
    while start < sample.num_frames && regions.len() < max_regions {
        let end = (start + frames_per_region).min(sample.num_frames);
        if start < end {
            regions.push((start, end));
        }
        start += frames_per_region;
    }

    if regions.is_empty() && max_regions > 0 {
        regions.push((0, sample.num_frames));
    }

    Slices { regions }
}
