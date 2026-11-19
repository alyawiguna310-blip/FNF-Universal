//! Adobe Animate texture atlases.
//!
//! An Animate export is two JSON files and one image in the same folder:
//!
//! * `Animation.json`: symbol definitions and their timelines. `AN` is the root
//!   timeline, `SD.S` lists the symbols, and `MD.FRT` is the frame rate. A
//!   timeline (`TL.L`) is a list of layers, listed top layer first; each layer
//!   (`FR`) is a list of keyframes with a start index `I`, a duration `DU`, an
//!   optional label `N` and a list of elements `E`. An element is either a symbol
//!   instance (`SI`: symbol name `SN`, first frame `FF`, loop mode `LP`, matrix
//!   `MX`) or an atlas sprite instance (`ASI`: sprite name `N`, matrix `MX`).
//! * `spritemap1.json`: the rectangle of every named sprite in the sheet image
//!   (`ATLAS.SPRITES[].SPRITE`) and the image's file name and size (`meta`).
//! * the sheet image named by `meta.image`, usually `spritemap1.png`.
//!
//! [`AnimateAtlas`] reads the two JSON files and flattens any symbol (or the root
//! timeline) at a frame into a back-to-front list of [`AnimateQuad`]s: a sheet
//! rectangle, its UVs and the affine transform that places it relative to the
//! symbol's origin. Nothing here decodes the image or draws anything.
//!
//! Supported: nested symbols, `LP`/`PO`/`SF` loop modes, 6-value `MX` and
//! 16-value `M3D` matrices, frame labels, and the frame rate. Not supported, and
//! reported rather than ignored: rotated spritemap entries (an error), and
//! elements that are neither `SI` nor `ASI` (counted in
//! [`AnimateAtlas::skipped_elements`]). Colour effects, filters, masks and the
//! transformation point `TRP` are not read.

use std::collections::BTreeMap;
use std::fmt;

use fu_vfs::{Vfs, VfsPath};
use serde::Deserialize;

use crate::atlas::{AtlasRect, UvRect};

const ANIMATION_FILE: &str = "Animation.json";
const SPRITEMAP_FILE: &str = "spritemap1.json";
/// Deepest symbol nesting accepted; a symbol that contains itself would
/// otherwise recurse forever.
const MAX_DEPTH: usize = 32;
/// Most sprites one frame may draw.
const MAX_QUADS_PER_FRAME: usize = 4096;
/// Frame rate used when `MD.FRT` is missing or unusable.
const DEFAULT_FRAME_RATE: f32 = 24.0;

/// A 2x3 affine transform in Animate's convention:
/// `x' = a*x + c*y + tx`, `y' = b*x + d*y + ty`, with y pointing down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    /// x scale / cosine term.
    pub a: f32,
    /// y shear / sine term.
    pub b: f32,
    /// x shear / negative sine term.
    pub c: f32,
    /// y scale / cosine term.
    pub d: f32,
    /// x translation.
    pub tx: f32,
    /// y translation.
    pub ty: f32,
}

