use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self as std_mpsc, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer as _, Observer as _, Producer as _};

use super::resample::{Resampler, pcm16le_to_f32};
use super::ring;

const SOURCE_HZ: u32 = 24_000;
const CHUNK_MS: u32 = 20;
// 250ms rather than the original 120ms: real audio.delta arrival has been
// observed with gaps up to ~200ms (network/generation jitter), and since
// priming is now sticky per response (only resets on barge-in, not on every
// transient empty read), a wider one-time cushion at the start of a response
// trades a bit more latency for not running dry mid-sentence.
const PRIMING_MS: u64 = 250;
// How often the message-processing thread rechecks the ring buffer for free
// space while backpressured (see `PlaybackMsg::Append` handling) — not
// latency-critical since this thread isn't the realtime audio callback.
const DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(5);
// How often the message-processing thread checks whether an `on_drained`
// caller's audio has finished playing, while one is waiting. Only paces
// UI (the subtitle fading out), so it can be coarse.
const DRAINED_POLL_INTERVAL: Duration = Duration::from_millis(50);
// Ceiling on audio waiting to be played, as bytes of 24 kHz mono PCM16.
//
// The queue drains at the speed sound comes out of the speakers, so however
// far ahead the server runs, the backlog is bounded by how much it sends.
// Normally that self-corrects: a reply arrives in a burst, plays out, done.
// A server that keeps sending faster than realtime without end would grow
// this without end too, and the sender can't block to push back — it is the
// session actor, which also has to stay responsive to barge-in.
//
// So the queue is capped instead, at roughly a minute of speech: far past
// any real reply (they run seconds), and small enough that the pathological
// case costs a few megabytes rather than everything.
const MAX_QUEUED_BYTES: usize = (SOURCE_HZ as usize) * 2 * 60;

enum PlaybackMsg {
    /// PCM16LE mono bytes at 24 kHz, decoded from a `response.audio.delta`
    /// event, tagged with the `clear_gen` value seen at enqueue time. When a
    /// burst piles up several `Append`s faster than they can drain, a
    /// `clear()` may land while some of them are still queued behind the one
    /// currently being processed — those must be recognized as stale and
    /// skipped outright, not just the one mid-flight when `clear()` was
    /// called, otherwise the interrupted reply keeps audibly playing until
    /// the whole backlog drains.
    Append(Vec<u8>, u64),
    /// Immediately silences and drops any buffered audio (barge-in).
    Clear,
    /// See `PlaybackHandle::on_drained`.
    OnDrained(DrainedFn),
    Shutdown,
}

type DrainedFn = Box<dyn FnOnce() + Send>;

pub struct PlaybackHandle {
    tx: std_mpsc::Sender<PlaybackMsg>,
    /// Bytes sitting in `tx` that the playback thread hasn't taken yet.
    /// Tracked rather than bounding the channel by message count, because
    /// `audio.delta` chunks vary in size — it is the memory that needs a
    /// ceiling, not the number of messages holding it.
    queued_bytes: Arc<AtomicUsize>,
    /// Bumped synchronously by `clear()`. A plain "should clear" bool can't
    /// be shared between two independent readers (the realtime callback and
    /// the message thread's `Append` wait loop below): whichever reads it
    /// first via swap-and-reset would hide the signal from the other. A
    /// monotonic generation counter lets both sides just compare snapshots.
    clear_gen: Arc<AtomicU64>,
    thread: Option<thread::JoinHandle<()>>,
}

