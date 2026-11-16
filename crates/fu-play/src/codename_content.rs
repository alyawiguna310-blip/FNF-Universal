//! Optional real Codename Engine character and stage assets for the playfield.
//! 
//! The playable runtime ships a generated default pack, so it runs with no
//! Codename installation. When one is available (`--codename <assets dir>`), this
//! module reads the stage and character XML plus their Sparrow atlases so the
//! playfield shows the game's own characters and backdrop instead of the
//! generated placeholders.
//! 
//! Only what the playfield draws is read: stage sprite layers, character sprite
//! sheets and their named animations. Scripts, Animate atlases and stage scripts
//! are reported as warnings rather than silently ignored.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use fu_assets::{AtlasFrame, SparrowAtlas, TextureAsset};

use crate::resources::{keys, GameResources};

// Helper functions - must be defined before they are used

/// Extracts a floating-point attribute, returning `default` when it is
/// missing or not a number. This mirrors Codename's
/// `Std.parseFloat(...).getDefault(...)`: an absent attribute must not be
/// confused with a zero one, and Codename never clamps negative offsets.
fn number(map: &BTreeMap<String, String>, key: &str, default: f32) -> f32 {
    map.get(key)
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(default)
}

/// True when `relative` stays inside the root it is joined to: not empty, not
/// absolute, no drive prefix or backslash, and no `..` component.
///
/// Stage folders, sprite names and character ids come from mod XML and song
/// metadata, which are untrusted, so every lookup goes through this check
/// before it touches the file system.
fn safe_relative(relative: &str) -> bool {
    !relative.is_empty()
        && !relative.starts_with('/')
        && !relative.contains('\\')
        && !relative.contains(':')
        && !relative.contains('\0')
        && relative.split('/').all(|part| part != "..")
}

/// `relative` inside one root, when it is a safe path to an existing file.
fn in_root(root: &Path, relative: &str) -> Option<PathBuf> {
    if !safe_relative(relative) {
        return None;
    }
    let candidate = root.join(relative);
    candidate.is_file().then_some(candidate)
}

/// Find a file in mod_root first, then assets.
fn find_file(mod_root: Option<&Path>, assets: &Path, relative: &str) -> Option<PathBuf> {
    if let Some(found) = mod_root.and_then(|root| in_root(root, relative)) {
        return Some(found);
    }
    in_root(assets, relative)
}

fn read_texture(path: &Path) -> Option<TextureAsset> {
    let bytes = std::fs::read(path).ok()?;
    TextureAsset::decode_png(&bytes).ok()
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn string_field(value: &serde_json::Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(|value| value.as_str()))
        .map(str::to_string)
}

/// Every start or empty tag in document order.
fn elements(xml: &str) -> Vec<String> {
    let mut elements = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = xml[cursor..].find('<') {
        let start = cursor + offset;
        let Some(end_offset) = xml[start..].find('>') else {
            break;
        };
        let end = start + end_offset;
        let tag = xml[start + 1..end].trim();
        cursor = end + 1;
        if tag.starts_with('!') || tag.starts_with('?') || tag.starts_with('/') {
            continue;
        }
        let name: String = tag
            .chars()
            .take_while(|character| !character.is_whitespace() && *character != '/')
            .collect();
        if name.is_empty() {
            continue;
        }
        let body: String = tag
            .chars()
            .skip(name.chars().count())
            .collect::<String>()
            .trim()
            .trim_end_matches('/')
            .trim()
            .to_string();
        elements.push(format!("{name} {body}"));
    }
    elements
}

fn first_tag(xml: &str, name: &str) -> Option<String> {
    elements(xml)
        .into_iter()
        .find(|element| element_name(element) == name)
}

