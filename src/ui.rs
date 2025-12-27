use nih_plug::params::persist::PersistentField;
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc};

use crossbeam_utils::atomic::AtomicCell;
use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets, EguiState};
use parking_lot::RwLock;
use rfd::FileDialog;

use crate::preset::{load_preset, save_preset, PresetData};
use crate::slicing::SliceAlgorithm;
use crate::{ClapChop, ClapChopParams, SharedState, UiPadEvent};

pub const DEFAULT_EDITOR_WIDTH: u32 = 850;
pub const DEFAULT_EDITOR_HEIGHT: u32 = 600;
const PAD_WIDTH: f32 = 72.0;
const PAD_HEIGHT: f32 = 64.0;
const PAD_SPACING: f32 = 6.0;

pub struct GuiState {
    last_loaded_path: Option<String>,
    pressed_pads: Vec<bool>,
    last_preset_path: Option<String>,
    preset_message: Option<String>,
    preset_error: Option<String>,
    // Unscaled baseline style captured on first frame to avoid compounding scale.
    base_style: Option<egui::Style>,
}

impl Default for GuiState {
    fn default() -> Self {
        Self {
            last_loaded_path: None,
            pressed_pads: vec![false; crate::MAX_PADS],
            last_preset_path: None,
            preset_message: None,
            preset_error: None,
            base_style: None,
        }
    }
}

pub fn build_editor(
    params: Arc<ClapChopParams>,
    shared: Arc<RwLock<SharedState>>,
) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        params.editor_state.clone(),
        GuiState::default(),
        |_ctx, _state| {},
        move |egui_ctx, setter, state| {
            // Apply UI scale from persisted parameter by scaling the base style each frame.
            let scale = (*params.ui_scale.read()).clamp(0.5, 3.0);
            if state.base_style.is_none() {
                state.base_style = Some(egui_ctx.style().as_ref().clone());
            }
            if let Some(base) = &state.base_style {
                let mut styled = base.clone();
                scale_style_in_place(&mut styled, scale);
                egui_ctx.set_style(styled);
            }
            let mut content_size = egui::Vec2::ZERO;
            let mut pad_count = 0usize;
            egui::CentralPanel::default().show(egui_ctx, |ui| {
                sync_gui_state(state, &shared);

                ui.heading("clapchop");

                sample_loader_row(ui, state, &params, &shared);
                ui.separator();

                parameter_row(ui, setter, &params, &shared);

                pad_count = {
                    let shared_guard = shared.read();
                    shared_guard.slices.regions.len()
                };
                pad_count = pad_count.min(crate::MAX_PADS);

                pad_grid(
                    ui,
                    state,
                    &shared,
                    params.starting_note.value() as u8,
                    scale,
                    pad_count,
                );

                ui.separator();
                status_section(ui, state, &shared, &params);
                ui.separator();
                preset_row(ui, state, setter, &params, &shared);

                // Force UI to complete layout before measuring
                ui.ctx().request_repaint();
                content_size = ui.min_rect().size();
            });
            maybe_resize_editor(egui_ctx, &params.editor_state, content_size);
        },
    )
}

fn sync_gui_state(state: &mut GuiState, shared: &Arc<RwLock<SharedState>>) {
    let shared = shared.read();
    if let Some(path) = &shared.loaded_path {
        if state.last_loaded_path.as_ref() != Some(path) {
            state.last_loaded_path = Some(path.clone());
        }
    }
}

fn sample_loader_row(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    params: &Arc<ClapChopParams>,
    shared: &Arc<RwLock<SharedState>>,
) {
    let mut path_to_load: Option<String> = None;

    ui.horizontal(|ui| {
        let loading = shared.read().loading;

        if ui
            .add_enabled(!loading, egui::Button::new("load sample..."))
            .clicked()
        {
            let mut dialog =
                FileDialog::new().add_filter("audio", &["wav", "aif", "aiff", "flac", "mp3"]);

            if let Some(initial) = state.last_loaded_path.as_deref() {
                let initial_path = Path::new(initial);
                if initial_path.is_dir() {
                    dialog = dialog.set_directory(initial_path.to_path_buf());
                } else {
                    if let Some(parent) = initial_path.parent() {
                        dialog = dialog.set_directory(parent.to_path_buf());
                    }
                    if let Some(file_name) = initial_path.file_name().and_then(|f| f.to_str()) {
                        dialog = dialog.set_file_name(file_name.to_owned());
                    }
                }
            }

            if let Some(file) = dialog.pick_file() {
                let path_string = file.to_string_lossy().into_owned();
                state.last_loaded_path = Some(path_string.clone());
                path_to_load = Some(path_string);
            }
        }
    });

    if let Some(path) = path_to_load {
        ClapChop::request_sample_load(path, params.clone(), shared.clone());
    }
}

