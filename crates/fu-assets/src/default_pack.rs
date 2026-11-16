//! The built-in default resource pack.
//!
//! The pack is **generated**, not copied: every texture and sound here is drawn
//! or synthesised from code in this file, so it carries the project's own licence
//! and contains no proprietary Friday Night Funkin' artwork. It exists so the
//! runtime looks like a game out of the box, and it is installed as the
//! lowest-priority VFS mount, which means a mod or a user pack can replace any of
//! it file-for-file or by remapping a manifest key.
//!
//! Layout (all under the `/defaults` mount point):
//!
//! ```text
//! defaults/manifest.json
//! defaults/images/notes/note_<lane>.png
//! defaults/images/receptors/receptor.png
//! defaults/images/stage/stage.png
//! defaults/images/hud/{healthbar_frame,healthbar_fill,icon_player,icon_opponent}.png
//! defaults/images/countdown/{3,2,1,go}.png
//! defaults/images/judgement/{perfect,good,bad,miss}.png
//! defaults/images/ui/{menu_background,pause_panel,results_panel}.png
//! defaults/sounds/{countdown,hit,miss}.wav
//! ```

use std::f32::consts::PI;

use fu_vfs::{CaseSensitivity, MountPolicy, Vfs};

use crate::error::{AssetError, AssetResult};
use crate::manifest::ResourceManifest;
use crate::texture::TextureAsset;

/// Virtual path of the embedded manifest.
pub const DEFAULT_MANIFEST_PATH: &str = "/defaults/manifest.json";

/// Mount identifier of the default pack.
pub const DEFAULT_MOUNT_ID: &str = "defaults";

/// The manifest shipped with the default pack.
pub const DEFAULT_MANIFEST_JSON: &str = r#"{
  "name": "FNF Universal Default Pack",
  "version": "1.0.0",
  "assets": {
    "note.left": "/defaults/images/notes/note_left.png",
    "note.down": "/defaults/images/notes/note_down.png",
    "note.up": "/defaults/images/notes/note_up.png",
    "note.right": "/defaults/images/notes/note_right.png",
    "receptor.left": "/defaults/images/receptors/receptor.png",
    "receptor.down": "/defaults/images/receptors/receptor.png",
    "receptor.up": "/defaults/images/receptors/receptor.png",
    "receptor.right": "/defaults/images/receptors/receptor.png",
    "stage.default": "/defaults/images/stage/stage.png",
    "hud.health.frame": "/defaults/images/hud/healthbar_frame.png",
    "hud.health.fill": "/defaults/images/hud/healthbar_fill.png",
    "hud.icon.player": "/defaults/images/hud/icon_player.png",
    "hud.icon.opponent": "/defaults/images/hud/icon_opponent.png",
    "countdown.3": "/defaults/images/countdown/3.png",
    "countdown.2": "/defaults/images/countdown/2.png",
    "countdown.1": "/defaults/images/countdown/1.png",
    "countdown.go": "/defaults/images/countdown/go.png",
    "judgement.perfect": "/defaults/images/judgement/perfect.png",
    "judgement.good": "/defaults/images/judgement/good.png",
    "judgement.bad": "/defaults/images/judgement/bad.png",
    "judgement.miss": "/defaults/images/judgement/miss.png",
    "character.default.image": "/defaults/images/characters/character.png",
    "character.default.atlas": "/defaults/images/characters/character.xml",
    "ui.menu.background": "/defaults/images/ui/menu_background.png",
    "ui.pause.panel": "/defaults/images/ui/pause_panel.png",
    "ui.results.panel": "/defaults/images/ui/results_panel.png",
    "sfx.countdown": "/defaults/sounds/countdown.wav",
    "sfx.hit": "/defaults/sounds/hit.wav",
    "sfx.miss": "/defaults/sounds/miss.wav"
  }
}"#;

