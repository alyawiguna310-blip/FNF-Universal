//! Windowed runner for the playable prototype.
//!
//! Loads one song from a V-Slice mod, then drives it with a real window, GPU
//! surface, audio clock and keyboard input:
//!
//! ```text
//! fu-play --mod <mod dir> --song <song id> [--difficulty <difficulty>]
//! ```
//!
//! Judgement, note motion and audio timing all come from the library:
//! [`fu_play::session`] owns the state, [`fu_play::audio`] the clock and
//! [`fu_play::visuals`] the rendering. This file only wires them to winit.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fu_play::audio::AudioEngine;
use fu_play::codename_content::{song_meta_path, CodenameContent};
use fu_play::codename_song::load_song_auto;
use fu_play::resources::GameResources;
use fu_play::session::Session;
use fu_play::song::PlayableSong;
use fu_play::visuals::{camera, Screen, Visuals};
use fu_render3d::quality::{Platform, QualityLevel, QualitySettings};
use fu_render3d::renderer::Renderer3D;
use fu_render3d_wgpu::backend::register_builtin_shaders;
use fu_render3d_wgpu::surface::WindowedRenderer;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const HELP: &str = "\
usage: fu-play --mod <mod dir> --song <song id> [--difficulty <difficulty>]

  --mod <dir>            a mod directory (V-Slice or Codename Engine)
  --song <id>            song directory name inside the mod, for example dadbattle
  --difficulty <id>      difficulty to play; defaults to the first charted one
  --assets <dir>         extra resource pack layered over the built-in defaults
  --codename <dir>       a Codename installation's assets directory, for real
                         stages and characters
  --simulate             play the song headlessly at perfect timing and exit
  --audio-check          play the instrumental for ~3 s and report clock drift

controls: D F J K or arrow keys to hit notes (A S L ; cover lanes 5-8),
          hold for sustains and release at the tail, P pause, R restart, Esc quit
";

struct Options {
    mod_root: PathBuf,
    song_id: String,
    difficulty: Option<String>,
    assets: Option<PathBuf>,
    codename: Option<PathBuf>,
    simulate: bool,
    audio_check: bool,
}

fn parse_args() -> Result<Options, String> {
    let mut arguments = std::env::args().skip(1);
    let mut mod_root: Option<PathBuf> = None;
    let mut song_id: Option<String> = None;
    let mut difficulty: Option<String> = None;
    let mut assets: Option<PathBuf> = None;
    let mut codename: Option<PathBuf> = None;
    let mut simulate = false;
    let mut audio_check = false;
    let mut positional: Vec<String> = Vec::new();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--help" | "-h" => return Err(HELP.to_string()),
            "--simulate" => simulate = true,
            "--audio-check" => audio_check = true,
            "--mod" => {
                mod_root = Some(PathBuf::from(
                    arguments.next().ok_or("--mod needs a directory")?,
                ));
            }
            "--song" => song_id = Some(arguments.next().ok_or("--song needs an id")?),
            "--difficulty" => {
                difficulty = Some(arguments.next().ok_or("--difficulty needs an id")?)
            }
            "--assets" => {
                assets = Some(PathBuf::from(
                    arguments.next().ok_or("--assets needs a directory")?,
                ));
            }
            "--codename" => {
                codename = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or("--codename needs an assets directory")?,
                ));
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown option '{other}'\n\n{HELP}"))
            }
            other => positional.push(other.to_string()),
        }
    }

    if mod_root.is_none() {
        mod_root = positional.first().map(PathBuf::from);
    }
    if song_id.is_none() {
        song_id = positional.get(1).cloned();
    }
    if difficulty.is_none() {
        difficulty = positional.get(2).cloned();
    }

    Ok(Options {
        mod_root: mod_root.ok_or_else(|| format!("no mod directory given\n\n{HELP}"))?,
        song_id: song_id.ok_or_else(|| format!("no song id given\n\n{HELP}"))?,
        difficulty,
        assets,
        codename,
        simulate,
        audio_check,
    })
}

