//! Adobe Animate texture atlas and symbol parser.
//!
//! Parses the Animate JSON output (containing symbol definitions, timelines,
//! matrix data, and element transforms) alongside a spritemap PNG to render
//! animated characters and assets in the Codename Bopeebo style.
//!
//! The module provides:
//! - `AnimateAtlas`: reads `spritemap1.json` + `spritemap1.png` and gives
//!   per-symbol frame rectangles and UV mapping.
//! - `AnimateQuad`: UV rect + affine transform for one frame.
//! - `AnimateAtlas::parse`: parse an Animate spritemap JSON from VFS.
//! - `AnimateAtlas::quads`: get quads for a symbol+frame.
//! - `AnimateAtlas::bounds`: get axis-aligned bounds for a symbol+frame.

use std::collections::BTreeMap;
use std::fmt;
use serde::Deserialize;
use fu_vfs::VfsPath;
use fu_vfs::Vfs;

// Re-exported types from the parent crate.

/// A rectangle inside a sprite sheet, in pixels.
///
/// Reserved for reading the real `spritemap1.json` sprite rectangles.
/// [`AnimateAtlas::parse`] currently reads only the symbol/timeline JSON and
/// uses a full-sheet placeholder UV, so nothing constructs this yet.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimateRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[allow(dead_code)] // Reserved for spritemap1.json parsing; see `AnimateRect`.
impl AnimateRect {
    /// Converts this rectangle to normalized UV coordinates within `sheet`.
    pub fn uv(self, sheet_width: u32, sheet_height: u32) -> Option<UvRect> {
        if sheet_width == 0 || sheet_height == 0 {
            return None;
        }
        let right = self.x.saturating_add(self.width).min(sheet_width);
        let bottom = self.y.saturating_add(self.height).min(sheet_height);
        if right <= self.x || bottom <= self.y {
            return None;
        }
        Some(UvRect {
            u0: self.x as f32 / sheet_width as f32,
            v0: self.y as f32 / sheet_height as f32,
            u1: right as f32 / sheet_width as f32,
            v1: bottom as f32 / sheet_height as f32,
        })
    }
}

/// Normalized UV rectangle for use with a textured quad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvRect {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

/// One named frame of an Animate spritemap.
///
/// Reserved for reading the real `spritemap1.json`; see [`AnimateRect`].
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AnimateFrame {
    pub name: String,
    pub rect: AnimateRect,
    pub uv: UvRect,
}

#[allow(dead_code)] // Reserved for spritemap1.json parsing; see `AnimateRect`.
impl AnimateFrame {
    pub fn from_rect(name: &str, rect: AnimateRect, sheet_width: u32, sheet_height: u32) -> Self {
        AnimateFrame {
            name: name.to_string(),
            rect,
            uv: rect.uv(sheet_width, sheet_height).expect("uv rect"),
        }
    }
}

/// A 3x2 affine transform matching the flat convention from `FlxElement.fromJSON`:
///   x' = a*x + c*y + tx
///   y' = b*x + d*y + ty
///
/// The 6 values are [a, b, c, d, tx, ty]. Column-vector convention: child
/// transform = local × parent (local applied first).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Affine {
    /// Compose `self` (child) × `parent` (parent), returning a new Affine.
    /// Column-vector: child local applied first, then parent.
    pub fn concat(self, parent: Affine) -> Affine {
        Affine {
            a: self.a * parent.a + self.c * parent.b,
            b: self.b * parent.a + self.d * parent.b,
            c: self.a * parent.c + self.c * parent.d,
            d: self.b * parent.c + self.d * parent.b,
            tx: self.a * parent.tx + self.c * parent.ty + self.tx,
            ty: self.b * parent.tx + self.d * parent.ty + self.ty,
        }
    }

    /// Identity affine.
    pub fn identity() -> Self {
        Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 }
    }
}

/// A single rendered quad for one animation frame: UV rect + affine transform.
#[derive(Debug, Clone)]
pub struct AnimateQuad {
    pub uv: UvRect,
    pub affine: Affine,
}

