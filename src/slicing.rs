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
    #[name = "Transient"]
    Transient,
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
            SliceAlgorithm::Transient => "Transient",
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

    if sample.num_frames == 0 {
        return Slices::empty();
    }

    match algo {
        SliceAlgorithm::Transient => compute_transient_slices(sample, max_regions),
        _ => compute_tempo_based_slices(sample, bpm, algo, max_regions, playback_speed_percent),
    }
}

fn compute_tempo_based_slices(
    sample: &LoadedSample,
    bpm: f32,
    algo: SliceAlgorithm,
    max_regions: usize,
    playback_speed_percent: f32,
) -> Slices {
    if bpm <= 0.0 {
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
        SliceAlgorithm::Transient => unreachable!(), // handled separately
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

fn compute_transient_slices(sample: &LoadedSample, max_regions: usize) -> Slices {
    // Calculate amplitude envelope for each frame
    let mut amplitudes = Vec::with_capacity(sample.num_frames);
    for i in 0..sample.num_frames {
        let amp = if sample.stereo {
            (sample.data_l[i].abs() + sample.data_r[i].abs()) * 0.5
        } else {
            sample.data_l[i].abs()
        };
        amplitudes.push(amp);
    }

    // Apply a simple low-pass filter to get smoothed envelope
    // This helps reduce noise and makes transient detection more reliable
    let window_size = (sample.sample_rate * 0.01) as usize; // 10ms window
    let window_size = window_size.max(1).min(sample.num_frames);
    let mut smoothed = Vec::with_capacity(sample.num_frames);
    
    for i in 0..sample.num_frames {
        let start = i.saturating_sub(window_size / 2);
        let end = (i + window_size / 2 + 1).min(sample.num_frames);
        let sum: f32 = amplitudes[start..end].iter().sum();
        let count = (end - start) as f32;
        smoothed.push(sum / count);
    }

    // Detect transients by finding significant rises in amplitude
    // A transient is detected when the amplitude rises above a threshold
    // relative to the recent average
    let lookback_frames = (sample.sample_rate * 0.05) as usize; // 50ms lookback
    let lookback_frames = lookback_frames.max(1);
    let threshold_multiplier = 1.5; // Amplitude must be 1.5x the recent average
    let min_transient_gap = (sample.sample_rate * 0.01) as usize; // Minimum 10ms between transients
    let min_transient_gap = min_transient_gap.max(1);

    let mut transient_positions = Vec::new();
    
    // Always start with the first frame
    transient_positions.push(0);

    for i in lookback_frames..sample.num_frames {
        // Calculate recent average amplitude
        let lookback_start = i.saturating_sub(lookback_frames);
        let recent_avg: f32 = smoothed[lookback_start..i].iter().sum::<f32>() / (i - lookback_start) as f32;
        
        // Check if current amplitude is significantly higher than recent average
        if smoothed[i] > recent_avg * threshold_multiplier && smoothed[i] > 0.001 {
            // Check if we're far enough from the last transient
            if let Some(&last_pos) = transient_positions.last() {
                if i - last_pos >= min_transient_gap {
                    transient_positions.push(i);
                }
            } else {
                transient_positions.push(i);
            }
        }
    }

    // Limit to max_regions
    if transient_positions.len() > max_regions {
        // Distribute transients evenly across the sample
        let step = transient_positions.len() / max_regions;
        transient_positions = transient_positions
            .into_iter()
            .step_by(step.max(1))
            .take(max_regions)
            .collect();
    }

    // Always include the end of the sample
    if let Some(&last_transient) = transient_positions.last() {
        if last_transient < sample.num_frames {
            transient_positions.push(sample.num_frames);
        }
    } else {
        transient_positions.push(sample.num_frames);
    }

    // Create regions from transient positions
    let mut regions = Vec::new();
    for i in 0..transient_positions.len().saturating_sub(1) {
        let start = transient_positions[i];
        let end = transient_positions[i + 1];
        if start < end {
            regions.push((start, end));
        }
    }

    // Ensure we have at least one region
    if regions.is_empty() && max_regions > 0 {
        regions.push((0, sample.num_frames));
    }

    Slices { regions }
}