fn parameter_row(ui: &mut egui::Ui, setter: &ParamSetter, params: &Arc<ClapChopParams>, shared: &Arc<RwLock<SharedState>>) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label("chop algorithm");
            egui::ComboBox::from_id_salt("slice_algo_combo")
                .selected_text(params.slice_algo.value().label())
                .show_ui(ui, |ui| {
                    for (idx, variant_name) in SliceAlgorithm::variants().iter().enumerate() {
                        let variant = SliceAlgorithm::from_index(idx);
                        let selected = params.slice_algo.value() == variant;
                        if ui.selectable_label(selected, *variant_name).clicked() {
                            setter.begin_set_parameter(&params.slice_algo);
                            setter.set_parameter(&params.slice_algo, variant);
                            setter.end_set_parameter(&params.slice_algo);
                        }
                    }
                });

            ui.separator();
            ui.label("bpm");
            let scale = *params.ui_scale.read();
            ui.add(widgets::ParamSlider::for_param(&params.bpm, setter).with_width(160.0 * scale));
        });
        ui.horizontal(|ui| {
                ui.label("starting note");
                let mut start_note_value = params.starting_note.value();
                let slider_response = ui.add(
                    egui::Slider::new(&mut start_note_value, 0..=119)
                        .clamping(egui::SliderClamping::Always)
                        .text(""),
                );
                let note_name = midi_note_name(start_note_value as u8);
                ui.monospace(note_name);
                if slider_response.changed() {
                    setter.begin_set_parameter(&params.starting_note);
                    setter.set_parameter(&params.starting_note, start_note_value);
                    setter.end_set_parameter(&params.starting_note);
                }
            });

        ui.horizontal(|ui| {
            ui.label("playback speed");
            let scale = *params.ui_scale.read();
            ui.add(widgets::ParamSlider::for_param(&params.playback_speed, setter).with_width(160.0 * scale));
        });

        ui.horizontal(|ui| {
            ui.label("pitch");
            // Check if MIDI has updated the pitch value
            let midi_pitch = shared.read().midi_pitch_semitones;
            let mut pitch_value = midi_pitch.unwrap_or_else(|| params.pitch_semitones.value());
            let slider_response = ui.add(
                egui::Slider::new(&mut pitch_value, -24..=24)
                    .clamping(egui::SliderClamping::Always)
                    .text(""),
            );
            let pitch_label = if pitch_value == 0 {
                "0 st".to_string()
            } else if pitch_value > 0 {
                format!("+{} st", pitch_value)
            } else {
                format!("{} st", pitch_value)
            };
            ui.monospace(pitch_label);
            if slider_response.changed() {
                setter.begin_set_parameter(&params.pitch_semitones);
                setter.set_parameter(&params.pitch_semitones, pitch_value);
                setter.end_set_parameter(&params.pitch_semitones);
                // Clear MIDI pitch update flag since user is now controlling it
                shared.write().midi_pitch_semitones = None;
            }
        });

        ui.horizontal(|ui| {
            ui.label("pad chop MIDI channel");
            let pad_channel_value = params.pad_chop_channel.value();
            let pad_channel_label = if pad_channel_value == 16 {
                "All".to_string()
            } else {
                format!("{}", pad_channel_value + 1)
            };
            egui::ComboBox::from_id_salt("pad_chop_channel_combo")
                .selected_text(pad_channel_label)
                .show_ui(ui, |ui| {
                    for channel in 0..=16 {
                        let label = if channel == 16 {
                            "All".to_string()
                        } else {
                            format!("{}", channel + 1)
                        };
                        let selected = pad_channel_value == channel;
                        if ui.selectable_label(selected, label).clicked() {
                            setter.begin_set_parameter(&params.pad_chop_channel);
                            setter.set_parameter(&params.pad_chop_channel, channel);
                            setter.end_set_parameter(&params.pad_chop_channel);
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label("pitch reference MIDI channel");
            let pitch_ref_channel_value = params.pitch_reference_channel.value();
            let pitch_ref_channel_label = if pitch_ref_channel_value == 16 {
                "Off".to_string()
            } else {
                format!("{}", pitch_ref_channel_value + 1)
            };
            egui::ComboBox::from_id_salt("pitch_ref_channel_combo")
                .selected_text(pitch_ref_channel_label)
                .show_ui(ui, |ui| {
                    for channel in 0..=16 {
                        let label = if channel == 16 {
                            "Off".to_string()
                        } else {
                            format!("{}", channel + 1)
                        };
                        let selected = pitch_ref_channel_value == channel;
                        if ui.selectable_label(selected, label).clicked() {
                            setter.begin_set_parameter(&params.pitch_reference_channel);
                            setter.set_parameter(&params.pitch_reference_channel, channel);
                            setter.end_set_parameter(&params.pitch_reference_channel);
                        }
                    }
                });
        });

        let mut hold = params.hold_continue.value();
        if ui.checkbox(&mut hold, "hold beyond chop point")
            .on_hover_text("continuing to hold the trigger button will continue playing sample past the chop endpoint.")
            .changed() {
            setter.begin_set_parameter(&params.hold_continue);
            setter.set_parameter(&params.hold_continue, hold);
            setter.end_set_parameter(&params.hold_continue);
        }

        let mut gate = params.gate_on_release.value();
        if ui.checkbox(&mut gate, "stop chop on release")
            .on_hover_text("depressing the trigger button will stop sample playback before the chop endpoint.")
            .changed() {
            setter.begin_set_parameter(&params.gate_on_release);
            setter.set_parameter(&params.gate_on_release, gate);
            setter.end_set_parameter(&params.gate_on_release);
        }

        let mut trim = params.trim_silence.value();
        if ui.checkbox(&mut trim, "trim silence")
            .on_hover_text("automatically trim silent portions from the start and end of the sample when loading.")
            .changed() {
            setter.begin_set_parameter(&params.trim_silence);
            setter.set_parameter(&params.trim_silence, trim);
            setter.end_set_parameter(&params.trim_silence);
        }
    });
}

fn preset_row(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    setter: &ParamSetter,
    params: &Arc<ClapChopParams>,
    shared: &Arc<RwLock<SharedState>>,
) {
    ui.horizontal(|ui| {
        if ui.button("load preset").clicked() {
            let mut dialog = FileDialog::new().add_filter("clapchop preset", &["json", "clapchop"]);

            if let Some(initial) = preset_dialog_initial_path(state) {
                if initial.is_dir() {
                    dialog = dialog.set_directory(initial);
                } else if let Some(parent) = initial.parent() {
                    dialog = dialog.set_directory(parent.to_path_buf());
                }
            }

            if let Some(path) = dialog.pick_file() {
                match load_preset(path.as_path()) {
                    Ok(preset) => {
                        if let Err(err) = apply_preset(&preset, setter, params, state, shared) {
                            state.preset_error = Some(err);
                            state.preset_message = None;
                        } else {
                            state.preset_message =
                                Some(format!("preset loaded from {}", path.to_string_lossy()));
                            state.preset_error = None;
                            state.last_preset_path = Some(path.to_string_lossy().into_owned());
                        }
                    }
                    Err(err) => {
                        state.preset_error = Some(err);
                        state.preset_message = None;
                    }
                }
            }
        }

        let sample_loaded = shared.read().sample.is_some();
        let save_button = ui.add_enabled(sample_loaded, egui::Button::new("save preset"));

        if save_button.clicked() {
            let mut dialog = FileDialog::new().add_filter("clapchop preset", &["clapchop.json"]);

            if let Some(initial) = preset_dialog_initial_path(state) {
                if initial.is_dir() {
                    dialog = dialog.set_directory(initial);
                } else if let Some(parent) = initial.parent() {
                    dialog = dialog.set_directory(parent.to_path_buf());
                }
            }

            // Generate default filename based on currently loaded preset or sample name
            let default_filename = if let Some(ref preset_path) = state.last_preset_path {
                // Use current preset name if available
                Path::new(preset_path)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(|name| format!("{}.json", name))
                    .unwrap_or_else(|| "default.clapchop.json".to_string())
            } else if let Some(ref sample_path) = shared.read().loaded_path {
                // Use current sample name with .clapchop.json extension
                Path::new(sample_path)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(|name| format!("{}.clapchop.json", name))
                    .unwrap_or_else(|| "default.clapchop.json".to_string())
            } else {
                "default.clapchop.json".to_string()
            };

            dialog = dialog.set_file_name(&default_filename);

            if let Some(path) = dialog.save_file() {
                match save_preset(path.as_path(), params.as_ref(), shared.as_ref()) {
                    Ok(_) => {
                        state.preset_message =
                            Some(format!("preset saved to {}", path.to_string_lossy()));
                        state.preset_error = None;
                        state.last_preset_path = Some(path.to_string_lossy().into_owned());
                    }
                    Err(err) => {
                        state.preset_error = Some(err);
                        state.preset_message = None;
                    }
                }
            }
        }
    });
}

fn apply_preset(
    preset: &PresetData,
    setter: &ParamSetter,
    params: &Arc<ClapChopParams>,
    state: &mut GuiState,
    shared: &Arc<RwLock<SharedState>>,
) -> Result<(), String> {
    setter.begin_set_parameter(&params.starting_note);
    setter.set_parameter(&params.starting_note, preset.starting_note);
    setter.end_set_parameter(&params.starting_note);

    setter.begin_set_parameter(&params.bpm);
    setter.set_parameter(&params.bpm, preset.bpm);
    setter.end_set_parameter(&params.bpm);

    setter.begin_set_parameter(&params.slice_algo);
    setter.set_parameter(&params.slice_algo, preset.slice_algo);
    setter.end_set_parameter(&params.slice_algo);

    setter.begin_set_parameter(&params.hold_continue);
    setter.set_parameter(&params.hold_continue, preset.hold_continue);
    setter.end_set_parameter(&params.hold_continue);

    setter.begin_set_parameter(&params.gate_on_release);
    setter.set_parameter(&params.gate_on_release, preset.gate_on_release);
    setter.end_set_parameter(&params.gate_on_release);

    setter.begin_set_parameter(&params.playback_speed);
    setter.set_parameter(&params.playback_speed, preset.playback_speed);
    setter.end_set_parameter(&params.playback_speed);

    setter.begin_set_parameter(&params.pitch_semitones);
    setter.set_parameter(&params.pitch_semitones, preset.pitch_semitones);
    setter.end_set_parameter(&params.pitch_semitones);

    setter.begin_set_parameter(&params.trim_silence);
    setter.set_parameter(&params.trim_silence, preset.trim_silence);
    setter.end_set_parameter(&params.trim_silence);

    setter.begin_set_parameter(&params.pad_chop_channel);
    setter.set_parameter(&params.pad_chop_channel, preset.pad_chop_channel);
    setter.end_set_parameter(&params.pad_chop_channel);

    setter.begin_set_parameter(&params.pitch_reference_channel);
    setter.set_parameter(&params.pitch_reference_channel, preset.pitch_reference_channel);
    setter.end_set_parameter(&params.pitch_reference_channel);

    if let Some(path) = preset.sample_path.as_ref() {
        let path_string = path.to_string_lossy().into_owned();
        if !path_string.is_empty() {
            state.last_loaded_path = Some(path_string.clone());
            ClapChop::request_sample_load(path_string, params.clone(), shared.clone());
        }
    } else {
        state.last_loaded_path = None;
    }

    Ok(())
}

fn preset_dialog_initial_path(state: &GuiState) -> Option<PathBuf> {
    state
        .last_preset_path
        .as_deref()
        .or(state.last_loaded_path.as_deref())
        .map(PathBuf::from)
}

#[repr(C)]
struct EguiStateRepr {
    size: AtomicCell<(u32, u32)>,
    requested_size: AtomicCell<Option<(u32, u32)>>,
    open: AtomicBool,
}

fn maybe_resize_editor(
    _egui_ctx: &egui::Context,
    editor_state: &Arc<EguiState>,
    content_size: egui::Vec2,
) {
    if content_size == egui::Vec2::ZERO {
        return;
    }

    // Use logical points. The host expects logical sizes, not physical pixels.
    let mut desired_width = content_size.x.ceil() as u32;
    let mut desired_height = (content_size.y + 10.0).ceil() as u32;

    desired_width = desired_width.max(DEFAULT_EDITOR_WIDTH);
    // Cap the maximum height to prevent it from growing too large
    let max_height = 700u32;
    desired_height = desired_height.min(max_height);
    
    let desired = (desired_width, desired_height);
    let current = editor_state.size();

    let width_diff = desired.0.abs_diff(current.0);
    let height_diff = desired.1.abs_diff(current.1);
    
    // Only resize if difference is significant
    if width_diff <= 15 && height_diff <= 15 {
        return;
    }

    if let Ok(inner) = Arc::try_unwrap(EguiState::from_size(desired.0, desired.1)) {
        editor_state.set(inner);
    }

    if editor_state.is_open() {
        unsafe {
            request_resize(editor_state.as_ref(), desired);
        }
    }
}

unsafe fn request_resize(state: &EguiState, new_size: (u32, u32)) {
    let repr = state as *const EguiState as *const EguiStateRepr;
    // SAFETY: `EguiStateRepr` mirrors `nih_plug_egui::EguiState`'s field layout. This taps into the
    //         internal resize mechanism that the resizable window helper uses so we can nudge the
    //         host to resize programmatically.
    (*repr).requested_size.store(Some(new_size));
}

fn pad_grid(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    shared: &Arc<RwLock<SharedState>>,
    start_note: u8,
    scale: f32,
    pad_count: usize,
) {
    let pad_count = pad_count.min(crate::MAX_PADS);
    let mut trimmed_note_offs = Vec::new();
    if pad_count < state.pressed_pads.len() {
        for (idx, pressed) in state.pressed_pads.iter().enumerate().skip(pad_count) {
            if *pressed {
                trimmed_note_offs.push(idx);
            }
        }
        state.pressed_pads.truncate(pad_count);
    } else if pad_count > state.pressed_pads.len() {
        state.pressed_pads.resize(pad_count, false);
    }

    let pointer_pressed = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
    let pointer_released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));

    let mut note_on_events: Vec<usize> = Vec::new();

    let shared_snapshot = shared.read().clone();
    let slices = shared_snapshot.slices.regions;
    let visual_states = shared_snapshot.pad_visual_state;

    let cols = if pad_count == 0 {
        1
    } else {
        (pad_count as f32).sqrt().ceil().max(1.0) as usize
    };
    let cols = cols.clamp(1, 8);
    let rows = if pad_count == 0 {
        0
    } else {
        (pad_count + cols - 1) / cols
    };
    let pad_w = PAD_WIDTH * scale;
    let pad_h = PAD_HEIGHT * scale;
    let pad_spacing = PAD_SPACING * scale;
    let grid_width = if pad_count == 0 {
        pad_w
    } else {
        cols as f32 * pad_w + pad_spacing * (cols.saturating_sub(1) as f32)
    };
    let grid_height = if pad_count == 0 {
        pad_h
    } else {
        rows as f32 * pad_h + pad_spacing * (rows.saturating_sub(1) as f32)
    };
    ui.set_min_width(grid_width);
    ui.set_min_height(grid_height);

    egui::Grid::new("pad-grid")
        .spacing(egui::vec2(pad_spacing, pad_spacing))
        .show(ui, |ui| {
            for row in 0..rows {
                let display_row = rows - 1 - row;
                for col in 0..cols {
                    let pad_index = display_row * cols + col;
                    if pad_index >= pad_count {
                        ui.add_enabled(
                            false,
                            egui::Button::new("").min_size(egui::vec2(pad_w, pad_h)),
                        );
                        continue;
                    }

                    let midi_note = start_note + pad_index as u8;
                    let note_name = midi_note_name(midi_note);
                    let label = format!("{:02}\n{}", pad_index + 1, note_name);
                    let slice_info = slices.get(pad_index).copied();
                    let pad_active = state.pressed_pads[pad_index]
                        || visual_states.get(pad_index).copied().unwrap_or(false);

                    let mut button = egui::Button::new(label).min_size(egui::vec2(pad_w, pad_h));
                    if pad_active {
                        button = button.fill(egui::Color32::from_rgb(120, 180, 255));
                    }

                    let response = ui.add_enabled(slice_info.is_some(), button);

                    if response.hovered() {
                        if let Some((start, end)) = slice_info {
                            let frames = end.saturating_sub(start);
                            response.clone().on_hover_text(format!("frames: {frames}"));
                        }
                    }

                    if pointer_pressed
                        && response.hovered()
                        && response.enabled()
                        && !state.pressed_pads[pad_index]
                    {
                        state.pressed_pads[pad_index] = true;
                        note_on_events.push(pad_index);
                    }
                }
                ui.end_row();
            }
        });

    let mut note_off_events: Vec<usize> = Vec::new();
    if pointer_released {
        for (idx, pressed) in state.pressed_pads.iter_mut().enumerate() {
            if *pressed {
                *pressed = false;
                note_off_events.push(idx);
            }
        }
    }

    note_off_events.extend(trimmed_note_offs);

    if !note_on_events.is_empty() || !note_off_events.is_empty() {
        let mut guard = shared.write();
        for pad_index in note_on_events {
            guard.pending_pad_events.push(UiPadEvent::NoteOn {
                pad_index,
                velocity: 1.0,
            });
        }
        for pad_index in note_off_events {
            guard
                .pending_pad_events
                .push(UiPadEvent::NoteOff { pad_index });
        }
    }
}

