use std::io::Cursor;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use base64::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{SampleFormat, WavSpec, WavWriter};

use crate::audio::resample::Resampler;

const MAX_RECORD_SECS: u32 = 60;

/// The fastest rate a recording is kept at: what Voice Design's previews come
/// back as, and plenty for cloning. Microphones often default to 48 kHz
/// stereo — four times the bytes of 24 kHz mono — which took a recording near
/// the cap past what `voice::sample` keeps.
const TARGET_HZ: u32 = 24_000;

pub struct RecorderHandle {
    shutdown_tx: std_mpsc::Sender<()>,
    buffer: Arc<Mutex<Vec<i16>>>,
    sample_rate: u32,
    channels: u16,
    thread: Option<thread::JoinHandle<()>>,
}

pub fn start() -> Result<RecorderHandle, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| crate::tr!("No microphone device found", "未找到麦克风设备"))?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let sample_format = supported.sample_format();
    let stream_config: cpal::StreamConfig = supported.into();
    let sample_rate = stream_config.sample_rate;
    let channels = stream_config.channels;

    let buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let buffer_cb = buffer.clone();
    let (shutdown_tx, shutdown_rx) = std_mpsc::channel::<()>();
    let max_samples = MAX_RECORD_SECS as usize * sample_rate as usize * channels as usize;

    let thread = thread::Builder::new()
        .name("voice-chat-recorder".into())
        .spawn(move || {
            let err_fn = |e| tracing::error!("recorder stream error: {e}");
            let stream = match sample_format {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        push_capped(
                            &buffer_cb,
                            max_samples,
                            data.iter()
                                .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16),
                        );
                    },
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::I16 => device.build_input_stream(
                    stream_config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        push_capped(&buffer_cb, max_samples, data.iter().copied());
                    },
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::U16 => device.build_input_stream(
                    stream_config,
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        push_capped(
                            &buffer_cb,
                            max_samples,
                            data.iter()
                                .map(|&s| (s as i32 - i16::MAX as i32 - 1) as i16),
                        );
                    },
                    err_fn,
                    None,
                ),
                other => {
                    tracing::error!("unsupported recorder sample format: {other:?}");
                    return;
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("failed to build recorder stream: {e}");
                    return;
                }
            };
            if let Err(e) = stream.play() {
                tracing::error!("failed to start recorder stream: {e}");
                return;
            }

            let _ = shutdown_rx.recv();
            drop(stream);
        })
        .map_err(|e| e.to_string())?;

    Ok(RecorderHandle {
        shutdown_tx,
        buffer,
        sample_rate,
        channels,
        thread: Some(thread),
    })
}

fn push_capped(buffer: &Mutex<Vec<i16>>, max: usize, samples: impl Iterator<Item = i16>) {
    let mut buf = buffer.lock().expect("recorder buffer lock poisoned");
    if buf.len() >= max {
        return;
    }
    let remaining = max - buf.len();
    buf.extend(samples.take(remaining));
}

impl RecorderHandle {
    /// Stops the stream, encodes what was captured as a 16-bit mono WAV (see
    /// `encode_wav`), and returns it as a `data:` URI ready both for
    /// `<audio>` preview and as the clone API's `url` input.
    pub fn stop_and_encode(mut self) -> Result<String, String> {
        let _ = self.shutdown_tx.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let samples = self
            .buffer
            .lock()
            .map_err(|_| "recorder buffer lock poisoned".to_string())?
            .clone();
        if samples.is_empty() {
            return Err(crate::tr!("No audio was recorded", "没有录到音频").to_string());
        }

        let wav_bytes = encode_wav(&samples, self.sample_rate, self.channels)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);
        Ok(format!("data:audio/wav;base64,{b64}"))
    }
}

