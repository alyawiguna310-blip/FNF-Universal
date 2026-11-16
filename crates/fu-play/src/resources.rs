//! Loading the runtime's resources: VFS wiring, manifests, caching, fallback.
//!
//! Gameplay never mentions a file path. It asks this module for a logical key
//! (`note.left`, `stage.default`, `sfx.hit`) and gets pixels, sound bytes or a
//! parsed animation atlas.
//!
//! Layer order, lowest priority first:
//!
//! 1. The generated default pack, mounted read-only at priority 0.
//! 2. An optional user resource pack directory, priority 10 (`--assets <dir>`).
//! 3. The selected mod, priority 20.
//!
//! Because every layer is a VFS mount, a mod overrides a default by shipping the
//! same file, or by adding its own manifest that remaps a key. Nothing in this
//! module special-cases mods.

use std::collections::BTreeMap;
use std::path::Path;

use fu_assets::{
    install_default_pack, AssetCache, AtlasRect, ResourceManifest, ResourceStack, SparrowAtlas,
    TextureAsset, DEFAULT_MANIFEST_JSON, DEFAULT_MANIFEST_PATH,
};
use fu_vfs::{CaseSensitivity, MountPolicy, Vfs};

/// Priority of the generated default pack.
pub const DEFAULT_PRIORITY: i32 = 0;
/// Priority of an operator-provided resource pack.
pub const USER_PACK_PRIORITY: i32 = 10;
/// Priority of the selected mod.
pub const MOD_PRIORITY: i32 = 20;

/// Manifest a pack may ship to add or remap resources.
pub const PACK_MANIFEST_PATH: &str = "/assets/manifest.json";

/// Logical resource keys used by the runtime.
///
/// These are the only names gameplay and rendering code know about.
pub mod keys {
    /// Note texture for lane 0 (left).
    pub const NOTE_LEFT: &str = "note.left";
    /// Note texture for lane 1 (down).
    pub const NOTE_DOWN: &str = "note.down";
    /// Note texture for lane 2 (up).
    pub const NOTE_UP: &str = "note.up";
    /// Note texture for lane 3 (right).
    pub const NOTE_RIGHT: &str = "note.right";
    /// Receptor texture for lane 0.
    pub const RECEPTOR_LEFT: &str = "receptor.left";
    /// Receptor texture for lane 1.
    pub const RECEPTOR_DOWN: &str = "receptor.down";
    /// Receptor texture for lane 2.
    pub const RECEPTOR_UP: &str = "receptor.up";
    /// Receptor texture for lane 3.
    pub const RECEPTOR_RIGHT: &str = "receptor.right";
    /// Stage backdrop.
    pub const STAGE_DEFAULT: &str = "stage.default";
    /// Health bar frame.
    pub const HEALTH_FRAME: &str = "hud.health.frame";
    /// Health bar fill.
    pub const HEALTH_FILL: &str = "hud.health.fill";
    /// Player icon.
    pub const ICON_PLAYER: &str = "hud.icon.player";
    /// Opponent icon.
    pub const ICON_OPPONENT: &str = "hud.icon.opponent";
    /// Countdown "3".
    pub const COUNTDOWN_3: &str = "countdown.3";
    /// Countdown "2".
    pub const COUNTDOWN_2: &str = "countdown.2";
    /// Countdown "1".
    pub const COUNTDOWN_1: &str = "countdown.1";
    /// Countdown "GO".
    pub const COUNTDOWN_GO: &str = "countdown.go";
    /// `PERFECT` judgement graphic.
    pub const JUDGEMENT_PERFECT: &str = "judgement.perfect";
    /// `GOOD` judgement graphic.
    pub const JUDGEMENT_GOOD: &str = "judgement.good";
    /// `BAD` judgement graphic.
    pub const JUDGEMENT_BAD: &str = "judgement.bad";
    /// `MISS` judgement graphic.
    pub const JUDGEMENT_MISS: &str = "judgement.miss";
    /// Shit judgement graphic, when the asset pack provides one.
    pub const JUDGEMENT_SHIT: &str = "judgement.shit";
    /// Held-lane receptor frame, per lane.
    pub const RECEPTOR_PRESS_LEFT: &str = "receptor.press.left";
    /// Held-lane receptor frame for lane 1.
    pub const RECEPTOR_PRESS_DOWN: &str = "receptor.press.down";
    /// Held-lane receptor frame for lane 2.
    pub const RECEPTOR_PRESS_UP: &str = "receptor.press.up";
    /// Held-lane receptor frame for lane 3.
    pub const RECEPTOR_PRESS_RIGHT: &str = "receptor.press.right";
    /// Sustain body texture, per lane.
    pub const HOLD_LEFT: &str = "hold.left";
    /// Sustain body texture for lane 1.
    pub const HOLD_DOWN: &str = "hold.down";
    /// Sustain body texture for lane 2.
    pub const HOLD_UP: &str = "hold.up";
    /// Sustain body texture for lane 3.
    pub const HOLD_RIGHT: &str = "hold.right";
    /// Sustain end cap texture, per lane.
    pub const HOLD_END_LEFT: &str = "hold.end.left";
    /// Sustain end cap for lane 1.
    pub const HOLD_END_DOWN: &str = "hold.end.down";
    /// Sustain end cap for lane 2.
    pub const HOLD_END_UP: &str = "hold.end.up";
    /// Sustain end cap for lane 3.
    pub const HOLD_END_RIGHT: &str = "hold.end.right";