/// Plays the decoded instrumental and compares its clock with the wall clock.
///
/// This is the check that separates "the Ogg decoded" from "the song the player
/// hears is the clock gameplay follows": it opens the real device, starts the
/// stream, then samples `AudioEngine::position_ms` against `Instant::elapsed`.
fn run_audio_check(song: &PlayableSong) -> ExitCode {
    let Some(bytes) = song.audio.clone() else {
        eprintln!("audio-check: this song has no instrumental");
        return ExitCode::FAILURE;
    };

    let duration = Duration::from_millis(song.chart.length_ms as u64);
    let mut audio = AudioEngine::new(bytes, duration);
    if !audio.is_audio() {
        eprintln!("audio-check: no audio clock, the stream did not open");
        return ExitCode::FAILURE;
    }

    audio.start();
    let started = Instant::now();
    let mut worst_drift = 0.0f32;
    for sample in 1..=12 {
        std::thread::sleep(Duration::from_millis(250));
        let audio_ms = audio.position_ms();
        let wall_ms = started.elapsed().as_secs_f64() as f32 * 1000.0;
        let drift = (audio_ms - wall_ms).abs();
        worst_drift = worst_drift.max(drift);
        println!(
            "audio-check {sample:02}: audio {audio_ms:7.0} ms | wall {wall_ms:7.0} ms | drift {drift:5.0} ms"
        );
    }

    let audible = worst_drift < 250.0;
    println!(
        "audio-check: real ogg stream, max drift {worst_drift:.0} ms, {}",
        if audible {
            "audio clock tracks the wall clock"
        } else {
            "audio clock and wall clock disagree"
        }
    );
    if audible {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Logs the lane count, judgement preset and opponent content gameplay will
/// use, so a chart whose layout was misread is visible at startup.
fn print_chart(chart: &fu_play::game::Chart) {
    let windows = chart.windows();
    let opponent_notes: usize = chart.strum_lines.iter().map(|line| line.notes.len()).sum();
    println!(
        "chart: {} lane(s), preset {:?}, windows {:.2}/{:.2}/{:.2}/{:.2} ms, {} opponent line(s) with {} note(s)",
        chart.lanes,
        chart.preset,
        windows.sick,
        windows.good,
        windows.bad,
        windows.shit,
        chart.strum_lines.len(),
        opponent_notes
    );
}

/// Logs the tempo map gameplay follows, so a chart whose timing was
/// misread is visible at startup instead of only in the trace output.
fn print_timing(chart: &fu_play::game::Chart) {
    let timing = chart.timing();
    let changes = timing.changes();
    println!(
        "timing: {} section(s), {} rejected event(s)",
        changes.len(),
        timing.rejected().len()
    );
    for change in changes {
        let ramp = if change.continuous {
            format!(", ramps to {:.0} ms", change.end_song_time)
        } else {
            String::new()
        };
        println!(
            "  @ {:>9.2} ms  {:>7.2} bpm  {:>3} beat(s)/measure  {} steps/beat{}",
            change.song_time, change.bpm, change.beats_per_measure, change.steps_per_beat, ramp
        );
    }
    for rejected in timing.rejected() {
        println!(
            "  rejected: {} at {:.2} ms ({:?})",
            rejected.name, rejected.time, rejected.reason
        );
    }
}

fn lane_for_key(key: KeyCode) -> Option<usize> {
    match key {
        KeyCode::KeyD | KeyCode::ArrowLeft => Some(0),
        KeyCode::KeyF | KeyCode::ArrowDown => Some(1),
        KeyCode::KeyJ | KeyCode::ArrowUp => Some(2),
        KeyCode::KeyK | KeyCode::ArrowRight => Some(3),
        // Extra keys for charts with more than four lanes.
        KeyCode::KeyA => Some(4),
        KeyCode::KeyS => Some(5),
        KeyCode::KeyL => Some(6),
        KeyCode::Semicolon => Some(7),
        _ => None,
    }
}

/// Wall-clock guard for the song position.
///
/// Audio is authoritative while it is playing, but a stream that is shorter
/// than the chart (or that stops advancing) would otherwise freeze the song at
/// its last sample. Taking the later of the two clocks keeps a truncated track
/// playable and never rewinds real audio.
struct PlayClock {
    accumulated: Duration,
    started: Instant,
    running: bool,
}

impl PlayClock {
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

    fn position_ms(&self) -> f32 {
        let elapsed = if self.running {
            self.accumulated + self.started.elapsed()
        } else {
            self.accumulated
        };
        elapsed.as_secs_f64() as f32 * 1000.0
    }
}

struct Runner {
    song: PlayableSong,
    resources: GameResources,
    codename: Option<CodenameContent>,
    session: Session,
    audio: AudioEngine,
    clock: PlayClock,
    renderer: Option<Renderer3D>,
    windowed: Option<WindowedRenderer>,
    visuals: Option<Visuals>,
    title: String,
    aspect: f32,
    traced_complete: bool,
}

impl Runner {
    fn new(
        song: PlayableSong,
        resources: GameResources,
        codename: Option<CodenameContent>,
    ) -> Self {
        let duration = Duration::from_millis(song.chart.length_ms as u64);
        let audio = AudioEngine::new(song.audio.clone().unwrap_or_default(), duration);
        let session = Session::new(song.chart.clone());
        Self {
            song,
            resources,
            codename,
            session,
            audio,
            clock: PlayClock::new(),
            renderer: None,
            windowed: None,
            visuals: None,
            title: String::new(),
            aspect: 1280.0 / 720.0,
            traced_complete: false,
        }
    }

    fn initialise(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let title = format!("FNF Universal | {}", self.song.title);
        // Size in physical pixels. A logical size is multiplied by the monitor
        // scale factor, which on a 200% display asks for a 2560x1440 surface
        // while the window manager clamps the window to the work area; the
        // result is a full-size surface shown cropped. Physical pixels keep the
        // surface and the window the same size at every scale factor.
        let attributes = Window::default_attributes()
            .with_title(&title)
            .with_inner_size(PhysicalSize::new(1280u32, 720u32));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| format!("could not create window: {error}"))?,
        );

        let windowed = WindowedRenderer::new(window, true)
            .map_err(|error| format!("could not create renderer surface: {error}"))?;

        let quality = QualitySettings::preset(QualityLevel::Medium, Platform::Desktop);
        let mut renderer =
            Renderer3D::new(quality).map_err(|error| format!("renderer setup failed: {error}"))?;
        register_builtin_shaders(&mut renderer.shaders)
            .map_err(|error| format!("shader setup failed: {error}"))?;

        let mut visuals = match &self.codename {
            Some(content) => Visuals::with_codename(&mut renderer, &self.resources, content)?,
            None => Visuals::new(&mut renderer, &self.resources)?,
        };
        visuals.set_notes(&mut renderer, &self.song.chart)?;

        self.title = title;
        self.renderer = Some(renderer);
        self.windowed = Some(windowed);
        self.visuals = Some(visuals);
        Ok(())
    }

    fn restart(&mut self) {
        self.session.restart();
        self.audio.restart();
        self.clock.start();
    }

    fn frame(&mut self) {
        let song_ms = {
            let first_start = self.session.screen() == Screen::Loading;
            self.session.start();
            self.audio.start();
            if first_start {
                self.clock.start();
            }
            self.audio.position_ms().max(self.clock.position_ms())
        };

        let update = self.session.advance(song_ms);
        let screen = self.session.screen();
        if screen == Screen::Complete {
            self.audio.pause();
        }

        // `FU_PLAY_TRACE=1` logs judgement against the clock the game actually
        // follows, which is what makes audio/gameplay sync measurable in a real
        // windowed run rather than only in tests.
        if std::env::var_os("FU_PLAY_TRACE").is_some() {
            if update.misses > 0 {
                println!(
                    "trace: {song_ms:.0} ms - {} miss(es), audio {:.0} ms",
                    update.misses,
                    self.audio.position_ms()
                );
            }
            if screen == Screen::Complete && !self.traced_complete {
                self.traced_complete = true;
                let game = self.session.game();
                println!(
                    "trace: complete at {song_ms:.0} ms (audio {:.0} ms) - {} hits, {} misses, score {}, combo {}",
                    self.audio.position_ms(),
                    game.hits(),
                    game.misses(),
                    game.score(),
                    game.combo()
                );
                println!(
                    "trace: timing {:.2} bpm, step {}, beat {}, measure {}",
                    game.bpm(),
                    game.step(),
                    game.beat(),
                    game.measure()
                );
            }
        }

        let (Some(renderer), Some(windowed), Some(visuals)) = (
            self.renderer.as_mut(),
            self.windowed.as_mut(),
            self.visuals.as_mut(),
        ) else {
            return;
        };

        let game = self.session.game();
        visuals.update(renderer, screen, Some(game), song_ms, self.aspect);
        if let Err(error) = windowed.render_frame(renderer, &camera()) {
            eprintln!("frame skipped: {error}");
        }

        let title = format!("FNF Universal | {}", self.song.title);
        if title != self.title {
            windowed.window().set_title(&title);
            self.title = title;
        }
    }
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.windowed.is_some() {
            return;
        }
        if let Err(error) = self.initialise(event_loop) {
            eprintln!("{error}");
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if size.height > 0 {
                    self.aspect = size.width as f32 / size.height as f32;
                }
                if let Some(windowed) = self.windowed.as_mut() {
                    windowed.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.repeat {
                    return;
                }
                let PhysicalKey::Code(key) = event.physical_key else {
                    return;
                };
                if event.state == ElementState::Released {
                    if let Some(lane) = lane_for_key(key) {
                        let release = self.session.release(lane);
                        if std::env::var_os("FU_PLAY_TRACE").is_some() {
                            println!(
                                "trace: key {key:?} released lane {lane} at {:.0} ms: {release:?}",
                                self.session.position_ms()
                            );
                        }
                    }
                    return;
                }
                match key {
                    KeyCode::Escape => event_loop.exit(),
                    KeyCode::KeyR => self.restart(),
                    KeyCode::KeyP => {
                        let paused = self.session.toggle_pause();
                        if paused {
                            self.audio.pause();
                            self.clock.pause();
                        } else {
                            self.audio.resume();
                            self.clock.resume();
                        }
                    }
                    other => {
                        if let Some(lane) = lane_for_key(other) {
                            let event = self.session.press(lane);
                            if std::env::var_os("FU_PLAY_TRACE").is_some() {
                                println!(
                                    "trace: key {other:?} -> lane {lane} at {:.0} ms: {event:?}",
                                    self.session.position_ms()
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(windowed) = self.windowed.as_ref() {
            windowed.window().request_redraw();
        }
    }
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    let song = match load_song_auto(
        &options.mod_root,
        &options.song_id,
        options.difficulty.as_deref(),
    ) {
        Ok(song) => song,
        Err(error) => {
            eprintln!("cannot play: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "loaded '{}' ({}) - {} notes, {} ms, tempo {:.0} bpm, {}",
        song.title,
        song.difficulty,
        song.note_count(),
        song.chart.length_ms,
        song.chart.bpm,
        if song.has_audio() {
            format!(
                "audio {}",
                match song.audio_duration_ms {
                    Some(duration) => format!("({duration} ms)"),
                    None => "(duration unavailable)".to_string(),
                }
            )
        } else {
            "no instrumental, using clock timing".to_string()
        }
    );
    if song.import_warnings > 0 {
        println!(
            "note: the importer reported {} warning(s) for this mod",
            song.import_warnings
        );
    }
    print_timing(&song.chart);
    print_chart(&song.chart);

    if options.audio_check {
        return run_audio_check(&song);
    }

    if options.simulate {
        let result = fu_play::session::simulate(&song.chart, true);
        println!(
            "simulation: {} hits, {} misses, score {}, max combo {}, {} at {:.0} ms",
            result.hits,
            result.misses,
            result.score,
            result.max_combo,
            if result.completed {
                "COMPLETE"
            } else {
                "INCOMPLETE"
            },
            result.position_ms
        );
        return if result.completed {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    let codename = options.codename.as_ref().map(|assets| {
        let meta = song_meta_path(assets, Some(&options.mod_root), &options.song_id);
        let content = CodenameContent::load(assets, Some(&options.mod_root), meta.as_deref());
        for warning in &content.warnings {
            println!("codename: {warning}");
        }
        if !content.is_empty() {
            println!(
                "codename: {} character(s), {} stage layer(s) from {}",
                content.characters.len(),
                content.stage_layers.len(),
                assets.display()
            );
            for rig in content.characters.values() {
                let idle = rig.animation("idle");
                let frame = idle.and_then(|animation| animation.frames.first());
                println!(
                    "codename: character {} at {:.0},{:.0} scale {:.2} flip {} - {} idle frame(s), first {}x{}",
                    rig.name,
                    rig.position.0,
                    rig.position.1,
                    rig.scale,
                    rig.flip_x,
                    idle.map(|animation| animation.frames.len()).unwrap_or(0),
                    frame.map(|frame| frame.frame.width).unwrap_or(0),
                    frame.map(|frame| frame.frame.height).unwrap_or(0),
                );
            }
        }
        content
    });

    let resources = match GameResources::load(Some(&options.mod_root), options.assets.as_deref()) {
        Ok(resources) => resources,
        Err(error) => {
            eprintln!("resources: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut resources = resources;
    if let Some(content) = codename.as_ref() {
        content.apply_skin(&mut resources);
    }
    println!("resources: {}", resources.summary());

    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("could not start event loop: {error}");
            return ExitCode::FAILURE;
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut runner = Runner::new(song, resources, codename);
    if let Err(error) = event_loop.run_app(&mut runner) {
        eprintln!("event loop failed: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
