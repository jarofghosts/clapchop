use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use crate::slicing::Slices;

#[derive(Clone)]
pub struct LoadedSample {
    pub data_l: Arc<Vec<f32>>, // mono uses L only; stereo uses both
    pub data_r: Arc<Vec<f32>>, // empty if mono
    pub sample_rate: f32,
    pub num_frames: usize,
    pub stereo: bool,
}

pub fn load_sample(path: &str) -> Result<LoadedSample, String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());

    match ext.as_deref() {
        Some("wav") | Some("wave") => load_wav(path),
        Some("mp3") => load_mp3(path),
        _ => match load_wav(path) {
            Ok(sample) => Ok(sample),
            Err(wav_err) => match load_mp3(path) {
                Ok(sample) => Ok(sample),
                Err(mp3_err) => Err(format!(
                    "unsupported audio format. wav decoder error: {wav_err}; mp3 decoder error: {mp3_err}"
                )),
            },
        },
    }
}

fn load_wav(path: &str) -> Result<LoadedSample, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("failed to open wav: {e}"))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate as f32;
    let channels = spec.channels as usize;

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => match spec.bits_per_sample {
            8 => reader
                .samples::<i8>()
                .map(|s| (s.unwrap_or(0) as f32) / 128.0)
                .collect(),
            16 => reader
                .samples::<i16>()
                .map(|s| (s.unwrap_or(0) as f32) / 32768.0)
                .collect(),
            24 => reader
                .samples::<i32>()
                .map(|s| ((s.unwrap_or(0) >> 8) as f32) / 8388608.0)
                .collect(),
            32 => reader
                .samples::<i32>()
                .map(|s| (s.unwrap_or(0) as f32) / 2147483648.0)
                .collect(),
            _ => return Err("unsupported bit depth".to_string()),
        },
    };

    make_loaded_sample(interleaved, channels, sample_rate, "wav")
}

fn load_mp3(path: &str) -> Result<LoadedSample, String> {
    let file = File::open(path).map_err(|e| format!("failed to open mp3: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("failed to probe mp3: {e}"))?;
    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or_else(|| "mp3 has no audio track".to_string())?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| "mp3 missing sample rate".to_string())? as f32;

    let mut channels = codec_params.channels.map(|c| c.count()).unwrap_or(0);

    if channels > 2 {
        return Err("only mono or stereo mp3 supported".to_string());
    }

    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("failed to create mp3 decoder: {e}"))?;

    let mut interleaved: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err("mp3 decoder reset required".to_string());
            }
            Err(err) => return Err(format!("failed reading mp3 packet: {err}")),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::DecodeError(_)) => {
                continue;
            }
            Err(err) => return Err(format!("failed decoding mp3 packet: {err}")),
        };

        let spec = *decoded.spec();
        let decoded_channels = spec.channels.count();
        if decoded_channels == 0 || decoded_channels > 2 {
            return Err("only mono or stereo mp3 supported".to_string());
        }
        if channels == 0 {
            channels = decoded_channels;
        } else if channels != decoded_channels {
            return Err("mp3 channel count changed mid-stream".to_string());
        }

        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        interleaved.extend_from_slice(buf.samples());
    }

    if channels == 0 {
        return Err("mp3 contained no audio samples".to_string());
    }

    make_loaded_sample(interleaved, channels, sample_rate, "mp3")
}

fn make_loaded_sample(
    interleaved: Vec<f32>,
    channels: usize,
    sample_rate: f32,
    format_name: &str,
) -> Result<LoadedSample, String> {
    match channels {
        1 => {
            let num_frames = interleaved.len();
            Ok(LoadedSample {
                data_l: Arc::new(interleaved),
                data_r: Arc::new(Vec::new()),
                sample_rate,
                num_frames,
                stereo: false,
            })
        }
        2 => {
            if interleaved.len() % 2 != 0 {
                return Err(format!("{format_name} data had incomplete stereo frame"));
            }
            let num_frames = interleaved.len() / 2;
            let mut l = Vec::with_capacity(num_frames);
            let mut r = Vec::with_capacity(num_frames);
            for chunk in interleaved.chunks_exact(2) {
                l.push(chunk[0]);
                r.push(chunk[1]);
            }
            Ok(LoadedSample {
                data_l: Arc::new(l),
                data_r: Arc::new(r),
                sample_rate,
                num_frames,
                stereo: true,
            })
        }
        _ => Err(format!("only mono or stereo {format_name} supported")),
    }
}

