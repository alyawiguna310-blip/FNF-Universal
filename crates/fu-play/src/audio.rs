//! Small audio wrapper for the prototype.
//!
//! The chart is driven by `Player::get_pos()` when a default audio device and
//! decoder are available. If audio cannot be opened or decoded, the gameplay
//! falls back to a monotonic clock and reports that mode in the window title.

use std::io::Cursor;
use std::time::{Duration, Instant};

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

/// Reads the duration of `bytes` by opening a decoder for it.
///
/// Most decoders report a length directly. Ogg Vorbis streams do not, so their
/// duration is read from the granule position of the last Ogg page, which is the
/// standard way to get an exact length without decoding the whole stream.
///
/// Returns `None` when the format is unsupported or the length is unknown; the
/// caller then keeps a fallback length.
pub fn decode_duration_ms(bytes: &[u8]) -> Option<u32> {
    let source = Decoder::try_from(Cursor::new(bytes.to_vec())).ok()?;
    if let Some(duration) = source.total_duration() {
        let millis = duration.as_millis().min(u32::MAX as u128) as u32;
        if millis > 0 {
            return Some(millis);
        }
    }
    ogg_duration_ms(bytes, source.sample_rate().get())
}

/// Duration of an Ogg stream from the granule position of its last page.
fn ogg_duration_ms(bytes: &[u8], sample_rate: u32) -> Option<u32> {
    if sample_rate == 0 || !bytes.starts_with(b"OggS") {
        return None;
    }

    let mut last_granule: Option<u64> = None;
    let mut index = 0usize;
    while let Some(offset) = find(&bytes[index..], b"OggS") {
        let page = index + offset;
        if page + 14 > bytes.len() {
            break;
        }
        let granule = u64::from_le_bytes(bytes[page + 6..page + 14].try_into().ok()?);
        // `u64::MAX` marks a page that finishes no packet.
        if granule != u64::MAX {
            last_granule = Some(granule);
        }
        index = page + 4;
    }

    let granule = last_granule?;
    let millis = granule.saturating_mul(1000) / u64::from(sample_rate);
    (millis > 0).then_some(millis.min(u64::from(u32::MAX)) as u32)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

struct FallbackClock {
    accumulated: Duration,
    started: Instant,
    running: bool,
}

impl FallbackClock {
    fn new() -> Self {
        Self {
            accumulated: Duration::ZERO,
            started: Instant::now(),
            running: false,
        }
    }

    fn start(&mut self) {
        self.accumulated = Duration::ZERO;
        self.started = Instant::now();
        self.running = true;
    }

    fn reset(&mut self) {
        self.start();
    }

    fn pause(&mut self) {
        if self.running {
            self.accumulated += self.started.elapsed();
            self.running = false;
        }
    }

    fn resume(&mut self) {
        if !self.running {
            self.started = Instant::now();
            self.running = true;
        }
    }

    fn position(&self) -> Duration {
        if self.running {
            self.accumulated + self.started.elapsed()
        } else {
            self.accumulated
        }
    }
}

pub struct AudioEngine {
    device: Option<MixerDeviceSink>,
    player: Option<Player>,
    bytes: Vec<u8>,
    duration: Duration,
    fallback: FallbackClock,
    started: bool,
    paused: bool,
    audio_error: Option<String>,
}

impl AudioEngine {
    pub fn new(bytes: Vec<u8>, duration: Duration) -> Self {
        let mut engine = Self {
            device: None,
            player: None,
            bytes,
            duration,
            fallback: FallbackClock::new(),
            started: false,
            paused: false,
            audio_error: None,
        };

        match DeviceSinkBuilder::open_default_sink() {
            Ok(device) => {
                let player = Player::connect_new(device.mixer());
                engine.player = Some(player);
                engine.device = Some(device);
            }
            Err(error) => {
                engine.audio_error = Some(format!("no audio device: {error}"));
                eprintln!(
                    "audio unavailable; using monotonic clock timing: {}",
                    engine.audio_error.as_deref().unwrap_or("unknown error")
                );
            }
        }
        engine
    }

    /// Starts playback once. Safe to call every frame.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        self.paused = false;
        self.start_playback();
    }

    fn start_playback(&mut self) {
        let mut decoder_failed = None;
        if let Some(player) = &self.player {
            match Decoder::try_from(Cursor::new(self.bytes.clone())) {
                Ok(source) => {
                    player.append(source);
                    player.play();
                    return;
                }
                Err(error) => decoder_failed = Some(format!("decode failed: {error}")),
            }
        }

        if let Some(error) = decoder_failed {
            eprintln!("audio unavailable; using monotonic clock timing: {error}");
            self.audio_error = Some(error);
            self.player = None;
        }
        self.fallback.start();
    }

    pub fn restart(&mut self) {
        self.started = false;
        self.paused = false;
        self.player = None;
        if let Some(device) = &self.device {
            self.player = Some(Player::connect_new(device.mixer()));
        }
        self.fallback.reset();
        self.start();
    }

    pub fn pause(&mut self) {
        if !self.started || self.paused {
            return;
        }
        self.paused = true;
        if let Some(player) = &self.player {
            player.pause();
        } else {
            self.fallback.pause();
        }
    }

    pub fn resume(&mut self) {
        if !self.started || !self.paused {
            return;
        }
        self.paused = false;
        if let Some(player) = &self.player {
            player.play();
        } else {
            self.fallback.resume();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn is_audio(&self) -> bool {
        self.player.is_some()
    }

    pub fn audio_error(&self) -> Option<&str> {
        self.audio_error.as_deref()
    }

    pub fn position(&self) -> Duration {
        if !self.started {
            return Duration::ZERO;
        }
        if let Some(player) = &self.player {
            player.get_pos().min(self.duration)
        } else {
            self.fallback.position().min(self.duration)
        }
    }

    pub fn position_ms(&self) -> f32 {
        self.position().as_secs_f64() as f32 * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_duration_ms, ogg_duration_ms};

    /// Builds the beginning of an Ogg page: capture pattern, version, flags and
    /// the granule position, which is all the duration reader looks at.
    fn ogg_page(granule: u64) -> Vec<u8> {
        let mut page = b"OggS".to_vec();
        page.push(0);
        page.push(0x04);
        page.extend_from_slice(&granule.to_le_bytes());
        page.extend_from_slice(&[0; 16]);
        page
    }

    #[test]
    fn ogg_duration_comes_from_the_last_page_granule() {
        let mut stream = ogg_page(0);
        stream.extend_from_slice(&ogg_page(44_100 * 3));

        assert_eq!(ogg_duration_ms(&stream, 44_100), Some(3_000));
        assert_eq!(ogg_duration_ms(&stream, 48_000), Some(2_756));
    }

    #[test]
    fn a_page_without_a_finished_packet_does_not_shorten_the_song() {
        let mut stream = ogg_page(44_100 * 2);
        stream.extend_from_slice(&ogg_page(u64::MAX));

        assert_eq!(ogg_duration_ms(&stream, 44_100), Some(2_000));
    }

    #[test]
    fn non_ogg_and_empty_streams_have_no_granule_duration() {
        assert_eq!(ogg_duration_ms(b"RIFF....WAVE", 44_100), None);
        assert_eq!(ogg_duration_ms(b"OggS", 44_100), None);
        assert_eq!(ogg_duration_ms(b"", 44_100), None);
    }

    #[test]
    fn undecodable_audio_has_no_duration() {
        assert_eq!(decode_duration_ms(b"not really audio"), None);
    }
}