    /// Sustain bodies in lane order.
    pub const HOLD_FOR: [&str; 4] = [HOLD_LEFT, HOLD_DOWN, HOLD_UP, HOLD_RIGHT];
    /// Sustain end caps in lane order.
    pub const HOLD_END_FOR: [&str; 4] = [HOLD_END_LEFT, HOLD_END_DOWN, HOLD_END_UP, HOLD_END_RIGHT];
    /// Loading/menu backdrop.
    pub const MENU_BACKGROUND: &str = "ui.menu.background";
    /// Default character spritesheet image.
    pub const CHARACTER_IMAGE: &str = "character.default.image";
    /// Default character Sparrow atlas.
    pub const CHARACTER_ATLAS: &str = "character.default.atlas";
    /// Pause panel.
    pub const PAUSE_PANEL: &str = "ui.pause.panel";
    /// Results panel.
    pub const RESULTS_PANEL: &str = "ui.results.panel";
    /// Countdown blip.
    pub const SFX_COUNTDOWN: &str = "sfx.countdown";
    /// Note hit sound.
    pub const SFX_HIT: &str = "sfx.hit";
    /// Note miss sound.
    pub const SFX_MISS: &str = "sfx.miss";

    /// Every texture key the runtime loads eagerly.
    pub const TEXTURES: [&str; 23] = [
        NOTE_LEFT,
        NOTE_DOWN,
        NOTE_UP,
        NOTE_RIGHT,
        RECEPTOR_LEFT,
        RECEPTOR_DOWN,
        RECEPTOR_UP,
        RECEPTOR_RIGHT,
        STAGE_DEFAULT,
        HEALTH_FRAME,
        HEALTH_FILL,
        ICON_PLAYER,
        ICON_OPPONENT,
        COUNTDOWN_3,
        COUNTDOWN_2,
        COUNTDOWN_1,
        COUNTDOWN_GO,
        JUDGEMENT_PERFECT,
        JUDGEMENT_GOOD,
        JUDGEMENT_BAD,
        JUDGEMENT_MISS,
        MENU_BACKGROUND,
        CHARACTER_IMAGE,
    ];

    /// Panel textures, loaded eagerly as well.
    pub const PANELS: [&str; 2] = [PAUSE_PANEL, RESULTS_PANEL];

    /// Every sound key the runtime loads.
    pub const SOUNDS: [&str; 3] = [SFX_COUNTDOWN, SFX_HIT, SFX_MISS];
}

