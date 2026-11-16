//! End-to-end tests for the playable slice: imported V-Slice mod → chart →
//! judgement → rendering.
//!
//! Fixtures are written to a temporary directory in the V-Slice data layout, so
//! the whole path from `fu_import::vslice` to `fu_play::game` is exercised.

use std::path::PathBuf;

use fu_import::test_support::TempDir;
use fu_play::audio::decode_duration_ms;
use fu_play::game::{Gameplay, Judgement};
use fu_play::resources::{keys, GameResources};
use fu_play::session::{simulate, Session};
use fu_play::song::{load_song, SongError, TAIL_MS};
use fu_play::visuals::{camera, Screen, Visuals};
use fu_render3d::backend::Viewport;
use fu_render3d::quality::{Platform, QualityLevel, QualitySettings};
use fu_render3d::renderer::Renderer3D;
use fu_render3d_wgpu::backend::{register_builtin_shaders, WgpuBackend};

/// Headless GPU tests run in parallel threads of one test binary, and creating
/// two wgpu devices at once is not safe with every driver. Serialize them.
static GPU_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu_guard() -> std::sync::MutexGuard<'static, ()> {
    GPU_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_mod(temp: &TempDir) -> PathBuf {
    temp.write(
        "mod/_polymod_meta.json",
        r##"{
  "title": "Playable Fixture",
  "api_version": "0.5.0",
  "contributors": [{"name": "Tester"}]
}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/dadbattle/dadbattle-metadata.json",
        r##"{
  "version": "2.2.3",
  "songName": "Dad Battle",
  "artist": "Kawai Sprite",
  "timeFormat": "ms",
  "timeChanges": [{"t": 0, "bpm": 180}],
  "playData": {
    "difficulties": ["easy", "hard"],
    "characters": {"player": "bf", "opponent": "dad", "girlfriend": "gf"},
    "stage": "mainStage",
    "noteStyle": "funkin"
  }
}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/dadbattle/dadbattle-chart.json",
        r##"{
  "version": "2.0.0",
  "scrollSpeed": {"easy": 1.0, "hard": 1.6},
  "events": [{"t": 200, "e": "Zoom Camera", "v": {"zoom": 1.1}}],
  "notes": {
    "easy": [
      {"t": 1000, "d": 0},
      {"t": 1500, "d": 4},
      {"t": 2000, "d": 1}
    ],
    "hard": [
      {"t": 1000, "d": 0, "l": 250},
      {"t": 1200, "d": 1},
      {"t": 1400, "d": 2},
      {"t": 1600, "d": 3},
      {"t": 1800, "d": 5}
    ]
  }
}"##,
    )
    .unwrap();
    temp.write("mod/songs/dadbattle/Inst.ogg", "not really an ogg stream")
        .unwrap();
    temp.join("mod")
}

#[test]
fn loads_a_song_whose_charts_and_audio_live_in_different_directories() {
    // Real V-Slice mods keep the documents in `data/songs/<id>` and the audio in
    // `songs/<id>`; the same song must still be assembled from both.
    let temp = TempDir::new("playable-data-layout").unwrap();
    temp.write("mod/_polymod_meta.json", r##"{"title": "Real Layout"}"##)
        .unwrap();
    temp.write(
        "mod/data/songs/eggnog/eggnog-metadata.json",
        r##"{
  "version": "2.2.4",
  "songName": "Eggnog",
  "timeChanges": [{"t": 0, "bpm": 140}],
  "playData": {"difficulties": ["erect"], "stage": "mainStage"}
}"##,
    )
    .unwrap();
    temp.write(
        "mod/data/songs/eggnog/eggnog-chart.json",
        r##"{
  "version": "2.0.0",
  "notes": {"erect": [{"t": 500, "d": 0}, {"t": 900, "d": 3}]}
}"##,
    )
    .unwrap();
    temp.write("mod/songs/eggnog/Inst.ogg", "not really an ogg stream")
        .unwrap();

    let song = load_song(temp.join("mod").as_path(), "eggnog", None).expect("song loads");

    assert_eq!(song.title, "Eggnog");
    assert_eq!(song.difficulty, "erect");
    assert_eq!(song.note_count(), 2);
    assert!(song.has_audio(), "audio is found under songs/eggnog");
}