fn element_name(element: &str) -> String {
    element
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Attributes of an element string produced by [`elements`].
fn attributes(element: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut rest = element
        .char_indices()
        .find(|(_, character)| character.is_whitespace())
        .map(|(index, _)| &element[index..])
        .unwrap_or("");
    while let Some(equals) = rest.find('=') {
        let name = rest[..equals].trim();
        let after = &rest[equals + 1..];
        let Some(quote) = after.chars().next() else {
            break;
        };
        if quote != '\"' && quote != '\'' {
            // No opening quote before the value; skip a single character so
            // the scan always makes progress.
            rest = after.get(1..).unwrap_or("");
            continue;
        }
        let value_start = 1;
        let Some(value_end) = after[value_start..].find(quote) else {
            rest = after.get(value_start..).unwrap_or("");
            continue;
        };
        // `value_end` indexes into `after[value_start..]`; the value spans
        // `after[1..=value_end]` and the closing quote is at `value_start +
        // value_end`.
        let value = &after[value_start..value_start + value_end];
        map.insert(name.to_lowercase(), value.to_string());
        rest = after.get(value_start + value_end + 1..).unwrap_or("");
    }
    map
}

/// Codename's reference screen is 1280x720 and the playfield camera is 12 world
/// units tall, so one world unit is 60 pixels.
pub const PIXELS_PER_UNIT: f32 = 60.0;

/// Which gameplay slot a character fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// The player character, usually `bf`.
    Player,
    /// The opponent, usually `dad`.
    Opponent,
    /// The spectator, usually `gf`.
    Girlfriend,
}

impl Role {
    /// All roles, in draw order (back to front).
    pub const ALL: [Role; 3] = [Role::Girlfriend, Role::Opponent, Role::Player];

    /// Stable identifier used in node names and diagnostics.
    pub fn id(self) -> &'static str {
        match self {
            Role::Player => "player",
            Role::Opponent => "opponent",
            Role::Girlfriend => "girlfriend",
        }
    }

    fn default_position(self) -> (f32, f32) {
        // `Stage.getDefaultPos` in Codename's `Stage.hx`.
        match self {
            Role::Player => (770.0, 100.0),
            Role::Opponent => (100.0, 100.0),
            Role::Girlfriend => (400.0, 130.0),
        }
    }

    fn default_flip(self) -> bool {
        self == Role::Player
    }

    fn default_scroll(self) -> f32 {
        match self {
            Role::Girlfriend => 0.95,
            _ => 1.0,
        }
    }
}

/// One named character animation, already resolved to atlas frames.
#[derive(Debug, Clone, Default)]
pub struct RigAnimation {
    /// Frames in playback order.
    pub frames: Vec<AtlasFrame>,
    /// Animation framerate declared by the character XML.
    pub fps: f32,
    /// Frame offset declared by the character XML, in pixels.
    pub offset: (f32, f32),
    /// Whether the animation loops.
    pub looping: bool,
}

/// A character definition and the sprite sheet its frames live in.
#[derive(Debug)]
pub struct CharacterRig {
    /// Gameplay slot.
    pub role: Role,
    /// Character id, for example `bf`.
    pub name: String,
    /// Character sheet.
    pub texture: TextureAsset,
    /// Animations keyed by the name charts and gameplay use (`idle`, `singLEFT`, ...).
    pub animations: BTreeMap<String, RigAnimation>,
    /// Character position on the stage, in pixels.
    pub position: (f32, f32),
    /// Uniform scale multiplier.
    pub scale: f32,
    /// Whether the character is mirrored horizontally.
    pub flip_x: bool,
    /// Parallax factor.
    pub scroll: f32,
    /// Health-bar icon id declared by the character XML.
    pub icon: Option<String>,
}

impl CharacterRig {
    /// Animation by name, case-insensitively, as Codename matches them.
    pub fn animation(&self, name: &str) -> Option<&RigAnimation> {
        self.animations
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, animation)| animation)
    }

    /// Frame of `name` for an absolute song time. Falls back to `idle` so a
    /// missing sing pose does not blank the character.
    pub fn frame_at(&self, name: &str, song_ms: f32) -> Option<(String, &AtlasFrame)> {
        let animation = self
            .animation(name)
            .filter(|animation| !animation.frames.is_empty())
            .or_else(|| self.animation("idle"))?;
        let index = if animation.looping {
            ((song_ms.max(0.0) / 1000.0 * animation.fps) as usize) % animation.frames.len()
        } else {
            0
        };
        let frame = animation
            .frames
            .get(index)
            .or_else(|| animation.frames.first())?;
        Some((name.to_string(), frame))
    }
}