const PINK: [u8; 4] = [242, 64, 140, 255];
const BLUE: [u8; 4] = [51, 191, 242, 255];
const GREEN: [u8; 4] = [115, 242, 89, 255];
const ORANGE: [u8; 4] = [255, 153, 51, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const INK: [u8; 4] = [18, 20, 30, 255];
const PANEL: [u8; 4] = [26, 30, 46, 235];
const BORDER: [u8; 4] = [120, 205, 245, 255];
const GREY: [u8; 4] = [150, 155, 170, 255];

const LANE_COLORS: [[u8; 4]; 4] = [PINK, BLUE, GREEN, ORANGE];
const LANE_NAMES: [&str; 4] = ["left", "down", "up", "right"];

/// The generated default pack: virtual path to encoded file bytes.
pub struct DefaultPack {
    files: Vec<(String, Vec<u8>)>,
}

impl DefaultPack {
    /// Generates every default resource.
    pub fn build() -> AssetResult<Self> {
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut push = |path: &str, bytes: Vec<u8>| files.push((path.to_string(), bytes));

        push(
            DEFAULT_MANIFEST_PATH,
            DEFAULT_MANIFEST_JSON.as_bytes().to_vec(),
        );

        for (lane, name) in LANE_NAMES.iter().enumerate() {
            let note = arrow_texture(LANE_COLORS[lane], WHITE, lane);
            push(
                &format!("/defaults/images/notes/note_{name}.png"),
                note.encode_png()?,
            );
        }
        push(
            "/defaults/images/receptors/receptor.png",
            arrow_texture(GREY, WHITE, 2).encode_png()?,
        );

        push(
            "/defaults/images/stage/stage.png",
            stage_texture().encode_png()?,
        );
        push(
            "/defaults/images/hud/healthbar_frame.png",
            health_frame_texture().encode_png()?,
        );
        push(
            "/defaults/images/hud/healthbar_fill.png",
            health_fill_texture().encode_png()?,
        );
        push(
            "/defaults/images/hud/icon_player.png",
            icon_texture(BLUE).encode_png()?,
        );
        push(
            "/defaults/images/hud/icon_opponent.png",
            icon_texture(PINK).encode_png()?,
        );

        for label in ["3", "2", "1", "go"] {
            push(
                &format!("/defaults/images/countdown/{label}.png"),
                label_texture(label).encode_png()?,
            );
        }
        for label in ["perfect", "good", "bad", "miss"] {
            push(
                &format!("/defaults/images/judgement/{label}.png"),
                judgement_texture(label).encode_png()?,
            );
        }

        push(
            "/defaults/images/characters/character.png",
            character_texture().encode_png()?,
        );
        push(
            "/defaults/images/characters/character.xml",
            CHARACTER_ATLAS_XML.as_bytes().to_vec(),
        );

        push(
            "/defaults/images/ui/menu_background.png",
            menu_texture().encode_png()?,
        );
        push(
            "/defaults/images/ui/pause_panel.png",
            panel_texture(480, 240, "PAUSED", "P TO RESUME").encode_png()?,
        );
        push(
            "/defaults/images/ui/results_panel.png",
            panel_texture(640, 360, "RESULTS", "R TO RETRY").encode_png()?,
        );

        push(
            "/defaults/sounds/countdown.wav",
            wav_tone(880.0, 90, 0.35, true),
        );
        push("/defaults/sounds/hit.wav", wav_tone(540.0, 60, 0.30, true));
        push(
            "/defaults/sounds/miss.wav",
            wav_tone(170.0, 160, 0.30, true),
        );

        Ok(Self { files })
    }

    /// Every generated file, as `(virtual path, bytes)`.
    pub fn files(&self) -> &[(String, Vec<u8>)] {
        &self.files
    }

    /// Looks up one generated file.
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(candidate, _)| candidate == path)
            .map(|(_, bytes)| bytes.as_slice())
    }

    /// The parsed manifest.
    pub fn manifest(&self) -> AssetResult<ResourceManifest> {
        let bytes =
            self.file(DEFAULT_MANIFEST_PATH)
                .ok_or_else(|| AssetError::InvalidManifest {
                    reason: "default pack has no manifest".to_string(),
                })?;
        ResourceManifest::from_json(bytes)
    }

    /// Mounts the pack read-only at the lowest priority.
    ///
    /// Any physical pack or mod mounted at a positive priority shadows these
    /// files, which is how mods override default resources without a special case.
    pub fn install(&self, vfs: &mut Vfs) -> AssetResult<()> {
        let backend = vfs.mount_memory(
            DEFAULT_MOUNT_ID,
            "/",
            0,
            MountPolicy::ReadOnly,
            CaseSensitivity::Sensitive,
        )?;
        for (path, bytes) in &self.files {
            let relative = path.trim_start_matches('/');
            backend.set_file(relative, bytes.clone())?;
        }
        Ok(())
    }
}

