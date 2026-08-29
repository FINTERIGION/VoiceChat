use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer as _, Observer as _, Producer as _};
use tokio::sync::mpsc as tokio_mpsc;

use super::gate::{rms, LevelThrottle};
use super::resample::{f32_to_pcm16le, Resampler};
use super::ring;

const TARGET_HZ: u32 = 16_000;
const FRAME_MS: u32 = 20;
const FRAME_SAMPLES: usize = (TARGET_HZ * FRAME_MS / 1000) as usize; // 320
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Owns the input device stream's lifetime and the capturing gate. The stream
/// itself lives entirely inside the dedicated thread and never crosses a
/// thread boundary, so this handle only exposes Send+Sync-safe fields.
///
/// The frame/level receivers are returned separately (not as fields here) so
/// callers can hold `&mut` borrows of each independently — e.g. inside a
/// `tokio::select!` that also needs `&self` for `set_capturing`.
pub struct CaptureControl {
    capturing: Arc<AtomicBool>,
    shutdown_tx: std_mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}

impl CaptureControl {
    pub fn set_capturing(&self, on: bool) {
        self.capturing.store(on, Ordering::Release);
    }
}

impl Drop for CaptureControl {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub type FrameReceiver = tokio_mpsc::UnboundedReceiver<Vec<u8>>;
pub type LevelReceiver = tokio_mpsc::UnboundedReceiver<f32>;

pub fn start() -> Result<(CaptureControl, FrameReceiver, LevelReceiver), String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("未找到麦克风设备")?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let sample_format = supported.sample_format();
    let stream_config: cpal::StreamConfig = supported.into();
    let device_hz = stream_config.sample_rate;
    let channels = stream_config.channels as usize;