/// One stage background layer.
#[derive(Debug)]
pub struct StageLayer {
    /// Layer name from the stage XML, for diagnostics.
    pub name: String,
    /// Layer image.
    pub texture: TextureAsset,
    /// Position in screen pixels, relative to the 1280x720 top-left.
    pub position: (f32, f32),
    /// Uniform scale multiplier (XML scale times the stage zoom).
    pub scale: f32,
    /// Stage zoom factor, applied about the screen centre.
    pub zoom: f32,
    /// Parallax factors.
    pub scroll: (f32, f32),
    /// Opacity multiplier.
    pub alpha: f32,
    /// True for layers declared after the character anchors, which Codename
    /// draws in front of them.
    pub front: bool,
}

/// The real note skin: notes, receptors and sustain pieces in one atlas.
#[derive(Debug)]
pub struct NoteSkin {
    /// Sheet image the atlas rectangles refer to.
    pub texture: TextureAsset,
    /// Parsed Sparrow atlas.
    pub atlas: SparrowAtlas,
    /// Path the sheet was read from, for diagnostics.
    pub source: PathBuf,
}

impl NoteSkin {
    /// Crops one named atlas frame out of the sheet.
    pub fn cropped(&self, frame: &str) -> Option<TextureAsset> {
        let frame = self.atlas.frame(frame)?;
        Some(self.texture.crop(
            frame.frame.x,
            frame.frame.y,
            frame.frame.width,
            frame.frame.height,
        ))
    }

    /// Crops the first frame of an animation prefix.
    pub fn cropped_animation(&self, prefix: &str) -> Option<TextureAsset> {
        let frame = self.atlas.frames_for_prefix(prefix).into_iter().next()?;
        Some(self.texture.crop(
            frame.frame.x,
            frame.frame.y,
            frame.frame.width,
            frame.frame.height,
        ))
    }
}

/// Real Codename characters, stage and note skin for one song.
#[derive(Debug, Default)]
pub struct CodenameContent {
    /// Characters by role.
    pub characters: BTreeMap<Role, CharacterRig>,
    /// Stage layers in draw order.
    pub stage_layers: Vec<StageLayer>,
    /// The installed note skin, when one could be read.
    pub note_skin: Option<NoteSkin>,
    /// Stage id that was resolved, when one was.
    pub stage_id: Option<String>,
    /// Everything that could not be loaded, as readable diagnostics.
    pub warnings: Vec<String>,
    /// Codename assets directory.
    pub assets: PathBuf,
    /// Mod root checked before the base assets.
    pub mod_root: Option<PathBuf>,
    /// Set once a character anchor was seen; layers pushed afterwards draw
    /// in front of the characters (document order, as Codename does).
    char_anchor_seen: bool,
}

impl CodenameContent {
    /// True when there is nothing real to draw and the generated pack should be
    /// used unchanged.
    pub fn is_empty(&self) -> bool {
        self.characters.is_empty() && self.stage_layers.is_empty() && self.note_skin.is_none()
    }

    /// Loads the stage and characters one song uses.
    /// 
    /// `assets` is a Codename installation's `assets` directory; `mod_root` is
    /// the selected mod, checked first so modded characters win over base ones.
    /// `song_meta` is the song's `meta.json`, whose `stage`, `player1`,
    /// `player2` and `gfVersion` fields select the content.
    pub fn load(assets: &Path, mod_root: Option<&Path>, song_meta: Option<&Path>) -> Self {
        let mut content = Self {
            assets: assets.to_path_buf(),
            mod_root: mod_root.map(|p| p.to_path_buf()),
            ..Self::default()
        };
        match load_note_skin(assets, mod_root) {
            Ok(skin) => content.note_skin = Some(skin),
            Err(error) => content.warnings.push(format!("note skin: {error}")),
        }
        let meta = song_meta.and_then(read_json);
        let stage = meta
            .as_ref()
            .and_then(|meta| string_field(meta, &["stage"]))
            .unwrap_or_else(|| "stage".to_string());
        let player = meta
            .as_ref()
            .and_then(|meta| string_field(meta, &["player1", "player"]))
            .unwrap_or_else(|| "bf".to_string());
        let opponent = meta
            .as_ref()
            .and_then(|meta| string_field(meta, &["player2", "opponent"]))
            .unwrap_or_else(|| "dad".to_string());
        let girlfriend = meta
            .as_ref()
            .and_then(|meta| string_field(meta, &["gfVersion", "girlfriend"]))
            .unwrap_or_else(|| "gf".to_string());

        let mut anchors = BTreeMap::new();
        content.load_stage(assets, mod_root, &stage, &mut anchors);

        for (role, id) in [
            (Role::Player, player),
            (Role::Opponent, opponent),
            (Role::Girlfriend, girlfriend),
        ] {
            let anchor = anchors.get(&role).copied();
            match load_character(assets, mod_root, role, &id, anchor) {
                Ok(rig) => {
                    content.characters.insert(role, rig);
                }
                Err(error) => content
                    .warnings
                    .push(format!("character '{id}' ({role:?}): {error}")),
            }
        }
        content
    }