/// Builds and installs the default pack in one step.
pub fn install_default_pack(vfs: &mut Vfs) -> AssetResult<()> {
    DefaultPack::build()?.install(vfs)
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

/// Simple RGBA canvas with coverage-based antialiasing.
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize) * 4],
        }
    }

    fn blend(&mut self, x: i64, y: i64, color: [u8; 4], coverage: f32) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        let coverage = coverage.clamp(0.0, 1.0) * (color[3] as f32 / 255.0);
        if coverage <= 0.0 {
            return;
        }
        let index = ((y as usize) * (self.width as usize) + x as usize) * 4;
        for (channel, value) in color.iter().take(3).enumerate() {
            let source = *value as f32;
            let destination = self.pixels[index + channel] as f32;
            self.pixels[index + channel] =
                (source * coverage + destination * (1.0 - coverage)).round() as u8;
        }
        let alpha = self.pixels[index + 3] as f32 / 255.0;
        let out_alpha = coverage + alpha * (1.0 - coverage);
        self.pixels[index + 3] = (out_alpha * 255.0).round() as u8;
    }

    fn vertical_gradient(&mut self, top: [u8; 4], bottom: [u8; 4]) {
        for y in 0..self.height {
            let t = y as f32 / (self.height.max(2) - 1) as f32;
            let color = [
                lerp(top[0], bottom[0], t),
                lerp(top[1], bottom[1], t),
                lerp(top[2], bottom[2], t),
                255,
            ];
            for x in 0..self.width as i64 {
                self.blend(x, y as i64, color, 1.0);
            }
        }
    }

    fn rect(&mut self, x: i64, y: i64, width: i64, height: i64, color: [u8; 4]) {
        for row in y..y + height {
            for column in x..x + width {
                self.blend(column, row, color, 1.0);
            }
        }
    }

    fn rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: [u8; 4],
    ) {
        let half = (width * 0.5, height * 0.5);
        let center = (x + half.0, y + half.1);
        let inner = (half.0 - radius, half.1 - radius);
        let start_x = (x - 1.0).floor() as i64;
        let end_x = (x + width + 1.0).ceil() as i64;
        let start_y = (y - 1.0).floor() as i64;
        let end_y = (y + height + 1.0).ceil() as i64;
        for row in start_y..end_y {
            for column in start_x..end_x {
                let dx = (column as f32 + 0.5) - center.0;
                let dy = (row as f32 + 0.5) - center.1;
                let qx = dx.abs() - inner.0;
                let qy = dy.abs() - inner.1;
                let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                let distance = outside + qx.max(qy).min(0.0) - radius;
                let coverage = -distance + 0.5;
                self.blend(column, row, color, coverage);
            }
        }
    }

    fn circle(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        let start = (cx - radius - 1.0).floor() as i64;
        let end = (cx + radius + 1.0).ceil() as i64;
        for row in start..=end {
            for column in start..=end {
                let dx = column as f32 + 0.5 - cx;
                let dy = row as f32 + 0.5 - cy;
                let coverage = radius - (dx * dx + dy * dy).sqrt() + 0.5;
                self.blend(column, row, color, coverage);
            }
        }
    }

    fn polygon(&mut self, points: &[(f32, f32)], color: [u8; 4]) {
        if points.len() < 3 {
            return;
        }
        let min_y = points.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let max_y = points.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        let min_x = points.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let max_x = points.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        for row in (min_y.floor() as i64)..=(max_y.ceil() as i64) {
            for column in (min_x.floor() as i64)..=(max_x.ceil() as i64) {
                // 2x2 subsamples give a clean edge without a rasterizer.
                let mut inside = 0;
                for sub_y in [0.25, 0.75] {
                    for sub_x in [0.25, 0.75] {
                        if point_in_polygon(column as f32 + sub_x, row as f32 + sub_y, points) {
                            inside += 1;
                        }
                    }
                }
                if inside > 0 {
                    self.blend(column, row, color, inside as f32 / 4.0);
                }
            }
        }
    }

    /// Draws blocky 3x5 text, returning the width used.
    fn text(&mut self, text: &str, x: f32, y: f32, scale: f32, color: [u8; 4]) -> f32 {
        let mut cursor = x;
        for character in text.chars() {
            let Some(rows) = font_rows(character) else {
                cursor += 4.0 * scale;
                continue;
            };
            for (row, mask) in rows.iter().enumerate() {
                for column in 0..3usize {
                    if mask & (1 << (2 - column)) == 0 {
                        continue;
                    }
                    let left = cursor + column as f32 * scale;
                    let top = y + row as f32 * scale;
                    self.rect(
                        left.round() as i64,
                        top.round() as i64,
                        scale.max(1.0).round() as i64,
                        scale.max(1.0).round() as i64,
                        color,
                    );
                }
            }
            cursor += 4.0 * scale;
        }
        cursor - x
    }

    fn into_texture(self) -> AssetResult<TextureAsset> {
        TextureAsset::from_rgba8(self.width, self.height, self.pixels).map_err(|error| {
            AssetError::Decode {
                path: fu_vfs::VfsPath::new("/defaults/generated.png").expect("static path"),
                reason: error.to_string(),
            }
        })
    }
}