#[test]
fn loads_a_song_into_the_engine() {
    let temp = TempDir::new("playable-load").unwrap();
    let root = write_mod(&temp);
    let root = root.as_path();

    let song = load_song(root, "dadbattle", None).expect("song loads");
    assert_eq!(song.title, "Dad Battle");
    assert_eq!(song.song_id, "dadbattle");
    assert_eq!(
        song.difficulty, "easy",
        "first declared, charted difficulty"
    );
    assert_eq!(song.chart.song, "Dad Battle");
    assert_eq!(song.chart.bpm, 180.0);
    assert_eq!(song.available_difficulties, vec!["easy", "hard"]);
    assert!(song.has_audio());
    assert_eq!(song.audio_path.as_deref(), Some("songs/dadbattle/Inst.ogg"));
    assert!(song.chart.length_ms > 0);
}

#[test]
fn opponent_notes_are_not_judged() {
    let temp = TempDir::new("playable-opponent").unwrap();
    let root = write_mod(&temp);
    let root = root.as_path();

    let song = load_song(root, "dadbattle", Some("easy")).expect("song loads");
    let lanes: Vec<u8> = song.chart.notes.iter().map(|note| note.lane).collect();
    assert_eq!(
        song.chart.notes.len(),
        2,
        "the d=4 opponent note is excluded"
    );
    assert_eq!(lanes, vec![0, 1]);
    assert_eq!(song.chart.notes[0].time_ms, 1000.0);
    assert_eq!(song.chart.notes[1].time_ms, 2000.0);
}

#[test]
fn difficulty_selection_is_case_insensitive() {
    let temp = TempDir::new("playable-difficulty").unwrap();
    let root = write_mod(&temp);
    let root = root.as_path();

    let song = load_song(root, "DADBATTLE", Some("HARD")).expect("song loads");
    assert_eq!(song.difficulty, "hard");
    assert_eq!(
        song.chart.notes.len(),
        4,
        "the d=5 note is an opponent note"
    );
    assert_eq!(song.chart.notes[3].lane, 3);
}

#[test]
fn unknown_songs_and_difficulties_are_reported() {
    let temp = TempDir::new("playable-errors").unwrap();
    let root = write_mod(&temp);
    let root = root.as_path();

    let error = load_song(root, "missing", None).unwrap_err();
    assert!(matches!(error, SongError::SongNotFound { .. }));
    assert!(error.to_string().contains("dadbattle"));

    let error = load_song(root, "dadbattle", Some("nightmare")).unwrap_err();
    assert!(matches!(error, SongError::NoSuchDifficulty { .. }));
    assert!(error.to_string().contains("nightmare"));
}

