//! Sprite-sheet and animation metadata.
//!
//! Friday Night Funkin' ships sprites as *Sparrow* atlases: one PNG image plus an
//! XML file describing `<SubTexture>` rectangles. This module parses that XML,
//! groups frames into animations by label prefix, and exposes frame rectangles so
//! the renderer can draw a sub-rectangle of a texture.
//!
//! Adobe Animate texture-atlas bundles (a zip containing `spritemap1.json` plus a
//! PNG) are **not** parsed here; they are inventoried by the importer only.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

/// Why atlas metadata could not be read.
#[derive(Debug)]
pub struct AtlasError {
    message: String,
}

impl AtlasError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AtlasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AtlasError {}

/// A rectangle inside a sprite sheet, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasRect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Normalized UV rectangle for use with a textured quad.
///
/// The renderer's quad has `(0, 0)` at the top-left, matching image space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvRect {
    /// Left edge in `0..=1`.
    pub u0: f32,
    /// Top edge in `0..=1`.
    pub v0: f32,
    /// Right edge in `0..=1`.
    pub u1: f32,
    /// Bottom edge in `0..=1`.
    pub v1: f32,
}

impl AtlasRect {
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

/// One named frame of a sprite sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasFrame {
    /// Frame name as written in the XML, e.g. `BF NOTE LEFT0000`.
    pub name: String,
    /// Rectangle in the sheet image.
    pub frame: AtlasRect,
    /// Original untrimmed size, when the frame was trimmed.
    pub source_size: Option<(u32, u32)>,
    /// Trim offset in pixels, when the frame was trimmed.
    pub offset: (i32, i32),
    /// True when the stored frame is rotated inside the sheet.
    pub rotated: bool,
}

/// A parsed Sparrow atlas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SparrowAtlas {
    /// Sheet image reference written in the XML, when present.
    pub image_path: Option<String>,
    frames: Vec<AtlasFrame>,
}

impl SparrowAtlas {
    /// Parses a Sparrow XML document.
    ///
    /// Unknown attributes are ignored, and malformed `<SubTexture>` elements are
    /// reported instead of being dropped silently. The parser deliberately accepts
    /// both `<SubTexture .../>` and `<SubTexture ...></SubTexture>`.
    pub fn parse_xml(text: &str) -> Result<Self, AtlasError> {
        let mut atlas = SparrowAtlas::default();
        let mut cursor = 0usize;

        while let Some(tag_start) = text[cursor..].find('<') {
            let start = cursor + tag_start;
            let Some(tag_end_rel) = text[start..].find('>') else {
                break;
            };
            let tag_end = start + tag_end_rel;
            let tag = &text[start + 1..tag_end];
            let tag = tag.trim();
            cursor = tag_end + 1;

            if tag.starts_with("SubTexture") {
                let attributes = parse_attributes(tag.trim_start_matches("SubTexture"));
                let name = attributes
                    .get("name")
                    .ok_or_else(|| AtlasError::new("<SubTexture> is missing a name"))?
                    .clone();
                let frame = AtlasRect {
                    x: attribute_number(&attributes, "x")?,
                    y: attribute_number(&attributes, "y")?,
                    width: attribute_number(&attributes, "width")?,
                    height: attribute_number(&attributes, "height")?,
                };
                if frame.width == 0 || frame.height == 0 {
                    return Err(AtlasError::new(format!(
                        "frame '{name}' has a zero dimension"
                    )));
                }
                let frame_width = attributes
                    .get("frameWidth")
                    .and_then(|raw| raw.parse::<u32>().ok());
                let frame_height = attributes
                    .get("frameHeight")
                    .and_then(|raw| raw.parse::<u32>().ok());
                let source_size = match (frame_width, frame_height) {
                    (Some(width), Some(height)) if width > 0 && height > 0 => Some((width, height)),
                    _ => None,
                };
                let offset = (
                    attributes
                        .get("frameX")
                        .and_then(|raw| raw.parse::<i32>().ok())
                        .unwrap_or(0),
                    attributes
                        .get("frameY")
                        .and_then(|raw| raw.parse::<i32>().ok())
                        .unwrap_or(0),
                );
                let rotated = attributes
                    .get("rotated")
                    .is_some_and(|raw| raw.eq_ignore_ascii_case("true"));
                atlas.frames.push(AtlasFrame {
                    name,
                    frame,
                    source_size,
                    offset,
                    rotated,
                });
            } else if tag.starts_with("TextureAtlas") {
                let attributes = parse_attributes(tag.trim_start_matches("TextureAtlas"));
                if let Some(image) = attributes.get("imagePath") {
                    atlas.image_path = Some(image.clone());
                }
            }
        }

        if atlas.frames.is_empty() {
            return Err(AtlasError::new("atlas declares no <SubTexture> frames"));
        }
        Ok(atlas)
    }