/// A decoded character spritesheet plus its animation atlas.
#[derive(Debug, Clone)]
pub struct CharacterSprites {
    /// Sheet image the atlas rectangles refer to.
    pub texture: TextureAsset,
    /// Parsed Sparrow atlas.
    pub atlas: SparrowAtlas,
    /// Frame names of the idle loop.
    pub idle: Vec<String>,
    /// Frame names of the left-sing animation.
    pub sing: Vec<String>,
}

impl CharacterSprites {
    /// Frame rectangle for a frame name.
    pub fn frame(&self, name: &str) -> Option<AtlasRect> {
        self.atlas.frame(name).map(|frame| frame.frame)
    }

    /// True when the sheet provides both an idle loop and a sing pose.
    pub fn is_animated(&self) -> bool {
        !self.idle.is_empty() && !self.sing.is_empty()
    }
}

/// Why resources could not be prepared.
#[derive(Debug)]
pub struct ResourceError(pub String);

impl std::fmt::Display for ResourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ResourceError {}

/// Loaded, decoded resources for one run.
pub struct GameResources {
    vfs: Vfs,
    stack: ResourceStack,
    cache: AssetCache,
    textures: BTreeMap<String, TextureAsset>,
    sounds: BTreeMap<String, Vec<u8>>,
    atlases: BTreeMap<String, SparrowAtlas>,
    missing: Vec<String>,
    fallback: TextureAsset,
    character: Option<CharacterSprites>,
}

impl GameResources {
    /// Loads resources for an optional mod root and an optional user pack.
    ///
    /// Missing optional layers are not an error. A missing *resource* is not fatal
    /// either: it is recorded in [`GameResources::missing`] and replaced by a
    /// visible fallback so the game still starts and the problem is obvious.
    pub fn load(mod_root: Option<&Path>, user_pack: Option<&Path>) -> Result<Self, ResourceError> {
        let mut vfs = Vfs::new();
        install_default_pack(&mut vfs)
            .map_err(|error| ResourceError(format!("could not install default pack: {error}")))?;

        if let Some(pack) = user_pack {
            vfs.mount_physical(
                "pack:user",
                "/",
                pack,
                USER_PACK_PRIORITY,
                MountPolicy::ReadOnly,
                CaseSensitivity::Insensitive,
            )
            .map_err(|error| {
                ResourceError(format!(
                    "could not mount resource pack '{}': {error}",
                    pack.display()
                ))
            })?;
        }
        if let Some(root) = mod_root {
            vfs.mount_physical(
                "mod",
                "/",
                root,
                MOD_PRIORITY,
                MountPolicy::ReadOnly,
                CaseSensitivity::Insensitive,
            )
            .map_err(|error| {
                ResourceError(format!(
                    "could not mount mod root '{}': {error}",
                    root.display()
                ))
            })?;
        }

        let stack = build_stack(&vfs)?;
        let mut resources = Self {
            vfs,
            stack,
            cache: AssetCache::new(32),
            textures: BTreeMap::new(),
            sounds: BTreeMap::new(),
            atlases: BTreeMap::new(),
            missing: Vec::new(),
            fallback: fallback_texture(),
            character: None,
        };
        resources.load_eagerly();
        resources.load_character();
        Ok(resources)
    }

    /// Loads only the generated default pack. Used by tests and `--no-mod` runs.
    pub fn defaults() -> Result<Self, ResourceError> {
        Self::load(None, None)
    }

    fn load_eagerly(&mut self) {
        let keys: Vec<&str> = keys::TEXTURES
            .iter()
            .chain(keys::PANELS.iter())
            .copied()
            .collect();

        for key in keys {
            let Some(path) = self.stack.path(key).map(str::to_string) else {
                self.missing.push(key.to_string());
                continue;
            };
            match self.cache.pin::<TextureAsset>(&self.vfs, &path) {
                Ok(texture) => {
                    self.textures.insert(key.to_string(), (*texture).clone());
                }
                Err(error) => {
                    self.missing.push(format!("{key} ({error})"));
                }
            }
        }

        for key in keys::SOUNDS {
            let Some(path) = self.stack.path(key).map(str::to_string) else {
                self.missing.push(key.to_string());
                continue;
            };
            match self.cache.pin::<Vec<u8>>(&self.vfs, &path) {
                Ok(bytes) if !bytes.is_empty() => {
                    self.sounds.insert(key.to_string(), (*bytes).clone());
                }
                Ok(_) => self.missing.push(format!("{key} (empty file)")),
                Err(error) => self.missing.push(format!("{key} ({error})")),
            }
        }
    }