fn lerp(from: u8, to: u8, t: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * t).round() as u8
}

fn point_in_polygon(x: f32, y: f32, points: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn rotate(points: &[(f32, f32)], turns: f32) -> Vec<(f32, f32)> {
    let angle = turns * PI * 0.5;
    let (sin, cos) = angle.sin_cos();
    points
        .iter()
        .map(|(x, y)| {
            let dx = x - 0.5;
            let dy = y - 0.5;
            (0.5 + dx * cos - dy * sin, 0.5 + dx * sin + dy * cos)
        })
        .collect()
}

fn scale_about_center(points: &[(f32, f32)], factor: f32) -> Vec<(f32, f32)> {
    points
        .iter()
        .map(|(x, y)| (0.5 + (x - 0.5) * factor, 0.5 + (y - 0.5) * factor))
        .collect()
}

/// Canonical arrow pointing up, in normalized coordinates (y grows downwards).
const ARROW: [(f32, f32); 7] = [
    (0.50, 0.05),
    (0.82, 0.45),
    (0.64, 0.45),
    (0.64, 0.92),
    (0.36, 0.92),
    (0.36, 0.45),
    (0.18, 0.45),
];

fn arrow_texture(fill: [u8; 4], outline: [u8; 4], lane: usize) -> TextureAsset {
    const SIZE: u32 = 128;
    let mut canvas = Canvas::new(SIZE, SIZE);
    let turns = match lane {
        0 => -1.0,
        1 => 2.0,
        3 => 1.0,
        _ => 0.0,
    };
    // Rotate and shrink in normalized space, then scale to pixels: scaling about
    // the centre only works while coordinates are still 0..1.
    let rotated = rotate(&ARROW, turns);
    let inner_normalized = scale_about_center(&rotated, 0.78);
    let to_pixels = |(x, y): (f32, f32)| (x * SIZE as f32, y * SIZE as f32);
    let outer: Vec<(f32, f32)> = rotated.into_iter().map(to_pixels).collect();
    let inner: Vec<(f32, f32)> = inner_normalized.into_iter().map(to_pixels).collect();
    canvas.polygon(&outer, outline);
    canvas.polygon(&inner, fill);
    canvas.into_texture().expect("arrow texture")
}

fn stage_texture() -> TextureAsset {
    let mut canvas = Canvas::new(1280, 720);
    canvas.vertical_gradient([30, 18, 54, 255], [8, 9, 18, 255]);

    // Neon horizon and floor.
    canvas.rect(0, 520, 1280, 4, [120, 205, 245, 255]);
    canvas.vertical_gradient_call(540, 720, [24, 26, 46, 255], [12, 13, 24, 255]);

    // Stage platform.
    canvas.polygon(
        &[
            (440.0, 520.0),
            (840.0, 520.0),
            (900.0, 600.0),
            (380.0, 600.0),
        ],
        [46, 34, 72, 255],
    );
    canvas.rect(440, 520, 400, 3, [200, 130, 240, 255]);

    // Speakers either side.
    for base_x in [90.0f32, 1030.0f32] {
        canvas.rounded_rect(base_x, 360.0, 160.0, 280.0, 14.0, [22, 24, 38, 255]);
        canvas.rounded_rect(base_x + 8.0, 368.0, 144.0, 264.0, 10.0, [40, 44, 64, 255]);
        for (index, radius) in [46.0f32, 30.0, 16.0].iter().enumerate() {
            canvas.circle(
                base_x + 80.0,
                412.0 + index as f32 * 88.0,
                *radius,
                [16, 18, 28, 255],
            );
        }
    }

    // Simple crowd dots along the bottom.
    for index in 0..24 {
        let x = 40.0 + index as f32 * 52.0;
        let y = 660.0 + ((index % 3) as f32) * 12.0;
        canvas.circle(x, y, 10.0, [70, 60, 100, 255]);
    }
    canvas.into_texture().expect("stage texture")
}

impl Canvas {
    fn vertical_gradient_call(&mut self, from_y: u32, to_y: u32, top: [u8; 4], bottom: [u8; 4]) {
        let span = to_y.saturating_sub(from_y).max(1);
        for y in from_y..to_y.min(self.height) {
            let t = (y - from_y) as f32 / span as f32;
            let color = [
                lerp(top[0], bottom[0], t),
                lerp(top[1], bottom[1], t),
                lerp(top[2], bottom[2], t),
                255,
            ];
            for x in 0..self.width as i64 {
                self.blend(x, y as i64, color, 1.0);
            }
        }
    }
}

fn health_frame_texture() -> TextureAsset {
    let mut canvas = Canvas::new(600, 48);
    canvas.rounded_rect(0.0, 0.0, 600.0, 48.0, 18.0, INK);
    canvas.rounded_rect(4.0, 4.0, 592.0, 40.0, 15.0, [58, 62, 82, 255]);
    canvas.rounded_rect(6.0, 6.0, 588.0, 36.0, 14.0, INK);
    canvas.into_texture().expect("health frame")
}

fn health_fill_texture() -> TextureAsset {
    let mut canvas = Canvas::new(600, 48);
    for x in 0..600 {
        let t = x as f32 / 599.0;
        let color = [lerp(232, 92, t), lerp(72, 226, t), lerp(84, 118, t), 255];
        canvas.rect(x as i64, 0, 1, 48, color);
    }
    canvas.into_texture().expect("health fill")
}

fn icon_texture(color: [u8; 4]) -> TextureAsset {
    let mut canvas = Canvas::new(112, 112);
    canvas.rounded_rect(0.0, 0.0, 112.0, 112.0, 26.0, INK);
    canvas.rounded_rect(4.0, 4.0, 104.0, 104.0, 22.0, color);
    canvas.circle(40.0, 46.0, 9.0, WHITE);
    canvas.circle(72.0, 46.0, 9.0, WHITE);
    canvas.circle(40.0, 46.0, 4.0, INK);
    canvas.circle(72.0, 46.0, 4.0, INK);
    canvas.rounded_rect(36.0, 72.0, 40.0, 12.0, 6.0, INK);
    canvas.into_texture().expect("icon")
}

fn label_texture(label: &str) -> TextureAsset {
    let mut canvas = Canvas::new(240, 160);
    let text = label.to_ascii_uppercase();
    let scale = 22.0;
    let width = text.chars().count() as f32 * 4.0 * scale - scale;
    let height = 5.0 * scale;
    let x = (240.0 - width) * 0.5;
    let y = (160.0 - height) * 0.5;
    canvas.text(&text, x + 3.0, y + 3.0, scale, [10, 10, 16, 200]);
    canvas.text(&text, x, y, scale, WHITE);
    canvas.into_texture().expect("countdown")
}

fn judgement_texture(label: &str) -> TextureAsset {
    let text = label.to_ascii_uppercase();
    let scale = 8.0;
    let width = text.chars().count() as f32 * 4.0 * scale;
    let canvas_width = width as u32 + 32;
    let mut canvas = Canvas::new(canvas_width, 64);
    let x = 16.0;
    let y = (64.0 - 5.0 * scale) * 0.5;
    let color = match label {
        "perfect" => GREEN,
        "good" => BLUE,
        "bad" => ORANGE,
        _ => PINK,
    };
    canvas.text(&text, x + 2.0, y + 2.0, scale, [12, 12, 18, 220]);
    canvas.text(&text, x, y, scale, color);
    canvas.into_texture().expect("judgement")
}

fn panel_texture(width: u32, height: u32, title: &str, hint: &str) -> TextureAsset {
    let mut canvas = Canvas::new(width, height);
    let w = width as f32;
    let h = height as f32;
    canvas.rounded_rect(0.0, 0.0, w, h, 22.0, PANEL);
    canvas.rounded_rect(6.0, 6.0, w - 12.0, h - 12.0, 18.0, BORDER);
    canvas.rounded_rect(10.0, 10.0, w - 20.0, h - 20.0, 15.0, [16, 18, 30, 245]);

    let title_scale = 11.0;
    let title_width = title.chars().count() as f32 * 4.0 * title_scale;
    canvas.text(title, (w - title_width) * 0.5, h * 0.22, title_scale, WHITE);
    let hint_scale = 5.0;
    let hint_width = hint.chars().count() as f32 * 4.0 * hint_scale;
    canvas.text(hint, (w - hint_width) * 0.5, h * 0.62, hint_scale, BORDER);
    canvas.into_texture().expect("panel")
}

/// Sparrow atlas for the generated character sheet: a 2x2 grid of 128 px frames.
///
/// Two idle frames (breathing) and two sing frames exercise animation playback
/// without shipping third-party artwork.
const CHARACTER_ATLAS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<TextureAtlas imagePath="character.png">
    <SubTexture name="idle0000" x="0" y="0" width="128" height="128"/>
    <SubTexture name="idle0001" x="128" y="0" width="128" height="128"/>
    <SubTexture name="singLEFT0000" x="0" y="128" width="128" height="128"/>
    <SubTexture name="singLEFT0001" x="128" y="128" width="128" height="128"/>
</TextureAtlas>
"#;

/// Draws one 128 px character frame: a body, head, eyes and an arm pose.
fn character_frame(canvas: &mut Canvas, origin_x: u32, origin_y: u32, bob: i32, singing: bool) {
    let x = origin_x as f32;
    let y = origin_y as f32 + bob as f32;
    let body = [92, 208, 255, 255];
    let shade = [52, 150, 200, 255];
    let skin = [255, 214, 170, 255];

    // Legs and body.
    canvas.rounded_rect(x + 44.0, y + 88.0, 16.0, 30.0, 6.0, shade);
    canvas.rounded_rect(x + 68.0, y + 88.0, 16.0, 30.0, 6.0, shade);
    canvas.rounded_rect(x + 36.0, y + 48.0, 56.0, 48.0, 16.0, body);

    // Head.
    canvas.circle(x + 64.0, y + 34.0, 26.0, skin);
    canvas.circle(x + 55.0, y + 30.0, 4.0, INK);
    canvas.circle(x + 73.0, y + 30.0, 4.0, INK);
    if singing {
        canvas.rounded_rect(x + 54.0, y + 44.0, 20.0, 12.0, 5.0, INK);
    } else {
        canvas.rounded_rect(x + 56.0, y + 46.0, 16.0, 5.0, 2.0, INK);
    }

    // Arms: raised while singing, down while idle.
    if singing {
        canvas.rounded_rect(x + 20.0, y + 30.0, 14.0, 34.0, 6.0, body);
        canvas.rounded_rect(x + 94.0, y + 30.0, 14.0, 34.0, 6.0, body);
    } else {
        canvas.rounded_rect(x + 20.0, y + 54.0, 14.0, 34.0, 6.0, body);
        canvas.rounded_rect(x + 94.0, y + 54.0, 14.0, 34.0, 6.0, body);
    }
}

fn character_texture() -> TextureAsset {
    let mut canvas = Canvas::new(256, 256);
    character_frame(&mut canvas, 0, 0, 0, false);
    character_frame(&mut canvas, 128, 0, 3, false);
    character_frame(&mut canvas, 0, 128, 0, true);
    character_frame(&mut canvas, 128, 128, -2, true);
    canvas.into_texture().expect("character sheet")
}

fn menu_texture() -> TextureAsset {
    let mut canvas = Canvas::new(1280, 720);
    canvas.vertical_gradient([46, 26, 78, 255], [10, 11, 22, 255]);
    for index in 0..14 {
        canvas.circle(
            60.0 + index as f32 * 92.0,
            120.0 + ((index % 4) as f32) * 28.0,
            6.0,
            [140, 120, 220, 150],
        );
    }
    canvas.rect(0, 500, 1280, 3, BORDER);
    canvas.vertical_gradient_call(500, 720, [26, 22, 48, 255], [8, 9, 18, 255]);

    let title = "FNF UNIVERSAL";
    let scale = 16.0;
    let width = title.chars().count() as f32 * 4.0 * scale;
    canvas.text(title, (1280.0 - width) * 0.5, 280.0, scale, WHITE);
    let subtitle = "DEFAULT RESOURCE PACK";
    let sub_scale = 6.0;
    let sub_width = subtitle.chars().count() as f32 * 4.0 * sub_scale;
    canvas.text(
        subtitle,
        (1280.0 - sub_width) * 0.5,
        386.0,
        sub_scale,
        BORDER,
    );
    canvas.into_texture().expect("menu")
}

/// 16-bit mono WAV of a decaying sine tone.
fn wav_tone(frequency: f32, duration_ms: u32, amplitude: f32, decay: bool) -> Vec<u8> {
    const RATE: u32 = 22_050;
    let samples = (RATE * duration_ms / 1000) as usize;
    let data_len = (samples * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);

    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());

    for index in 0..samples {
        let t = index as f32 / RATE as f32;
        let envelope = if decay {
            1.0 - index as f32 / samples.max(1) as f32
        } else {
            1.0
        };
        let value = (t * frequency * 2.0 * PI).sin() * amplitude * envelope;
        bytes.extend_from_slice(&((value * i16::MAX as f32) as i16).to_le_bytes());
    }
    bytes
}