    let (shutdown_tx, shutdown_rx) = std_mpsc::channel::<()>();
    let (frame_tx, frame_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
    let (level_tx, level_rx) = tokio_mpsc::unbounded_channel::<f32>();
    let capturing = Arc::new(AtomicBool::new(false));
    let capturing_cb = capturing.clone();

    let thread = thread::Builder::new()
        .name("voicechat-capture".into())
        .spawn(move || {
            let (mut prod, mut cons) = ring::spsc(device_hz as usize * 2);

            // `Xrun` means WASAPI detected a buffer discontinuity — the OS
            // dropped a few frames because this thread (or another one
            // competing for the CPU) didn't service the device in time.
            // cpal reports it purely as a diagnostic: it doesn't stop the
            // stream, `process_input` carries on with the next buffer right
            // after. Logging it as an error every time it happens under
            // real-world CPU contention (mid-conversation, with playback and
            // network I/O also active) is misleadingly alarming for a
            // one-frame glitch that's usually inaudible — unlike every other
            // `cpal::Error`, which does end the stream and warrants a loud
            // log.
            let err_fn = |e: cpal::Error| {
                if e.kind() == cpal::ErrorKind::Xrun {
                    tracing::debug!("capture stream xrun (buffer discontinuity): {e}");
                } else {
                    tracing::error!("capture stream error: {e}");
                }
            };
            // Each callback owns a scratch buffer for the mono downmix so no
            // allocation happens on the realtime audio thread: `Vec::resize`
            // to the same length every call is a no-op once warmed up.
            let stream = match sample_format {
                cpal::SampleFormat::F32 => {
                    let mut scratch: Vec<f32> = Vec::new();
                    device.build_input_stream(
                        stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            downmix_push(&mut prod, data, channels, &mut scratch, |s| s);
                        },
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::I16 => {
                    let mut scratch: Vec<f32> = Vec::new();
                    device.build_input_stream(
                        stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            downmix_push(&mut prod, data, channels, &mut scratch, |s| {
                                s as f32 / i16::MAX as f32
                            });
                        },
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let mut scratch: Vec<f32> = Vec::new();
                    device.build_input_stream(
                        stream_config,
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            downmix_push(&mut prod, data, channels, &mut scratch, |s| {
                                (s as f32 / u16::MAX as f32) * 2.0 - 1.0
                            });
                        },
                        err_fn,
                        None,
                    )
                }
                other => {
                    tracing::error!("unsupported input sample format: {other:?}");
                    return;
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("failed to build capture stream: {e}");
                    return;
                }
            };
            // Deliberately not started here. A cpal stream is built paused,
            // and it stays that way until the user actually opens the mic:
            // starting it at launch would hold an active capture session — and
            // light up the OS microphone indicator — for the entire time the
            // app is running, including while it sits idle. `set_capturing`
            // drives play/pause from the loop below instead.
            let mut mic_live = false;

            let chunk_size = ((device_hz as u64 * FRAME_MS as u64) / 1000).max(1) as usize;
            let mut resampler = Resampler::new(device_hz, TARGET_HZ, chunk_size);
            let mut raw_leftover: Vec<f32> = Vec::with_capacity(chunk_size * 2);
            let mut pcm16k_leftover: Vec<f32> = Vec::with_capacity(FRAME_SAMPLES * 2);
            let mut level_throttle = LevelThrottle::new(20.0);

            loop {
                match shutdown_rx.try_recv() {
                    Ok(()) | Err(std_mpsc::TryRecvError::Disconnected) => break,
                    Err(std_mpsc::TryRecvError::Empty) => {}
                }

                // Start or stop the device to match what the user asked for.
                // On WASAPI `pause` issues IAudioClient::Stop, so no capture
                // session is held open while the mic is closed.
                let want_live = capturing_cb.load(Ordering::Acquire);
                if want_live != mic_live {
                    let outcome = if want_live {
                        stream.play()
                    } else {
                        stream.pause()
                    };
                    match outcome {
                        Ok(()) => {
                            mic_live = want_live;
                            if !want_live {
                                // Discard everything captured up to the moment
                                // the mic closed: reopening it must not replay
                                // audio from the previous turn. Reset the
                                // meter too, or the UI keeps showing the last
                                // level forever now that no more arrive.
                                cons.clear();
                                raw_leftover.clear();
                                pcm16k_leftover.clear();
                                let _ = level_tx.send(0.0);
                            }
                        }
                        // Leave `mic_live` alone and retry on the next tick:
                        // a device that refuses to start now may succeed once
                        // it is no longer contended.
                        Err(e) => tracing::error!(
                            "failed to {} capture stream: {e}",
                            if want_live { "start" } else { "pause" }
                        ),
                    }
                }
                if !mic_live {
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }

                let available = cons.occupied_len();
                if available == 0 {
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }
                let start = raw_leftover.len();
                raw_leftover.resize(start + available, 0.0);
                let popped = cons.pop_slice(&mut raw_leftover[start..]);
                raw_leftover.truncate(start + popped);

                if level_throttle.should_emit() {
                    let _ = level_tx.send(rms(&raw_leftover[start..]));
                }

                let need = resampler.input_frames_next();
                let mut offset = 0;
                while raw_leftover.len() - offset >= need {
                    let out = resampler.process(&raw_leftover[offset..offset + need]);
                    pcm16k_leftover.extend_from_slice(out);
                    offset += need;
                }
                raw_leftover.drain(0..offset);

                if capturing_cb.load(Ordering::Acquire) {
                    let mut frame_offset = 0;
                    while pcm16k_leftover.len() - frame_offset >= FRAME_SAMPLES {
                        let frame =
                            &pcm16k_leftover[frame_offset..frame_offset + FRAME_SAMPLES];
                        if frame_tx.send(f32_to_pcm16le(frame)).is_err() {
                            break;
                        }
                        frame_offset += FRAME_SAMPLES;
                    }
                    pcm16k_leftover.drain(0..frame_offset);
                } else {
                    pcm16k_leftover.clear();
                }

                thread::sleep(POLL_INTERVAL);
            }

            drop(stream);
        })
        .map_err(|e| e.to_string())?;

    let control = CaptureControl {
        capturing,
        shutdown_tx,
        thread: Some(thread),
    };
    Ok((control, frame_rx, level_rx))
}

/// Converts each sample to f32 via `to_f32`, downmixes to mono if needed, and
/// pushes into the ring buffer — all without allocating: `scratch` is reused
/// across calls (only its first call, which grows it to size, allocates).
fn downmix_push<T: Copy>(
    prod: &mut ring::Producer,
    data: &[T],
    channels: usize,
    scratch: &mut Vec<f32>,
    to_f32: impl Fn(T) -> f32,
) {
    if channels <= 1 {
        scratch.clear();
        scratch.extend(data.iter().map(|&s| to_f32(s)));
        prod.push_slice(scratch);
        return;
    }
    let frames = data.len() / channels;
    scratch.resize(frames, 0.0);
    for (out, chunk) in scratch.iter_mut().zip(data.chunks_exact(channels)) {
        let sum: f32 = chunk.iter().map(|&s| to_f32(s)).sum();
        *out = sum / channels as f32;
    }
    prod.push_slice(scratch);
}