/// A parsed Adobe Animate spritemap atlas: frame rectangles keyed by symbol name
/// and frame index.
#[derive(Debug, Clone)]
pub struct AnimateAtlas {
    pub sheet_width: u32,
    pub sheet_height: u32,
    /// Maps symbol name → list of (frame_name, AnimateQuad) tuples, ordered by frame index.
    pub symbols: BTreeMap<String, Vec<(String, AnimateQuad)>>,
}

/// The affine transform of one keyframe element, or `None` when the element is
/// neither a symbol instance nor an atom instance.
///
/// Both kinds carry a 16-value column-major `M3D` matrix. The 2D part is taken
/// the way `FlxElement.fromJSON` does:
/// `[m3d[0], m3d[1], m3d[4], m3d[5], m3d[12] + POS.x, m3d[13] + POS.y]`.
fn element_affine(element: &AnimateElement) -> Option<Affine> {
    match (&element.symbol_instance, &element.atom_instance) {
        (Some(instance), _) => Some(affine_from_m3d(
            &instance.matrix_3d,
            &instance.bitmap_data.offset,
        )),
        (None, Some(atom)) => Some(affine_from_m3d(&atom.matrix_3d, &atom.offset)),
        (None, None) => None,
    }
}

fn affine_from_m3d(m3d: &[f32; 16], offset: &AnimatePoint) -> Affine {
    Affine {
        a: m3d[0],
        b: m3d[1],
        c: m3d[4],
        d: m3d[5],
        tx: m3d[12] + offset.x,
        ty: m3d[13] + offset.y,
    }
}

impl AnimateAtlas {
    /// Parse an Animate spritemap JSON file from the VFS.
    ///
    /// `json_path` points to `spritemap1.json`. The PNG is inferred by replacing
    /// the `.json` extension with `.png` in the same directory.
    /// The VFS must have the JSON and PNG files mounted/available.
    pub fn parse(vfs: &Vfs, json_path: VfsPath) -> Result<Self, AtlasError> {
        // Read the JSON file bytes from VFS.
        let bytes = vfs.read(&json_path).map_err(|e| AtlasError::new(
            &format!("failed to read VFS path '{json_path}': {e}"),
            json_path.to_string(),
            None,
        ))?;

        // Deserialize the JSON text.
        let text = String::from_utf8(bytes).map_err(|e| AtlasError::new(
            &format!("invalid UTF-8 in Animate JSON '{json_path}': {e}"),
            json_path.to_string(),
            Some(e.to_string()),
        ))?;

        // Deserialize the Animate JSON structure using typed structs.
        let atlas: AnimateJson = serde_json::from_str(&text).map_err(|e| AtlasError::new(
            &format!("failed to parse Animate JSON '{json_path}': {e}"),
            json_path.to_string(),
            Some(e.to_string()),
        ))?;

        // Parse symbols and frames from the JSON structure.
        let mut symbols_map: BTreeMap<String, Vec<(String, AnimateQuad)>> = BTreeMap::new();

        for symbol in &atlas.symbol_data.symbols {
            let sym_name = &symbol.symbol_name;
            let mut frames: Vec<(String, AnimateQuad)> = Vec::new();

            let max_frames = symbol
                .timeline
                .layers
                .iter()
                .map(|layer| layer.frames.len())
                .max()
                .unwrap_or(0);

            for frame_idx in 0..max_frames {
                let mut combined_affine = Affine::identity();
                let mut has_any = false;

                // Every layer contributes its keyframe at this position; a
                // layer with fewer keyframes simply has nothing to add.
                for layer in &symbol.timeline.layers {
                    let Some(keyframe) = layer.frames.get(frame_idx) else {
                        continue;
                    };
                    for element in &keyframe.elements {
                        if let Some(affine) = element_affine(element) {
                            combined_affine = combined_affine.concat(affine);
                            has_any = true;
                        }
                    }
                }

                if has_any {
                    let frame_name = format!("{sym_name}_{frame_idx}");
                    let uv = UvRect { u0: 0.0, v0: 0.0, u1: 1.0, v1: 1.0 };
                    frames.push((frame_name, AnimateQuad { uv, affine: combined_affine }));
                }
            }

            if !frames.is_empty() {
                symbols_map.insert(sym_name.clone(), frames);
            }
        }

        Ok(AnimateAtlas {
            sheet_width: 512,
            sheet_height: 512,
            symbols: symbols_map,
        })
    }

