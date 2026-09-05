use std::io::Cursor;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use base64::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{SampleFormat, WavSpec, WavWriter};

const MAX_RECORD_SECS: u32 = 60;

pub struct RecorderHandle {
    shutdown_tx: std_mpsc::Sender<()>,
    buffer: Arc<Mutex<Vec<i16>>>,
    sample_rate: u32,
    channels: u16,
    thread: Option<thread::JoinHandle<()>>,
}

pub fn start() -> Result<RecorderHandle, String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| crate::tr!("No microphone device found", "未找到麦克风设备"))?;
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
        .name("voicechat-recorder".into())
        .spawn(move || {
            let err_fn = |e| tracing::error!("recorder stream error: {e}");
            let stream = match sample_format {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        push_capped(&buffer_cb, max_samples, data.iter().map(|&s| {
                            (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                        }));
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
                            data.iter().map(|&s| (s as i32 - i16::MAX as i32 - 1) as i16),
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
    /// Stops the stream, encodes what was captured as a 16-bit WAV, and
    /// returns it as a `data:` URI ready both for `<audio>` preview and as
    /// the clone API's `url` input.
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

        let spec = WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
            for &s in &samples {
                writer.write_sample(s).map_err(|e| e.to_string())?;
            }
            writer.finalize().map_err(|e| e.to_string())?;
        }
        let wav_bytes = cursor.into_inner();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);
        Ok(format!("data:audio/wav;base64,{b64}"))
    }
}
