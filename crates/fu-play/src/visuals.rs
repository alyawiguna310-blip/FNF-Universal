//! Scene construction and per-frame updates for the playfield.
//!
//! Every sprite comes from [`crate::resources`] by logical key; this module only
//! turns decoded pixels into GPU textures and nodes. Nothing here knows a file
//! path, so a mod or resource pack can swap any sprite without code changes.
//!
//! Placeholder rectangles remain only where no resource exists at all: the lane
//! backdrop and the song-progress bar.

use std::collections::{BTreeMap, HashMap};

use fu_assets::{AtlasFrame, TextureAsset};
use fu_render3d::camera::{Camera, Projection};
use fu_render3d::material::{AlphaMode, Material, TextureData};
use fu_render3d::mesh::{MeshData, Vertex};
use fu_render3d::renderer::Renderer3D;
use fu_render3d::resources::Handle;
use fu_render3d::scene::{Node, NodeId};
use fu_render3d::shader::ShaderId;
use fu_render3d::transform::Transform;
use glam::{Vec2, Vec3};

use crate::codename_content::{CharacterRig, CodenameContent, Role};
use crate::game::{Gameplay, Judgement, APPROACH_MS};
use crate::resources::{keys, GameResources};
use fu_timing::HitRating;

/// Receptors and lane backdrops are built for this many lanes; a chart that
/// uses fewer hides the extras, so the stock four-lane layout is unchanged.
const MAX_UI_LANES: usize = 8;
/// Stock spacing between four lanes.
const LANE_SPACING: f32 = 2.4;
/// Widest span the playfield allows, used to tighten spacing above four lanes.
const MAX_LANE_SPAN: f32 = 9.0;
const LANE_WIDTH: f32 = 1.9;

/// Horizontal positions for `lanes` receptors, centred on the playfield.
///
/// Four lanes reproduce the original stock layout exactly (`[-3.6, -1.2, 1.2,
/// 3.6]`); wider charts tighten their spacing so the receptors stay inside the
/// camera.
fn lane_positions(lanes: usize) -> Vec<f32> {
    let lanes = lanes.max(1);
    let spacing = if lanes <= 4 {
        LANE_SPACING
    } else {
        MAX_LANE_SPAN / lanes as f32
    };
    (0..lanes)
        .map(|lane| (lane as f32 - (lanes as f32 - 1.0) * 0.5) * spacing)
        .collect()
}
const LANE_HEIGHT: f32 = 9.6;
const RECEPTOR_Y: f32 = -4.0;
const SPAWN_Y: f32 = 5.2;
const RECEPTOR_SIZE: f32 = 1.35;

// Depth ordering. The camera sits at +z looking towards -z, so a larger z is
// nearer to it. The lane backdrop must stay behind the notes and receptors:
// while it was in front, its opaque quads hid both and the playfield looked
// blank.
const BACKGROUND_Z: f32 = 0.0;
const LANE_Z: f32 = 0.2;
const CHARACTER_Z: f32 = 0.1;
const RECEPTOR_Z: f32 = 0.6;
const NOTE_Z: f32 = 0.8;
const PROGRESS_Z: f32 = 1.0;
const PROGRESS_FILL_Z: f32 = 1.02;
const HUD_Z: f32 = 1.2;
const EFFECT_Z: f32 = 1.4;
const PANEL_Z: f32 = 1.6;
const STATUS_Z: f32 = 2.0;

/// Distance kept between the HUD text and the edge of the viewport, in world
/// units. The viewport is 12 units tall, so 12 * aspect units wide.
const HUD_MARGIN: f32 = 0.45;
const SCORE_Y: f32 = 5.4;
const SECOND_HUD_LINE_Y: f32 = 4.7;

/// Health bar geometry, below the lane backdrop.
const HEALTH_Y: f32 = -5.45;
const HEALTH_WIDTH: f32 = 10.0;
const HEALTH_HEIGHT: f32 = 0.42;
const ICON_SIZE: f32 = 0.62;
const ICON_X: f32 = HEALTH_WIDTH * 0.5 + ICON_SIZE * 0.62;
const PROGRESS_Y: f32 = -5.95;

const COUNTDOWN_WINDOW_MS: f32 = 2_000.0;

/// Default character placement and size.
const CHARACTER_X: f32 = -6.1;
const CHARACTER_Y: f32 = 0.4;
const CHARACTER_SIZE: f32 = 2.5;
/// Frame duration of the generated character animations.
const CHARACTER_FRAME_MS: f32 = 250.0;
/// Codename's stage and character coordinates are pixels on a 1280x720 screen.
const CODENAME_PIXELS_PER_UNIT: f32 = 60.0;
/// The note skin's art is drawn smaller than its pixels: at 0.62 a stock note
/// is about 1.6 world units wide on a 12-unit playfield, matching the game.
const NOTE_SPRITE_SCALE: f32 = 0.62;

const GLYPH_COLUMNS: usize = 3;
const GLYPH_ROWS: usize = 5;
/// Horizontal advance of one character, in glyph pixels (3 columns + 1 gap).
const GLYPH_ADVANCE: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Loading,
    Playing,
    Paused,
    Complete,
    Error,
}

/// Horizontal alignment of a rendered text line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug)]
struct LaneVisual {
    receptor: NodeId,
    base_material: Handle<Material>,
    active_material: Handle<Material>,
    miss_material: Handle<Material>,
}

struct NoteVisual {
    node: NodeId,
    /// Sustain body, drawn only for held notes.
    tail: Option<NodeId>,
    /// Sustain end cap, drawn with the body.
    tail_end: Option<NodeId>,
    tail_height: f32,
    /// Real sprite sizes in world units.
    head: Vec2,
    body: Vec2,
    cap: Vec2,
}

/// One rigged text line: a fixed pool of character nodes, one per position.
///
/// Nodes are reused rather than rebuilt because the GPU backend uploads a mesh
/// once per handle; switching `node.mesh` between prebuilt glyph meshes keeps
/// every cached upload valid while the text changes.
struct TextLine {
    nodes: Vec<NodeId>,
    text: String,
    y: f32,
    z: f32,
    pixel: f32,
    align: Align,
}

impl TextLine {
    fn set(
        &mut self,
        renderer: &mut Renderer3D,
        glyphs: &HashMap<char, Handle<MeshData>>,
        text: &str,
        aspect: f32,
    ) {
        if self.text == text {
            return;
        }
        self.text.clear();
        self.text.push_str(text);

        let half_width = 6.0 * aspect.max(0.1);
        let cell = GLYPH_ADVANCE * self.pixel;
        let width = text.chars().count() as f32 * cell;
        let x0 = match self.align {
            Align::Left => -half_width + HUD_MARGIN,
            Align::Center => -width * 0.5,
            Align::Right => half_width - HUD_MARGIN - width,
        };

        for (index, character) in text.chars().enumerate() {
            let Some(node_id) = self.nodes.get(index).copied() else {
                break;
            };
            let Some(node) = renderer.scene.node_mut(node_id) else {
                continue;
            };
            node.transform.translation =
                Vec3::new(x0 + (index as f32 + 0.5) * cell, self.y, self.z);
            node.transform.scale = Vec3::splat(self.pixel);
            match glyphs.get(&character.to_ascii_uppercase()) {
                Some(mesh) => {
                    node.mesh = Some(*mesh);
                    node.visible = true;
                }
                None => node.visible = false,
            }
        }
        for node_id in self.nodes.iter().skip(text.chars().count()) {
            if let Some(node) = renderer.scene.node_mut(*node_id) {
                node.visible = false;
            }
        }
    }

    fn clear(&mut self, renderer: &mut Renderer3D) {
        self.text.clear();
        for node_id in &self.nodes {
            if let Some(node) = renderer.scene.node_mut(*node_id) {
                node.visible = false;
            }
        }
    }
}

/// The HUD values currently drawn in the scene, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudSnapshot {
    pub score: String,
    pub combo: String,
    pub misses: String,
    pub accuracy: String,
}

pub struct Visuals {
    quad: Handle<MeshData>,
    lanes: Vec<LaneVisual>,
    lane_backdrops: Vec<NodeId>,
    lane_x: Vec<f32>,
    note_materials: Vec<Handle<Material>>,
    notes: Vec<NoteVisual>,
    glyphs: HashMap<char, Handle<MeshData>>,
    stage: NodeId,
    status: NodeId,
    status_material: Handle<Material>,
    status_text: TextLine,
    progress_fill: NodeId,
    health_frame: NodeId,
    health_fill: NodeId,
    icon_player: NodeId,
    icon_opponent: NodeId,
    countdown: [NodeId; 4],
    judgement: NodeId,
    judgement_materials: [Handle<Material>; 5],
    judgement_sizes: [Vec2; 5],
    note_sizes: Vec<Vec2>,
    receptor_sizes: Vec<Vec2>,
    hold_materials: Vec<Handle<Material>>,
    hold_sizes: Vec<Vec2>,
    end_materials: Vec<Handle<Material>>,
    end_sizes: Vec<Vec2>,
    pause_panel: NodeId,
    results_panel: NodeId,
    character: Option<NodeId>,
    character_idle: Vec<Handle<MeshData>>,
    character_sing: Vec<Handle<MeshData>>,
    rigs: Vec<RigVisual>,
    codename_layers: Vec<NodeId>,
    last_opponent_sing: Option<(usize, f32)>,
    score: TextLine,
    combo: TextLine,
    misses: TextLine,
    accuracy: TextLine,
    branding: TextLine,
}