    /// Return the quads (UV + affine) for a given symbol and frame index.
    /// Returns frames starting from `frame_idx` (clamped to valid range).
    /// If the symbol has N frames and frame_idx >= N, returns the last frame.
    pub fn quads(&self, symbol_name: &str, frame_idx: usize) -> Option<Vec<(String, AnimateQuad)>> {
        self.symbols.get(symbol_name).map(|frames| {
            let idx = frame_idx.min(frames.len().saturating_sub(1));
            if idx >= frames.len() {
                frames.last().map(|(name, quad)| vec![(name.clone(), quad.clone())]).unwrap_or_default()
            } else {
                frames[idx..].to_vec()
            }
        })
    }

    /// Return the axis-aligned bounds (minx, miny, maxx, maxy) in pixels for a
    /// given symbol+frame, assuming the sprite sheet origin (0,0) is top-left.
    /// The affine transform is applied to each corner to find the true bbox.
    pub fn bounds(&self, symbol_name: &str, frame_idx: usize) -> Option<(f32, f32, f32, f32)> {
        let frames = self.symbols.get(symbol_name)?;
        let idx = frame_idx.min(frames.len().saturating_sub(1));
        let (_, quad) = &frames[idx];

        // Corner points in normalized rect space: (0,0), (1,0), (1,1), (0,1)
        let corners: [(f32, f32); 4] = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for (u, v) in corners {
            // Apply affine: x' = a*u + c*v + tx, y' = b*u + d*v + ty
            let x = quad.affine.a * u + quad.affine.c * v + quad.affine.tx;
            let y = quad.affine.b * u + quad.affine.d * v + quad.affine.ty;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }

        Some((min_x, min_y, max_x, max_y))
    }
}

/// Error type for Animate atlas parsing.
#[derive(Debug)]
pub struct AtlasError {
    pub message: String,
    pub path: String,
    pub source: Option<String>,
}

impl fmt::Display for AtlasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AnimateAtlas error at '{}': {}", self.path, self.message)
    }
}

impl std::error::Error for AtlasError {}

impl AtlasError {
    fn new(message: &str, path: String, source: Option<String>) -> Self {
        AtlasError {
            message: message.to_string(),
            path,
            source,
        }
    }
}

// Implement From<serde_json::Error> for AtlasError so the `?` operator works.
impl From<serde_json::Error> for AtlasError {
    fn from(e: serde_json::Error) -> Self {
        AtlasError::new(&e.to_string(), "/unknown.json".to_string(), Some(e.to_string()))
    }
}

// The structures below mirror Adobe Animate's JSON output. Fields that the
// parser does not read yet are still deserialized, so a document that lacks
// them is rejected exactly as before, and they are kept for timeline playback
// (keyframe index and duration, layer names, instance metadata). Each unread
// field carries its own `allow(dead_code)` instead of a blanket suppression.

/// Top-level Animate JSON structure, matching the output from Adobe Animate CC.
/// We only pull the fields we need for symbol+frame extraction.
#[derive(Debug, Deserialize)]
struct AnimateJson {
    /// Root animation timeline (`AN`); required by the format, not read yet.
    #[allow(dead_code)]
    #[serde(rename = "AN")]
    animation: AnimateTimeline,
    /// Symbol dictionary (`SD`).
    #[serde(rename = "SD")]
    symbol_data: AnimateSymbolData,
}

