use std::sync::Arc;

use nih_plug::params::persist::PersistentField;
use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use parking_lot::RwLock;

mod preset;
mod sample;
mod slicing;
mod ui;

use sample::{LoadedSample, SamplePlayer};
use slicing::{SliceAlgorithm, Slices};

pub const MAX_PADS: usize = 64;

#[derive(Clone)]
pub enum UiPadEvent {
    NoteOn { pad_index: usize, velocity: f32 },
    NoteOff { pad_index: usize },
}

#[derive(Clone)]
pub struct SharedState {
    pub sample: Option<LoadedSample>,
    pub slices: Slices,
    pub sample_generation: u64,
    pub slices_generation: u64,
    pub loaded_path: Option<String>,
    pub loading: bool,
    pub last_error: Option<String>,
    pub pending_reslice: bool,
    pub pending_pad_events: Vec<UiPadEvent>,
    pub pad_visual_state: Vec<bool>,
    pub pad_visual_generation: u64,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            sample: None,
            slices: Slices::default(),
            sample_generation: 0,
            slices_generation: 0,
            loaded_path: None,
            loading: false,
            last_error: None,
            pending_reslice: false,
            pending_pad_events: Vec::new(),
            pad_visual_state: Vec::new(),
            pad_visual_generation: 0,
        }
    }
}

#[derive(Params)]
pub struct ClapChopParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    #[persist = "last-sample-path"]
    pub last_sample_path: Arc<RwLock<Option<String>>>,

    #[persist = "ui-scale"]
    pub ui_scale: Arc<RwLock<f32>>,

    #[id = "startnote"]
    pub starting_note: IntParam,

    #[id = "bpm"]
    pub bpm: FloatParam,

    #[id = "algo"]
    pub slice_algo: EnumParam<SliceAlgorithm>,

    #[id = "holdcont"]
    pub hold_continue: BoolParam,

    #[id = "gate"]
    pub gate_on_release: BoolParam,
}

impl Default for ClapChopParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(ui::DEFAULT_EDITOR_WIDTH, ui::DEFAULT_EDITOR_HEIGHT),
            last_sample_path: Arc::new(RwLock::new(None)),
            ui_scale: Arc::new(RwLock::new(1.0)),
            starting_note: IntParam::new(
                "starting note",
                36,
                IntRange::Linear { min: 0, max: 119 },
            ),
            bpm: FloatParam::new(
                "bpm",
                120.0,
                FloatRange::SymmetricalSkewed {
                    min: 40.0,
                    center: 120.0,
                    max: 240.0,
                    factor: 0.5,
                },
            )
            .with_step_size(1.0)
            .with_unit(" BPM"),
            slice_algo: EnumParam::new("slice algorithm", SliceAlgorithm::Quarter),
            hold_continue: BoolParam::new("hold beyond chop", true),
            gate_on_release: BoolParam::new("stop chop on release", true),
        }
    }
}

pub struct ClapChop {
    params: Arc<ClapChopParams>,
    shared: Arc<RwLock<SharedState>>,
    player: SamplePlayer,
    sample_generation_seen: u64,
    slices_generation_seen: u64,
    last_bpm: f32,
    last_algo: SliceAlgorithm,
    last_num_pads: usize,
    persisted_path_seen: Option<String>,
}

impl Default for ClapChop {
    fn default() -> Self {
        let params = Arc::new(ClapChopParams::default());
        let shared = Arc::new(RwLock::new(SharedState::default()));
        {
            let mut shared_guard = shared.write();
            shared_guard
                .pad_visual_state
                .resize(Self::default_num_pads(), false);
        }

        Self {
            params,
            shared,
            player: SamplePlayer::new(Self::default_num_pads()),
            sample_generation_seen: 0,
            slices_generation_seen: 0,
            last_bpm: 120.0,
            last_algo: SliceAlgorithm::Quarter,
            last_num_pads: Self::default_num_pads(),
            persisted_path_seen: None,
        }
    }
}

impl Plugin for ClapChop {
    const NAME: &'static str = "clapchop";
    const VENDOR: &'static str = "grimoire.supply";
    const URL: &'static str = "https://github.com/jarofghosts/clapchop";
    const EMAIL: &'static str = "me@jessekeane.me";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(0),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        ui::build_editor(self.params.clone(), self.shared.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.player.set_sample_rate(buffer_config.sample_rate);
        self.last_bpm = self.params.bpm.value();
        self.last_algo = self.params.slice_algo.value();
        let desired = {
            let shared = self.shared.read();
            let slice_count = shared.slices.regions.len();
            if slice_count > 0 {
                slice_count
            } else if !shared.pad_visual_state.is_empty() {
                shared.pad_visual_state.len()
            } else {
                Self::default_num_pads()
            }
        };
        self.set_pad_count(desired);
        true
    }

    fn reset(&mut self) {
        self.player.reset();
        self.clear_pad_visuals();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.ensure_persisted_sample_loaded();
        self.sync_shared_state();
        self.sync_num_pads();
        self.handle_reslice_requests();
        self.handle_midi(context);
        self.handle_ui_events();
        self.render_audio(buffer);

        ProcessStatus::Normal
    }
}