impl Visuals {
    /// Builds the scene from loaded resources.
    pub fn new(renderer: &mut Renderer3D, resources: &GameResources) -> Result<Self, String> {
        Self::build(renderer, resources, None)
    }

    /// Builds the scene with real Codename characters and stage layers where
    /// they are available, falling back to the generated pack per resource.
    pub fn with_codename(
        renderer: &mut Renderer3D,
        resources: &GameResources,
        content: &CodenameContent,
    ) -> Result<Self, String> {
        Self::build(renderer, resources, Some(content))
    }

    /// True when the playfield is drawing a real Codename stage or characters.
    pub fn has_codename_content(&self) -> bool {
        !self.rigs.is_empty() || !self.codename_layers.is_empty()
    }

    fn build(
        renderer: &mut Renderer3D,
        resources: &GameResources,
        codename: Option<&CodenameContent>,
    ) -> Result<Self, String> {
        let quad = renderer.meshes.insert(quad_mesh()?);
        let lane_material = material(renderer, "lane", [0.08, 0.10, 0.16, 1.0]);
        let lane_alt_material = material(renderer, "lane_alt", [0.06, 0.08, 0.13, 1.0]);
        let progress_bg = material(renderer, "progress_bg", [0.12, 0.14, 0.20, 1.0]);
        let progress_mat = material(renderer, "progress", [0.35, 0.85, 0.95, 1.0]);

        // Stage backdrop.
        let stage_texture = upload(renderer, "stage", resources.texture(keys::STAGE_DEFAULT))?;
        let stage_material = textured_material(
            renderer,
            "stage",
            stage_texture,
            [1.0, 1.0, 1.0, 1.0],
            false,
        );
        let stage = spawn_quad(
            renderer,
            quad,
            stage_material,
            "stage",
            Vec3::new(0.0, 0.0, BACKGROUND_Z),
            Vec3::new(40.0, 24.0, 1.0),
        );

        // Real Codename stage layers, when the installation was passed in. The
        // generated backdrop stays as the fallback when no stage was found.
        let mut codename_layers = Vec::new();
        if let Some(content) = codename.filter(|content| !content.stage_layers.is_empty()) {
            set_visible(renderer, stage, false);
            for (index, layer) in content.stage_layers.iter().enumerate() {
                let texture = upload(renderer, &format!("stage_layer_{index}"), &layer.texture)?;
                let material = textured_material(
                    renderer,
                    &format!("stage_layer_material_{index}"),
                    texture,
                    [1.0, 1.0, 1.0, layer.alpha.clamp(0.0, 1.0)],
                    true,
                );
                let scale = layer.scale.max(0.01);
                let zoom = layer.zoom.max(0.01);
                let size_w = layer.texture.width as f32 * scale;
                let size_h = layer.texture.height as f32 * scale;
                // Codename places a sprite's top-left at (x, y) in screen
                // pixels and the camera applies the stage zoom about the
                // screen centre. Convert that to this engine's world space:
                // the origin is the screen centre, +y is up, 60 px per unit.
                let left = 640.0 + (layer.position.0 - 640.0) * zoom;
                let top = 360.0 + (layer.position.1 - 360.0) * zoom;
                let centre_x = left + size_w * 0.5;
                let centre_y = top + size_h * 0.5;
                let z = if layer.front {
                    // Drawn over the characters, like Codename's document order.
                    0.5 + index as f32 * 0.01
                } else {
                    0.02 + index as f32 * 0.002
                };
                let node = spawn_quad(
                    renderer,
                    quad,
                    material,
                    &format!("stage_layer.{}", layer.name),
                    Vec3::new((centre_x - 640.0) / 60.0, (360.0 - centre_y) / 60.0, z),
                    Vec3::new(size_w / 60.0, size_h / 60.0, 1.0),
                );
                codename_layers.push(node);
            }
        }

        let mut lane_backdrops = Vec::with_capacity(MAX_UI_LANES);
        for lane in 0..MAX_UI_LANES {
            let node = spawn_quad(
                renderer,
                quad,
                if lane % 2 == 0 {
                    lane_material
                } else {
                    lane_alt_material
                },
                &format!("lane_{lane}"),
                Vec3::new(0.0, -0.5, LANE_Z),
                Vec3::new(LANE_WIDTH, LANE_HEIGHT, 1.0),
            );
            renderer.scene.node_mut(node).expect("lane").visible = false;
            lane_backdrops.push(node);
        }

        // Notes: one textured material per lane.
        let note_keys = [
            keys::NOTE_LEFT,
            keys::NOTE_DOWN,
            keys::NOTE_UP,
            keys::NOTE_RIGHT,
        ];
        // A never-drawn material only serves as the array seed before the real
        // per-lane materials are uploaded below.
        let mut note_materials = Vec::with_capacity(MAX_UI_LANES);
        let mut note_sizes = Vec::with_capacity(MAX_UI_LANES);
        for lane in 0..MAX_UI_LANES {
            let key = note_keys[lane % note_keys.len()];
            let pixels = resources.texture(key);
            note_sizes.push(sprite_size(pixels));
            let texture = upload(renderer, &format!("note_{lane}"), pixels)?;
            note_materials.push(textured_material(
                renderer,
                &format!("note_material_{lane}"),
                texture,
                [1.0, 1.0, 1.0, 1.0],
                true,
            ));
        }

        // Sustain bodies and end caps, per lane.
        let mut hold_materials = Vec::with_capacity(MAX_UI_LANES);
        let mut hold_sizes = Vec::with_capacity(MAX_UI_LANES);
        let mut end_materials = Vec::with_capacity(MAX_UI_LANES);
        let mut end_sizes = Vec::with_capacity(MAX_UI_LANES);
        for lane in 0..MAX_UI_LANES {
            let hold_key = keys::HOLD_FOR[lane % 4];
            let end_key = keys::HOLD_END_FOR[lane % 4];
            let hold_pixels = if resources.has_texture(hold_key) {
                resources.texture(hold_key)
            } else {
                resources.texture(note_keys[lane % note_keys.len()])
            };
            let end_pixels = if resources.has_texture(end_key) {
                resources.texture(end_key)
            } else {
                hold_pixels
            };
            hold_sizes.push(sprite_size(hold_pixels));
            end_sizes.push(sprite_size(end_pixels));
            let hold_texture = upload(renderer, &format!("hold_{lane}"), hold_pixels)?;
            hold_materials.push(textured_material(
                renderer,
                &format!("hold_material_{lane}"),
                hold_texture,
                [1.0, 1.0, 1.0, 1.0],
                true,
            ));
            let end_texture = upload(renderer, &format!("hold_end_{lane}"), end_pixels)?;
            end_materials.push(textured_material(
                renderer,
                &format!("hold_end_material_{lane}"),
                end_texture,
                [1.0, 1.0, 1.0, 1.0],
                true,
            ));
        }

        // A never-drawn material only serves as the judgement array seed
        // before the real per-rating materials are uploaded below.
        let seed_material = material(renderer, "texture_seed", [1.0, 1.0, 1.0, 1.0]);

        // Receptors: the skin's own arrow per lane and its pressed frame. The
        // real art is already coloured, so it is drawn untinted.
        let receptor_keys = [
            keys::RECEPTOR_LEFT,
            keys::RECEPTOR_DOWN,
            keys::RECEPTOR_UP,
            keys::RECEPTOR_RIGHT,
        ];
        let press_keys = [
            keys::RECEPTOR_PRESS_LEFT,
            keys::RECEPTOR_PRESS_DOWN,
            keys::RECEPTOR_PRESS_UP,
            keys::RECEPTOR_PRESS_RIGHT,
        ];
        let mut receptor_sizes = Vec::with_capacity(MAX_UI_LANES);
        let mut lanes = Vec::with_capacity(MAX_UI_LANES);
        for lane in 0..MAX_UI_LANES {
            let key = receptor_keys[lane % 4];
            let pixels = resources.texture(key);
            receptor_sizes.push(sprite_size(pixels));
            let receptor_texture = upload(renderer, &format!("receptor_{lane}"), pixels)?;
            let base_material = textured_material(
                renderer,
                &format!("receptor_{lane}"),
                receptor_texture,
                [1.0, 1.0, 1.0, 1.0],
                true,
            );
            let press_key = press_keys[lane % 4];
            let active_texture = if resources.has_texture(press_key) {
                upload(
                    renderer,
                    &format!("receptor_press_{lane}"),
                    resources.texture(press_key),
                )?
            } else {
                receptor_texture
            };
            let active_material = textured_material(
                renderer,
                &format!("receptor_active_{lane}"),
                active_texture,
                [1.0, 1.0, 1.0, 1.0],
                true,
            );
            let miss_material = textured_material(
                renderer,
                &format!("receptor_miss_{lane}"),
                receptor_texture,
                [0.85, 0.2, 0.25, 1.0],
                true,
            );
            let receptor = spawn_quad(
                renderer,
                quad,
                base_material,
                &format!("receptor_node_{lane}"),
                Vec3::new(0.0, RECEPTOR_Y, RECEPTOR_Z),
                Vec3::splat(RECEPTOR_SIZE),
            );
            renderer.scene.node_mut(receptor).expect("receptor").visible = false;
            lanes.push(LaneVisual {
                receptor,
                base_material,
                active_material,
                miss_material,
            });
        }

        // Song progress bar (placeholder geometry, no texture resource).
        spawn_quad(
            renderer,
            quad,
            progress_bg,
            "progress_bg",
            Vec3::new(0.0, PROGRESS_Y, PROGRESS_Z),
            Vec3::new(12.0, 0.12, 1.0),
        );
        let progress_fill = spawn_quad(
            renderer,
            quad,
            progress_mat,
            "progress_fill",
            Vec3::new(-6.0, PROGRESS_Y, PROGRESS_FILL_Z),
            Vec3::new(0.0, 0.16, 1.0),
        );

        // Health bar and icons.
        let health_frame_texture = upload(
            renderer,
            "health_frame",
            resources.texture(keys::HEALTH_FRAME),
        )?;
        let health_frame_material = textured_material(
            renderer,
            "health_frame",
            health_frame_texture,
            [1.0, 1.0, 1.0, 1.0],
            true,
        );
        let health_frame = spawn_quad(
            renderer,
            quad,
            health_frame_material,
            "health_frame",
            Vec3::new(0.0, HEALTH_Y, HUD_Z),
            Vec3::new(HEALTH_WIDTH, HEALTH_HEIGHT, 1.0),
        );

        let health_fill_texture = upload(
            renderer,
            "health_fill",
            resources.texture(keys::HEALTH_FILL),
        )?;
        let health_fill_material = textured_material(
            renderer,
            "health_fill",
            health_fill_texture,
            [1.0, 1.0, 1.0, 1.0],
            true,
        );
        let health_fill = spawn_quad(
            renderer,
            quad,
            health_fill_material,
            "health_fill",
            Vec3::new(0.0, HEALTH_Y, HUD_Z + 0.02),
            Vec3::new(HEALTH_WIDTH * 0.5, HEALTH_HEIGHT * 0.7, 1.0),
        );

        let icon_player_texture = upload(
            renderer,
            "icon_player",
            resources.texture(keys::ICON_PLAYER),
        )?;
        let icon_player_material = textured_material(
            renderer,
            "icon_player",
            icon_player_texture,
            [1.0, 1.0, 1.0, 1.0],
            true,
        );
        let icon_player = spawn_quad(
            renderer,
            quad,
            icon_player_material,
            "icon_player",
            Vec3::new(ICON_X, HEALTH_Y, HUD_Z),
            Vec3::splat(ICON_SIZE),
        );

        let icon_opponent_texture = upload(
            renderer,
            "icon_opponent",
            resources.texture(keys::ICON_OPPONENT),
        )?;
        let icon_opponent_material = textured_material(
            renderer,
            "icon_opponent",
            icon_opponent_texture,
            [1.0, 1.0, 1.0, 1.0],
            true,
        );
        let icon_opponent = spawn_quad(
            renderer,
            quad,
            icon_opponent_material,
            "icon_opponent",
            Vec3::new(-ICON_X, HEALTH_Y, HUD_Z),
            Vec3::splat(ICON_SIZE),
        );

        // Countdown frames.
        let countdown_keys = [
            keys::COUNTDOWN_3,
            keys::COUNTDOWN_2,
            keys::COUNTDOWN_1,
            keys::COUNTDOWN_GO,
        ];
        let mut countdown = [progress_fill; 4];
        for (index, key) in countdown_keys.iter().enumerate() {
            let texture = upload(
                renderer,
                &format!("countdown_{index}"),
                resources.texture(key),
            )?;
            let material = textured_material(
                renderer,
                &format!("countdown_material_{index}"),
                texture,
                [1.0, 1.0, 1.0, 1.0],
                true,
            );
            let node = spawn_quad(
                renderer,
                quad,
                material,
                &format!("countdown_{index}"),
                Vec3::new(0.0, 0.6, EFFECT_Z),
                Vec3::new(2.2, 1.47, 1.0),
            );
            renderer.scene.node_mut(node).expect("countdown").visible = false;
            countdown[index] = node;
        }

        // Judgement graphic, one material per rating.
        let judgement_keys = [
            keys::JUDGEMENT_PERFECT,
            keys::JUDGEMENT_GOOD,
            keys::JUDGEMENT_BAD,
            keys::JUDGEMENT_SHIT,
            keys::JUDGEMENT_MISS,
        ];
        let mut judgement_materials = [seed_material; 5];
        let mut judgement_sizes = [Vec2::ONE; 5];
        for (index, key) in judgement_keys.iter().enumerate() {
            let pixels = resources.texture(key);
            judgement_sizes[index] = sprite_size(pixels);
            let texture = upload(renderer, &format!("judgement_{index}"), pixels)?;
            judgement_materials[index] = textured_material(
                renderer,
                &format!("judgement_material_{index}"),
                texture,
                [1.0, 1.0, 1.0, 1.0],
                true,
            );
        }
        let judgement = spawn_quad(
            renderer,
            quad,
            judgement_materials[0],
            "judgement",
            Vec3::new(0.0, -2.4, EFFECT_Z),
            Vec3::new(2.6, 0.65, 1.0),
        );
        renderer
            .scene
            .node_mut(judgement)
            .expect("judgement")
            .visible = false;

        // Pause and results panels.
        let pause_texture = upload(
            renderer,
            "pause_panel",
            resources.texture(keys::PAUSE_PANEL),
        )?;
        let pause_material = textured_material(
            renderer,
            "pause_panel",
            pause_texture,
            [1.0, 1.0, 1.0, 1.0],
            true,
        );
        let pause_panel = spawn_quad(
            renderer,
            quad,
            pause_material,
            "pause_panel",
            Vec3::new(0.0, 0.3, PANEL_Z),
            Vec3::new(6.0, 3.0, 1.0),
        );
        renderer.scene.node_mut(pause_panel).expect("pause").visible = false;

        let results_texture = upload(
            renderer,
            "results_panel",
            resources.texture(keys::RESULTS_PANEL),
        )?;
        let results_material = textured_material(
            renderer,
            "results_panel",
            results_texture,
            [1.0, 1.0, 1.0, 1.0],
            true,
        );
        let results_panel = spawn_quad(
            renderer,
            quad,
            results_material,
            "results_panel",
            Vec3::new(0.0, 0.3, PANEL_Z),
            Vec3::new(7.0, 3.94, 1.0),
        );
        renderer
            .scene
            .node_mut(results_panel)
            .expect("results")
            .visible = false;

        // Default character: one prebuilt quad mesh per atlas frame, so swapping
        // frames only changes `node.mesh` and keeps every GPU upload valid.
        let (character, character_idle, character_sing) = match resources.character() {
            Some(sprites) if sprites.is_animated() => {
                let texture = upload(renderer, "character", &sprites.texture)?;
                let material =
                    textured_material(renderer, "character", texture, [1.0, 1.0, 1.0, 1.0], true);
                let mut frames = |names: &[String]| -> Result<Vec<Handle<MeshData>>, String> {
                    names
                        .iter()
                        .filter_map(|name| sprites.atlas.frame(name))
                        .map(|frame| -> Result<Handle<MeshData>, String> {
                            Ok(renderer
                                .meshes
                                .insert(uv_quad_mesh(frame, &sprites.texture)?))
                        })
                        .collect()
                };
                let idle = frames(&sprites.idle)?;
                let sing = frames(&sprites.sing)?;
                let node = idle.first().map(|first| {
                    spawn_quad(
                        renderer,
                        *first,
                        material,
                        "character",
                        Vec3::new(CHARACTER_X, CHARACTER_Y, CHARACTER_Z),
                        Vec3::splat(CHARACTER_SIZE),
                    )
                });
                (node, idle, sing)
            }
            _ => (None, Vec::new(), Vec::new()),
        };

        // Real Codename characters, one rig per role, drawn over the stage
        // layers at the depth Codename's draw order gives them.
        let mut rigs = Vec::new();
        if let Some(content) = codename {
            for role in Role::ALL {
                let Some(rig) = content.characters.get(&role) else {
                    continue;
                };
                rigs.push(build_rig(renderer, rig, quad)?);
            }
        }

        // Loading / error backdrop uses the menu background resource.
        let menu_texture = upload(
            renderer,
            "menu_background",
            resources.texture(keys::MENU_BACKGROUND),
        )?;
        let menu_material = textured_material(
            renderer,
            "menu_background",
            menu_texture,
            [1.0, 1.0, 1.0, 1.0],
            false,
        );
        let status = spawn_quad(
            renderer,
            quad,
            menu_material,
            "status",
            Vec3::new(0.0, 0.0, STATUS_Z),
            Vec3::new(40.0, 24.0, 1.0),
        );
        renderer.scene.node_mut(status).expect("status").visible = false;

        let glyphs = build_glyphs(renderer)?;
        let score_material = material(renderer, "hud_score", [0.95, 0.25, 0.55, 1.0]);
        let combo_material = material(renderer, "hud_combo", [0.20, 0.75, 0.95, 1.0]);
        let misses_material = material(renderer, "hud_misses", [0.95, 0.30, 0.30, 1.0]);
        let accuracy_material = material(renderer, "hud_accuracy", [0.45, 0.95, 0.35, 1.0]);
        let branding_material = material(renderer, "hud_branding", [0.85, 0.85, 0.95, 1.0]);
        let status_text_material = material(renderer, "status_text", [0.92, 0.94, 1.0, 1.0]);

        let score = spawn_text_line(
            renderer,
            score_material,
            16,
            SCORE_Y,
            HUD_Z,
            0.065,
            Align::Left,
        );
        let combo = spawn_text_line(
            renderer,
            combo_material,
            14,
            SCORE_Y,
            HUD_Z,
            0.065,
            Align::Right,
        );
        let misses = spawn_text_line(
            renderer,
            misses_material,
            14,
            SECOND_HUD_LINE_Y,
            HUD_Z,
            0.065,
            Align::Left,
        );
        let accuracy = spawn_text_line(
            renderer,
            accuracy_material,
            14,
            SECOND_HUD_LINE_Y,
            HUD_Z,
            0.065,
            Align::Right,
        );
        let branding = spawn_text_line(
            renderer,
            branding_material,
            16,
            SCORE_Y,
            HUD_Z,
            0.045,
            Align::Center,
        );
        let status_text = spawn_text_line(
            renderer,
            status_text_material,
            12,
            0.0,
            STATUS_Z + 0.02,
            0.09,
            Align::Center,
        );

        Ok(Self {
            quad,
            lanes,
            lane_backdrops,
            lane_x: lane_positions(crate::game::LANES),
            note_materials,
            notes: Vec::new(),
            glyphs,
            stage,
            status,
            status_material: menu_material,
            status_text,
            progress_fill,
            health_frame,
            health_fill,
            icon_player,
            icon_opponent,
            countdown,
            judgement,
            judgement_materials,
            judgement_sizes,
            pause_panel,
            results_panel,
            character,
            character_idle,
            character_sing,
            note_sizes,
            receptor_sizes,
            hold_materials,
            hold_sizes,
            end_materials,
            end_sizes,
            rigs,
            codename_layers,
            last_opponent_sing: None,
            score,
            combo,
            misses,
            accuracy,
            branding,
        })
    }