impl Affine {
    /// The transform that changes nothing.
    pub fn identity() -> Self {
        Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Transforms the point `(x, y)`.
    pub fn apply(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    /// The transform that applies `self` first and then `parent`.
    ///
    /// A sprite's matrix places it inside its symbol; the symbol instance's matrix
    /// places the symbol inside its parent, so a sprite's final transform is
    /// `sprite.then(instance).then(grandparent)...`.
    pub fn then(self, parent: Affine) -> Affine {
        Affine {
            a: parent.a * self.a + parent.c * self.b,
            b: parent.b * self.a + parent.d * self.b,
            c: parent.a * self.c + parent.c * self.d,
            d: parent.b * self.c + parent.d * self.d,
            tx: parent.a * self.tx + parent.c * self.ty + parent.tx,
            ty: parent.b * self.tx + parent.d * self.ty + parent.ty,
        }
    }
}

/// One sprite of a flattened frame.
#[derive(Debug, Clone)]
pub struct AnimateQuad {
    /// Sprite name in the spritemap.
    pub sprite: String,
    /// The sprite's rectangle in the sheet image, in pixels.
    pub rect: AtlasRect,
    /// The same rectangle as UV coordinates (`v0` is the top edge).
    pub uv: UvRect,
    /// Maps the sprite's own pixel space (`0..width`, `0..height`, y down) to the
    /// symbol's origin.
    pub affine: Affine,
}

/// A named span of the root timeline, such as an animation's `Idle` label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimateLabel {
    /// Label text.
    pub name: String,
    /// First root frame of the span.
    pub start: usize,
    /// Number of frames in the span.
    pub length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopMode {
    Loop,
    PlayOnce,
    SingleFrame,
}

impl LoopMode {
    fn parse(text: Option<&str>) -> Self {
        match text {
            Some("PO") => LoopMode::PlayOnce,
            Some("SF") => LoopMode::SingleFrame,
            // "LP", a missing value and anything unknown loop, which is Animate's
            // default for graphic symbols.
            _ => LoopMode::Loop,
        }
    }
}

#[derive(Debug, Clone)]
enum Element {
    Symbol {
        name: String,
        first_frame: usize,
        mode: LoopMode,
        matrix: Affine,
    },
    Sprite {
        name: String,
        matrix: Affine,
    },
}

#[derive(Debug, Clone)]
struct Keyframe {
    start: usize,
    duration: usize,
    elements: Vec<Element>,
}

#[derive(Debug, Clone)]
struct Layer {
    frames: Vec<Keyframe>,
}

#[derive(Debug, Clone)]
struct Timeline {
    /// Top layer first, as in the file.
    layers: Vec<Layer>,
    /// One past the last frame any keyframe covers.
    length: usize,
}

/// A parsed Animate atlas: sprite rectangles plus symbol timelines.
#[derive(Debug, Clone)]
pub struct AnimateAtlas {
    /// Width of the sheet image, from `spritemap1.json`.
    pub sheet_width: u32,
    /// Height of the sheet image, from `spritemap1.json`.
    pub sheet_height: u32,
    /// File name of the sheet image, from `spritemap1.json`.
    pub image_name: String,
    /// Path of the sheet image: [`AnimateAtlas::image_name`], prefixed with the
    /// folder when the atlas was read through [`AnimateAtlas::parse`].
    pub image_path: String,
    /// Frames per second, from `MD.FRT` (24 when absent).
    pub frame_rate: f32,
    /// Elements that were neither symbol nor sprite instances and were skipped.
    pub skipped_elements: usize,
    sprites: BTreeMap<String, AtlasRect>,
    symbols: BTreeMap<String, Timeline>,
    root: Timeline,
    labels: Vec<AnimateLabel>,
}

impl AnimateAtlas {
    /// Reads an Animate atlas through the VFS.
    ///
    /// `json_path` points at `Animation.json`; `spritemap1.json` is read from the
    /// same folder.
    pub fn parse(vfs: &Vfs, json_path: VfsPath) -> Result<Self, AtlasError> {
        let location = json_path.to_string();
        let animation = read_utf8(vfs, &json_path, &location)?;

        let directory = location.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let spritemap_path = VfsPath::new(&format!("{directory}/{SPRITEMAP_FILE}"))
            .map_err(|e| AtlasError::new(&e.to_string(), location.clone(), None))?;
        let spritemap = read_utf8(vfs, &spritemap_path, &location)?;

        let mut atlas = Self::from_json(&animation, &spritemap, &location)?;
        atlas.image_path = format!("{directory}/{}", atlas.image_name);
        Ok(atlas)
    }

    /// Builds an atlas from the text of `Animation.json` and `spritemap1.json`.
    ///
    /// `source` names where they came from, for error messages.
    pub fn from_json(
        animation_json: &str,
        spritemap_json: &str,
        source: &str,
    ) -> Result<Self, AtlasError> {
        let raw: RawAnimation = serde_json::from_str(animation_json).map_err(|e| {
            AtlasError::new(
                &format!("invalid {ANIMATION_FILE}: {e}"),
                source.to_string(),
                Some(e.to_string()),
            )
        })?;
        let map: RawSpritemap = serde_json::from_str(spritemap_json).map_err(|e| {
            AtlasError::new(
                &format!("invalid {SPRITEMAP_FILE}: {e}"),
                source.to_string(),
                Some(e.to_string()),
            )
        })?;

        let mut sprites = BTreeMap::new();
        for entry in map.atlas.sprites {
            let sprite = entry.sprite;
            if sprite.rotated {
                return Err(AtlasError::new(
                    &format!(
                        "sprite '{}' is stored rotated in the sheet, which is not supported",
                        sprite.name
                    ),
                    source.to_string(),
                    None,
                ));
            }
            sprites.insert(
                sprite.name,
                AtlasRect {
                    x: sprite.x,
                    y: sprite.y,
                    width: sprite.w,
                    height: sprite.h,
                },
            );
        }

        let mut skipped_elements = 0usize;
        let root = build_timeline(&raw.an.timeline, &mut skipped_elements, source)?;
        let mut symbols = BTreeMap::new();
        if let Some(data) = raw.sd {
            for symbol in &data.symbols {
                let timeline = build_timeline(&symbol.timeline, &mut skipped_elements, source)?;
                symbols.insert(symbol.name.clone(), timeline);
            }
        }

        let mut labels = Vec::new();
        for layer in &raw.an.timeline.layers {
            for frame in &layer.frames {
                if let Some(name) = &frame.label {
                    labels.push(AnimateLabel {
                        name: name.clone(),
                        start: frame.index,
                        length: frame.duration,
                    });
                }
            }
        }

        let frame_rate = raw
            .md
            .and_then(|metadata| metadata.frame_rate)
            .filter(|rate| rate.is_finite() && *rate > 0.0)
            .unwrap_or(DEFAULT_FRAME_RATE);

        Ok(AnimateAtlas {
            sheet_width: map.meta.size.w,
            sheet_height: map.meta.size.h,
            image_path: map.meta.image.clone(),
            image_name: map.meta.image,
            frame_rate,
            skipped_elements,
            sprites,
            symbols,
            root,
            labels,
        })
    }

    /// The sheet rectangle of a named sprite.
    pub fn sprite(&self, name: &str) -> Option<AtlasRect> {
        self.sprites.get(name).copied()
    }

    /// True when `name` is a defined symbol (case-sensitive, as in the file).
    pub fn has_symbol(&self, name: &str) -> bool {
        self.symbols.contains_key(name)
    }

    /// Names of every defined symbol.
    pub fn symbol_names(&self) -> impl Iterator<Item = &str> {
        self.symbols.keys().map(String::as_str)
    }

    /// Number of frames in a symbol's timeline.
    pub fn symbol_length(&self, name: &str) -> Option<usize> {
        self.symbols.get(name).map(|timeline| timeline.length)
    }

    /// Number of frames in the root timeline.
    pub fn root_length(&self) -> usize {
        self.root.length
    }

    /// Every label on the root timeline, in file order.
    pub fn labels(&self) -> &[AnimateLabel] {
        &self.labels
    }

    /// The first root label called `name`.
    pub fn label(&self, name: &str) -> Option<&AnimateLabel> {
        self.labels.iter().find(|label| label.name == name)
    }

    /// The sprites a symbol draws at `frame`, back to front.
    ///
    /// A frame no keyframe covers draws nothing. Fails when the symbol, or a
    /// symbol or sprite it uses, is not defined.
    pub fn symbol_quads(&self, symbol: &str, frame: usize) -> Result<Vec<AnimateQuad>, AtlasError> {
        let timeline = self
            .symbols
            .get(symbol)
            .ok_or_else(|| error(format!("symbol '{symbol}' is not defined"), ANIMATION_FILE))?;
        let mut quads = Vec::new();
        self.collect(timeline, frame, Affine::identity(), 0, &mut quads)?;
        Ok(quads)
    }

    /// The sprites the root timeline draws at `frame`, back to front.
    pub fn root_quads(&self, frame: usize) -> Result<Vec<AnimateQuad>, AtlasError> {
        let mut quads = Vec::new();
        self.collect(&self.root, frame, Affine::identity(), 0, &mut quads)?;
        Ok(quads)
    }

    fn collect(
        &self,
        timeline: &Timeline,
        frame: usize,
        parent: Affine,
        depth: usize,
        out: &mut Vec<AnimateQuad>,
    ) -> Result<(), AtlasError> {
        if depth > MAX_DEPTH {
            return Err(error(
                format!("symbols are nested more than {MAX_DEPTH} deep (is one inside itself?)"),
                ANIMATION_FILE,
            ));
        }
        // Layers are listed top first, so the last layer is drawn first.
        for layer in timeline.layers.iter().rev() {
            let Some(keyframe) = layer
                .frames
                .iter()
                .find(|key| frame >= key.start && frame - key.start < key.duration)
            else {
                continue;
            };
            let local = frame - keyframe.start;
            for element in &keyframe.elements {
                match element {
                    Element::Sprite { name, matrix } => {
                        let rect = *self.sprites.get(name).ok_or_else(|| {
                            error(
                                format!("sprite '{name}' is not in the spritemap"),
                                SPRITEMAP_FILE,
                            )
                        })?;
                        let uv = rect.uv(self.sheet_width, self.sheet_height).ok_or_else(|| {
                            error(
                                format!(
                                    "sprite '{name}' lies outside the {}x{} sheet",
                                    self.sheet_width, self.sheet_height
                                ),
                                SPRITEMAP_FILE,
                            )
                        })?;
                        if out.len() >= MAX_QUADS_PER_FRAME {
                            return Err(error(
                                format!("a frame draws more than {MAX_QUADS_PER_FRAME} sprites"),
                                ANIMATION_FILE,
                            ));
                        }
                        out.push(AnimateQuad {
                            sprite: name.clone(),
                            rect,
                            uv,
                            affine: matrix.then(parent),
                        });
                    }
                    Element::Symbol {
                        name,
                        first_frame,
                        mode,
                        matrix,
                    } => {
                        let child = self.symbols.get(name).ok_or_else(|| {
                            error(format!("symbol '{name}' is not defined"), ANIMATION_FILE)
                        })?;
                        let last = child.length.saturating_sub(1);
                        let child_frame = match mode {
                            LoopMode::Loop => (first_frame + local) % child.length.max(1),
                            LoopMode::PlayOnce => (first_frame + local).min(last),
                            LoopMode::SingleFrame => (*first_frame).min(last),
                        };
                        self.collect(child, child_frame, matrix.then(parent), depth + 1, out)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn read_utf8(vfs: &Vfs, path: &VfsPath, location: &str) -> Result<String, AtlasError> {
    let bytes = vfs
        .read(path)
        .map_err(|e| AtlasError::new(&format!("failed to read '{path}': {e}"), location.to_string(), None))?;
    String::from_utf8(bytes).map_err(|e| {
        AtlasError::new(
            &format!("'{path}' is not valid UTF-8: {e}"),
            location.to_string(),
            Some(e.to_string()),
        )
    })
}

fn error(message: String, file: &str) -> AtlasError {
    AtlasError::new(&message, file.to_string(), None)
}

/// Reads a matrix from `MX` (`[a, b, c, d, tx, ty]`) or, failing that, the 2D
/// part of a 16-value column-major `M3D`. A missing matrix is the identity.
fn matrix(mx: Option<&[f32]>, m3d: Option<&[f32]>) -> Result<Affine, String> {
    if let Some(values) = mx {
        if values.len() != 6 {
            return Err(format!("MX has {} values, expected 6", values.len()));
        }
        return Ok(Affine {
            a: values[0],
            b: values[1],
            c: values[2],
            d: values[3],
            tx: values[4],
            ty: values[5],
        });
    }
    if let Some(values) = m3d {
        if values.len() != 16 {
            return Err(format!("M3D has {} values, expected 16", values.len()));
        }
        return Ok(Affine {
            a: values[0],
            b: values[1],
            c: values[4],
            d: values[5],
            tx: values[12],
            ty: values[13],
        });
    }
    Ok(Affine::identity())
}

fn build_timeline(
    raw: &RawTimeline,
    skipped: &mut usize,
    source: &str,
) -> Result<Timeline, AtlasError> {
    let mut layers = Vec::with_capacity(raw.layers.len());
    let mut length = 0usize;
    for raw_layer in &raw.layers {
        let mut frames = Vec::with_capacity(raw_layer.frames.len());
        for raw_frame in &raw_layer.frames {
            length = length.max(raw_frame.index.saturating_add(raw_frame.duration));
            let mut elements = Vec::with_capacity(raw_frame.elements.len());
            for raw_element in &raw_frame.elements {
                match (&raw_element.symbol_instance, &raw_element.sprite_instance) {
                    (Some(instance), _) => {
                        let placed = matrix(instance.mx.as_deref(), instance.m3d.as_deref())
                            .map_err(|message| {
                                AtlasError::new(
                                    &format!("symbol instance '{}': {message}", instance.symbol),
                                    source.to_string(),
                                    None,
                                )
                            })?;
                        elements.push(Element::Symbol {
                            name: instance.symbol.clone(),
                            first_frame: instance.first_frame,
                            mode: LoopMode::parse(instance.loop_mode.as_deref()),
                            matrix: placed,
                        });
                    }
                    (None, Some(sprite)) => {
                        let placed = matrix(sprite.mx.as_deref(), sprite.m3d.as_deref())
                            .map_err(|message| {
                                AtlasError::new(
                                    &format!("sprite instance '{}': {message}", sprite.sprite),
                                    source.to_string(),
                                    None,
                                )
                            })?;
                        elements.push(Element::Sprite {
                            name: sprite.sprite.clone(),
                            matrix: placed,
                        });
                    }
                    (None, None) => *skipped += 1,
                }
            }
            frames.push(Keyframe {
                start: raw_frame.index,
                duration: raw_frame.duration,
                elements,
            });
        }
        layers.push(Layer { frames });
    }
    Ok(Timeline { layers, length })
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

// The structures below mirror the parts of Animate's JSON that are read. Keys
// that are not listed (layer names `LN`, `ST`, `TRP`, `STI`, colour effects, ...)
// are ignored by serde.

#[derive(Debug, Deserialize)]
struct RawAnimation {
    #[serde(rename = "AN")]
    an: RawRootTimeline,
    #[serde(rename = "SD", default)]
    sd: Option<RawSymbolData>,
    #[serde(rename = "MD", default)]
    md: Option<RawMetadata>,
}

#[derive(Debug, Deserialize)]
struct RawRootTimeline {
    #[serde(rename = "TL")]
    timeline: RawTimeline,
}

#[derive(Debug, Deserialize)]
struct RawTimeline {
    #[serde(rename = "L", default)]
    layers: Vec<RawLayer>,
}

#[derive(Debug, Deserialize)]
struct RawLayer {
    #[serde(rename = "FR", default)]
    frames: Vec<RawFrame>,
}

#[derive(Debug, Deserialize)]
struct RawFrame {
    #[serde(rename = "N", default)]
    label: Option<String>,
    #[serde(rename = "I")]
    index: usize,
    #[serde(rename = "DU")]
    duration: usize,
    #[serde(rename = "E", default)]
    elements: Vec<RawElement>,
}

#[derive(Debug, Deserialize)]
struct RawElement {
    #[serde(rename = "SI", default)]
    symbol_instance: Option<RawSymbolInstance>,
    #[serde(rename = "ASI", default)]
    sprite_instance: Option<RawSpriteInstance>,
}

#[derive(Debug, Deserialize)]
struct RawSymbolInstance {
    #[serde(rename = "SN")]
    symbol: String,
    #[serde(rename = "FF", default)]
    first_frame: usize,
    #[serde(rename = "LP", default)]
    loop_mode: Option<String>,
    #[serde(rename = "MX", default)]
    mx: Option<Vec<f32>>,
    #[serde(rename = "M3D", default)]
    m3d: Option<Vec<f32>>,
}

#[derive(Debug, Deserialize)]
struct RawSpriteInstance {
    #[serde(rename = "N")]
    sprite: String,
    #[serde(rename = "MX", default)]
    mx: Option<Vec<f32>>,
    #[serde(rename = "M3D", default)]
    m3d: Option<Vec<f32>>,
}

#[derive(Debug, Deserialize)]
struct RawSymbolData {
    #[serde(rename = "S", default)]
    symbols: Vec<RawSymbol>,
}

#[derive(Debug, Deserialize)]
struct RawSymbol {
    #[serde(rename = "SN")]
    name: String,
    #[serde(rename = "TL")]
    timeline: RawTimeline,
}

#[derive(Debug, Deserialize)]
struct RawMetadata {
    #[serde(rename = "FRT", default)]
    frame_rate: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct RawSpritemap {
    #[serde(rename = "ATLAS")]
    atlas: RawAtlas,
    meta: RawMeta,
}

#[derive(Debug, Deserialize)]
struct RawAtlas {
    #[serde(rename = "SPRITES", default)]
    sprites: Vec<RawSpriteEntry>,
}

#[derive(Debug, Deserialize)]
struct RawSpriteEntry {
    #[serde(rename = "SPRITE")]
    sprite: RawSprite,
}

#[derive(Debug, Deserialize)]
struct RawSprite {
    name: String,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    #[serde(default)]
    rotated: bool,
}

#[derive(Debug, Deserialize)]
struct RawMeta {
    image: String,
    size: RawSize,
}

#[derive(Debug, Deserialize)]
struct RawSize {
    w: u32,
    h: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two sprites on a 64x32 sheet: "0" is 10x20 at (0, 0), "1" is 8x8 at (10, 0).
    const SPRITEMAP: &str = r#"{"ATLAS":{"SPRITES":[
{"SPRITE":{"name":"0","x":0,"y":0,"w":10,"h":20,"rotated":false}},
{"SPRITE":{"name":"1","x":10,"y":0,"w":8,"h":8,"rotated":false}}]},
"meta":{"app":"Adobe Animate","image":"spritemap1.png","format":"RGBA8888","size":{"w":64,"h":32},"resolution":"1"}}"#;

    /// The structure of a real export: a root timeline with a label layer and an
    /// art layer, nested symbols, all three loop modes, an M3D matrix and one
    /// element of an unsupported kind.
    const ANIMATION: &str = r#"{"AN":{"SN":"Root","TL":{"L":[
{"LN":"Labels","FR":[{"N":"Idle","I":0,"DU":2,"E":[]},{"N":"Run","I":2,"DU":3,"E":[]}]},
{"LN":"Body","FR":[{"I":0,"DU":5,"E":[{"SI":{"SN":"Pose","FF":0,"ST":"G","LP":"LP","MX":[1,0,0,1,100,50]}}]}]}
]}},
"SD":{"S":[
{"SN":"Pose","TL":{"L":[
 {"LN":"Top","FR":[{"I":0,"DU":1,"E":[{"ASI":{"N":"1","MX":[1,0,0,1,0,0]}}]}]},
 {"LN":"Bottom","FR":[{"I":0,"DU":1,"E":[{"ASI":{"N":"0","MX":[2,0,0,2,5,5]}}]}]}
]}},
{"SN":"Leaf","TL":{"L":[{"LN":"L","FR":[
 {"I":0,"DU":1,"E":[{"ASI":{"N":"0","MX":[1,0,0,1,0,0]}}]},
 {"I":1,"DU":1,"E":[{"ASI":{"N":"1","MX":[1,0,0,1,0,0]}}]}]}]}},
{"SN":"Looper","TL":{"L":[{"LN":"L","FR":[{"I":0,"DU":4,"E":[{"SI":{"SN":"Leaf","FF":0,"ST":"G","LP":"LP","MX":[1,0,0,1,0,0]}}]}]}]}},
{"SN":"Once","TL":{"L":[{"LN":"L","FR":[{"I":0,"DU":4,"E":[{"SI":{"SN":"Leaf","FF":0,"ST":"G","LP":"PO","MX":[1,0,0,1,0,0]}}]}]}]}},
{"SN":"Still","TL":{"L":[{"LN":"L","FR":[{"I":0,"DU":4,"E":[{"SI":{"SN":"Leaf","FF":1,"ST":"G","LP":"SF","MX":[1,0,0,1,0,0]}}]}]}]}},
{"SN":"Matrix3D","TL":{"L":[{"LN":"L","FR":[{"I":0,"DU":1,"E":[{"ASI":{"N":"0","M3D":[1,0,0,0,0,1,0,0,0,0,1,0,7,9,0,1]}}]}]}]}},
{"SN":"Mystery","TL":{"L":[{"LN":"L","FR":[{"I":0,"DU":1,"E":[{"TXT":{"x":1}}]}]}]}}
]},
"MD":{"FRT":30}}"#;

    fn atlas() -> AnimateAtlas {
        AnimateAtlas::from_json(ANIMATION, SPRITEMAP, "test").expect("atlas")
    }

    fn sprites(quads: &[AnimateQuad]) -> Vec<&str> {
        quads.iter().map(|quad| quad.sprite.as_str()).collect()
    }

    #[test]
    fn reads_the_sheet_the_frame_rate_and_the_labels() {
        let atlas = atlas();
        assert_eq!((atlas.sheet_width, atlas.sheet_height), (64, 32));
        assert_eq!(atlas.image_name, "spritemap1.png");
        assert_eq!(atlas.frame_rate, 30.0);
        assert_eq!(atlas.root_length(), 5);
        assert_eq!(
            atlas.labels(),
            [
                AnimateLabel {
                    name: "Idle".to_string(),
                    start: 0,
                    length: 2
                },
                AnimateLabel {
                    name: "Run".to_string(),
                    start: 2,
                    length: 3
                }
            ]
        );
        assert_eq!(atlas.label("Run").map(|label| label.start), Some(2));
        assert!(atlas.label("Nope").is_none());
        assert!(atlas.has_symbol("Pose"));
        assert!(!atlas.has_symbol("pose"), "symbol names are case-sensitive");
        assert_eq!(atlas.symbol_length("Leaf"), Some(2));
        assert_eq!(
            atlas.sprite("1"),
            Some(AtlasRect {
                x: 10,
                y: 0,
                width: 8,
                height: 8
            })
        );
    }

    #[test]
    fn the_frame_rate_defaults_to_24() {
        // The metadata block follows a newline, so match it without the comma.
        let without_rate = ANIMATION.replace(r#""MD":{"FRT":30}"#, r#""MD":{}"#);
        assert_ne!(without_rate, ANIMATION, "the fixture really changed");
        let atlas = AnimateAtlas::from_json(&without_rate, SPRITEMAP, "test").expect("atlas");
        assert_eq!(atlas.frame_rate, 24.0);
    }

    #[test]
    fn sprites_are_drawn_back_to_front_with_real_uvs() {
        let atlas = atlas();
        // Pose lists its top layer first, so the bottom layer ("0") is drawn first.
        let quads = atlas.symbol_quads("Pose", 0).expect("quads");
        assert_eq!(sprites(&quads), ["0", "1"]);

        let small = &quads[1];
        assert_eq!(
            small.rect,
            AtlasRect {
                x: 10,
                y: 0,
                width: 8,
                height: 8
            }
        );
        assert_eq!(small.uv.u0, 10.0 / 64.0);
        assert_eq!(small.uv.v0, 0.0);
        assert_eq!(small.uv.u1, 18.0 / 64.0);
        assert_eq!(small.uv.v1, 8.0 / 32.0);
    }

    #[test]
    fn matrices_compose_from_the_sprite_up_through_every_parent() {
        let atlas = atlas();
        // The root places Pose at (100, 50); Pose scales sprite "0" by 2 and moves
        // it by (5, 5) inside itself: x' = 2x + 5 + 100, y' = 2y + 5 + 50.
        let quads = atlas.root_quads(0).expect("quads");
        assert_eq!(sprites(&quads), ["0", "1"]);
        let affine = quads[0].affine;
        assert_eq!(
            affine,
            Affine {
                a: 2.0,
                b: 0.0,
                c: 0.0,
                d: 2.0,
                tx: 105.0,
                ty: 55.0
            }
        );
        assert_eq!(affine.apply(10.0, 20.0), (125.0, 95.0));
        // Sprite "1" has the identity inside Pose, so only the root's move applies.
        assert_eq!(quads[1].affine.apply(0.0, 0.0), (100.0, 50.0));
    }

    #[test]
    fn then_applies_the_child_first() {
        let child = Affine {
            a: 2.0,
            b: 0.0,
            c: 0.0,
            d: 3.0,
            tx: 1.0,
            ty: 1.0,
        };
        let parent = Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 10.0,
            ty: 20.0,
        };
        let (x, y) = child.apply(4.0, 5.0);
        assert_eq!(parent.apply(x, y), child.then(parent).apply(4.0, 5.0));
        assert_eq!(child.then(parent).apply(4.0, 5.0), (19.0, 36.0));

        // A rotation by 90 degrees followed by a move: (1, 0) -> (0, 1) -> (5, 1).
        let rotate = Affine {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            tx: 0.0,
            ty: 0.0,
        };
        let shift = Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 5.0,
            ty: 0.0,
        };
        assert_eq!(rotate.then(shift).apply(1.0, 0.0), (5.0, 1.0));
    }

    #[test]
    fn loop_modes_choose_the_frame_inside_a_nested_symbol() {
        let atlas = atlas();
        let at = |symbol: &str, frame: usize| -> Vec<String> {
            atlas
                .symbol_quads(symbol, frame)
                .expect("quads")
                .into_iter()
                .map(|quad| quad.sprite)
                .collect()
        };
        // Leaf has two frames: "0" then "1".
        assert_eq!(at("Looper", 0), ["0"]);
        assert_eq!(at("Looper", 1), ["1"]);
        assert_eq!(at("Looper", 2), ["0"], "a looping instance wraps");
        assert_eq!(at("Looper", 3), ["1"]);
        assert!(at("Looper", 4).is_empty(), "no keyframe covers frame 4");

        assert_eq!(at("Once", 0), ["0"]);
        assert_eq!(at("Once", 1), ["1"]);
        assert_eq!(at("Once", 3), ["1"], "a play-once instance holds its last frame");

        for frame in 0..4 {
            assert_eq!(at("Still", frame), ["1"], "a single-frame instance never moves");
        }
    }

    #[test]
    fn a_16_value_matrix_uses_its_2d_part() {
        let atlas = atlas();
        let quads = atlas.symbol_quads("Matrix3D", 0).expect("quads");
        assert_eq!(
            quads[0].affine,
            Affine {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                tx: 7.0,
                ty: 9.0
            }
        );
    }

    #[test]
    fn unsupported_elements_are_counted_not_hidden() {
        let atlas = atlas();
        assert_eq!(atlas.skipped_elements, 1);
        assert!(atlas.symbol_quads("Mystery", 0).expect("quads").is_empty());
    }

    #[test]
    fn broken_references_and_unsupported_features_are_errors() {
        let atlas = atlas();
        let error = atlas.symbol_quads("Nowhere", 0).unwrap_err();
        assert!(error.to_string().contains("'Nowhere'"), "{error}");

        let rotated = SPRITEMAP.replace(
            r#""name":"1","x":10,"y":0,"w":8,"h":8,"rotated":false"#,
            r#""name":"1","x":10,"y":0,"w":8,"h":8,"rotated":true"#,
        );
        assert_ne!(rotated, SPRITEMAP, "the fixture really changed");
        let error = AnimateAtlas::from_json(ANIMATION, &rotated, "test").unwrap_err();
        assert!(error.to_string().contains("rotated"), "{error}");

        let unknown_sprite = ANIMATION.replace(
            r#""N":"1","MX":[1,0,0,1,0,0]"#,
            r#""N":"9","MX":[1,0,0,1,0,0]"#,
        );
        assert_ne!(unknown_sprite, ANIMATION, "the fixture really changed");
        let atlas = AnimateAtlas::from_json(&unknown_sprite, SPRITEMAP, "test").expect("atlas");
        let error = atlas.symbol_quads("Pose", 0).unwrap_err();
        assert!(error.to_string().contains("sprite '9'"), "{error}");

        let bad_matrix = ANIMATION.replace("[2,0,0,2,5,5]", "[2,0,0]");
        assert_ne!(bad_matrix, ANIMATION, "the fixture really changed");
        let error = AnimateAtlas::from_json(&bad_matrix, SPRITEMAP, "test").unwrap_err();
        assert!(error.to_string().contains("MX"), "{error}");

        assert!(AnimateAtlas::from_json("{}", SPRITEMAP, "test").is_err());
        assert!(AnimateAtlas::from_json(ANIMATION, "{}", "test").is_err());
    }

    #[test]
    fn a_symbol_inside_itself_is_refused_instead_of_recursing_forever() {
        let animation = r#"{"AN":{"SN":"Root","TL":{"L":[]}},
"SD":{"S":[{"SN":"Loop","TL":{"L":[{"LN":"L","FR":[{"I":0,"DU":1,"E":[{"SI":{"SN":"Loop","FF":0,"LP":"LP","MX":[1,0,0,1,0,0]}}]}]}]}}]}}"#;
        let atlas = AnimateAtlas::from_json(animation, SPRITEMAP, "test").expect("atlas");
        let error = atlas.symbol_quads("Loop", 0).unwrap_err();
        assert!(error.to_string().contains("nested"), "{error}");
    }
}