impl PlaybackHandle {
    /// Queues decoded audio for playback, dropping it if the backlog is
    /// already at `MAX_QUEUED_BYTES`.
    ///
    /// Dropping is the only option that keeps this non-blocking: the caller
    /// is the session actor's event loop, and stalling it here would also
    /// stall the barge-in and mic commands it is meant to be handling. A
    /// drop is audible — the reply skips — but reaching this point already
    /// means the server has sent a minute of speech faster than a minute,
    /// which is not something a working conversation does.
    pub fn append(&self, pcm24k_bytes: Vec<u8>) {
        let len = pcm24k_bytes.len();
        let queued = self.queued_bytes.load(Ordering::Acquire);
        if queued.saturating_add(len) > MAX_QUEUED_BYTES {
            tracing::warn!(
                queued,
                dropped = len,
                "playback backlog at its ceiling; dropping this chunk"
            );
            return;
        }
        self.queued_bytes.fetch_add(len, Ordering::Release);
        let current_gen = self.clear_gen.load(Ordering::Acquire);
        if self
            .tx
            .send(PlaybackMsg::Append(pcm24k_bytes, current_gen))
            .is_err()
        {
            // The playback thread is gone, so nothing will ever subtract
            // this back off; undo it here or the counter ratchets up and
            // starts rejecting against a queue that no longer exists.
            self.queued_bytes.fetch_sub(len, Ordering::Release);
        }
    }

    /// Synchronous: the generation counter is bumped before this returns, so
    /// the output callback picks it up on its very next tick regardless of
    /// when the caller goes on to send `response.cancel` over the network.
    /// This also lets the message thread's `Append` handling notice mid-chunk
    /// instead of only between messages, since it can now block waiting for
    /// ring buffer space when the network delivers faster than realtime.
    pub fn clear(&self) {
        self.clear_gen.fetch_add(1, Ordering::Release);
        let _ = self.tx.send(PlaybackMsg::Clear);
    }

    /// Calls `f`, on the playback thread, once everything appended before
    /// this has actually come out of the speakers (or been cut by `clear()`).
    /// Audio appended afterwards doesn't hold it up.
    ///
    /// The server finishes sending a reply well before it finishes playing —
    /// for a long one, by many seconds — so this, not `response.done`, is
    /// when the character has actually stopped talking.
    pub fn on_drained(&self, f: impl FnOnce() + Send + 'static) {
        if let Err(std_mpsc::SendError(PlaybackMsg::OnDrained(f))) =
            self.tx.send(PlaybackMsg::OnDrained(Box::new(f)))
        {
            // The playback thread is gone, so nothing is left to play.
            f();
        }
    }
}