    /// Loads the character sheet and its animation atlas, when both exist.
    fn load_character(&mut self) {
        let Some(texture) = self.textures.get(keys::CHARACTER_IMAGE).cloned() else {
            return;
        };
        let Some(atlas_path) = self.stack.path(keys::CHARACTER_ATLAS).map(str::to_string) else {
            self.missing.push(keys::CHARACTER_ATLAS.to_string());
            return;
        };
        let parsed = self
            .vfs
            .read_to_string(&atlas_path)
            .map_err(|error| error.to_string())
            .and_then(|xml| SparrowAtlas::parse_xml(&xml).map_err(|error| error.to_string()));
        let atlas = match parsed {
            Ok(atlas) => atlas,
            Err(error) => {
                self.missing
                    .push(format!("{} ({error})", keys::CHARACTER_ATLAS));
                return;
            }
        };
        let names = |prefix: &str| -> Vec<String> {
            atlas
                .frames_for_prefix(prefix)
                .into_iter()
                .map(|frame| frame.name.clone())
                .collect()
        };
        self.character = Some(CharacterSprites {
            texture,
            idle: names("idle"),
            sing: names("singLEFT"),
            atlas,
        });
    }

    /// The default character sheet and its animations, when loaded.
    pub fn character(&self) -> Option<&CharacterSprites> {
        self.character.as_ref()
    }

    /// Pixels for a logical key, or the fallback when it could not be loaded.
    pub fn texture(&self, key: &str) -> &TextureAsset {
        self.textures.get(key).unwrap_or(&self.fallback)
    }

    /// True when `key` loaded from a real resource.
    pub fn has_texture(&self, key: &str) -> bool {
        self.textures.contains_key(key)
    }

    /// Replaces one decoded texture, used to layer a real Codename skin over
    /// the generated defaults without changing any consumer.
    pub fn install_texture(&mut self, key: &str, texture: TextureAsset) {
        self.textures.insert(key.to_string(), texture);
    }

    /// Encoded sound bytes for a logical key.
    pub fn sound(&self, key: &str) -> Option<&[u8]> {
        self.sounds.get(key).map(Vec::as_slice)
    }

    /// The pack layer that supplied `key`, for diagnostics.
    pub fn provenance(&self, key: &str) -> Option<&str> {
        self.stack.resolve(key).map(|resolved| resolved.source)
    }

    /// Keys (and reasons) that fell back instead of loading.
    pub fn missing(&self) -> &[String] {
        &self.missing
    }

    /// True when every eagerly loaded resource was found.
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }

    /// Total decoded texture bytes held by this run.
    pub fn texture_bytes(&self) -> usize {
        self.textures
            .values()
            .map(|texture| texture.rgba8.len())
            .sum()
    }

    /// Loads and caches a Sparrow atlas by virtual path.
    ///
    /// Mods address atlases by path because the spritesheet layout is theirs; the
    /// manifest supplies only the well-known defaults.
    pub fn atlas(&mut self, virtual_path: &str) -> Option<&SparrowAtlas> {
        if !self.atlases.contains_key(virtual_path) {
            let text = self.vfs.read_to_string(virtual_path).ok()?;
            let atlas = SparrowAtlas::parse_xml(&text).ok()?;
            self.atlases.insert(virtual_path.to_string(), atlas);
        }
        self.atlases.get(virtual_path)
    }

    /// The virtual file system these resources were resolved through.
    pub fn vfs(&self) -> &Vfs {
        &self.vfs
    }

    /// The manifest layers in resolution order.
    pub fn stack(&self) -> &ResourceStack {
        &self.stack
    }

    /// A one-line description of the loaded layers and any gaps.
    pub fn summary(&self) -> String {
        let mut summary = format!(
            "{} textures, {} sounds from {} layer(s)",
            self.textures.len(),
            self.sounds.len(),
            self.stack.len()
        );
        if self.missing.is_empty() {
            summary.push_str(", no missing resources");
        } else {
            summary.push_str(&format!(
                ", {} missing: {}",
                self.missing.len(),
                self.missing.join("; ")
            ));
        }
        summary
    }
}