    /// Lays out the receptors for the chart's lane count and creates one node
    /// per chart note (plus a body for sustains).
    ///
    /// The stock four-lane chart keeps the original receptor positions; wider
    /// charts get tighter spacing. Lanes past `MAX_UI_LANES` are still judged
    /// but not drawn.
    pub fn set_notes(
        &mut self,
        renderer: &mut Renderer3D,
        chart: &crate::game::Chart,
    ) -> Result<(), String> {
        let quad = self.quad;
        let lanes = chart.lanes.max(1);
        self.lane_x = lane_positions(lanes.min(MAX_UI_LANES));

        for (index, lane_visual) in self.lanes.iter().enumerate() {
            let visible = index < lanes.min(MAX_UI_LANES);
            let x = self.lane_x.get(index).copied().unwrap_or(0.0);
            if let Some(node) = renderer.scene.node_mut(lane_visual.receptor) {
                node.visible = visible;
                if visible {
                    node.material = Some(lane_visual.base_material);
                    node.transform.translation = Vec3::new(x, RECEPTOR_Y, RECEPTOR_Z);
                    node.transform.scale = receptor_scale(&self.receptor_sizes[index]);
                }
            }
            if let Some(node) = renderer.scene.node_mut(self.lane_backdrops[index]) {
                node.visible = visible && self.codename_layers.is_empty();
                if visible {
                    node.transform.translation = Vec3::new(x, -0.5, LANE_Z);
                }
            }
        }

        self.notes.clear();
        for (index, note) in chart.notes.iter().enumerate() {
            let Some(x) = self.lane_x.get(note.lane as usize).copied() else {
                continue;
            };
            let lane = note.lane as usize;
            let material = self.note_materials[lane % self.note_materials.len()];
            let note_size = self.note_sizes[lane % self.note_sizes.len()];
            let body_size = self.hold_sizes[lane % self.hold_sizes.len()];
            let cap_size = self.end_sizes[lane % self.end_sizes.len()];
            let node = spawn_quad(
                renderer,
                quad,
                material,
                &format!("note_{index}"),
                Vec3::new(x, SPAWN_Y, NOTE_Z),
                Vec3::new(note_size.x, note_size.y, 1.0),
            );
            renderer.scene.node_mut(node).expect("note node").visible = false;

            let mut tail = None;
            let mut tail_end = None;
            let mut tail_height = 0.0;
            if note.sustain_ms > 0.0 {
                tail_height = ((note.sustain_ms / APPROACH_MS as f64) as f32
                    * (SPAWN_Y - RECEPTOR_Y))
                    .clamp(0.12, SPAWN_Y - RECEPTOR_Y);
                let hold_material = self.hold_materials[lane % self.hold_materials.len()];
                let hold_size = self.hold_sizes[lane % self.hold_sizes.len()];
                let tail_node = spawn_quad(
                    renderer,
                    quad,
                    hold_material,
                    &format!("note_tail_{index}"),
                    Vec3::new(x, SPAWN_Y, NOTE_Z - 0.01),
                    Vec3::new(hold_size.x, tail_height, 1.0),
                );
                renderer
                    .scene
                    .node_mut(tail_node)
                    .expect("tail node")
                    .visible = false;
                tail = Some(tail_node);

                let end_material = self.end_materials[lane % self.end_materials.len()];
                let end_size = self.end_sizes[lane % self.end_sizes.len()];
                let end_node = spawn_quad(
                    renderer,
                    quad,
                    end_material,
                    &format!("note_tail_end_{index}"),
                    Vec3::new(x, SPAWN_Y, NOTE_Z - 0.005),
                    Vec3::new(end_size.x, end_size.y, 1.0),
                );
                renderer
                    .scene
                    .node_mut(end_node)
                    .expect("tail end node")
                    .visible = false;
                tail_end = Some(end_node);
            }
            self.notes.push(NoteVisual {
                node,
                tail,
                tail_end,
                tail_height,
                head: note_size,
                body: body_size,
                cap: cap_size,
            });
        }
        Ok(())
    }