    /// Every frame in document order.
    pub fn frames(&self) -> &[AtlasFrame] {
        &self.frames
    }

    /// Exact frame lookup by name.
    pub fn frame(&self, name: &str) -> Option<&AtlasFrame> {
        self.frames.iter().find(|frame| frame.name == name)
    }

    /// Frames whose name starts with `prefix`, ordered by their numeric suffix.
    ///
    /// Sparrow exports name animation frames like `spawn0000`, `spawn0001`, and
    /// older tools use `spawn 0000`. Both spellings sort correctly because only
    /// the trailing digits are compared.
    pub fn frames_for_prefix(&self, prefix: &str) -> Vec<&AtlasFrame> {
        let mut matching: Vec<(u32, &AtlasFrame)> = self
            .frames
            .iter()
            .filter(|frame| frame.name.starts_with(prefix))
            .map(|frame| (trailing_number(&frame.name).unwrap_or(0), frame))
            .collect();
        matching.sort_by_key(|(index, _)| *index);
        matching.into_iter().map(|(_, frame)| frame).collect()
    }

    /// Number of frames belonging to `prefix`.
    pub fn animation_len(&self, prefix: &str) -> usize {
        self.frames_for_prefix(prefix).len()
    }
}

fn trailing_number(name: &str) -> Option<u32> {
    let digits: String = name
        .chars()
        .rev()
        .take_while(|character| character.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

fn attribute_number(attributes: &BTreeMap<String, String>, name: &str) -> Result<u32, AtlasError> {
    let raw = attributes
        .get(name)
        .ok_or_else(|| AtlasError::new(format!("<SubTexture> is missing '{name}'")))?;
    raw.parse::<u32>()
        .map_err(|_| AtlasError::new(format!("attribute '{name}' is not a number: '{raw}'")))
}

/// Splits `key="value"` pairs out of a tag body, tolerating either quote style.
fn parse_attributes(body: &str) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::new();
    let bytes: Vec<char> = body.chars().collect();
    let mut index = 0usize;
    while index < bytes.len() {
        while index < bytes.len() && !bytes[index].is_ascii_alphabetic() {
            index += 1;
        }
        let key_start = index;
        while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == '_') {
            index += 1;
        }
        if key_start == index {
            break;
        }
        let key: String = bytes[key_start..index].iter().collect();
        while index < bytes.len() && bytes[index] != '=' && bytes[index] != '>' {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != '=' {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let quote = bytes[index];
        if quote != '"' && quote != '\'' {
            continue;
        }
        index += 1;
        let value_start = index;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        let value: String = bytes[value_start..index].iter().collect();
        attributes.insert(key, value);
        index += 1;
    }
    attributes
}

/// One animation sequence resolved against an atlas.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    /// Animation name, which is also its frame prefix.
    pub name: String,
    /// Frame names in playback order.
    pub frames: Vec<String>,
    /// Playback rate in frames per second.
    pub frame_rate: f32,
    /// Whether the animation repeats.
    pub looped: bool,
}

impl Animation {
    /// Builds an animation from an atlas prefix.
    ///
    /// Returns `None` when the atlas has no frames for that prefix, which is how
    /// callers detect that a mod character is missing an expected animation.
    pub fn from_atlas(
        atlas: &SparrowAtlas,
        prefix: &str,
        frame_rate: f32,
        looped: bool,
    ) -> Option<Self> {
        if !(frame_rate.is_finite() && frame_rate > 0.0) {
            return None;
        }
        let frames: Vec<String> = atlas
            .frames_for_prefix(prefix)
            .into_iter()
            .map(|frame| frame.name.clone())
            .collect();
        if frames.is_empty() {
            return None;
        }
        Some(Self {
            name: prefix.to_string(),
            frames,
            frame_rate,
            looped,
        })
    }

    /// The frame displayed `elapsed_seconds` into the animation.
    pub fn frame_at(&self, elapsed_seconds: f32) -> Option<&str> {
        if self.frames.is_empty() {
            return None;
        }
        let elapsed = if elapsed_seconds.is_finite() {
            elapsed_seconds.max(0.0)
        } else {
            0.0
        };
        let index = (elapsed * self.frame_rate).floor() as usize;
        let index = if self.looped {
            index % self.frames.len()
        } else {
            index.min(self.frames.len() - 1)
        };
        self.frames.get(index).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATLAS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<TextureAtlas imagePath="pixel.png">
    <SubTexture name="idle0001" x="0" y="0" width="32" height="32"/>
    <SubTexture name="idle0000" x="32" y="0" width="32" height="32"/>
    <SubTexture name="singLEFT0000" x="64" y="0" width="32" height="40" frameX="-2" frameY="-4" frameWidth="36" frameHeight="44"/>
    <SubTexture name='note0000' x='96' y='0' width='24' height='24' rotated='false'></SubTexture>
</TextureAtlas>"#;

    #[test]
    fn parses_sparrow_frames_and_image_path() {
        let atlas = SparrowAtlas::parse_xml(ATLAS).expect("atlas");
        assert_eq!(atlas.image_path.as_deref(), Some("pixel.png"));
        assert_eq!(atlas.frames().len(), 4);

        let sing = atlas.frame("singLEFT0000").expect("sing frame");
        assert_eq!(
            sing.frame,
            AtlasRect {
                x: 64,
                y: 0,
                width: 32,
                height: 40
            }
        );
        assert_eq!(sing.source_size, Some((36, 44)));
        assert_eq!(sing.offset, (-2, -4));
        assert!(!sing.rotated);
    }

    #[test]
    fn animation_frames_sort_by_numeric_suffix() {
        let atlas = SparrowAtlas::parse_xml(ATLAS).expect("atlas");
        let names: Vec<&str> = atlas
            .frames_for_prefix("idle")
            .into_iter()
            .map(|frame| frame.name.as_str())
            .collect();
        assert_eq!(names, vec!["idle0000", "idle0001"]);
        assert_eq!(atlas.animation_len("singLEFT"), 1);
        assert_eq!(atlas.animation_len("missing"), 0);
    }

    #[test]
    fn animation_playback_respects_rate_and_looping() {
        let atlas = SparrowAtlas::parse_xml(ATLAS).expect("atlas");
        let animation = Animation::from_atlas(&atlas, "idle", 10.0, true).expect("animation");
        assert_eq!(animation.frames.len(), 2);
        assert_eq!(animation.frame_at(0.0), Some("idle0000"));
        assert_eq!(animation.frame_at(0.15), Some("idle0001"));
        assert_eq!(animation.frame_at(0.25), Some("idle0000"), "loops");
        assert_eq!(animation.frame_at(f32::NAN), Some("idle0000"));

        let once = Animation::from_atlas(&atlas, "idle", 10.0, false).expect("animation");
        assert_eq!(once.frame_at(10.0), Some("idle0001"), "clamps at the end");

        assert!(Animation::from_atlas(&atlas, "idle", 0.0, true).is_none());
        assert!(Animation::from_atlas(&atlas, "nope", 10.0, true).is_none());
    }

    #[test]
    fn uv_rect_maps_pixels_to_texture_space() {
        let rect = AtlasRect {
            x: 32,
            y: 16,
            width: 32,
            height: 16,
        };
        let uv = rect.uv(128, 64).expect("uv");
        assert_eq!(uv.u0, 0.25);
        assert_eq!(uv.v0, 0.25);
        assert_eq!(uv.u1, 0.5);
        assert_eq!(uv.v1, 0.5);

        let clamped = AtlasRect {
            x: 120,
            y: 60,
            width: 32,
            height: 16,
        }
        .uv(128, 64)
        .expect("uv");
        assert_eq!(clamped.u1, 1.0);
        assert_eq!(clamped.v1, 1.0);

        assert!(AtlasRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4
        }
        .uv(0, 0)
        .is_none());
    }

    #[test]
    fn malformed_atlases_are_rejected() {
        assert!(SparrowAtlas::parse_xml("<TextureAtlas></TextureAtlas>").is_err());
        let error =
            SparrowAtlas::parse_xml(r#"<SubTexture name="a" x="0" y="0" width="1"/>"#).unwrap_err();
        assert!(error.to_string().contains("height"), "got: {error}");
        let error = SparrowAtlas::parse_xml(
            r#"<SubTexture name="a" x="zero" y="0" width="1" height="1"/>"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not a number"), "got: {error}");
    }
}
