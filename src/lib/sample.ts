/**
 * What an audio file picked as a voice sample may be — the same wherever one
 * is picked: cloning a voice in the Voice Studio, or sharing a character
 * whose voice has no kept sample.
 *
 * The formats voice enrollment clones from (see `voice::sample::Format`),
 * which is also all that a voice's sample is kept as and a shared character
 * file may carry, so a file either picker offers can be cloned from, kept
 * and shared alike. What a file really is gets decided by its bytes once
 * picked; this only narrows what the dialog offers.
 */
export const SAMPLE_ACCEPT = [
  ".wav",
  ".mp3",
  ".m4a",
  ".flac",
  ".ogg",
  ".opus", // Opus in an Ogg container, which the bytes show as OGG
  ".aac",
  ".webm",
  "audio/wav",
  "audio/mpeg",
  "audio/mp4",
  "audio/x-m4a",
  "audio/flac",
  "audio/ogg",
  "audio/aac",
  "audio/webm",
].join(",");

/** Mirrors `voice::sample::MAX_BYTES`. */
export const SAMPLE_MAX_BYTES = 10 * 1024 * 1024;