    /// The HUD strings currently rendered into the scene.
    pub fn hud(&self) -> HudSnapshot {
        HudSnapshot {
            score: self.score.text.clone(),
            combo: self.combo.text.clone(),
            misses: self.misses.text.clone(),
            accuracy: self.accuracy.text.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn branding_text(&self) -> &str {
        &self.branding.text
    }

    /// Advances the scene to `song_ms`. `aspect` is the viewport aspect ratio,
    /// used to keep the HUD anchored to the real screen edges.
    pub fn update(
        &mut self,
        renderer: &mut Renderer3D,
        screen: Screen,
        game: Option<&Gameplay>,
        song_ms: f32,
        aspect: f32,
    ) {
        let status_visible = matches!(screen, Screen::Loading | Screen::Error);
        set_visible(renderer, self.status, status_visible);
        if let Some(material) = renderer.materials.get_mut(self.status_material) {
            material.base_color = if screen == Screen::Error {
                [1.0, 0.35, 0.35, 1.0]
            } else {
                [1.0, 1.0, 1.0, 1.0]
            };
        }
        if status_visible {
            let label = if screen == Screen::Error {
                "ERROR"
            } else {
                "LOADING"
            };
            self.status_text.set(renderer, &self.glyphs, label, aspect);
        } else {
            self.status_text.clear(renderer);
        }

        self.branding
            .set(renderer, &self.glyphs, "FNF UNIVERSAL", aspect);

        set_visible(renderer, self.pause_panel, screen == Screen::Paused);
        set_visible(renderer, self.results_panel, screen == Screen::Complete);
        set_visible(renderer, self.stage, self.codename_layers.is_empty());

        let Some(game) = game else {
            for note in &self.notes {
                set_visible(renderer, note.node, false);
            }
            for lane in &self.lanes {
                if let Some(node) = renderer.scene.node_mut(lane.receptor) {
                    node.material = Some(lane.base_material);
                    node.transform.scale = Vec3::splat(RECEPTOR_SIZE);
                }
            }
            for node in self.countdown {
                set_visible(renderer, node, false);
            }
            set_visible(renderer, self.judgement, false);
            self.progress_fill(renderer, 0.0);
            self.health(renderer, 0.5);
            self.score
                .set(renderer, &self.glyphs, "SCORE 000000", aspect);
            self.combo.set(renderer, &self.glyphs, "COMBO 0", aspect);
            self.misses.set(renderer, &self.glyphs, "MISSES 0", aspect);
            self.accuracy
                .set(renderer, &self.glyphs, "ACC 100.0%", aspect);
            return;
        };

        for (index, note_visual) in self.notes.iter().enumerate() {
            let Some(note) = game.notes().get(index) else {
                continue;
            };
            let pending = game.judgement(index) == Judgement::Pending;
            let delta = (note.time_ms - song_ms as f64) as f32;
            let hit_window = game.windows().shit as f32;
            let visible = pending
                && matches!(screen, Screen::Playing | Screen::Paused)
                && (-hit_window..=APPROACH_MS).contains(&delta);
            let x = self.lane_x.get(note.lane as usize).copied().unwrap_or(0.0);
            let progress = delta / APPROACH_MS;
            let y = RECEPTOR_Y + progress * (SPAWN_Y - RECEPTOR_Y);
            if let Some(node) = renderer.scene.node_mut(note_visual.node) {
                node.visible = visible;
                if visible {
                    node.transform.translation = Vec3::new(x, y, NOTE_Z);
                    node.transform.scale = Vec3::new(note_visual.head.x, note_visual.head.y, 1.0);
                }
            }
            if let Some(tail) = note_visual.tail {
                if let Some(node) = renderer.scene.node_mut(tail) {
                    node.visible = visible;
                    if visible {
                        node.transform.translation =
                            Vec3::new(x, y + note_visual.tail_height * 0.5, NOTE_Z - 0.01);
                        node.transform.scale =
                            Vec3::new(note_visual.body.x, note_visual.tail_height, 1.0);
                    }
                }
            }
            if let Some(end) = note_visual.tail_end {
                if let Some(node) = renderer.scene.node_mut(end) {
                    node.visible = visible;
                    if visible {
                        node.transform.translation =
                            Vec3::new(x, y + note_visual.tail_height, NOTE_Z - 0.005);
                        node.transform.scale = Vec3::new(note_visual.cap.x, note_visual.cap.y, 1.0);
                    }
                }
            }
        }

        let hit = game.hit_feedback();
        let miss = game.miss_feedback();
        for (lane, lane_visual) in self.lanes.iter().enumerate() {
            if lane >= game.lanes() {
                continue;
            }
            let hit_lane =
                hit.filter(|feedback| feedback.lane == lane && feedback.until_ms >= song_ms);
            let miss_lane =
                miss.filter(|feedback| feedback.lane == lane && feedback.until_ms >= song_ms);
            let (material, scale) = if hit_lane.is_some() {
                (
                    lane_visual.active_material,
                    receptor_scale(&self.receptor_sizes[lane]) * 1.05,
                )
            } else if miss_lane.is_some() {
                (
                    lane_visual.miss_material,
                    receptor_scale(&self.receptor_sizes[lane]) * 0.94,
                )
            } else {
                (
                    lane_visual.base_material,
                    receptor_scale(&self.receptor_sizes[lane]),
                )
            };
            if let Some(node) = renderer.scene.node_mut(lane_visual.receptor) {
                node.material = Some(material);
                node.transform.scale = scale;
            }
        }

        self.progress_fill(renderer, game.progress(song_ms));
        self.health(renderer, game.health());
        self.countdown(renderer, screen, song_ms);
        self.judgement(renderer, screen, game, song_ms);
        self.character(renderer, screen, game, song_ms);
        self.codename_characters(renderer, screen, game, song_ms);

        let score_text = format!("SCORE {:06}", game.score());
        let combo_text = format!("COMBO {}", game.combo());
        let misses_text = format!("MISSES {}", game.misses());
        let accuracy_text = format!("ACC {:05.1}%", game.accuracy() * 100.0);

        self.score.set(renderer, &self.glyphs, &score_text, aspect);
        self.combo.set(renderer, &self.glyphs, &combo_text, aspect);
        self.misses
            .set(renderer, &self.glyphs, &misses_text, aspect);
        self.accuracy
            .set(renderer, &self.glyphs, &accuracy_text, aspect);
    }

    fn progress_fill(&mut self, renderer: &mut Renderer3D, progress: f32) {
        if let Some(node) = renderer.scene.node_mut(self.progress_fill) {
            node.transform.scale.x = 12.0 * progress;
            node.transform.translation.x = -6.0 + 6.0 * progress;
        }
    }

    /// The bar grows from the left edge and the icons sit at its ends, mirroring
    /// how the series shows player/opponent health.
    fn health(&mut self, renderer: &mut Renderer3D, health: f32) {
        let health = health.clamp(0.0, 1.0);
        let width = HEALTH_WIDTH * health;
        if let Some(node) = renderer.scene.node_mut(self.health_fill) {
            node.transform.scale.x = width.max(0.001);
            node.transform.translation.x = -HEALTH_WIDTH * 0.5 + width * 0.5;
        }
        // The bar is inset inside the frame texture.
        set_visible(renderer, self.health_frame, true);
        set_visible(renderer, self.health_fill, health > 0.005);
        set_visible(renderer, self.icon_player, true);
        set_visible(renderer, self.icon_opponent, true);
    }

    /// Plays the generated idle loop, switching to the sing pose while a note is
    /// hit or missed. Frame selection is deterministic from the song position.
    fn character(
        &mut self,
        renderer: &mut Renderer3D,
        screen: Screen,
        game: &Gameplay,
        song_ms: f32,
    ) {
        let Some(node_id) = self.character else {
            return;
        };
        // Real Codename rigs replace the generated default entirely.
        if !self.rigs.is_empty() {
            set_visible(renderer, node_id, false);
            return;
        }
        let visible = matches!(screen, Screen::Playing | Screen::Paused);
        set_visible(renderer, node_id, visible);
        if !visible {
            return;
        }
        let singing = game
            .hit_feedback()
            .is_some_and(|feedback| feedback.until_ms >= song_ms)
            || game
                .miss_feedback()
                .is_some_and(|feedback| feedback.until_ms >= song_ms);
        let frames = if singing {
            &self.character_sing
        } else {
            &self.character_idle
        };
        if frames.is_empty() {
            return;
        }
        let index = ((song_ms.max(0.0) / CHARACTER_FRAME_MS) as usize) % frames.len();
        if let Some(node) = renderer.scene.node_mut(node_id) {
            node.mesh = Some(frames[index]);
        }
    }

    fn countdown(&mut self, renderer: &mut Renderer3D, screen: Screen, song_ms: f32) {
        let visible = screen == Screen::Playing && (0.0..COUNTDOWN_WINDOW_MS).contains(&song_ms);
        for (index, node) in self.countdown.iter().enumerate() {
            let active = visible && (song_ms / 500.0) as usize == index;
            set_visible(renderer, *node, active);
        }
    }

    fn judgement(
        &mut self,
        renderer: &mut Renderer3D,
        screen: Screen,
        game: &Gameplay,
        song_ms: f32,
    ) {
        if screen != Screen::Playing {
            set_visible(renderer, self.judgement, false);
            return;
        }
        let index = game
            .hit_feedback()
            .filter(|feedback| feedback.until_ms >= song_ms)
            .and_then(|feedback| feedback.rating)
            .map(|rating| match rating {
                HitRating::Sick => 0,
                HitRating::Good => 1,
                HitRating::Bad => 2,
                // No "shit" texture ships with the default pack; the miss
                // graphic is the closest existing asset.
                HitRating::Shit => 3,
                HitRating::Miss => 4,
            })
            .or_else(|| {
                game.miss_feedback()
                    .filter(|feedback| feedback.until_ms >= song_ms)
                    .map(|_| 4)
            });

        match index {
            Some(index) => {
                if let Some(node) = renderer.scene.node_mut(self.judgement) {
                    node.material = Some(self.judgement_materials[index]);
                    // Draw the real graphic undistorted.
                    let size = self.judgement_sizes[index];
                    let height = 0.8;
                    let width = (size.x / size.y.max(1.0)) * height;
                    node.transform.scale = Vec3::new(width, height, 1.0);
                    node.visible = true;
                }
            }
            None => set_visible(renderer, self.judgement, false),
        }
    }

    /// Plays the real Codename character rigs: player sings on its hit lane,
    /// opponent sings when its simulated notes fire, girlfriend idles.
    fn codename_characters(
        &mut self,
        renderer: &mut Renderer3D,
        screen: Screen,
        game: &Gameplay,
        song_ms: f32,
    ) {
        if self.rigs.is_empty() {
            return;
        }
        let visible = matches!(screen, Screen::Playing | Screen::Paused);

        // Opponent hits are reported per update, so remember the last one long
        // enough for its sing pose to read.
        if let Some(hit) = game
            .opponent_hits()
            .iter()
            .find(|hit| hit.slot == crate::game::StrumSlot::Opponent)
        {
            self.last_opponent_sing = Some((hit.lane as usize, song_ms));
        }

        for index in 0..self.rigs.len() {
            let rig = &self.rigs[index];
            set_visible(renderer, rig.node, visible);
            if !visible {
                continue;
            }
            let animation = match rig.role {
                Role::Player => {
                    let hit = game
                        .hit_feedback()
                        .filter(|feedback| feedback.until_ms >= song_ms)
                        .map(|feedback| (feedback.lane, false));
                    let miss = game
                        .miss_feedback()
                        .filter(|feedback| feedback.until_ms >= song_ms)
                        .map(|feedback| (feedback.lane, true));
                    hit.or(miss)
                        .map(|(lane, missed)| sing_animation(lane, missed))
                        .unwrap_or_else(|| "idle".to_string())
                }
                Role::Opponent => self
                    .last_opponent_sing
                    .filter(|(_, at)| song_ms - at <= 300.0)
                    .map(|(lane, _)| sing_animation(lane, false))
                    .unwrap_or_else(|| "idle".to_string()),
                Role::Girlfriend => "idle".to_string(),
            };
            let Some(anim) = rig
                .animations
                .get(&animation)
                .or_else(|| rig.animations.get("idle"))
                .filter(|anim| !anim.frames.is_empty())
            else {
                continue;
            };
            let frame_index = if anim.looping {
                ((song_ms.max(0.0) / 1000.0 * anim.fps) as usize) % anim.frames.len()
            } else {
                0
            };
            let frame = &anim.frames[frame_index];
            if let Some(node) = renderer.scene.node_mut(rig.node) {
                node.mesh = Some(frame.mesh);
                node.transform.translation = rig.origin
                    + Vec3::new(
                        anim.offset.x + frame.offset.x,
                        anim.offset.y + frame.offset.y,
                        0.0,
                    );
                // Mirrored rigs use mirrored UVs, so the scale stays positive
                // and the quads keep their front face.
                node.transform.scale =
                    Vec3::new(frame.size.x * rig.scale, frame.size.y * rig.scale, 1.0);
            }
        }
    }
}

/// One drawable frame of a Codename character rig.
struct RigFrameVisual {
    mesh: Handle<MeshData>,
    /// Trim compensation, in world units, relative to the character origin.
    offset: Vec2,
    /// Quad size in world units.
    size: Vec2,
}

/// One animation of a Codename character rig.
struct RigAnimVisual {
    frames: Vec<RigFrameVisual>,
    fps: f32,
    /// Animation offset declared by the character XML, in world units.
    offset: Vec2,
    looping: bool,
}

/// A Codename character placed in the playfield.
struct RigVisual {
    role: Role,
    node: NodeId,
    origin: Vec3,
    scale: f32,
    animations: BTreeMap<String, RigAnimVisual>,
}

fn build_rig(
    renderer: &mut Renderer3D,
    rig: &CharacterRig,
    fallback_mesh: Handle<MeshData>,
) -> Result<RigVisual, String> {
    let texture = upload(
        renderer,
        &format!("character_{}", rig.role.id()),
        &rig.texture,
    )?;
    let material = textured_material(
        renderer,
        &format!("character_material_{}", rig.role.id()),
        texture,
        [1.0, 1.0, 1.0, 1.0],
        true,
    );
    // Mirrored characters use a negative x scale, which reverses the winding.
    if let Some(material) = renderer.materials.get_mut(material) {
        material.double_sided = true;
    }

    let mut animations = BTreeMap::new();
    for (name, animation) in &rig.animations {
        let mut frames = Vec::with_capacity(animation.frames.len());
        for frame in &animation.frames {
            let mesh = if rig.flip_x {
                renderer
                    .meshes
                    .insert(mirrored_uv_quad_mesh(frame, &rig.texture)?)
            } else {
                renderer.meshes.insert(uv_quad_mesh(frame, &rig.texture)?)
            };
            let (source_width, source_height) = frame
                .source_size
                .unwrap_or((frame.frame.width, frame.frame.height));
            // Trimmed Sparrow frames store a smaller rect plus an offset from
            // the original frame's top-left. Centring the trimmed rect on the
            // source frame keeps the character still between frames.
            let offset = Vec2::new(
                (frame.offset.0 as f32 + frame.frame.width as f32 * 0.5
                    - source_width as f32 * 0.5)
                    / CODENAME_PIXELS_PER_UNIT,
                -(frame.offset.1 as f32 + frame.frame.height as f32 * 0.5
                    - source_height as f32 * 0.5)
                    / CODENAME_PIXELS_PER_UNIT,
            );
            frames.push(RigFrameVisual {
                mesh,
                offset,
                size: Vec2::new(
                    frame.frame.width as f32 / CODENAME_PIXELS_PER_UNIT,
                    frame.frame.height as f32 / CODENAME_PIXELS_PER_UNIT,
                ),
            });
        }
        animations.insert(
            name.clone(),
            RigAnimVisual {
                frames,
                fps: animation.fps.max(1.0),
                offset: Vec2::new(
                    animation.offset.0 / CODENAME_PIXELS_PER_UNIT,
                    -animation.offset.1 / CODENAME_PIXELS_PER_UNIT,
                ),
                looping: animation.looping,
            },
        );
    }

    let first = animations
        .get("idle")
        .and_then(|animation| animation.frames.first())
        .map(|frame| frame.mesh)
        .unwrap_or(fallback_mesh);
    let z = match rig.role {
        Role::Girlfriend => 0.40,
        Role::Opponent => 0.42,
        Role::Player => 0.44,
    };
    // The playfield camera is fixed, while Codename's follows the player, so
    // very tall sprites are fitted to the visible stage and stand on a common
    // floor line. Their world position still decides left/right placement.
    let idle_height = animations
        .get("idle")
        .and_then(|animation| animation.frames.first())
        .map(|frame| frame.size.y)
        .unwrap_or(6.0);
    let fit = (7.6 / f32::max(idle_height, 0.01)).min(1.0);
    let x = ((rig.position.0 - 640.0) / CODENAME_PIXELS_PER_UNIT).clamp(-7.0, 7.0);
    let origin = Vec3::new(x, -3.8 + idle_height * fit * 0.5, z);
    let node = spawn_quad(
        renderer,
        first,
        material,
        &format!("character.{}", rig.role.id()),
        origin,
        Vec3::ONE,
    );
    set_visible(renderer, node, false);
    Ok(RigVisual {
        role: rig.role,
        node,
        origin,
        scale: rig.scale.max(0.01) * fit,
        animations,
    })
}

/// Codename animation name for a lane: 0 left, 1 down, 2 up, 3 right.
/// Builds a quad whose texture is mirrored horizontally, used for characters
/// the XML marks as flipped.
fn mirrored_uv_quad_mesh(
    frame: &AtlasFrame,
    texture: &fu_assets::TextureAsset,
) -> Result<MeshData, String> {
    let uv = frame
        .frame
        .uv(texture.width, texture.height)
        .ok_or_else(|| {
            format!(
                "atlas frame '{}' lies outside the {}x{} sheet",
                frame.name, texture.width, texture.height
            )
        })?;
    let vertices = vec![
        Vertex {
            position: [-0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [uv.u1, uv.v1],
        },
        Vertex {
            position: [0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [uv.u0, uv.v1],
        },
        Vertex {
            position: [0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [uv.u0, uv.v0],
        },
        Vertex {
            position: [-0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [uv.u1, uv.v0],
        },
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];
    MeshData::new(format!("{}_mirrored", frame.name), vertices, indices)
        .map_err(|error| format!("mirrored quad for '{}': {error}", frame.name))
}

fn sing_animation(lane: usize, missed: bool) -> String {
    let direction = match lane % 4 {
        0 => "LEFT",
        1 => "DOWN",
        2 => "UP",
        _ => "RIGHT",
    };
    format!("sing{direction}{}", if missed { "miss" } else { "" })
}

/// World size of a skin texture at the note-sprite scale.
fn sprite_size(texture: &fu_assets::TextureAsset) -> Vec2 {
    Vec2::new(
        texture.width as f32 / CODENAME_PIXELS_PER_UNIT * NOTE_SPRITE_SCALE,
        texture.height as f32 / CODENAME_PIXELS_PER_UNIT * NOTE_SPRITE_SCALE,
    )
}

fn receptor_scale(size: &Vec2) -> Vec3 {
    Vec3::new(size.x, size.y, 1.0)
}

fn set_visible(renderer: &mut Renderer3D, node: NodeId, visible: bool) {
    if let Some(node) = renderer.scene.node_mut(node) {
        node.visible = visible;
    }
}

pub fn camera() -> Camera {
    Camera {
        transform: Transform::from_translation(Vec3::new(0.0, 0.0, 10.0)),
        projection: Projection::Orthographic {
            height: 12.0,
            near: 0.1,
            far: 100.0,
        },
    }
}

fn material(renderer: &mut Renderer3D, name: &str, color: [f32; 4]) -> Handle<Material> {
    let mut material = Material::new(name, ShaderId::unlit());
    material.base_color = color;
    renderer.materials.insert(material)
}

fn upload(
    renderer: &mut Renderer3D,
    name: &str,
    texture: &TextureAsset,
) -> Result<Handle<TextureData>, String> {
    let data = TextureData::new(name, texture.width, texture.height, texture.rgba8.clone())
        .map_err(|error| format!("texture '{name}': {error}"))?;
    Ok(renderer.textures.insert(data))
}

fn textured_material(
    renderer: &mut Renderer3D,
    name: &str,
    texture: Handle<TextureData>,
    tint: [f32; 4],
    blended: bool,
) -> Handle<Material> {
    let mut material = Material::new(name, ShaderId::unlit());
    material.base_color = tint;
    material.base_color_texture = Some(texture);
    material.alpha_mode = if blended {
        AlphaMode::Blend
    } else {
        AlphaMode::Opaque
    };
    renderer.materials.insert(material)
}

fn spawn_quad(
    renderer: &mut Renderer3D,
    mesh: Handle<MeshData>,
    material: Handle<Material>,
    name: &str,
    translation: Vec3,
    scale: Vec3,
) -> NodeId {
    let mut node = Node::new(name);
    node.mesh = Some(mesh);
    node.material = Some(material);
    node.transform = Transform {
        translation,
        scale,
        ..Transform::IDENTITY
    };
    renderer.scene.add_node(node)
}

fn spawn_text_line(
    renderer: &mut Renderer3D,
    material: Handle<Material>,
    slots: usize,
    y: f32,
    z: f32,
    pixel: f32,
    align: Align,
) -> TextLine {
    let mut nodes = Vec::with_capacity(slots);
    for index in 0..slots {
        let mut node = Node::new(format!("hud_char_{index}"));
        node.material = Some(material);
        node.visible = false;
        node.transform = Transform {
            translation: Vec3::new(0.0, y, z),
            scale: Vec3::splat(pixel),
            ..Transform::IDENTITY
        };
        nodes.push(renderer.scene.add_node(node));
    }
    TextLine {
        nodes,
        text: String::new(),
        y,
        z,
        pixel,
        align,
    }
}

/// A 3x5 bitmap font covering the characters the HUD needs.
///
/// Each row is a 3-bit mask, most significant bit on the left.
fn glyph_rows(character: char) -> Option<[u8; GLYPH_ROWS]> {
    Some(match character {
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
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        '%' => [0b101, 0b001, 0b010, 0b100, 0b101],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        _ => return None,
    })
}

fn build_glyphs(renderer: &mut Renderer3D) -> Result<HashMap<char, Handle<MeshData>>, String> {
    let mut glyphs = HashMap::new();
    let characters = "0123456789.%ABCDEFGILMNOPRSTUVY";
    for character in characters.chars() {
        let mesh = glyph_mesh(character)?;
        glyphs.insert(character, renderer.meshes.insert(mesh));
    }
    Ok(glyphs)
}

/// Builds one glyph as a mesh of unit-sized pixel quads centred on the origin.
///
/// Building a mesh per glyph once means a text line only swaps `node.mesh`,
/// which keeps the backend's per-handle GPU uploads valid.
fn glyph_mesh(character: char) -> Result<MeshData, String> {
    let rows =
        glyph_rows(character).ok_or_else(|| format!("font has no glyph for '{character}'"))?;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (row, mask) in rows.iter().enumerate() {
        for column in 0..GLYPH_COLUMNS {
            let lit = mask & (1 << (GLYPH_COLUMNS - 1 - column)) != 0;
            if !lit {
                continue;
            }
            let x = column as f32 - (GLYPH_COLUMNS as f32 - 1.0) * 0.5;
            let y = (GLYPH_ROWS as f32 - 1.0) * 0.5 - row as f32;
            push_pixel(&mut vertices, &mut indices, x, y);
        }
    }
    MeshData::new(format!("glyph_{character}"), vertices, indices)
        .map_err(|error| format!("glyph mesh for '{character}': {error}"))
}

fn push_pixel(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, x: f32, y: f32) {
    let base = vertices.len() as u32;
    let corners = [
        (x - 0.5, y - 0.5, [0.0, 1.0]),
        (x + 0.5, y - 0.5, [1.0, 1.0]),
        (x + 0.5, y + 0.5, [1.0, 0.0]),
        (x - 0.5, y + 0.5, [0.0, 0.0]),
    ];
    for (px, py, uv) in corners {
        vertices.push(Vertex {
            position: [px, py, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// A unit quad whose UVs address one atlas frame of `texture`.
fn uv_quad_mesh(frame: &AtlasFrame, texture: &TextureAsset) -> Result<MeshData, String> {
    let uv = frame
        .frame
        .uv(texture.width, texture.height)
        .ok_or_else(|| {
            format!(
                "atlas frame '{}' lies outside the {}x{} sheet",
                frame.name, texture.width, texture.height
            )
        })?;
    let vertices = vec![
        Vertex {
            position: [-0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [uv.u0, uv.v1],
        },
        Vertex {
            position: [0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [uv.u1, uv.v1],
        },
        Vertex {
            position: [0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [uv.u1, uv.v0],
        },
        Vertex {
            position: [-0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [uv.u0, uv.v0],
        },
    ];
    MeshData::new(
        format!("frame_{}", frame.name),
        vertices,
        vec![0, 1, 2, 0, 2, 3],
    )
    .map_err(|error| format!("frame mesh '{}': {error}", frame.name))
}

fn quad_mesh() -> Result<MeshData, String> {
    let vertices = vec![
        Vertex {
            position: [-0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 1.0],
        },
        Vertex {
            position: [0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 1.0],
        },
        Vertex {
            position: [0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
        },
        Vertex {
            position: [-0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
    ];
    MeshData::new("ui_quad_xy", vertices, vec![0, 1, 2, 0, 2, 3])
        .map_err(|error| format!("quad mesh: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Chart, ChartNote};

    #[test]
    fn real_codename_content_replaces_the_placeholders() {
        use crate::codename_content::CodenameContent;
        use fu_render3d::quality::{Platform, QualityLevel, QualitySettings};

        let assets = std::path::Path::new("D:/idk/Codename/assets");
        if !assets.is_dir() {
            eprintln!("skipping: no Codename installation at {assets:?}");
            return;
        }
        let content = CodenameContent::load(assets, None, None);
        if content.characters.is_empty() || content.stage_layers.is_empty() {
            eprintln!("skipping: no real stage or characters loaded");
            return;
        }

        let resources = GameResources::defaults().expect("resources");
        let mut renderer = Renderer3D::new(QualitySettings::preset(
            QualityLevel::Low,
            Platform::Desktop,
        ))
        .expect("renderer");
        let mut visuals =
            Visuals::with_codename(&mut renderer, &resources, &content).expect("visuals");
        assert!(visuals.has_codename_content());
        assert!(
            !node(&renderer, "stage").visible,
            "the generated backdrop is hidden when a real stage loaded"
        );
        assert!(
            renderer
                .scene
                .iter()
                .any(|(_, node)| node.name.starts_with("stage_layer.")),
            "stage layers are in the scene"
        );

        let chart = chart();
        let mut game = Gameplay::new(chart);
        game.update(1_000.0);
        game.press(0, 1_000.0).expect("lane 0 hit");
        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            1_000.0,
            16.0 / 9.0,
        );

        assert!(
            !node(&renderer, "character").visible,
            "the generated default character is hidden"
        );
        let player = node(&renderer, "character.player");
        assert!(player.visible && player.mesh.is_some());
        let opponent = node(&renderer, "character.opponent");
        assert!(opponent.visible && opponent.mesh.is_some());
        assert!(
            content
                .characters
                .get(&Role::Player)
                .is_some_and(|rig| rig.flip_x),
            "the player is mirrored like Codename"
        );
        assert!(
            player.transform.scale.x > 0.0 && opponent.transform.scale.x > 0.0,
            "mirroring uses UVs, not a culled negative scale"
        );
        assert!(
            player.transform.scale.y > 1.0 && opponent.transform.scale.y > 1.0,
            "character frames are sized from the real sheet"
        );
    }

    fn fixture() -> (Renderer3D, Visuals) {
        let resources = GameResources::defaults().expect("resources");
        let mut renderer = Renderer3D::new(fu_render3d::quality::QualitySettings::preset(
            fu_render3d::quality::QualityLevel::Low,
            fu_render3d::quality::Platform::Desktop,
        ))
        .expect("renderer");
        let visuals = Visuals::new(&mut renderer, &resources).expect("visuals");
        (renderer, visuals)
    }

    fn chart() -> Chart {
        Chart::new(
            "Hud",
            120.0,
            4_000,
            vec![ChartNote::tap(1_000.0, 0), ChartNote::tap(1_200.0, 1)],
        )
    }

    fn node<'a>(renderer: &'a Renderer3D, name: &str) -> &'a Node {
        renderer
            .scene
            .iter()
            .find(|(_, node)| node.name == name)
            .map(|(_, node)| node)
            .unwrap_or_else(|| panic!("node '{name}' exists"))
    }

    #[test]
    fn every_hud_glyph_has_a_mesh() {
        for character in "0123456789.%ABCDEFGILMNOPRSTUVY".chars() {
            let mesh = glyph_mesh(character).expect("glyph mesh");
            assert!(
                !mesh.indices().is_empty(),
                "glyph '{character}' must have lit pixels"
            );
        }
        assert!(glyph_mesh('?').is_err(), "unsupported glyphs are rejected");
    }

    #[test]
    fn lanes_render_behind_notes_and_receptors() {
        // Compile-time guarantees of the playfield depth order.
        const {
            assert!(BACKGROUND_Z < LANE_Z);
            assert!(LANE_Z < RECEPTOR_Z);
            assert!(LANE_Z < NOTE_Z);
            assert!(NOTE_Z < HUD_Z);
            assert!(HUD_Z < PANEL_Z);
            assert!(PANEL_Z < STATUS_Z);
        }
    }

    #[test]
    fn the_stage_notes_receptors_and_hud_use_uploaded_textures() {
        let (mut renderer, mut visuals) = fixture();
        visuals.set_notes(&mut renderer, &chart()).expect("notes");
        assert!(
            renderer.textures.len() >= 20,
            "default pack textures are uploaded, got {}",
            renderer.textures.len()
        );

        for name in [
            "stage",
            "health_frame",
            "health_fill",
            "icon_player",
            "receptor_node_0",
            "note_0",
        ] {
            let node = node(&renderer, name);
            let material = node.material.expect("material");
            let material = renderer.materials.get(material).expect("material data");
            assert!(
                material.base_color_texture.is_some(),
                "{name} must be drawn with a texture"
            );
        }
        let note_material = renderer
            .materials
            .get(node(&renderer, "note_0").material.expect("material"))
            .expect("material");
        assert_eq!(note_material.alpha_mode, AlphaMode::Blend, "notes blend");
    }

    #[test]
    fn hud_shows_live_score_combo_misses_and_accuracy() {
        let (mut renderer, mut visuals) = fixture();
        visuals.set_notes(&mut renderer, &chart()).expect("notes");
        let mut game = Gameplay::new(chart());

        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            1_000.0,
            16.0 / 9.0,
        );
        let initial = visuals.hud();
        assert_eq!(initial.score, "SCORE 000000");
        assert_eq!(initial.combo, "COMBO 0");
        assert_eq!(initial.misses, "MISSES 0");
        assert_eq!(visuals.branding_text(), "FNF UNIVERSAL");

        game.press(0, 1_000.0).expect("perfect hit");
        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            1_040.0,
            16.0 / 9.0,
        );
        let after = visuals.hud();
        assert_eq!(after.score, format!("SCORE {:06}", game.score()));
        assert_eq!(after.combo, "COMBO 1");
        assert_ne!(after.score, initial.score);

        game.update(1_400.0);
        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            1_400.0,
            16.0 / 9.0,
        );
        let missed = visuals.hud();
        assert_eq!(missed.misses, "MISSES 1");
        assert_eq!(missed.combo, "COMBO 0");
        assert_eq!(
            missed.accuracy, after.accuracy,
            "a miss adds no accuracy sample"
        );
    }

    #[test]
    fn hud_text_nodes_are_visible_and_anchored_to_the_edges() {
        let (mut renderer, mut visuals) = fixture();
        visuals.set_notes(&mut renderer, &chart()).expect("notes");
        let game = Gameplay::new(chart());
        visuals.update(&mut renderer, Screen::Playing, Some(&game), 0.0, 16.0 / 9.0);

        let visible_hud = renderer
            .scene
            .iter()
            .filter(|(_, node)| node.visible && node.name.starts_with("hud_char_"))
            .count();
        assert!(
            visible_hud >= "SCORE 000000".len() + "COMBO 0".len(),
            "expected score and combo glyphs to be visible, found {visible_hud}"
        );

        let half_width = 6.0 * 16.0 / 9.0;
        let leftmost = renderer
            .scene
            .iter()
            .filter(|(_, node)| node.visible && node.name.starts_with("hud_char_"))
            .map(|(_, node)| node.transform.translation.x)
            .fold(f32::INFINITY, f32::min);
        assert!(
            leftmost >= -half_width && leftmost <= half_width,
            "HUD must stay inside the viewport, leftmost glyph at {leftmost}"
        );
    }

    #[test]
    fn visible_notes_sit_at_the_receptor_line_as_the_song_advances() {
        let (mut renderer, mut visuals) = fixture();
        visuals.set_notes(&mut renderer, &chart()).expect("notes");
        let game = Gameplay::new(chart());
        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            1_190.0,
            16.0 / 9.0,
        );

        let note = node(&renderer, "note_1");
        assert!(note.visible, "the note approaching its time is visible");
        assert!(
            (note.transform.translation.y - RECEPTOR_Y).abs() < 0.5,
            "a note 10 ms from its time must be near the receptor, y={}",
            note.transform.translation.y
        );
        assert!(
            note.transform.translation.z > LANE_Z,
            "the note must render in front of the lane backdrop"
        );
    }

    #[test]
    fn countdown_health_and_panels_follow_the_screen_state() {
        let (mut renderer, mut visuals) = fixture();
        visuals.set_notes(&mut renderer, &chart()).expect("notes");
        let mut game = Gameplay::new(chart());

        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            200.0,
            16.0 / 9.0,
        );
        assert!(node(&renderer, "countdown_0").visible, "shows the 3");
        assert!(!node(&renderer, "countdown_3").visible);

        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            1_700.0,
            16.0 / 9.0,
        );
        assert!(node(&renderer, "countdown_3").visible, "shows GO");

        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            2_500.0,
            16.0 / 9.0,
        );
        for index in 0..4 {
            assert!(!node(&renderer, &format!("countdown_{index}")).visible);
        }

        // Health bar width follows the gameplay value.
        visuals.update(
            &mut renderer,
            Screen::Paused,
            Some(&game),
            2_500.0,
            16.0 / 9.0,
        );
        assert!(node(&renderer, "pause_panel").visible, "pause panel shows");
        assert!(!node(&renderer, "results_panel").visible);
        let health_width = node(&renderer, "health_fill").transform.scale.x;
        assert!(
            (health_width - HEALTH_WIDTH * game.health()).abs() < 1e-3,
            "health bar width {health_width} tracks health {}",
            game.health()
        );

        game.press(0, 1_000.0);
        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            1_040.0,
            16.0 / 9.0,
        );
        assert!(node(&renderer, "judgement").visible, "hit feedback shows");
        assert_eq!(
            node(&renderer, "judgement").material,
            Some(visuals.judgement_materials[0]),
            "a perfect hit selects the perfect graphic"
        );

        visuals.update(
            &mut renderer,
            Screen::Complete,
            Some(&game),
            4_100.0,
            16.0 / 9.0,
        );
        assert!(
            node(&renderer, "results_panel").visible,
            "results panel shows"
        );
        assert!(!node(&renderer, "pause_panel").visible);
        assert!(!node(&renderer, "judgement").visible);
    }

    #[test]
    fn the_default_character_animates_and_sings_on_a_hit() {
        let (mut renderer, mut visuals) = fixture();
        visuals.set_notes(&mut renderer, &chart()).expect("notes");
        let mut game = Gameplay::new(chart());

        visuals.update(&mut renderer, Screen::Playing, Some(&game), 0.0, 16.0 / 9.0);
        let character = node(&renderer, "character");
        assert!(character.visible, "the default character is drawn");
        assert!(character.mesh.is_some());
        let idle_first = character.mesh;

        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            300.0,
            16.0 / 9.0,
        );
        assert_ne!(
            node(&renderer, "character").mesh,
            idle_first,
            "the idle loop advances to its second frame"
        );

        game.press(0, 1_000.0).expect("hit");
        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            1_040.0,
            16.0 / 9.0,
        );
        let singing = node(&renderer, "character").mesh;
        assert!(visuals.character_sing.contains(&singing.expect("mesh")));

        visuals.update(
            &mut renderer,
            Screen::Loading,
            Some(&game),
            1_040.0,
            16.0 / 9.0,
        );
        assert!(
            !node(&renderer, "character").visible,
            "hidden while loading"
        );
    }

    #[test]
    fn loading_shows_the_menu_background_and_hides_it_while_playing() {
        let (mut renderer, mut visuals) = fixture();
        let game = Gameplay::new(chart());
        visuals.update(&mut renderer, Screen::Loading, Some(&game), 0.0, 16.0 / 9.0);
        assert!(node(&renderer, "status").visible);
        assert_eq!(visuals.status_text.text, "LOADING");

        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            100.0,
            16.0 / 9.0,
        );
        assert!(!node(&renderer, "status").visible);
    }

    #[test]
    fn receptors_follow_the_chart_lane_count() {
        let (mut renderer, mut visuals) = fixture();
        let six = Chart::new("Six", 120.0, 4_000, vec![ChartNote::tap(1_000.0, 5)]).with_lanes(6);
        visuals.set_notes(&mut renderer, &six).expect("six lanes");

        assert!(node(&renderer, "receptor_node_5").visible);
        assert!(!node(&renderer, "receptor_node_6").visible);
        let xs: Vec<f32> = (0..6)
            .map(|lane| {
                node(&renderer, &format!("receptor_node_{lane}"))
                    .transform
                    .translation
                    .x
            })
            .collect();
        assert!(
            xs.windows(2).all(|pair| pair[0] < pair[1]),
            "lanes are ordered left to right: {xs:?}"
        );
        assert!((xs[0] + xs[5]).abs() < 1e-5, "the layout is centred");
        assert!(
            xs.iter().all(|x| *x > -6.0 && *x < 6.0),
            "receptors stay inside the camera: {xs:?}"
        );

        // Four lanes reproduce the original stock layout.
        visuals
            .set_notes(&mut renderer, &chart())
            .expect("four lanes");
        let stock = [
            node(&renderer, "receptor_node_0").transform.translation.x,
            node(&renderer, "receptor_node_3").transform.translation.x,
        ];
        assert!(
            (stock[0] + 3.6).abs() < 1e-4 && (stock[1] - 3.6).abs() < 1e-4,
            "four lanes keep the stock positions: {stock:?}"
        );
        assert!(!node(&renderer, "receptor_node_4").visible);
    }

    #[test]
    fn a_sustain_note_gets_a_body_that_follows_the_head() {
        let (mut renderer, mut visuals) = fixture();
        let held = Chart::new(
            "Hold",
            120.0,
            4_000,
            vec![ChartNote::held(1_000.0, 0, 1_000.0)],
        );
        visuals.set_notes(&mut renderer, &held).expect("notes");
        let game = Gameplay::new(held);
        visuals.update(
            &mut renderer,
            Screen::Playing,
            Some(&game),
            200.0,
            16.0 / 9.0,
        );

        let head_y = node(&renderer, "note_0").transform.translation.y;
        let tail = node(&renderer, "note_tail_0");
        assert!(tail.visible, "the sustain body is drawn with its head");
        assert!(tail.transform.scale.y > 0.5, "the body has length");
        assert!(
            tail.transform.translation.y > head_y,
            "the body trails above the head"
        );
    }
}