impl ClapChop {
    fn ensure_persisted_sample_loaded(&mut self) {
        let persisted_path = self.params.last_sample_path.read().clone();
        let (loaded_path, loading) = {
            let shared = self.shared.read();
            (shared.loaded_path.clone(), shared.loading)
        };

        if let Some(path) = persisted_path {
            if path.is_empty() {
                self.persisted_path_seen = None;
                return;
            }

            if loading {
                self.persisted_path_seen = Some(path);
                return;
            }

            if loaded_path.as_deref() == Some(path.as_str()) {
                self.persisted_path_seen = Some(path);
                return;
            }

            if self.persisted_path_seen.as_deref() == Some(path.as_str()) {
                return;
            }

            Self::request_sample_load(path.clone(), self.params.clone(), self.shared.clone());
            self.persisted_path_seen = Some(path);
        } else {
            self.persisted_path_seen = None;
        }
    }

    fn sync_shared_state(&mut self) {
        let (new_sample, new_slices) = {
            let shared = self.shared.read();
            let sample = if shared.sample_generation != self.sample_generation_seen {
                self.sample_generation_seen = shared.sample_generation;
                shared.sample.clone()
            } else {
                None
            };

            let slices = if shared.slices_generation != self.slices_generation_seen {
                self.slices_generation_seen = shared.slices_generation;
                Some(shared.slices.clone())
            } else {
                None
            };

            (sample, slices)
        };

        if let Some(sample) = new_sample {
            self.player.set_sample(sample);
        }
        if let Some(slices) = new_slices {
            let pad_count = slices.regions.len();
            self.player.set_slices(slices);
            self.set_pad_count(pad_count);
        }
    }

    fn handle_reslice_requests(&mut self) {
        let bpm = self.params.bpm.value();
        let algo = self.params.slice_algo.value();

        let reslice_due_to_param =
            (bpm - self.last_bpm).abs() > f32::EPSILON || algo != self.last_algo;
        let reslice_due_to_ui = {
            let mut shared = self.shared.write();
            if shared.pending_reslice {
                shared.pending_reslice = false;
                true
            } else {
                false
            }
        };

        if reslice_due_to_param || reslice_due_to_ui {
            if let Some(sample) = {
                let shared = self.shared.read();
                shared.sample.clone()
            } {
                let slices = slicing::compute_slices(&sample, bpm, algo, MAX_PADS);
                let pad_count = slices.regions.len();
                self.player.set_slices(slices.clone());
                let current_gen = {
                    let mut shared = self.shared.write();
                    shared.slices = slices;
                    shared.slices_generation = shared.slices_generation.wrapping_add(1).max(1);
                    if shared.pad_visual_state.len() != pad_count {
                        shared.pad_visual_state.resize(pad_count, false);
                        shared.pad_visual_generation = shared.pad_visual_generation.wrapping_add(1);
                    }
                    shared.slices_generation
                };
                self.slices_generation_seen = current_gen;
                self.set_pad_count(pad_count);
            }
        }

        self.last_bpm = bpm;
        self.last_algo = algo;
    }

