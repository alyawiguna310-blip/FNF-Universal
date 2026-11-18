//! Optional real Codename Engine character and stage assets for the playfield.
//!
//! The playable runtime ships a generated default pack, so it runs with no
//! Codename installation. When one is available (`--codename <assets dir>`), this
//! module reads the stage and character XML plus their Sparrow atlases so the
//! playfield shows the game's own characters and backdrop instead of the
//! generated placeholders.
//!
//! Content is read through `fu-vfs`. [`mount_codename`] mounts the user's
//! Codename assets directory and the selected mod as read-only layers of one
//! virtual file system (the mod wins, the install is the fallback), and every
//! character, stage, note skin and icon lookup goes through it as a virtual path
//! such as `/images/game/notes/default.png`. Nothing is copied out of the
//! installation and nothing in it is ever written.
//!
//! Only what the playfield draws is read: stage sprite layers, character sprite
//! sheets and their named animations. Scripts, Animate atlases and stage scripts
//! are reported as warnings rather than silently ignored.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use fu_assets::{AtlasFrame, SparrowAtlas, TextureAsset};
use fu_vfs::{CaseSensitivity, MountPolicy, Vfs, VfsError};

use crate::resources::{keys, GameResources};

/// Mount id of the user's Codename assets directory.
pub const CODENAME_ASSETS_MOUNT: &str = "codename-assets";
/// Mount id of the selected mod.
pub const CODENAME_MOD_MOUNT: &str = "codename-mod";
/// Priority of the Codename installation, the fallback layer.
pub const CODENAME_ASSETS_PRIORITY: i32 = 10;
/// Priority of the mod, which overrides the installation.
pub const CODENAME_MOD_PRIORITY: i32 = 20;

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
/// before it reaches the file system or the virtual file system.
fn safe_relative(relative: &str) -> bool {
    !relative.is_empty()
        && !relative.starts_with('/')
        && !relative.contains('\\')
        && !relative.contains(':')
        && !relative.contains('\0')
        && relative.split('/').all(|part| part != "..")
}

/// `relative` inside one root, when it is a safe path to an existing file.
///
/// Only [`song_meta_path`] still resolves physical paths: it hands a host path
/// to callers, which is not something the virtual file system returns.
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

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Mounts a Codename assets directory and, optionally, the selected mod as
/// read-only layers of a new virtual file system, both rooted at `/`.
///
/// The mod (priority [`CODENAME_MOD_PRIORITY`]) overrides the installation
/// (priority [`CODENAME_ASSETS_PRIORITY`]), and a file missing from the mod falls
/// back to the installation. Both layers are read-only and match names
/// case-insensitively, as Codename does on Windows.
///
/// A directory that cannot be mounted (missing, or not a directory) is reported
/// in the returned warnings and that layer is left out; nothing else on disk is
/// consulted in its place. An assets directory that mounts but holds neither
/// `data/` nor `images/` (typically the installation root instead of its
/// `assets` folder) is reported too, because every lookup would otherwise fail
/// with nothing pointing at the cause.
pub fn mount_codename(assets: &Path, mod_root: Option<&Path>) -> (Vfs, Vec<String>) {
    let mut vfs = Vfs::new();
    let mut warnings = Vec::new();
    match vfs.mount_physical(
        CODENAME_ASSETS_MOUNT,
        "/",
        assets,
        CODENAME_ASSETS_PRIORITY,
        MountPolicy::ReadOnly,
        CaseSensitivity::Insensitive,
    ) {
        Ok(_) => {
            if !vfs.exists("/data") && !vfs.exists("/images") {
                warnings.push(format!(
                    "codename assets directory {} has no data/ or images/ folder; pass the installation's assets directory",
                    assets.display()
                ));
            }
        }
        Err(error) => warnings.push(format!(
            "codename assets directory {}: {error}",
            assets.display()
        )),
    }
    if let Some(mod_root) = mod_root {
        if let Err(error) = vfs.mount_physical(
            CODENAME_MOD_MOUNT,
            "/",
            mod_root,
            CODENAME_MOD_PRIORITY,
            MountPolicy::ReadOnly,
            CaseSensitivity::Insensitive,
        ) {
            warnings.push(format!("mod directory {}: {error}", mod_root.display()));
        }
    }
    (vfs, warnings)
}