/// 3x5 bitmap font used to rasterise the generated labels.
fn font_rows(character: char) -> Option<[u8; 5]> {
    Some(match character.to_ascii_uppercase() {
        ' ' => return None,
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b111],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'Q' => [0b010, 0b101, 0b101, 0b111, 0b011],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture::TextureAsset;

    #[test]
    fn manifest_entries_all_resolve_to_generated_files() {
        let pack = DefaultPack::build().expect("pack");
        let manifest = pack.manifest().expect("manifest");
        assert!(manifest.assets.len() >= 20, "pack covers every default key");
        for (key, path) in &manifest.assets {
            assert!(
                pack.file(path).is_some(),
                "manifest key '{key}' points at '{path}' which the pack does not generate"
            );
        }
    }

    #[test]
    fn generated_textures_decode_and_contain_visible_pixels() {
        let pack = DefaultPack::build().expect("pack");
        for (path, bytes) in pack.files() {
            if !path.ends_with(".png") {
                continue;
            }
            let texture = TextureAsset::decode_png(bytes)
                .unwrap_or_else(|error| panic!("{path} does not decode: {error}"));
            let opaque = texture
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[3] > 0)
                .count();
            assert!(
                opaque > 100,
                "{path} is {}x{} but only {opaque} visible pixels",
                texture.width,
                texture.height
            );
        }
    }

    #[test]
    fn lane_notes_differ_per_direction() {
        let pack = DefaultPack::build().expect("pack");
        let left = pack
            .file("/defaults/images/notes/note_left.png")
            .expect("left");
        let right = pack
            .file("/defaults/images/notes/note_right.png")
            .expect("right");
        assert_ne!(left, right, "each lane note is drawn pointing its own way");

        let left = TextureAsset::decode_png(left).expect("decode");
        let right = TextureAsset::decode_png(right).expect("decode");
        // Compare the mass of visible pixels in the left and right halves: a
        // left-pointing arrow is heavier on the left and vice versa.
        let mass = |texture: &TextureAsset, from: u32, to: u32| {
            let mut total = 0usize;
            for y in 0..texture.height {
                for x in from..to {
                    if texture.pixel(x, y).is_some_and(|p| p[3] > 128) {
                        total += 1;
                    }
                }
            }
            total
        };
        let half = left.width / 2;
        assert!(mass(&left, 0, half) > mass(&left, half, left.width));
        assert!(mass(&right, half, right.width) > mass(&right, 0, half));
    }

    #[test]
    fn sounds_are_valid_wav_streams() {
        let pack = DefaultPack::build().expect("pack");
        for name in ["countdown", "hit", "miss"] {
            let bytes = pack
                .file(&format!("/defaults/sounds/{name}.wav"))
                .expect("sound");
            assert!(bytes.starts_with(b"RIFF"), "{name} is a RIFF file");
            assert_eq!(&bytes[8..12], b"WAVE");
            assert!(bytes.len() > 44, "{name} has samples");
        }
    }

    #[test]
    fn installing_mounts_a_readable_low_priority_pack() {
        let mut vfs = Vfs::new();
        install_default_pack(&mut vfs).expect("install");

        let manifest = vfs.read(DEFAULT_MANIFEST_PATH).expect("manifest");
        let manifest = ResourceManifest::from_json(&manifest).expect("parse");
        assert_eq!(manifest.name, "FNF Universal Default Pack");

        let note = vfs
            .read(manifest.lookup("note.left").expect("note.left"))
            .expect("note bytes");
        assert!(TextureAsset::decode_png(&note).is_ok());

        let mount = vfs.get_mount_info(DEFAULT_MOUNT_ID).expect("mount");
        assert_eq!(mount.priority, 0, "defaults sit below every mod");
    }
}