    fn load_stage(
        &mut self,
        assets: &Path,
        mod_root: Option<&Path>,
        stage: &str,
        anchors: &mut BTreeMap<Role, (f32, f32)>,
    ) {
        let Some(path) = find_file(mod_root, assets, &format!("data/stages/{stage}.xml")) else {
            self.warnings
                .push(format!("stage '{stage}' has no definition in data/stages"));
            return;
        };
        let Ok(xml) = std::fs::read_to_string(&path) else {
            self.warnings
                .push(format!("stage '{stage}' could not be read at {path:?}"));
            return;
        };
        let Some(stage_tag) = first_tag(&xml, "stage") else {
            self.warnings
                .push(format!("stage '{stage}' has no <stage> element"));
            return;
        };
        self.char_anchor_seen = false;
        let stage_attributes = attributes(&stage_tag);
        let folder = stage_attributes
            .get("folder")
            .map(String::as_str)
            .unwrap_or("stages/stage/");
        let stage_scale = number(&stage_attributes, "zoom", 1.0);

        for element in elements(&xml) {
            let tag = element_name(&element);
            match tag.as_str() {
                "sprite" => {
                    let sprite_attributes = attributes(&element);
                    let Some(sprite) = sprite_attributes.get("sprite") else {
                        self.warnings
                            .push(format!("stage '{stage}': <sprite> without a sprite name"));
                        continue;
                    };
                    let relative = format!("images/{folder}{sprite}.png");
                    let Some(image) = find_file(mod_root, assets, &relative) else {
                        self.warnings
                            .push(format!("stage '{stage}': missing image {relative}"));
                        continue;
                    };
                    let Ok(bytes) = std::fs::read(&image) else {
                        self.warnings
                            .push(format!("stage '{stage}': unreadable image {relative}"));
                        continue;
                    };
                    let texture = match TextureAsset::decode_png(&bytes) {
                        Ok(texture) => texture,
                        Err(error) => {
                            self.warnings.push(format!(
                                "stage '{stage}': could not decode {relative}: {error}"
                            ));
                            continue;
                        }
                    };
                    let scroll = number(&sprite_attributes, "scroll", 1.0);
                    self.stage_layers.push(StageLayer {
                        name: sprite_attributes
                            .get("name")
                            .cloned()
                            .unwrap_or_else(|| sprite.clone()),
                        texture,
                        position: (
                            number(&sprite_attributes, "x", 0.0),
                            number(&sprite_attributes, "y", 0.0),
                        ),
                        scale: number(&sprite_attributes, "scale", 1.0) * stage_scale,
                        zoom: stage_scale,
                        scroll: (
                            number(&sprite_attributes, "scrollx", scroll),
                            number(&sprite_attributes, "scrolly", scroll),
                        ),
                        alpha: number(&sprite_attributes, "alpha", 1.0).clamp(0.0, 1.0),
                        front: self.char_anchor_seen,
                    });
                }
                "dad" | "opponent" | "boyfriend" | "bf" | "player" | "girlfriend" | "gf" => {
                    let role = match tag.as_str() {
                        "boyfriend" | "bf" | "player" => Role::Player,
                        "girlfriend" | "gf" => Role::Girlfriend,
                        _ => Role::Opponent,
                    };
                    let stage_attributes = attributes(&element);
                    let (default_x, default_y) = role.default_position();
                    anchors.insert(
                        role,
                        (
                            number(&stage_attributes, "x", default_x),
                            number(&stage_attributes, "y", default_y),
                        ),
                    );
                    // Codename draws the stage in document order, so every
                    // layer declared after a character anchor belongs in
                    // front of the characters.
                    self.char_anchor_seen = true;
                }
                _ => {}
            }
        }
    }
}