impl Drop for PlaybackHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(PlaybackMsg::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn start() -> Result<PlaybackHandle, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| crate::tr!("No speaker device found", "未找到扬声器设备"))?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let sample_format = supported.sample_format();
    let stream_config: cpal::StreamConfig = supported.into();
    let device_hz = stream_config.sample_rate;
    let channels = stream_config.channels as usize;

    let (tx, rx) = std_mpsc::channel::<PlaybackMsg>();
    let clear_gen = Arc::new(AtomicU64::new(0));
    let clear_gen_cb = clear_gen.clone();
    let clear_gen_handle = clear_gen.clone();
    let queued_bytes = Arc::new(AtomicUsize::new(0));
    let queued_bytes_thread = queued_bytes.clone();
    // Frames (mono, at the device rate) the output callback has taken off
    // the ring buffer, whether it played them or dropped them for a
    // `clear()`. Compared against how many the message thread has put on it
    // to tell when `on_drained` callers' audio is through.
    let played_frames = Arc::new(AtomicU64::new(0));
    let played_frames_cb = played_frames.clone();

    let thread = thread::Builder::new()
        .name("voicechat-playback".into())
        .spawn(move || {
            let priming_frames = (device_hz as u64 * PRIMING_MS / 1000) as usize;
            let (mut prod, mut cons) = ring::spsc(device_hz as usize * 2);
            let mut primed = false;
            let mut last_clear_gen: u64 = 0;
            // Diagnostic only: measures how long each genuine mid-response
            // underrun lasts, to tell network/generation jitter apart from a
            // sustained shortfall. Remove once the playback-stutter report is
            // resolved.
            let mut underrun_since: Option<Instant> = None;

            let err_fn = |e| tracing::error!("playback stream error: {e}");
            // Each callback owns its scratch buffers so nothing allocates on
            // the realtime audio thread once warmed up (see downmix_push in
            // capture.rs for the same pattern on the input side).
            let stream = match sample_format {
                cpal::SampleFormat::F32 => {
                    let mut mono_scratch: Vec<f32> = Vec::new();
                    device.build_output_stream(
                        stream_config,
                        move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            fill_output(
                                output,
                                &mut cons,
                                channels,
                                &clear_gen_cb,
                                &mut last_clear_gen,
                                &mut primed,
                                priming_frames,
                                &mut mono_scratch,
                                &mut underrun_since,
                                &played_frames_cb,
                            );
                        },
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::I16 => {
                    let mut scratch: Vec<f32> = Vec::new();
                    let mut mono_scratch: Vec<f32> = Vec::new();
                    device.build_output_stream(
                        stream_config,
                        move |output: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            scratch.resize(output.len(), 0.0);
                            fill_output(
                                &mut scratch,
                                &mut cons,
                                channels,
                                &clear_gen_cb,
                                &mut last_clear_gen,
                                &mut primed,
                                priming_frames,
                                &mut mono_scratch,
                                &mut underrun_since,
                                &played_frames_cb,
                            );
                            for (o, s) in output.iter_mut().zip(scratch.iter()) {
                                *o = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                            }
                        },
                        err_fn,
                        None,
                    )
                }
                other => {
                    tracing::error!("unsupported output sample format: {other:?}");
                    return;
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("failed to build playback stream: {e}");
                    return;
                }
            };
            if let Err(e) = stream.play() {
                tracing::error!("failed to start playback stream: {e}");
                return;
            }

            let chunk_size = ((SOURCE_HZ as u64 * CHUNK_MS as u64) / 1000).max(1) as usize;
            let mut resampler = Resampler::new(SOURCE_HZ, device_hz, chunk_size);
            let mut leftover: Vec<f32> = Vec::new();
            // Frames put on the ring buffer so far — the counterpart to
            // `played_frames`.
            let mut pushed_frames: u64 = 0;
            // `on_drained` callbacks, each with the `pushed_frames` it waits
            // for `played_frames` to reach. Queued in order, so the targets
            // only ever go up.
            let mut waiting: VecDeque<(u64, DrainedFn)> = VecDeque::new();

            loop {
                fire_drained(&mut waiting, &played_frames);
                let msg = if waiting.is_empty() {
                    rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
                } else {
                    rx.recv_timeout(DRAINED_POLL_INTERVAL)
                };
                match msg {
                    Ok(PlaybackMsg::Append(bytes, msg_gen)) => {
                        // Off the backlog the moment it is in hand, ahead of
                        // the staleness check below: a skipped chunk is just
                        // as much no longer queued as a played one, and
                        // missing this on the `continue` path would leak the
                        // count until the ceiling rejected everything.
                        queued_bytes_thread.fetch_sub(bytes.len(), Ordering::Release);
                        // Stale even before we start: this chunk was decoded
                        // and enqueued before a `clear()` that's already
                        // happened (common when a burst has queued several
                        // `Append`s faster than they can drain — a barge-in
                        // can land while most of them are still waiting).
                        // Skip outright instead of playing any of it.
                        if msg_gen != clear_gen.load(Ordering::Acquire) {
                            continue;
                        }
                        leftover.extend(pcm16le_to_f32(&bytes));
                        let need = resampler.input_frames_next();
                        let mut offset = 0;
                        while leftover.len() - offset >= need
                            && clear_gen.load(Ordering::Acquire) == msg_gen
                        {
                            let out = resampler.process(&leftover[offset..offset + need]);
                            // This thread isn't the realtime audio callback, so
                            // when the network delivers faster than playback
                            // consumes, wait for the ring buffer to drain
                            // instead of silently dropping the overflow (which
                            // used to sound like the reply being compressed /
                            // sped up).
                            let mut remaining = out;
                            while !remaining.is_empty() {
                                let written = prod.push_slice(remaining);
                                pushed_frames += written as u64;
                                remaining = &remaining[written..];
                                if remaining.is_empty()
                                    || clear_gen.load(Ordering::Acquire) != msg_gen
                                {
                                    break;
                                }
                                // The previous reply's audio is what's
                                // draining to make room — its callers
                                // shouldn't wait on this one's backlog too.
                                fire_drained(&mut waiting, &played_frames);
                                thread::sleep(DRAIN_POLL_INTERVAL);
                            }
                            offset += need;
                        }
                        leftover.drain(0..offset);
                    }
                    Ok(PlaybackMsg::Clear) => {
                        leftover.clear();
                    }
                    Ok(PlaybackMsg::OnDrained(f)) => {
                        waiting.push_back((pushed_frames, f));
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(PlaybackMsg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                }
            }

            drop(stream);
            // Whatever they were waiting on will never play now.
            for (_, f) in waiting {
                f();
            }
        })
        .map_err(|e| e.to_string())?;

    Ok(PlaybackHandle {
        tx,
        queued_bytes,
        clear_gen: clear_gen_handle,
        thread: Some(thread),
    })
}

/// Runs every `on_drained` callback whose audio has all been taken off the
/// ring buffer.
fn fire_drained(waiting: &mut VecDeque<(u64, DrainedFn)>, played_frames: &AtomicU64) {
    let played = played_frames.load(Ordering::Acquire);
    while waiting.front().is_some_and(|(target, _)| *target <= played) {
        if let Some((_, f)) = waiting.pop_front() {
            f();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_output(
    output: &mut [f32],
    cons: &mut ring::Consumer,
    channels: usize,
    clear_gen: &AtomicU64,
    last_clear_gen: &mut u64,
    primed: &mut bool,
    priming_frames: usize,
    mono_scratch: &mut Vec<f32>,
    underrun_since: &mut Option<Instant>,
    played_frames: &AtomicU64,
) {
    let current_gen = clear_gen.load(Ordering::Acquire);
    if current_gen != *last_clear_gen {
        *last_clear_gen = current_gen;
        // Counted as played: cut short is still over, as far as anyone
        // waiting on `on_drained` is concerned.
        let dropped = cons.clear();
        played_frames.fetch_add(dropped as u64, Ordering::Release);
        *primed = false;
        *underrun_since = None;
        output.fill(0.0);
        return;
    }

    if !*primed {
        if cons.occupied_len() >= priming_frames {
            *primed = true;
        } else {
            output.fill(0.0);
            return;
        }
    }

    // Diagnostic only (see declaration above): log how long a genuine
    // mid-response starvation lasts, measured from when the buffer first
    // went empty to when it has audio again.
    if cons.occupied_len() == 0 {
        if underrun_since.is_none() {
            *underrun_since = Some(Instant::now());
        }
    } else if let Some(since) = underrun_since.take() {
        let gap_ms = since.elapsed().as_millis();
        tracing::info!(gap_ms, "playback buffer starved then refilled");
    }

    // Once primed, stay primed: audio.delta chunks arrive in bursts with
    // network/generation jitter, so the ring buffer transiently draining to
    // empty between chunks is normal, not a sign the stream ended. Treating
    // that as "unprimed" (as an earlier version did) forced a fresh 120ms
    // re-buffering stall on every gap, which sounds like the reply
    // stuttering/restarting. A genuine underrun here just means outputting
    // silence for this tick — cheap and inaudible — while playback resumes
    // instantly once the next chunk arrives. `primed` only resets on an
    // explicit `Clear` (barge-in), above.
    if channels <= 1 {
        let n = cons.pop_slice(output);
        played_frames.fetch_add(n as u64, Ordering::Release);
        if n < output.len() {
            output[n..].fill(0.0);
        }
    } else {
        // `mono_scratch` is reused across calls: resizing to the same
        // length every callback is a no-op once warmed up, so this never
        // allocates on the realtime audio thread.
        let frames = output.len() / channels;
        mono_scratch.resize(frames, 0.0);
        let n = cons.pop_slice(mono_scratch);
        played_frames.fetch_add(n as u64, Ordering::Release);
        for i in 0..frames {
            let v = if i < n { mono_scratch[i] } else { 0.0 };
            for c in 0..channels {
                output[i * channels + c] = v;
            }
        }
    }
}