fn scale_style_in_place(style: &mut egui::Style, scale: f32) {
    // Scale text sizes
    for (_ts, font_id) in style.text_styles.iter_mut() {
        font_id.size *= scale;
    }
    // Scale common spacing attributes
    style.spacing.item_spacing *= scale;
    style.spacing.button_padding *= scale;
    style.spacing.window_margin *= scale;
    style.spacing.indent *= scale;
    style.spacing.interact_size *= scale;
}
fn midi_note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "c", "c#", "d", "d#", "e", "f", "f#", "g", "g#", "a", "a#", "b",
    ];
    let octave = (note as i32 / 12) - 1;
    let name = NAMES[(note % 12) as usize];
    format!("{}{}", name, octave)
}

fn status_section(
    ui: &mut egui::Ui,
    state: &GuiState,
    shared: &Arc<RwLock<SharedState>>,
    params: &Arc<ClapChopParams>,
) {
    if let Some(message) = &state.preset_message {
        ui.colored_label(egui::Color32::from_rgb(100, 200, 140), message);
    }

    if let Some(error) = &state.preset_error {
        ui.colored_label(egui::Color32::RED, format!("preset error: {error}"));
    }

    let shared = shared.read();

    if shared.loading {
        ui.label(egui::RichText::new("loading sample...").italics());
    }

    if let Some(error) = &shared.last_error {
        ui.colored_label(egui::Color32::RED, format!("error: {error}"));
    }

    if let Some(sample) = shared.sample.as_ref() {
        let filename = shared
            .loaded_path
            .as_ref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");

        ui.label(format!(
            "loaded: {} ({} Hz, {} frames, {})",
            filename,
            sample.sample_rate,
            sample.num_frames,
            if sample.stereo { "stereo" } else { "mono" }
        ));
    } else {
        ui.label("no sample loaded :(");
    }

    ui.separator();

    // UI Scale selector
    ui.horizontal(|ui| {
        ui.label("ui scale:");
        let current_scale = *params.ui_scale.read();
        let current_label = if (current_scale - 2.0).abs() < 0.01 {
            "200%"
        } else if (current_scale - 1.5).abs() < 0.01 {
            "150%"
        } else {
            "100%"
        };
        egui::ComboBox::from_id_salt("ui_scale_combo")
            .selected_text(current_label)
            .show_ui(ui, |ui| {
                let set_scale = |val: f32| {
                    *params.ui_scale.write() = val;
                };
                if ui
                    .selectable_label((current_scale - 1.0).abs() < 0.01, "100%")
                    .clicked()
                {
                    set_scale(1.0);
                }
                if ui
                    .selectable_label((current_scale - 1.5).abs() < 0.01, "150%")
                    .clicked()
                {
                    set_scale(1.5);
                }
                if ui
                    .selectable_label((current_scale - 2.0).abs() < 0.01, "200%")
                    .clicked()
                {
                    set_scale(2.0);
                }
            });
    });
}