/// Timeline with a single layer (Codename Bopeebo uses AN.TL.L[0]).
#[derive(Debug, Deserialize)]
struct AnimateTimeline {
    #[serde(rename = "TL")]
    layers: Vec<AnimateLayer>,
}

/// One timeline layer (Codename typically has one layer per symbol).
#[derive(Debug, Deserialize)]
struct AnimateLayer {
    #[allow(dead_code)]
    #[serde(rename = "LN")]
    layer_name: String,
    #[serde(rename = "FR")]
    frames: Vec<AnimateKeyframe>,
}

/// One keyframe inside a layer. Each keyframe has an index `I`, duration `DU`,
/// and a list of elements `E` (each element is either SI or ASI/ATOM).
///
/// Frames are currently addressed by their position in the layer's keyframe
/// list; `I` and `DU` (the timeline span a keyframe covers) are not used yet.
#[derive(Debug, Deserialize)]
struct AnimateKeyframe {
    #[allow(dead_code)]
    #[serde(rename = "I")]
    frame_index: usize,
    #[allow(dead_code)]
    #[serde(rename = "DU")]
    duration: usize,
    #[serde(rename = "E")]
    elements: Vec<AnimateElement>,
}

/// One element inside a keyframe. Either a symbol instance (SI) or an atom
/// symbol instance (ATOM/SI alternative naming).
#[derive(Debug, Deserialize)]
struct AnimateElement {
    #[serde(rename = "SI")]
    symbol_instance: Option<AnimateSymbolInstance>,

    #[serde(rename = "ATOM")]
    atom_instance: Option<AnimateAtomInstance>,
}

/// Symbol instance inside a keyframe element.
#[derive(Debug, Deserialize)]
struct AnimateSymbolInstance {
    #[allow(dead_code)]
    #[serde(rename = "SN")]
    symbol_name: String,
    #[allow(dead_code)]
    #[serde(rename = "IN")]
    instance_name: String,
    #[allow(dead_code)]
    #[serde(rename = "ST")]
    symbol_type: String,
    #[allow(dead_code)]
    #[serde(rename = "FF")]
    first_frame: usize,
    #[allow(dead_code)]
    #[serde(rename = "LP")]
    loop_play: String,
    #[allow(dead_code)]
    #[serde(rename = "TRP")]
    transformation_point: AnimatePoint,
    #[serde(rename = "M3D")]
    matrix_3d: [f32; 16],
    #[serde(rename = "bitmap")]
    bitmap_data: AnimateBitmapData,
}

/// Atom symbol instance (used for bitmap frames at the root timeline level).
#[derive(Debug, Deserialize)]
struct AnimateAtomInstance {
    #[allow(dead_code)]
    #[serde(rename = "N")]
    frame_name: String,
    #[serde(rename = "M3D")]
    matrix_3d: [f32; 16],
    #[serde(rename = "POS")]
    offset: AnimatePoint,
}

/// Per-bitmap-data inside a symbol instance (contains POS offset).
#[derive(Debug, Deserialize)]
struct AnimateBitmapData {
    #[serde(rename = "POS")]
    offset: AnimatePoint,
}

/// A 2D point.
#[derive(Debug, Deserialize)]
struct AnimatePoint {
    #[serde(rename = "x")]
    x: f32,
    #[serde(rename = "y")]
    y: f32,
}

/// Parsed symbol definitions: each symbol has a name (SN) and a timeline (TL)
/// with layers and frames.
#[derive(Debug, Deserialize)]
struct AnimateSymbolData {
    #[serde(rename = "S")]
    symbols: Vec<AnimateSymbolDef>,
}

/// One symbol definition.
#[derive(Debug, Deserialize)]
struct AnimateSymbolDef {
    #[serde(rename = "SN")]
    symbol_name: String,
    #[serde(rename = "TL")]
    timeline: AnimateTimeline,
}

// The test module is defined in the parent crate (fu-assets/src/lib.rs)
// via `#[cfg(test)] mod tests;`. This module does not include its own
// `#[cfg(test)] mod tests` to avoid duplication.