/// True when `relative` names an existing entry in `vfs`.
///
/// This is `false` both for an absent entry and for one the virtual file system
/// refuses (a symlink leaving the mount, for example); use [`refusal_detail`] to
/// tell them apart in a diagnostic.
fn vfs_exists(vfs: &Vfs, relative: &str) -> bool {
    safe_relative(relative) && vfs.exists(format!("/{relative}"))
}

/// Why the virtual file system refused `relative`, as a parenthesised suffix for
/// a "missing" diagnostic, or an empty string when the entry is simply absent.
fn refusal_detail(vfs: &Vfs, relative: &str) -> String {
    match vfs.metadata(format!("/{relative}")) {
        Err(VfsError::NotFound(_)) | Ok(_) => String::new(),
        Err(error) => format!(" ({error})"),
    }
}

/// Reads `relative` (such as `images/game/ready.png`) from `vfs`.
fn read_bytes(vfs: &Vfs, relative: &str) -> Result<Vec<u8>, String> {
    if !safe_relative(relative) {
        return Err(format!("'{relative}' is not a safe relative path"));
    }
    vfs.read(format!("/{relative}"))
        .map_err(|error| format!("{relative}: {error}"))
}

fn read_text(vfs: &Vfs, relative: &str) -> Result<String, String> {
    let bytes = read_bytes(vfs, relative)?;
    String::from_utf8(bytes).map_err(|error| format!("{relative} is not valid UTF-8: {error}"))
}