#[test]
fn the_song_length_falls_back_to_the_last_note() {
    let temp = TempDir::new("playable-length").unwrap();
    temp.write("mod/_polymod_meta.json", r##"{"title": "No Audio"}"##)
        .unwrap();
    temp.write(
        "mod/songs/quiet/quiet-chart.json",
        r##"{"version": "2.0.0", "notes": {"hard": [{"t": 1000, "d": 0}, {"t": 3000, "d": 2}]}}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/quiet/quiet-metadata.json",
        r##"{"version": "2.2.3", "songName": "Quiet", "timeChanges": [{"t": 0, "bpm": 140}], "playData": {"difficulties": ["hard"]}}"##,
    )
    .unwrap();

    let song = load_song(&temp.join("mod"), "quiet", None).expect("song loads");
    assert!(!song.has_audio());
    assert_eq!(song.chart.length_ms, 3_000 + TAIL_MS);
    assert_eq!(song.chart.bpm, 140.0);
}

#[test]
fn a_song_plays_to_completion_with_perfect_input() {
    let temp = TempDir::new("playable-autoplay").unwrap();
    let root = write_mod(&temp);
    let root = root.as_path();
    let song = load_song(root, "dadbattle", Some("hard")).expect("song loads");

    let result = simulate(&song.chart, true);
    assert!(result.completed, "autoplay must finish the song");
    assert_eq!(result.hits as usize, song.chart.notes.len());
    assert_eq!(result.misses, 0);
    assert!(result.score > 0);
    assert_eq!(result.max_combo as usize, song.chart.notes.len());
    assert!(result.position_ms >= song.chart.duration_ms());
}

#[test]
fn a_song_without_input_misses_every_note() {
    let temp = TempDir::new("playable-misses").unwrap();
    let root = write_mod(&temp);
    let root = root.as_path();
    let song = load_song(root, "dadbattle", Some("easy")).expect("song loads");

    let result = simulate(&song.chart, false);
    assert!(result.completed);
    assert_eq!(result.hits, 0);
    assert_eq!(result.misses as usize, song.chart.notes.len());
    assert_eq!(result.score, 0);
    assert_eq!(result.max_combo, 0);
}

#[test]
fn gameplay_state_matches_the_chart_after_a_full_run() {
    let temp = TempDir::new("playable-state").unwrap();
    let root = write_mod(&temp);
    let root = root.as_path();
    let song = load_song(root, "dadbattle", Some("hard")).expect("song loads");

    let mut session = Session::new(song.chart.clone());
    session.start();
    for note in &song.chart.notes {
        session.advance(note.time_ms as f32);
        assert!(
            session.press(note.lane as usize).is_some(),
            "note at {} ms in lane {} must be hittable",
            note.time_ms,
            note.lane
        );
    }
    session.advance(song.chart.duration_ms());

    assert_eq!(session.screen(), Screen::Complete);
    for index in 0..song.chart.notes.len() {
        assert_ne!(session.game().judgement(index), Judgement::Pending);
    }
}

#[test]
fn wav_durations_are_decoded() {
    // 8000 mono 8-bit samples at 8000 Hz = exactly one second.
    let bytes = wav_bytes(8_000, 8_000);
    assert_eq!(decode_duration_ms(&bytes), Some(1_000));
    assert_eq!(decode_duration_ms(b"not audio at all"), None);
}

fn wav_bytes(samples: usize, sample_rate: u32) -> Vec<u8> {
    let data_len = samples as u32;
    let mut bytes = Vec::with_capacity(44 + samples);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    bytes.extend(std::iter::repeat_n(128u8, samples));
    bytes
}

#[test]
fn gameplay_renders_headlessly() {
    let _gpu = gpu_guard();
    let temp = TempDir::new("playable-render").unwrap();
    let root = write_mod(&temp);
    let root = root.as_path();
    let song = load_song(root, "dadbattle", Some("hard")).expect("song loads");

    let mut backend = match WgpuBackend::new_headless() {
        Ok(backend) => backend,
        Err(error) => {
            eprintln!("skipping render test: no GPU adapter ({error})");
            return;
        }
    };

    let quality = QualitySettings::preset(QualityLevel::Low, Platform::Desktop);
    let mut renderer = Renderer3D::new(quality).expect("renderer");
    register_builtin_shaders(&mut renderer.shaders).expect("shaders");
    let resources = GameResources::defaults().expect("default resources");
    let mut visuals = Visuals::new(&mut renderer, &resources).expect("visuals");
    visuals
        .set_notes(&mut renderer, &song.chart)
        .expect("note visuals");

    let mut game = Gameplay::new(song.chart.clone());
    for song_ms in [0.0, 900.0, 1_000.0, 1_500.0, 2_500.0] {
        game.update(song_ms);
        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            song_ms,
            16.0 / 9.0,
        );
        renderer
            .render(
                &camera(),
                fu_render3d::backend::Viewport {
                    width: 640,
                    height: 360,
                },
                &mut backend,
            )
            .expect("frame renders");
    }
}