impl CodenameContent {
    /// Replaces the generated gameplay textures with the real Codename art.
    /// 
    /// Keys are the same logical names the playfield already uses, so every
    /// consumer switches to the real sprites without knowing where they came
    /// from. Anything missing is left as the generated fallback and reported.
    pub fn apply_skin(&self, resources: &mut GameResources) {
        let mut missing = Vec::new();
        if let Some(skin) = &self.note_skin {
            let frames: [(&str, &str); 24] = [
                (keys::NOTE_LEFT, "purple0000"),
                (keys::NOTE_DOWN, "blue0000"),
                (keys::NOTE_UP, "green0000"),
                (keys::NOTE_RIGHT, "red0000"),
                (keys::RECEPTOR_LEFT, "arrowLEFT0000"),
                (keys::RECEPTOR_DOWN, "arrowDOWN0000"),
                (keys::RECEPTOR_UP, "arrowUP0000"),
                (keys::RECEPTOR_RIGHT, "arrowRIGHT0000"),
                (keys::RECEPTOR_PRESS_LEFT, "left press0002"),
                (keys::RECEPTOR_PRESS_DOWN, "down press0002"),
                (keys::RECEPTOR_PRESS_UP, "up press0002"),
                (keys::RECEPTOR_PRESS_RIGHT, "right press0002"),
                (keys::HOLD_LEFT, "purple hold piece0000"),
                (keys::HOLD_DOWN, "blue hold piece0000"),
                (keys::HOLD_UP, "green hold piece0000"),
                (keys::HOLD_RIGHT, "red hold piece0000"),
                (keys::HOLD_END_LEFT, "pruple end hold0000"),
                (keys::HOLD_END_DOWN, "blue hold end0000"),
                (keys::HOLD_END_UP, "green hold end0000"),
                (keys::HOLD_END_RIGHT, "red hold end0000"),
                (keys::JUDGEMENT_PERFECT, "score/sick.png"),
                (keys::JUDGEMENT_GOOD, "score/good.png"),
                (keys::JUDGEMENT_BAD, "score/bad.png"),
                (keys::JUDGEMENT_SHIT, "score/shit.png"),
            ];
            for (key, frame) in frames {
                let texture = if frame.ends_with(".png") {
                    self.image(&format!("images/game/{frame}"))
                } else {
                    skin.cropped(frame)
                };
                match texture {
                    Some(texture) => resources.install_texture(key, texture),
                    None => missing.push(format!("{key} <- {frame}")),
                }
            }
        } else {
            missing.push("note skin (images/game/notes/default.xml)".to_string());
        }

        // Health-bar icons named by the characters that loaded.
        for (role, key) in [(Role::Player, keys::ICON_PLAYER), (Role::Opponent, keys::ICON_OPPONENT)] {
            let icon = self
                .characters
                .get(&role)
                .and_then(|rig| rig.icon.as_deref());
            if let Some(texture) = icon.and_then(|icon| self.icon_image(icon)) {
                resources.install_texture(key, texture);
            }
        }

        // Countdown art, when the installation ships it.
        for (key, path) in [
            (keys::COUNTDOWN_3, "images/game/ready.png"),
            (keys::COUNTDOWN_2, "images/game/set.png"),
            (keys::COUNTDOWN_1, "images/game/go.png"),
        ] {
            if let Some(texture) = self.image(path) {
                resources.install_texture(key, texture);
            }
        }

        for key in missing {
            println!("codename: missing {key}");
        }
    }