#[derive(Clone)]
pub struct VoiceState {
    pub active: bool,
    pub pos: f64,       // in source sample frames (not host)
    pub slice_end: f64, // end position in frames
    pub velocity: f32,
    pub hold_continue: bool, // continue beyond slice_end while note held
    pub held: bool,
    pub gate_on_release: bool,
}

pub struct SamplePlayer {
    sample: Option<LoadedSample>,
    slices: Slices,
    pub host_sample_rate: f32,
    src_to_host_ratio: f64,
    voices: Vec<VoiceState>,
}

impl SamplePlayer {
    pub fn new(num_voices: usize) -> Self {
        let mut player = Self {
            sample: None,
            slices: Slices::empty(),
            host_sample_rate: 44100.0,
            src_to_host_ratio: 1.0,
            voices: Vec::new(),
        };
        player.set_num_voices(num_voices);
        player
    }

    pub fn set_num_voices(&mut self, num_voices: usize) {
        if self.voices.len() == num_voices {
            return;
        }

        let mut voices = Vec::with_capacity(num_voices);
        for i in 0..num_voices {
            if let Some(existing) = self.voices.get(i).cloned() {
                voices.push(existing);
            } else {
                voices.push(VoiceState {
                    active: false,
                    pos: 0.0,
                    slice_end: 0.0,
                    velocity: 0.0,
                    hold_continue: false,
                    held: false,
                    gate_on_release: false,
                });
            }
        }
        self.voices = voices;
    }

    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    pub fn set_sample_rate(&mut self, rate: f32) {
        self.host_sample_rate = rate;
        self.update_ratio();
    }

    pub fn set_sample(&mut self, sample: LoadedSample) {
        self.src_to_host_ratio = (sample.sample_rate as f64) / (self.host_sample_rate as f64);
        self.sample = Some(sample);
    }

    pub fn set_slices(&mut self, slices: Slices) {
        self.slices = slices;
    }

    pub fn reset(&mut self) {
        for v in &mut self.voices {
            v.active = false;
            v.held = false;
            v.gate_on_release = false;
        }
    }

    pub fn note_on(
        &mut self,
        pad_index: usize,
        velocity: f32,
        hold_continue: bool,
        gate_on_release: bool,
    ) {
        if let Some((start, end)) = self.slices.get_slice_bounds(pad_index) {
            if let Some(v) = self.voices.get_mut(pad_index) {
                v.active = true;
                v.held = true;
                v.pos = start as f64;
                v.slice_end = end as f64;
                v.velocity = velocity.max(0.0001);
                v.hold_continue = hold_continue;
                v.gate_on_release = gate_on_release;
            }
        }
    }

    pub fn note_off(&mut self, pad_index: usize) {
        if let Some(v) = self.voices.get_mut(pad_index) {
            v.held = false;
            if v.gate_on_release || !v.hold_continue {
                v.active = false;
            }
        }
    }

    pub fn process(&mut self) -> (f32, f32) {
        let Some(sample) = &self.sample else {
            return (0.0, 0.0);
        };

        let mut l_acc = 0.0f32;
        let mut r_acc = 0.0f32;

        for v in &mut self.voices {
            if !v.active {
                continue;
            }

            let (l, r) = read_interp(sample, v.pos);
            let amp = v.velocity;
            l_acc += l * amp;
            r_acc += r * amp;

            // Advance in source frames relative to host rate.
            v.pos += self.src_to_host_ratio;

            let beyond_slice = v.pos >= v.slice_end;
            if beyond_slice && !v.hold_continue {
                v.active = false;
            }
            if beyond_slice && !v.held {
                v.active = false;
            }

            if v.pos >= sample.num_frames as f64 {
                v.active = false;
            }
        }

        (l_acc, r_acc)
    }

    fn update_ratio(&mut self) {
        if let Some(s) = &self.sample {
            self.src_to_host_ratio = (s.sample_rate as f64) / (self.host_sample_rate as f64);
        }
    }
}

fn read_interp(sample: &LoadedSample, pos: f64) -> (f32, f32) {
    let idx = pos.floor() as isize;
    let frac = (pos - (idx as f64)) as f32;
    let i0 = idx.max(0) as usize;
    let i1 = (i0 + 1).min(sample.num_frames - 1);

    let l0 = sample.data_l[i0];
    let l1 = sample.data_l[i1];
    let l = l0 + (l1 - l0) * frac;

    let r = if sample.stereo {
        let r0 = sample.data_r[i0];
        let r1 = sample.data_r[i1];
        r0 + (r1 - r0) * frac
    } else {
        l
    };

    (l, r)
}