    fn handle_midi(&mut self, context: &mut impl ProcessContext<Self>) {
        let start_note = self.params.starting_note.value() as u8;
        let num_pads = self.player.voice_count();
        if num_pads == 0 {
            return;
        }
        let num_pads_u8 = num_pads.min(u8::MAX as usize) as u8;

        while let Some(event) = context.next_event() {
            match event {
                NoteEvent::NoteOn { note, velocity, .. } => {
                    if note >= start_note && (note - start_note) < num_pads_u8 {
                        let pad_index = (note - start_note) as usize;
                        let hold = self.params.hold_continue.value();
                        let gate = self.params.gate_on_release.value();
                        self.player.note_on(pad_index, velocity, hold, gate);
                        self.set_pad_visual(pad_index, true);
                    }
                }
                NoteEvent::NoteOff { note, .. } => {
                    if note >= start_note && (note - start_note) < num_pads_u8 {
                        let pad_index = (note - start_note) as usize;
                        self.player.note_off(pad_index);
                        self.set_pad_visual(pad_index, false);
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_ui_events(&mut self) {
        let events = {
            let mut shared = self.shared.write();
            if shared.pending_pad_events.is_empty() {
                return;
            }
            std::mem::take(&mut shared.pending_pad_events)
        };

        let hold = self.params.hold_continue.value();
        let gate = self.params.gate_on_release.value();
        let num_pads = self.player.voice_count();
        if num_pads == 0 {
            return;
        }

        for event in events {
            match event {
                UiPadEvent::NoteOn {
                    pad_index,
                    velocity,
                } => {
                    if pad_index >= num_pads {
                        continue;
                    }
                    let velocity = velocity.clamp(0.0, 1.0).max(0.0001);
                    self.player.note_on(pad_index, velocity, hold, gate);
                    self.set_pad_visual(pad_index, true);
                }
                UiPadEvent::NoteOff { pad_index } => {
                    if pad_index >= num_pads {
                        continue;
                    }
                    self.player.note_off(pad_index);
                    self.set_pad_visual(pad_index, false);
                }
            }
        }
    }

    fn set_pad_count(&mut self, desired: usize) {
        let clamped = desired.min(MAX_PADS);
        if clamped == self.last_num_pads && self.player.voice_count() == clamped {
            return;
        }

        self.player.set_num_voices(clamped);
        self.last_num_pads = clamped;
        self.ensure_pad_visual_len(clamped);
    }

    fn sync_num_pads(&mut self) {
        let desired = {
            let shared = self.shared.read();
            let slice_count = shared.slices.regions.len();
            if slice_count > 0 {
                slice_count
            } else if shared.sample.is_some() {
                0
            } else {
                self.last_num_pads
            }
        };
        self.set_pad_count(desired);
    }

    const fn default_num_pads() -> usize {
        16
    }

    fn render_audio(&mut self, buffer: &mut Buffer) {
        if buffer.channels() == 0 {
            return;
        }

        for mut frame in buffer.iter_samples() {
            let (left, right) = self.player.process();
            let mut channels = frame.iter_mut();
            if let Some(sample) = channels.next() {
                *sample = left;
            }
            if let Some(sample) = channels.next() {
                *sample = right;
            }
            for sample in channels {
                *sample = 0.0;
            }
        }
    }

    fn ensure_pad_visual_len(&self, desired: usize) {
        let mut shared = self.shared.write();
        if shared.pad_visual_state.len() != desired {
            shared.pad_visual_state.resize(desired, false);
            shared.pad_visual_generation = shared.pad_visual_generation.wrapping_add(1);
        }
    }

    fn clear_pad_visuals(&self) {
        let mut shared = self.shared.write();
        if shared.pad_visual_state.iter().any(|&v| v) {
            for active in &mut shared.pad_visual_state {
                *active = false;
            }
            shared.pad_visual_generation = shared.pad_visual_generation.wrapping_add(1);
        }
    }

    fn set_pad_visual(&self, pad_index: usize, active: bool) {
        let mut shared = self.shared.write();
        if shared.pad_visual_state.len() <= pad_index {
            shared.pad_visual_state.resize(pad_index + 1, false);
        }
        if shared.pad_visual_state[pad_index] != active {
            shared.pad_visual_state[pad_index] = active;
            shared.pad_visual_generation = shared.pad_visual_generation.wrapping_add(1);
        }
    }

    pub(crate) fn request_sample_load(
        path: String,
        params: Arc<ClapChopParams>,
        shared: Arc<RwLock<SharedState>>,
    ) {
        params.last_sample_path.set(Some(path.clone()));
        {
            let mut guard = shared.write();
            guard.loading = true;
            guard.last_error = None;
        }

        std::thread::spawn(move || match sample::load_sample(&path) {
            Ok(sample) => {
                let bpm = params.bpm.value();
                let algo = params.slice_algo.value();
                let slices = slicing::compute_slices(&sample, bpm, algo, MAX_PADS);
                let pad_count = slices.regions.len();

                let mut guard = shared.write();
                guard.sample = Some(sample);
                guard.slices = slices;
                guard.sample_generation = guard.sample_generation.wrapping_add(1).max(1);
                guard.slices_generation = guard
                    .slices_generation
                    .wrapping_add(1)
                    .max(guard.sample_generation);
                guard.loaded_path = Some(path);
                guard.loading = false;
                guard.last_error = None;
                guard.pending_reslice = false;
                guard.pad_visual_state.resize(pad_count, false);
                guard.pad_visual_generation = guard.pad_visual_generation.wrapping_add(1);
            }
            Err(err) => {
                let mut guard = shared.write();
                guard.loading = false;
                guard.last_error = Some(err);
            }
        });
    }
}

impl ClapPlugin for ClapChop {
    const CLAP_ID: &'static str = "com.clapchop.sampler";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("ergonomic sample chopper");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = Some(Self::EMAIL);
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Sampler,
        ClapFeature::Stereo,
    ];
}

nih_export_clap!(ClapChop);