/// Builds the manifest stack: embedded defaults, then `/defaults/manifest.json`
/// as resolved through the VFS (a mod may replace it), then a pack manifest.
fn build_stack(vfs: &Vfs) -> Result<ResourceStack, ResourceError> {
    let mut stack = ResourceStack::new();

    let embedded = ResourceManifest::from_json(DEFAULT_MANIFEST_JSON.as_bytes())
        .map_err(|error| ResourceError(format!("embedded manifest is broken: {error}")))?;
    stack.push("builtin", embedded);

    if vfs.exists(DEFAULT_MANIFEST_PATH) {
        if let Ok(bytes) = vfs.read(DEFAULT_MANIFEST_PATH) {
            if bytes.as_slice() != DEFAULT_MANIFEST_JSON.as_bytes() {
                match ResourceManifest::from_json(&bytes) {
                    Ok(manifest) => stack.push("defaults/manifest.json", manifest),
                    Err(error) => {
                        return Err(ResourceError(format!(
                            "{DEFAULT_MANIFEST_PATH} is invalid: {error}"
                        )))
                    }
                }
            }
        }
    }

    if vfs.exists(PACK_MANIFEST_PATH) {
        let bytes = vfs
            .read(PACK_MANIFEST_PATH)
            .map_err(|error| ResourceError(format!("{PACK_MANIFEST_PATH}: {error}")))?;
        let manifest = ResourceManifest::from_json(&bytes)
            .map_err(|error| ResourceError(format!("{PACK_MANIFEST_PATH} is invalid: {error}")))?;
        stack.push(PACK_MANIFEST_PATH, manifest);
    }

    Ok(stack)
}

/// A magenta/black checker, so a missing texture is unmistakable rather than invisible.
fn fallback_texture() -> TextureAsset {
    const SIZE: u32 = 64;
    let mut texture = TextureAsset::transparent(SIZE, SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let checker = ((x / 8) + (y / 8)) % 2 == 0;
            let color = if checker {
                [255, 0, 255, 255]
            } else {
                [24, 24, 24, 255]
            };
            let index = ((y as usize) * (SIZE as usize) + x as usize) * 4;
            texture.rgba8[index..index + 4].copy_from_slice(&color);
        }
    }
    texture
}

#[cfg(test)]
mod tests {
    use super::*;
    use fu_import::test_support::TempDir;

    #[test]
    fn the_default_pack_provides_every_runtime_resource() {
        let resources = GameResources::defaults().expect("resources");
        assert!(
            resources.is_complete(),
            "defaults must cover the runtime, missing: {:?}",
            resources.missing()
        );
        for key in keys::TEXTURES.iter().chain(keys::PANELS.iter()) {
            assert!(resources.has_texture(key), "{key} is loaded");
            let texture = resources.texture(key);
            assert!(texture.width > 0 && texture.height > 0);
        }
        for key in keys::SOUNDS {
            assert!(resources.sound(key).is_some(), "{key} is loaded");
        }
        assert!(resources.texture_bytes() > 0);
        assert!(resources.summary().contains("no missing resources"));
        assert_eq!(resources.provenance(keys::NOTE_LEFT), Some("builtin"));

        let character = resources.character().expect("default character loads");
        assert!(character.is_animated(), "idle and sing animations exist");
        assert_eq!(character.idle.len(), 2);
        assert_eq!(character.sing.len(), 2);
        assert_eq!(
            (character.texture.width, character.texture.height),
            (256, 256)
        );
        assert_eq!(
            character.frame("singLEFT0001"),
            Some(AtlasRect {
                x: 128,
                y: 128,
                width: 128,
                height: 128
            })
        );
    }

