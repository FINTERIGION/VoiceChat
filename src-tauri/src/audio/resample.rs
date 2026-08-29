use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, PolynomialDegree, Resampler as _};

/// Fixed-ratio mono resampler. Always consumes exactly [`Self::input_frames_next`]
/// input frames per call and produces a variable (but bounded) number of output
/// frames, since rubato's `Async` resampler only holds the input side fixed.
pub struct Resampler {
    inner: Async<f32>,
    out_buf: Vec<f32>,
}

impl Resampler {
    pub fn new(from_hz: u32, to_hz: u32, chunk_size: usize) -> Self {
        let ratio = to_hz as f64 / from_hz as f64;
        let inner = Async::<f32>::new_poly(
            ratio,
            1.0,
            PolynomialDegree::Cubic,
            chunk_size,
            1,
            FixedAsync::Input,
        )
        .expect("valid resampler parameters");
        let out_buf = vec![0f32; inner.output_frames_max()];
        Self { inner, out_buf }
    }

    pub fn input_frames_next(&self) -> usize {
        self.inner.input_frames_next()
    }

    /// Resample exactly `input_frames_next()` input frames. Returns the
    /// resampled slice, or empty if resampling failed (logged, not fatal —
    /// a virtualized/redirected audio device can renegotiate its format
    /// mid-stream, and a panic here would silently kill the whole playback
    /// thread for the rest of the response instead of just dropping one
    /// chunk).
    pub fn process(&mut self, input: &[f32]) -> &[f32] {
        let needed = self.inner.input_frames_next();
        if input.len() != needed {
            tracing::warn!(
                got = input.len(),
                expected = needed,
                "resampler input length mismatch, dropping chunk"
            );
            return &self.out_buf[..0];
        }

        let out_len = self.out_buf.len();
        let nbr_out = match InterleavedSlice::new(input, 1, input.len()) {
            Ok(in_adapter) => match InterleavedSlice::new_mut(&mut self.out_buf, 1, out_len) {
                Ok(mut out_adapter) => {
                    match self
                        .inner
                        .process_into_buffer(&in_adapter, &mut out_adapter, None)
                    {
                        Ok((_nbr_in, n)) => Some(n),
                        Err(e) => {
                            tracing::warn!("resampling failed: {e}");
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("resampler output adapter error: {e}");
                    None
                }
            },
            Err(e) => {
                tracing::warn!("resampler input adapter error: {e}");
                None
            }
        };

        &self.out_buf[..nbr_out.unwrap_or(0)]
    }
}

/// Converts interleaved little-endian PCM16 bytes to normalized f32 samples.
pub fn pcm16le_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / i16::MAX as f32)
        .collect()
}

/// Converts normalized f32 samples to interleaved little-endian PCM16 bytes.
pub fn f32_to_pcm16le(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let v = (clamped * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}