/// Inverse of the sRGB transfer function for one framebuffer channel.
fn srgb_to_linear(byte: u8) -> f32 {
    let channel = byte as f32 / 255.0;
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn pixel_at(width: u32, pixels: &[u8], x: u32, y: u32) -> [f32; 3] {
    let index = ((y * width + x) * 4) as usize;
    [
        srgb_to_linear(pixels[index]),
        srgb_to_linear(pixels[index + 1]),
        srgb_to_linear(pixels[index + 2]),
    ]
}

/// Maps a world-space point of the orthographic playfield to a framebuffer
/// pixel, using the same 12-unit-tall projection the game camera uses.
fn world_to_pixel(world: (f32, f32), width: u32, height: u32) -> (u32, u32) {
    let half_width = 6.0 * (width as f32 / height as f32);
    let x = ((world.0 / half_width) * 0.5 + 0.5) * width as f32;
    let y = (0.5 - (world.1 / 12.0)) * height as f32;
    (
        (x.round() as u32).min(width - 1),
        (y.round() as u32).min(height - 1),
    )
}

fn is_close(pixel: [f32; 3], expected: [f32; 3], tolerance: f32) -> bool {
    (0..3).all(|channel| (pixel[channel] - expected[channel]).abs() <= tolerance)
}

#[test]
fn a_gameplay_frame_draws_receptors_notes_and_hud() {
    let _gpu = gpu_guard();
    let temp = TempDir::new("playable-pixels").unwrap();
    let root = write_mod(&temp);
    let root = root.as_path();
    let song = load_song(root, "dadbattle", Some("hard")).expect("song loads");

    let mut backend = match WgpuBackend::new_headless() {
        Ok(backend) => backend,
        Err(error) => {
            eprintln!("skipping pixel test: no GPU adapter ({error})");
            return;
        }
    };

    let quality = QualitySettings::preset(QualityLevel::Low, Platform::Desktop);
    let mut renderer = Renderer3D::new(quality).expect("renderer");
    register_builtin_shaders(&mut renderer.shaders).expect("shaders");
    let resources = GameResources::defaults().expect("default resources");
    let mut visuals = Visuals::new(&mut renderer, &resources).expect("visuals");
    visuals
        .set_notes(&mut renderer, &song.chart)
        .expect("note visuals");

    let viewport = Viewport {
        width: 640,
        height: 360,
    };
    let aspect = viewport.aspect();
    let mut game = Gameplay::new(song.chart.clone());

    // At the start of the song no note covers the receptors. The lane backdrop
    // is opaque, so a visible receptor pixel proves the receptor draws in front
    // of it instead of being hidden behind it.
    game.update(0.0);
    visuals.update(&mut renderer, Screen::Playing, Some(&game), 0.0, aspect);
    renderer
        .render(&camera(), viewport, &mut backend)
        .expect("frame renders");
    let (width, height, pixels) = backend.read_target_pixels().expect("readback");

    let (receptor_x, receptor_y) = world_to_pixel((-3.6, -4.0), width, height);
    let receptor_pixel = pixel_at(width, &pixels, receptor_x, receptor_y);
    let lane_backdrop = [0.08, 0.10, 0.16];
    assert!(
        receptor_pixel[0] > 0.18 && receptor_pixel[1] > 0.18,
        "receptor pixel {receptor_pixel:?} should be the lit receptor sprite, not the lane \
         backdrop {lane_backdrop:?} which would mean it is occluded"
    );

    // The HUD score is drawn in the top-left corner in pink.
    let score_pink = [0.95, 0.25, 0.55];
    let hud_pixels = (0..60)
        .flat_map(|y| (0..220).map(move |x| (x, y)))
        .filter(|(x, y)| is_close(pixel_at(width, &pixels, *x, *y), score_pink, 0.12))
        .count();
    assert!(
        hud_pixels > 20,
        "expected score glyphs in the top-left corner, found {hud_pixels} matching pixels"
    );

    // 50 ms before its time the lane 1 note sits just above the receptor line,
    // in front of the lane backdrop.
    game.update(1_150.0);
    visuals.update(&mut renderer, Screen::Playing, Some(&game), 1_150.0, aspect);
    renderer
        .render(&camera(), viewport, &mut backend)
        .expect("frame renders");
    let (width, height, pixels) = backend.read_target_pixels().expect("readback");

    let note_y = -4.0 + (50.0 / 2_000.0) * (5.2 + 4.0);
    let (note_x, note_y_px) = world_to_pixel((-1.2, note_y), width, height);
    let note_pixel = pixel_at(width, &pixels, note_x, note_y_px);
    // The generated note sprite paints its lane colour, so the pixel must match
    // the down-lane note texture rather than the lane backdrop behind it.
    let note_blue = [0.033, 0.53, 0.887];
    assert!(
        is_close(note_pixel, note_blue, 0.14),
        "note pixel {note_pixel:?} should be the down-lane note sprite {note_blue:?}, \
         not the lane backdrop {lane_backdrop:?}"
    );
    assert!(
        !resources.missing().iter().any(|key| key.contains("note")),
        "the note textures came from the default pack: {:?}",
        resources.missing()
    );
    assert!(resources.has_texture(keys::STAGE_DEFAULT));
}

#[test]
fn a_chart_with_only_opponent_notes_is_rejected() {
    let temp = TempDir::new("playable-empty").unwrap();
    temp.write("mod/_polymod_meta.json", r##"{"title": "Opponent Only"}"##)
        .unwrap();
    temp.write(
        "mod/songs/other/other-chart.json",
        r##"{"version": "2.0.0", "notes": {"hard": [{"t": 1000, "d": 4}, {"t": 1200, "d": 6}]}}"##,
    )
    .unwrap();

    let error = load_song(&temp.join("mod"), "other", None).unwrap_err();
    assert!(matches!(error, SongError::NoPlayableNotes { .. }));
}