/// `samples`, interleaved as the device captured them, as a WAV file: mixed
/// down to mono — a voice has one channel whatever the microphone reports —
/// and brought down to `TARGET_HZ` if the device ran faster. A slower device
/// is kept at its own rate; resampling up would add bytes and nothing else.
fn encode_wav(samples: &[i16], sample_rate: u32, channels: u16) -> Result<Vec<u8>, String> {
    let to_hz = sample_rate.min(TARGET_HZ);
    let mono = resample(&downmix(samples, channels), sample_rate, to_hz);

    let spec = WavSpec {
        channels: 1,
        sample_rate: to_hz,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
        for &s in &mono {
            let s = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(s).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    Ok(cursor.into_inner())
}

/// Averages each frame's channels into one, as the live capture does.
fn downmix(samples: &[i16], channels: u16) -> Vec<f32> {
    let channels = usize::from(channels.max(1));
    samples
        .chunks_exact(channels)
        .map(|frame| {
            let sum: f32 = frame.iter().map(|&s| f32::from(s) / i16::MAX as f32).sum();
            sum / channels as f32
        })
        .collect()
}

fn resample(mono: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || mono.is_empty() {
        return mono.to_vec();
    }
    const CHUNK: usize = 1024;
    let expected = (mono.len() as u64 * u64::from(to_hz) / u64::from(from_hz)) as usize;
    let mut resampler = Resampler::new(from_hz, to_hz, CHUNK);
    let mut out = Vec::with_capacity(expected + CHUNK);
    let mut offset = 0;
    while offset < mono.len() {
        let need = resampler.input_frames_next();
        let end = offset + need;
        if end <= mono.len() {
            out.extend_from_slice(resampler.process(&mono[offset..end]));
        } else {
            // The resampler only takes whole chunks: pad the last one with
            // silence, then drop what the padding turned into.
            let mut tail = mono[offset..].to_vec();
            tail.resize(need, 0.0);
            out.extend_from_slice(resampler.process(&tail));
        }
        offset = end;
    }
    out.truncate(expected);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_back(wav: &[u8]) -> (WavSpec, usize) {
        let reader = hound::WavReader::new(Cursor::new(wav)).expect("a readable WAV");
        (reader.spec(), reader.len() as usize)
    }

    fn peak(wav: &[u8]) -> i16 {
        hound::WavReader::new(Cursor::new(wav))
            .expect("a readable WAV")
            .samples::<i16>()
            .map(|s| s.expect("a sample").saturating_abs())
            .max()
            .unwrap_or(0)
    }

    /// Before this, a full-length recording on a 48 kHz stereo microphone
    /// was over 11 MB, and so never kept as its voice's sample.
    #[test]
    fn a_recording_at_the_cap_fits_in_a_kept_sample() {
        let at_cap = MAX_RECORD_SECS as usize * TARGET_HZ as usize * 2 + 44;
        assert!(at_cap < crate::voice::sample::MAX_BYTES, "{at_cap}");
    }

    #[test]
    fn mixes_down_to_mono_at_the_target_rate() {
        // One second of 48 kHz stereo: a tone on the left, silence on the right.
        let stereo: Vec<i16> = (0..48_000)
            .flat_map(|i| {
                let t = i as f32 / 48_000.0;
                [((t * 440.0 * std::f32::consts::TAU).sin() * 16_000.0) as i16, 0]
            })
            .collect();
        let wav = encode_wav(&stereo, 48_000, 2).expect("encode");
        let (spec, frames) = read_back(&wav);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, TARGET_HZ);
        assert_eq!(frames, 24_000, "the same second, at half the rate");
        // Averaged with the silent channel: half the tone's 16 000 peak.
        assert!((7_500..=8_500).contains(&peak(&wav)), "{}", peak(&wav));
        assert_eq!(
            crate::voice::sample::Format::sniff(&wav),
            Some(crate::voice::sample::Format::Wav)
        );
    }

    #[test]
    fn keeps_a_slower_device_at_its_own_rate() {
        let wav = encode_wav(&vec![0i16; 16_000], 16_000, 1).expect("encode");
        let (spec, frames) = read_back(&wav);
        assert_eq!((spec.channels, spec.sample_rate), (1, 16_000));
        assert_eq!(frames, 16_000);
    }
}