    #[test]
    fn a_mod_file_shadows_the_default_texture_without_a_manifest() {
        let temp = TempDir::new("resources-mod-override").unwrap();
        temp.write(
            "mod/_polymod_meta.json",
            r#"{"title":"Override Mod","api_version":"0.5.0"}"#,
        )
        .unwrap();
        // A 2x2 solid white PNG stands in for the mod's note texture.
        let marker = TextureAsset::solid(2, 2, [255, 255, 255, 255]);
        temp.write_bytes(
            "mod/defaults/images/notes/note_left.png",
            &marker.encode_png().expect("png"),
        )
        .unwrap();

        let resources =
            GameResources::load(Some(temp.join("mod").as_path()), None).expect("resources");
        let texture = resources.texture(keys::NOTE_LEFT);
        assert_eq!((texture.width, texture.height), (2, 2));
        assert_eq!(texture.pixel(0, 0), Some([255, 255, 255, 255]));

        let untouched = resources.texture(keys::NOTE_RIGHT);
        assert!(untouched.width > 2, "other keys keep the default texture");
        assert!(resources.is_complete());
    }

    #[test]
    fn a_pack_manifest_can_remap_keys() {
        let temp = TempDir::new("resources-manifest-remap").unwrap();
        temp.write(
            "pack/assets/manifest.json",
            r#"{"name":"Remap","assets":{"note.left":"/custom/left.png"}}"#,
        )
        .unwrap();
        temp.write_bytes(
            "pack/custom/left.png",
            &TextureAsset::solid(3, 1, [1, 2, 3, 255])
                .encode_png()
                .expect("png"),
        )
        .unwrap();

        let resources =
            GameResources::load(None, Some(temp.join("pack").as_path())).expect("resources");
        let texture = resources.texture(keys::NOTE_LEFT);
        assert_eq!((texture.width, texture.height), (3, 1));
        assert_eq!(texture.pixel(0, 0), Some([1, 2, 3, 255]));
        assert_eq!(
            resources.provenance(keys::NOTE_LEFT),
            Some(PACK_MANIFEST_PATH)
        );
        assert_eq!(resources.provenance(keys::NOTE_RIGHT), Some("builtin"));
    }

    #[test]
    fn missing_resources_fall_back_and_are_reported() {
        let temp = TempDir::new("resources-missing").unwrap();
        temp.write(
            "pack/assets/manifest.json",
            r#"{"name":"Broken","assets":{"note.left":"/does-not-exist.png"}}"#,
        )
        .unwrap();

        let resources =
            GameResources::load(None, Some(temp.join("pack").as_path())).expect("resources");
        assert!(!resources.has_texture(keys::NOTE_LEFT));
        assert!(!resources.is_complete());
        assert!(resources
            .missing()
            .iter()
            .any(|entry| entry.starts_with(keys::NOTE_LEFT)));

        // The fallback is a visible magenta checker, never a transparent hole.
        let fallback = resources.texture(keys::NOTE_LEFT);
        assert_eq!(fallback.pixel(0, 0), Some([255, 0, 255, 255]));
        assert!(!resources.summary().contains("no missing resources"));
    }

    #[test]
    fn atlases_load_through_the_vfs_once() {
        let temp = TempDir::new("resources-atlas").unwrap();
        temp.write(
            "mod/sprites/bf.xml",
            r#"<TextureAtlas imagePath="bf.png">
                 <SubTexture name="idle0000" x="0" y="0" width="8" height="8"/>
                 <SubTexture name="idle0001" x="8" y="0" width="8" height="8"/>
               </TextureAtlas>"#,
        )
        .unwrap();

        let mut resources =
            GameResources::load(Some(temp.join("mod").as_path()), None).expect("resources");
        let atlas = resources.atlas("/sprites/bf.xml").expect("atlas");
        assert_eq!(atlas.animation_len("idle"), 2);
        assert!(resources.atlas("/sprites/missing.xml").is_none());
    }
}