    /// Reads an image from the mod first, then the base assets.
    fn image(&self, relative: &str) -> Option<TextureAsset> {
        let path = find_file(self.mod_root.as_deref(), &self.assets, relative)?;
        read_texture(&path)
    }

    /// Reads a health-icon sheet.
    ///
    /// Codename's base installation keeps each icon in its own folder
    /// (`images/icons/bf/icon.png`), while mods such as Vs Var Overhaul ship
    /// flat files (`images/icons/neu.png`). Both layouts are accepted, and the
    /// mod's files win over the base installation's whichever layout they use.
    fn icon_image(&self, icon: &str) -> Option<TextureAsset> {
        let candidates = [
            format!("images/icons/{icon}/icon.png"),
            format!("images/icons/{icon}.png"),
        ];
        let roots = self
            .mod_root
            .as_deref()
            .into_iter()
            .chain(std::iter::once(self.assets.as_path()));
        for root in roots {
            for relative in &candidates {
                if let Some(texture) = in_root(root, relative).and_then(|path| read_texture(&path))
                {
                    return Some(texture);
                }
            }
        }
        None
    }
}

fn load_note_skin(assets: &Path, mod_root: Option<&Path>) -> Result<NoteSkin, String> {
    let image = find_file(mod_root, assets, "images/game/notes/default.png")
        .ok_or("missing images/game/notes/default.png")?;
    let atlas_path = find_file(mod_root, assets, "images/game/notes/default.xml")
        .ok_or("missing images/game/notes/default.xml")?;
    let bytes = std::fs::read(&image).map_err(|error| error.to_string())?;
    let texture = TextureAsset::decode_png(&bytes)
        .map_err(|error| format!("{}: {error}", image.display()))?;
    let atlas_xml = std::fs::read_to_string(&atlas_path).map_err(|error| error.to_string())?;
    let atlas = SparrowAtlas::parse_xml(&atlas_xml)
        .map_err(|error| format!("{}: {error}", atlas_path.display()))?;
    Ok(NoteSkin {
        texture,
        atlas,
        source: atlas_path,
    })
}

fn load_character(
    assets: &Path,
    mod_root: Option<&Path>,
    role: Role,
    id: &str,
    anchor: Option<(f32, f32)>,
) -> Result<CharacterRig, String> {
    let relative = format!("data/characters/{id}.xml");
    let path = find_file(mod_root, assets, &relative)
        .ok_or_else(|| format!("no definition at {relative}"))?;
    let xml = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let tag = first_tag(&xml, "character").ok_or("no <character> element")?;
    let character_attributes = attributes(&tag);
    let sprite = character_attributes
        .get("sprite")
        .ok_or("<character> has no sprite attribute")?
        .clone();

    // Codename looks character sheets up under images/characters; older
    // layouts keep them directly under images, so both are accepted.
    let candidates = [
        format!("images/characters/{sprite}"),
        format!("images/{sprite}"),
    ];
    let (image, atlas_path) = candidates
        .iter()
        .find_map(|base| {
            Some((
                find_file(mod_root, assets, &format!("{base}.png"))?,
                find_file(mod_root, assets, &format!("{base}.xml"))?,
            ))
        })
        .ok_or_else(|| {
            format!(
                "missing Sparrow sheet for sprite '{sprite}' (looked for images/characters/{sprite}.png and its .xml; Animate atlases are not supported)"
            )
        })?;
    let bytes = std::fs::read(&image).map_err(|error| error.to_string())?;
    let texture = TextureAsset::decode_png(&bytes)
        .map_err(|error| format!("{}: {error}", image.display()))?;
    let atlas_xml = std::fs::read_to_string(&atlas_path).map_err(|error| error.to_string())?;
    let atlas = SparrowAtlas::parse_xml(&atlas_xml)
        .map_err(|error| format!("{}: {error}", atlas_path.display()))?;

    let mut animations = BTreeMap::new();
    for element in elements(&xml) {
        if element_name(&element) != "anim" {
            continue;
        }
        let anim_attributes = attributes(&element);
        let Some(name) = anim_attributes.get("name") else {
            continue;
        };
        let prefix = anim_attributes
            .get("anim")
            .cloned()
            .unwrap_or_else(|| name.clone());
        let frames: Vec<AtlasFrame> = atlas
            .frames_for_prefix(&prefix)
            .into_iter()
            .cloned()
            .collect();
        if frames.is_empty() {
            continue;
        }
        animations.insert(
            name.clone(),
            RigAnimation {
                frames,
                fps: number(&anim_attributes, "fps", 24.0),
                offset: (
                    number(&anim_attributes, "x", 0.0),
                    number(&anim_attributes, "y", 0.0),
                ),
                looping: anim_attributes
                    .get("loop")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true")),
            },
        );
    }

    let (anchor_x, anchor_y) = anchor.unwrap_or_else(|| role.default_position());
    Ok(CharacterRig {
        role,
        name: id.to_string(),
        texture,
        animations,
        position: (
            anchor_x + number(&character_attributes, "x", 0.0),
            anchor_y + number(&character_attributes, "y", 0.0),
        ),
        scale: number(&character_attributes, "scale", 1.0),
        icon: character_attributes.get("icon").cloned(),
        flip_x: character_attributes
            .get("flipx")
            .map(|value| value.eq_ignore_ascii_case("true"))
            .unwrap_or_else(|| role.default_flip()),
        scroll: Role::default_scroll(role),
    })
}