fn read_texture(vfs: &Vfs, relative: &str) -> Result<TextureAsset, String> {
    let bytes = read_bytes(vfs, relative)?;
    TextureAsset::decode_png(&bytes).map_err(|error| format!("{relative}: {error}"))
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
    /// Virtual path the atlas was read from (for example
    /// `/images/game/notes/default.xml`), for diagnostics.
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

/// The virtual file system a [`CodenameContent`] was loaded from.
///
/// It is kept so images requested after loading (health icons, countdown art,
/// judgements) resolve through the same layers. `Vfs` is `Send + Sync` (every
/// mount holds an `Arc<dyn VfsBackend>` and the trait requires both), so the
/// `Arc` is a plain shared handle.
#[derive(Clone, Default)]
struct ContentVfs(Option<Arc<Vfs>>);

impl ContentVfs {
    fn get(&self) -> Option<&Vfs> {
        self.0.as_deref()
    }
}

impl fmt::Debug for ContentVfs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(vfs) => write!(formatter, "ContentVfs({} mount(s))", vfs.list_mounts().len()),
            None => formatter.write_str("ContentVfs(unmounted)"),
        }
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
    /// Codename assets directory that was mounted, when loaded from host paths.
    pub assets: PathBuf,
    /// Mod root that was mounted over the base assets, when there is one.
    pub mod_root: Option<PathBuf>,
    /// The layered virtual file system everything was read through.
    vfs: ContentVfs,
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

    /// The virtual file system this content was read through, when one was
    /// mounted.
    pub fn vfs(&self) -> Option<&Vfs> {
        self.vfs.get()
    }

    /// Loads the stage and characters one song uses.
    ///
    /// `assets` is a Codename installation's `assets` directory; `mod_root` is
    /// the selected mod, mounted over it so modded characters win over base
    /// ones. Both are mounted read-only through [`mount_codename`]; a directory
    /// that cannot be mounted is reported in [`CodenameContent::warnings`].
    /// `song_meta` is the song's `meta.json`, whose `stage`, `player1`,
    /// `player2` and `gfVersion` fields select the content.
    pub fn load(assets: &Path, mod_root: Option<&Path>, song_meta: Option<&Path>) -> Self {
        let (vfs, mount_warnings) = mount_codename(assets, mod_root);
        let meta = song_meta.and_then(read_json);
        let mut content = Self::load_from_vfs(Arc::new(vfs), meta.as_ref());
        content.assets = assets.to_path_buf();
        content.mod_root = mod_root.map(|p| p.to_path_buf());

        let mut warnings = mount_warnings;
        warnings.append(&mut content.warnings);
        content.warnings = warnings;
        content
    }

    /// Loads the stage and characters one song uses from an already mounted
    /// virtual file system whose root holds Codename's `data/` and `images/`.
    ///
    /// `meta` is the song's parsed `meta.json`, when there is one.
    pub fn load_from_vfs(vfs: Arc<Vfs>, meta: Option<&serde_json::Value>) -> Self {
        let mut content = Self {
            vfs: ContentVfs(Some(Arc::clone(&vfs))),
            ..Self::default()
        };
        match load_note_skin(&vfs) {
            Ok(skin) => content.note_skin = Some(skin),
            Err(error) => content.warnings.push(format!("note skin: {error}")),
        }
        let stage = meta
            .and_then(|meta| string_field(meta, &["stage"]))
            .unwrap_or_else(|| "stage".to_string());
        let player = meta
            .and_then(|meta| string_field(meta, &["player1", "player"]))
            .unwrap_or_else(|| "bf".to_string());
        let opponent = meta
            .and_then(|meta| string_field(meta, &["player2", "opponent"]))
            .unwrap_or_else(|| "dad".to_string());
        let girlfriend = meta
            .and_then(|meta| string_field(meta, &["gfVersion", "girlfriend"]))
            .unwrap_or_else(|| "gf".to_string());

        let mut anchors = BTreeMap::new();
        content.load_stage(&vfs, &stage, &mut anchors);

        for (role, id) in [
            (Role::Player, player),
            (Role::Opponent, opponent),
            (Role::Girlfriend, girlfriend),
        ] {
            let anchor = anchors.get(&role).copied();
            match load_character(&vfs, role, &id, anchor) {
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
        vfs: &Vfs,
        stage: &str,
        anchors: &mut BTreeMap<Role, (f32, f32)>,
    ) {
        let definition = format!("data/stages/{stage}.xml");
        if !vfs_exists(vfs, &definition) {
            self.warnings.push(format!(
                "stage '{stage}' has no definition in data/stages{}",
                refusal_detail(vfs, &definition)
            ));
            return;
        }
        let xml = match read_text(vfs, &definition) {
            Ok(xml) => xml,
            Err(error) => {
                self.warnings
                    .push(format!("stage '{stage}' could not be read: {error}"));
                return;
            }
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
                    if !vfs_exists(vfs, &relative) {
                        self.warnings.push(format!(
                            "stage '{stage}': missing image {relative}{}",
                            refusal_detail(vfs, &relative)
                        ));
                        continue;
                    }
                    let texture = match read_texture(vfs, &relative) {
                        Ok(texture) => texture,
                        Err(error) => {
                            self.warnings.push(format!(
                                "stage '{stage}': could not load image {relative}: {error}"
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

    /// Reads an image through the virtual file system: the mod layer first,
    /// then the base assets.
    fn image(&self, relative: &str) -> Option<TextureAsset> {
        read_texture(self.vfs.get()?, relative).ok()
    }

    /// Reads a health-icon sheet.
    ///
    /// Codename's base installation keeps each icon in its own folder
    /// (`images/icons/bf/icon.png`), while mods such as Vs Var Overhaul ship
    /// flat files (`images/icons/neu.png`). Both layouts are accepted. The layer
    /// with the higher mount priority (the mod) wins whichever layout it uses; at
    /// equal priority the folder layout is preferred.
    fn icon_image(&self, icon: &str) -> Option<TextureAsset> {
        let vfs = self.vfs.get()?;
        let candidates = [
            format!("images/icons/{icon}/icon.png"),
            format!("images/icons/{icon}.png"),
        ];
        let mut best: Option<(i32, TextureAsset)> = None;
        for relative in &candidates {
            if !safe_relative(relative) {
                continue;
            }
            let Ok(output) = vfs.read_with_provenance(format!("/{relative}")) else {
                continue;
            };
            let Ok(texture) = TextureAsset::decode_png(&output.bytes) else {
                continue;
            };
            let better = match &best {
                None => true,
                Some((priority, _)) => output.mount_priority > *priority,
            };
            if better {
                best = Some((output.mount_priority, texture));
            }
        }
        best.map(|(_, texture)| texture)
    }
}

fn load_note_skin(vfs: &Vfs) -> Result<NoteSkin, String> {
    let image = "images/game/notes/default.png";
    let atlas_path = "images/game/notes/default.xml";
    if !vfs_exists(vfs, image) {
        return Err(format!("missing {image}{}", refusal_detail(vfs, image)));
    }
    if !vfs_exists(vfs, atlas_path) {
        return Err(format!(
            "missing {atlas_path}{}",
            refusal_detail(vfs, atlas_path)
        ));
    }
    let texture = read_texture(vfs, image)?;
    let atlas_xml = read_text(vfs, atlas_path)?;
    let atlas =
        SparrowAtlas::parse_xml(&atlas_xml).map_err(|error| format!("{atlas_path}: {error}"))?;
    Ok(NoteSkin {
        texture,
        atlas,
        source: PathBuf::from(format!("/{atlas_path}")),
    })
}

/// Longest `a..b` range an `indices` attribute may expand to. Real characters
/// stay far below this (Codename's `dad` uses at most 60 frames), so the cap
/// only stops a hostile mod from expanding one attribute into millions of
/// entries.
const MAX_INDEX_RANGE: u32 = 10_000;

/// Parses a Codename `indices` attribute such as `2..12,0,1`.
///
/// Entries are comma separated. An entry is either one frame number or an
/// inclusive ascending range `a..b`. The base game's `dad` character shows the
/// range is inclusive: its idle animation lists `2..12,0,1` and its atlas has
/// frames `0000` to `0012`, and `56..59` ends on the last of `Dad Sing note UP`'s
/// sixty frames. Returns `None` when any entry is malformed, descending, or
/// longer than [`MAX_INDEX_RANGE`].
fn parse_indices(text: &str) -> Option<Vec<u32>> {
    let mut indices = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once("..") {
            Some((start, end)) => {
                let start: u32 = start.trim().parse().ok()?;
                let end: u32 = end.trim().parse().ok()?;
                if start > end || end - start > MAX_INDEX_RANGE {
                    return None;
                }
                indices.extend(start..=end);
            }
            None => indices.push(part.parse().ok()?),
        }
    }
    Some(indices)
}

/// The frames an animation with an `indices` attribute plays, in that order.
///
/// Like Flixel's `addByIndices`, each index selects the frame named
/// `<prefix><index padded to four digits>`, so repeated indices repeat a frame
/// and an index with no such frame is skipped. A malformed attribute yields no
/// frames, which makes the animation absent instead of playing the wrong frames.
fn frames_for_indices(atlas: &SparrowAtlas, prefix: &str, indices: &str) -> Vec<AtlasFrame> {
    let Some(indices) = parse_indices(indices) else {
        return Vec::new();
    };
    indices
        .iter()
        .filter_map(|index| atlas.frame(&format!("{prefix}{index:04}")))
        .cloned()
        .collect()
}

fn load_character(
    vfs: &Vfs,
    role: Role,
    id: &str,
    anchor: Option<(f32, f32)>,
) -> Result<CharacterRig, String> {
    let relative = format!("data/characters/{id}.xml");
    if !vfs_exists(vfs, &relative) {
        return Err(format!(
            "no definition at {relative}{}",
            refusal_detail(vfs, &relative)
        ));
    }
    let xml = read_text(vfs, &relative)?;
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
            let image = format!("{base}.png");
            let atlas = format!("{base}.xml");
            (vfs_exists(vfs, &image) && vfs_exists(vfs, &atlas)).then_some((image, atlas))
        })
        .ok_or_else(|| {
            format!(
                "missing Sparrow sheet for sprite '{sprite}' (looked for images/characters/{sprite}.png and its .xml; Animate atlases are not supported)"
            )
        })?;
    let texture = read_texture(vfs, &image)?;
    let atlas_xml = read_text(vfs, &atlas_path)?;
    let atlas =
        SparrowAtlas::parse_xml(&atlas_xml).map_err(|error| format!("{atlas_path}: {error}"))?;

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
        // `indices` picks and orders frames explicitly (the base game's idle
        // loops and held sing poses rely on it); without it, every frame with
        // the prefix plays in numeric order.
        let frames: Vec<AtlasFrame> = match anim_attributes.get("indices") {
            Some(indices) => frames_for_indices(&atlas, &prefix, indices),
            None => atlas
                .frames_for_prefix(&prefix)
                .into_iter()
                .cloned()
                .collect(),
        };
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
///
/// This returns a host path for callers that want to read the file themselves,
/// so it is the one lookup that still resolves against the host file system
/// instead of the virtual one. Unlike the virtual file system it matches names
/// case-sensitively on case-sensitive hosts.
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

    /// The attribute syntax and frame naming of the base game's `dad`
    /// (spaces and mixed case in prefixes, `indices` with ranges and repeats,
    /// `-loop` variants), on a tiny synthetic sheet so no game asset is copied.
    const INDEXED_XML: &str = r#"<!DOCTYPE codename-engine-character>
<character sprite="dadSheet" icon="dad" holdTime="6.1">
	<anim name="idle" anim="Dad idle dance" fps="24" loop="false" indices="2..3,0,1"/>
	<anim name="idle-loop" anim="Dad idle dance" fps="24" loop="true" indices="3,3"/>
	<anim name="singUP" anim="Dad Sing note UP" fps="24" loop="false"/>
	<anim name="broken" anim="Dad idle dance" indices="a..b"/>
	<anim name="absent" anim="Dad idle dance" indices="40..41"/>
</character>"#;

    const INDEXED_SHEET_XML: &str = r#"<TextureAtlas imagePath="dadSheet.png">
	<SubTexture name="Dad idle dance0000" x="0" y="0" width="4" height="4"/>
	<SubTexture name="Dad idle dance0001" x="4" y="0" width="4" height="4"/>
	<SubTexture name="Dad idle dance0002" x="8" y="0" width="4" height="4"/>
	<SubTexture name="Dad idle dance0003" x="12" y="0" width="4" height="4"/>
	<SubTexture name="Dad Sing note UP0001" x="4" y="4" width="4" height="4"/>
	<SubTexture name="Dad Sing note UP0000" x="0" y="4" width="4" height="4"/>
</TextureAtlas>"#;

    /// A synthetic note skin: one note frame and nothing else.
    const NOTE_SKIN_XML: &str = r#"<TextureAtlas imagePath="default.png">
	<SubTexture name="purple0000" x="2" y="1" width="8" height="6"/>
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
    fn the_mounts_are_read_only_and_leave_the_install_untouched() {
        let temp = TempDir::new("codename-readonly").unwrap();
        temp.write("assets/images/a.txt", "base").unwrap();
        let assets = temp.join("assets");

        let (vfs, warnings) = mount_codename(assets.as_path(), None);
        assert!(warnings.is_empty(), "a good directory mounts cleanly: {warnings:?}");
        assert_eq!(vfs.read("/images/a.txt").unwrap(), b"base");
        assert_eq!(
            vfs.read("/IMAGES/A.TXT").unwrap(),
            b"base",
            "names match case-insensitively, as Codename does on Windows"
        );

        assert!(
            vfs.write("/images/a.txt", b"changed").is_err(),
            "an existing file cannot be overwritten"
        );
        assert!(
            vfs.write("/images/new.txt", b"new").is_err(),
            "a new file cannot be created"
        );
        assert!(vfs.create_dir("/images/fresh").is_err());

        assert_eq!(
            std::fs::read_to_string(assets.join("images/a.txt")).unwrap(),
            "base",
            "the file on disk is unchanged"
        );
        assert!(!assets.join("images/new.txt").exists());
        assert!(!assets.join("images/fresh").exists());
    }

    #[test]
    fn missing_or_invalid_directories_are_reported_not_hidden() {
        let temp = TempDir::new("codename-bad-mounts").unwrap();
        temp.write("a-file.txt", "not a directory").unwrap();

        let missing = temp.join("does-not-exist");
        let (_vfs, warnings) = mount_codename(missing.as_path(), None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("codename assets directory"), "{warnings:?}");

        let file = temp.join("a-file.txt");
        let (_vfs, warnings) = mount_codename(file.as_path(), None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("codename assets directory"), "{warnings:?}");

        let (_vfs, warnings) = mount_codename(missing.as_path(), Some(missing.as_path()));
        assert_eq!(warnings.len(), 2, "both layers fail separately: {warnings:?}");
        assert!(warnings[1].contains("mod directory"), "{warnings:?}");

        // Loading from a missing installation finds nothing and says why; it
        // does not wander off to other files.
        let content = CodenameContent::load(missing.as_path(), None, None);
        assert!(content.is_empty());
        assert!(content.vfs().is_some());
        assert!(
            content
                .warnings
                .iter()
                .any(|warning| warning.contains("codename assets directory")),
            "{:?}",
            content.warnings
        );
        assert!(
            content.warnings.iter().any(|warning| warning.contains("note skin")),
            "{:?}",
            content.warnings
        );
    }

    #[test]
    fn a_directory_that_is_not_an_assets_folder_is_flagged() {
        let temp = TempDir::new("codename-wrong-dir").unwrap();
        // The installation root: it mounts, but holds no data/ or images/.
        temp.write("install/README.txt", "not the assets folder")
            .unwrap();
        temp.write("install/assets/images/a.txt", "inside").unwrap();
        let install = temp.join("install");

        let (_vfs, warnings) = mount_codename(install.as_path(), None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("no data/ or images/ folder"),
            "{warnings:?}"
        );

        let (_vfs, warnings) = mount_codename(install.join("assets").as_path(), None);
        assert!(warnings.is_empty(), "the real assets folder is accepted: {warnings:?}");
    }

    #[test]
    fn the_mod_layer_wins_and_the_install_is_the_fallback() {
        let temp = TempDir::new("codename-layers").unwrap();
        temp.write("assets/data/shared.xml", "base").unwrap();
        temp.write("assets/data/base-only.xml", "base only").unwrap();
        temp.write("mod/data/shared.xml", "mod").unwrap();
        let assets = temp.join("assets");
        let mod_root = temp.join("mod");

        let (vfs, warnings) = mount_codename(assets.as_path(), Some(mod_root.as_path()));
        assert!(warnings.is_empty(), "{warnings:?}");

        assert_eq!(read_text(&vfs, "data/shared.xml"), Ok("mod".to_string()));
        assert_eq!(
            read_text(&vfs, "data/base-only.xml"),
            Ok("base only".to_string())
        );
        assert!(vfs_exists(&vfs, "data/base-only.xml"));
        assert!(!vfs_exists(&vfs, "data/nowhere.xml"));
    }

    #[test]
    fn unsafe_and_missing_paths_are_errors_not_reads() {
        let temp = TempDir::new("codename-invalid-paths").unwrap();
        temp.write("assets/images/ok.txt", "ok").unwrap();
        temp.write("secret.txt", "secret").unwrap();
        let assets = temp.join("assets");
        let (vfs, _) = mount_codename(assets.as_path(), None);

        assert!(read_bytes(&vfs, "images/ok.txt").is_ok());
        for bad in [
            "../secret.txt",
            "images/../../secret.txt",
            "/images/ok.txt",
            "C:/secret.txt",
            "images\\ok.txt",
            "",
        ] {
            assert!(read_bytes(&vfs, bad).is_err(), "'{bad}' must be refused");
            assert!(!vfs_exists(&vfs, bad), "'{bad}' must not exist");
        }
        assert!(read_bytes(&vfs, "images/missing.txt").is_err());
        assert!(read_texture(&vfs, "images/ok.txt").is_err(), "not a PNG");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_leaving_the_mount_is_refused_and_explained() {
        let temp = TempDir::new("codename-symlink").unwrap();
        temp.write(
            "assets/data/stages/stage.xml",
            r#"<stage name="stage" folder="stages/s/"><sprite name="link" sprite="link" x="0" y="0"/></stage>"#,
        )
        .unwrap();
        temp.write("assets/images/stages/s/keep.txt", "keep").unwrap();
        temp.write_bytes("outside.png", &png(4, 4, [255, 0, 0, 255]))
            .unwrap();
        std::os::unix::fs::symlink(
            temp.join("outside.png"),
            temp.join("assets/images/stages/s/link.png"),
        )
        .unwrap();
        let assets = temp.join("assets");

        let content = CodenameContent::load(assets.as_path(), None, None);

        assert!(
            content.stage_layers.is_empty(),
            "an image outside the mount is never loaded"
        );
        assert!(
            content.warnings.iter().any(|warning| {
                warning.contains("missing image images/stages/s/link.png")
                    && warning.contains("escaped mount root")
            }),
            "the refusal says why instead of a bare 'missing': {:?}",
            content.warnings
        );
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
    fn a_character_in_the_install_loads_through_the_vfs_without_a_mod() {
        let temp = TempDir::new("codename-install-character").unwrap();
        temp.write("assets/data/characters/dad.xml", DAD_XML).unwrap();
        temp.write_bytes(
            "assets/images/characters/dadSheet.png",
            &png(16, 16, [9, 9, 9, 255]),
        )
        .unwrap();
        temp.write("assets/images/characters/dadSheet.xml", SHEET_XML)
            .unwrap();
        let assets = temp.join("assets");

        let content = CodenameContent::load(assets.as_path(), None, None);
        assert!(content.characters.contains_key(&Role::Opponent));
        assert!(content.mod_root.is_none());
        assert_eq!(content.assets, assets);
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

    /// A character that only names an icon, for exercising icon lookup.
    fn icon_only_rig(icon: &str) -> CharacterRig {
        CharacterRig {
            role: Role::Opponent,
            name: "dad".to_string(),
            texture: TextureAsset::solid(1, 1, [0, 0, 0, 255]),
            animations: BTreeMap::new(),
            position: (0.0, 0.0),
            scale: 1.0,
            flip_x: false,
            scroll: 1.0,
            icon: Some(icon.to_string()),
        }
    }

    #[test]
    fn a_mod_icon_beats_the_installs_folder_layout_icon() {
        let temp = TempDir::new("codename-icon-priority").unwrap();
        temp.write("mod/data/characters/dad.xml", DAD_XML).unwrap();
        temp.write_bytes("mod/images/characters/dadSheet.png", &png(16, 16, [9, 9, 9, 255]))
            .unwrap();
        temp.write("mod/images/characters/dadSheet.xml", SHEET_XML)
            .unwrap();
        temp.write_bytes("mod/images/icons/neu.png", &png(7, 5, [200, 10, 10, 255]))
            .unwrap();
        // The install has the same icon in the base layout, which is tried first.
        temp.write_bytes(
            "assets/images/icons/neu/icon.png",
            &png(3, 3, [10, 200, 10, 255]),
        )
        .unwrap();
        let assets = temp.join("assets");
        let mod_root = temp.join("mod");

        let content = CodenameContent::load(assets.as_path(), Some(mod_root.as_path()), None);
        let mut resources = GameResources::defaults().expect("resources");
        content.apply_skin(&mut resources);
        let icon = resources.texture(keys::ICON_OPPONENT);
        assert_eq!((icon.width, icon.height), (7, 5), "the higher-priority layer wins");
    }

    #[test]
    fn the_installs_folder_layout_icon_is_the_fallback() {
        let temp = TempDir::new("codename-icon-fallback").unwrap();
        temp.write_bytes(
            "assets/images/icons/neu/icon.png",
            &png(3, 3, [10, 200, 10, 255]),
        )
        .unwrap();
        let assets = temp.join("assets");

        let (vfs, warnings) = mount_codename(assets.as_path(), None);
        assert!(warnings.is_empty(), "{warnings:?}");
        let mut content = CodenameContent::load_from_vfs(Arc::new(vfs), None);
        content
            .characters
            .insert(Role::Opponent, icon_only_rig("neu"));

        let mut resources = GameResources::defaults().expect("resources");
        content.apply_skin(&mut resources);
        let icon = resources.texture(keys::ICON_OPPONENT);
        assert_eq!((icon.width, icon.height), (3, 3), "the install's icon is used");
    }

    #[test]
    fn the_note_skin_judgements_and_countdown_load_through_the_vfs() {
        let temp = TempDir::new("codename-note-skin").unwrap();
        temp.write("assets/images/game/notes/default.xml", NOTE_SKIN_XML)
            .unwrap();
        temp.write_bytes(
            "assets/images/game/notes/default.png",
            &png(16, 8, [30, 60, 90, 255]),
        )
        .unwrap();
        temp.write_bytes("assets/images/game/score/sick.png", &png(5, 3, [1, 2, 3, 255]))
            .unwrap();
        temp.write_bytes("assets/images/game/ready.png", &png(6, 2, [4, 5, 6, 255]))
            .unwrap();
        let assets = temp.join("assets");

        let content = CodenameContent::load(assets.as_path(), None, None);
        let skin = content.note_skin.as_ref().expect("the note skin loads");
        assert_eq!(
            skin.source,
            PathBuf::from("/images/game/notes/default.xml"),
            "the source is a virtual path"
        );
        assert_eq!(skin.atlas.frames().len(), 1);

        let mut resources = GameResources::defaults().expect("resources");
        content.apply_skin(&mut resources);

        let note = resources.texture(keys::NOTE_LEFT);
        assert_eq!((note.width, note.height), (8, 6), "the frame is cropped from the sheet");
        let judgement = resources.texture(keys::JUDGEMENT_PERFECT);
        assert_eq!((judgement.width, judgement.height), (5, 3));
        let countdown = resources.texture(keys::COUNTDOWN_3);
        assert_eq!((countdown.width, countdown.height), (6, 2));
    }

    #[test]
    fn a_missing_note_skin_is_reported() {
        let temp = TempDir::new("codename-no-note-skin").unwrap();
        temp.write("assets/images/game/notes/default.xml", NOTE_SKIN_XML)
            .unwrap();
        // The XML exists but its sheet does not.
        let assets = temp.join("assets");

        let content = CodenameContent::load(assets.as_path(), None, None);
        assert!(content.note_skin.is_none());
        assert!(
            content
                .warnings
                .iter()
                .any(|warning| warning.contains("note skin: missing images/game/notes/default.png")),
            "{:?}",
            content.warnings
        );
    }

    #[test]
    fn stages_load_their_layers_through_the_vfs() {
        let temp = TempDir::new("codename-stage-layers").unwrap();
        temp.write(
            "assets/data/stages/stage.xml",
            r#"<!DOCTYPE codename-engine-stage>
<stage zoom="0.9" name="stage" folder="stages/default/">
	<sprite name="bg" x="-600" y="-200" sprite="stageback" scroll="0.9" />
	<girlfriend />
	<dad />
	<boyfriend />
	<sprite name="curtains" x="-500" y="-300" sprite="stagecurtains" scroll="1.3" />
	<sprite name="gone" x="0" y="0" sprite="nothing" />
</stage>"#,
        )
        .unwrap();
        temp.write_bytes(
            "assets/images/stages/default/stageback.png",
            &png(4, 4, [1, 1, 1, 255]),
        )
        .unwrap();
        temp.write_bytes(
            "assets/images/stages/default/stagecurtains.png",
            &png(2, 2, [2, 2, 2, 255]),
        )
        .unwrap();
        let assets = temp.join("assets");

        let content = CodenameContent::load(assets.as_path(), None, None);
        let names: Vec<&str> = content
            .stage_layers
            .iter()
            .map(|layer| layer.name.as_str())
            .collect();
        assert_eq!(names, ["bg", "curtains"], "layers keep document order");
        assert!(!content.stage_layers[0].front, "declared before the characters");
        assert!(content.stage_layers[1].front, "declared after the characters");
        assert_eq!(content.stage_layers[0].scroll, (0.9, 0.9));
        assert_eq!(content.stage_layers[0].zoom, 0.9);
        assert!(
            content
                .warnings
                .iter()
                .any(|warning| warning.contains("missing image images/stages/default/nothing.png")),
            "{:?}",
            content.warnings
        );
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

    #[test]
    fn indices_are_comma_separated_numbers_and_inclusive_ranges() {
        assert_eq!(
            parse_indices("2..12,0,1"),
            Some(vec![2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 0, 1]),
            "the base game's dad idle"
        );
        assert_eq!(parse_indices("10..12,12"), Some(vec![10, 11, 12, 12]));
        assert_eq!(parse_indices(" 56..59 "), Some(vec![56, 57, 58, 59]));
        assert_eq!(parse_indices("3"), Some(vec![3]));
        assert_eq!(parse_indices("1,,2"), Some(vec![1, 2]), "empty entries are skipped");

        assert_eq!(parse_indices("5..2"), None, "descending ranges are rejected");
        assert_eq!(parse_indices("a..b"), None);
        assert_eq!(parse_indices("1,x"), None);
        assert_eq!(parse_indices("0..4294967295"), None, "huge ranges are rejected");
    }

    #[test]
    fn character_animations_follow_their_indices() {
        let temp = TempDir::new("codename-indices").unwrap();
        temp.write("mod/data/characters/dad.xml", INDEXED_XML)
            .unwrap();
        temp.write_bytes("mod/images/characters/dadSheet.png", &png(16, 16, [9, 9, 9, 255]))
            .unwrap();
        temp.write("mod/images/characters/dadSheet.xml", INDEXED_SHEET_XML)
            .unwrap();
        let assets = temp.join("assets");
        let mod_root = temp.join("mod");

        let content = CodenameContent::load(assets.as_path(), Some(mod_root.as_path()), None);
        let dad = content
            .characters
            .get(&Role::Opponent)
            .expect("the opponent loads");
        let names = |animation: &str| -> Vec<&str> {
            dad.animation(animation)
                .unwrap_or_else(|| panic!("animation '{animation}' exists"))
                .frames
                .iter()
                .map(|frame| frame.name.as_str())
                .collect()
        };

        assert_eq!(
            names("idle"),
            [
                "Dad idle dance0002",
                "Dad idle dance0003",
                "Dad idle dance0000",
                "Dad idle dance0001"
            ],
            "indices choose the frames and their order"
        );
        let idle_loop = dad.animation("idle-loop").expect("idle-loop");
        assert!(idle_loop.looping);
        assert_eq!(
            names("idle-loop"),
            ["Dad idle dance0003", "Dad idle dance0003"],
            "a repeated index repeats the frame"
        );
        assert_eq!(
            names("singUP"),
            ["Dad Sing note UP0000", "Dad Sing note UP0001"],
            "without indices every prefixed frame plays in numeric order"
        );
        assert!(
            dad.animation("broken").is_none(),
            "a malformed indices attribute drops the animation instead of playing wrong frames"
        );
        assert!(
            dad.animation("absent").is_none(),
            "indices naming no frame leave nothing to play"
        );
        assert_eq!(dad.animations.len(), 3);
    }
}