/// Song metadata path used by Codename: the mod's copy first, then the base
/// installation's.
pub fn song_meta_path(assets: &Path, mod_root: Option<&Path>, song_id: &str) -> Option<PathBuf> {
    let relative = format!("songs/{song_id}/meta.json");
    find_file(mod_root, assets, &relative)
        .or_else(|| find_file(None, assets, &format!("data/{relative}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fu_import::test_support::TempDir;

    fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        TextureAsset::solid(width, height, rgba)
            .encode_png()
            .expect("png")
    }

    const DAD_XML: &str = r#"<!DOCTYPE codename-engine-character>
<character x="10" y="20" sprite="dadSheet" icon="neu">
	<anim name="idle" anim="idle" fps="12" loop="true"/>
	<anim name="singLEFT" anim="left" x="5" y="-3" fps="24"/>
</character>"#;

    const SHEET_XML: &str = r#"<TextureAtlas imagePath="dadSheet.png">
	<SubTexture name="idle0000" x="0" y="0" width="8" height="8"/>
	<SubTexture name="idle0001" x="8" y="0" width="8" height="8"/>
	<SubTexture name="left0000" x="0" y="8" width="8" height="8" frameX="-1" frameY="-2" frameWidth="10" frameHeight="12"/>
</TextureAtlas>"#;

    #[test]
    fn relative_paths_cannot_leave_their_root() {
        assert!(safe_relative("images/a.png"));
        assert!(safe_relative("images/..hidden/a.png"));
        assert!(!safe_relative(""));
        assert!(!safe_relative("../a.png"));
        assert!(!safe_relative("images/../../a.png"));
        assert!(!safe_relative("/etc/passwd"));
        assert!(!safe_relative("C:/Windows/win.ini"));
        assert!(!safe_relative("images\\a.png"));
    }

    #[test]
    fn lookups_prefer_the_mod_and_refuse_traversal() {
        let temp = TempDir::new("codename-find-file").unwrap();
        temp.write("assets/images/a.txt", "base").unwrap();
        temp.write("mod/images/a.txt", "mod").unwrap();
        temp.write("secret.txt", "secret").unwrap();
        let assets = temp.join("assets");
        let mod_root = temp.join("mod");

        let found = find_file(Some(mod_root.as_path()), assets.as_path(), "images/a.txt")
            .expect("file exists in both layers");
        assert!(found.starts_with(&mod_root), "the mod layer wins: {found:?}");

        let base_only = find_file(None, assets.as_path(), "images/a.txt").expect("base file");
        assert!(base_only.starts_with(&assets));

        assert!(find_file(Some(mod_root.as_path()), assets.as_path(), "../secret.txt").is_none());
        assert!(find_file(None, assets.as_path(), "images/../../secret.txt").is_none());
    }

    #[test]
    fn a_mod_character_keeps_its_animation_and_frame_metadata() {
        let temp = TempDir::new("codename-character").unwrap();
        temp.write("mod/data/characters/dad.xml", DAD_XML).unwrap();
        temp.write_bytes("mod/images/characters/dadSheet.png", &png(16, 16, [9, 9, 9, 255]))
            .unwrap();
        temp.write("mod/images/characters/dadSheet.xml", SHEET_XML)
            .unwrap();
        let assets = temp.join("assets");
        let mod_root = temp.join("mod");

        let content = CodenameContent::load(assets.as_path(), Some(mod_root.as_path()), None);

        let dad = content
            .characters
            .get(&Role::Opponent)
            .expect("the opponent loads from the mod");
        assert_eq!(dad.animations.len(), 2);
        assert_eq!(dad.position, (110.0, 120.0), "stage default plus the XML offset");
        assert_eq!(dad.icon.as_deref(), Some("neu"));

        let idle = dad.animation("idle").expect("idle");
        assert_eq!(idle.frames.len(), 2);
        assert!(idle.looping);
        assert_eq!(idle.fps, 12.0);

        let sing = dad.animation("singLEFT").expect("sing pose");
        assert!(!sing.looping, "looping defaults to off");
        assert_eq!(sing.fps, 24.0);
        assert_eq!(sing.offset, (5.0, -3.0));
        let frame = &sing.frames[0];
        assert_eq!(frame.offset, (-1, -2), "trim offset is preserved");
        assert_eq!(frame.source_size, Some((10, 12)), "source size is preserved");

        assert!(
            content
                .warnings
                .iter()
                .any(|warning| warning.contains("character 'bf'")),
            "a missing character is reported, not hidden: {:?}",
            content.warnings
        );
    }

    #[test]
    fn mod_health_icons_load_from_the_flat_layout() {
        let temp = TempDir::new("codename-icons").unwrap();
        temp.write("mod/data/characters/dad.xml", DAD_XML).unwrap();
        temp.write_bytes("mod/images/characters/dadSheet.png", &png(16, 16, [9, 9, 9, 255]))
            .unwrap();
        temp.write("mod/images/characters/dadSheet.xml", SHEET_XML)
            .unwrap();
        // Vs Var Overhaul keeps icons as images/icons/<name>.png.
        temp.write_bytes("mod/images/icons/neu.png", &png(7, 5, [200, 10, 10, 255]))
            .unwrap();
        let assets = temp.join("assets");
        let mod_root = temp.join("mod");

        let content = CodenameContent::load(assets.as_path(), Some(mod_root.as_path()), None);
        let mut resources = GameResources::defaults().expect("resources");
        content.apply_skin(&mut resources);

        let icon = resources.texture(keys::ICON_OPPONENT);
        assert_eq!((icon.width, icon.height), (7, 5), "the mod's own icon is used");
        assert_eq!(icon.pixel(0, 0), Some([200, 10, 10, 255]));
    }

    #[test]
    fn stage_sprites_cannot_name_files_outside_the_stage_folder() {
        let temp = TempDir::new("codename-stage").unwrap();
        temp.write(
            "mod/data/stages/stage.xml",
            r#"<!DOCTYPE codename-engine-stage>
<stage name="stage" folder="stages/stage/" zoom="1">
	<sprite name="ok" sprite="ok" x="0" y="0"/>
	<sprite name="evil" sprite="../../../secret" x="0" y="0"/>
</stage>"#,
        )
        .unwrap();
        temp.write_bytes("mod/images/stages/stage/ok.png", &png(4, 4, [1, 2, 3, 255]))
            .unwrap();
        // This file exists, and would be reached by the traversal above.
        temp.write_bytes("mod/secret.png", &png(4, 4, [255, 0, 0, 255]))
            .unwrap();
        let assets = temp.join("assets");
        let mod_root = temp.join("mod");

        let content = CodenameContent::load(assets.as_path(), Some(mod_root.as_path()), None);

        assert_eq!(content.stage_layers.len(), 1, "only the honest sprite loads");
        assert_eq!(content.stage_layers[0].name, "ok");
        assert!(
            content
                .warnings
                .iter()
                .any(|warning| warning.contains("secret")),
            "the refused sprite is reported: {:?}",
            content.warnings
        );
    }
}
