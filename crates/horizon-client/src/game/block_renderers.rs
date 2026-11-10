//! Block entity renderers of content mods' blocks, drawn from their data
//! (`data/<ns>/horizon/renderer/<block>.json`, API block 11): boxes, a fluid
//! that rises with a tank, parts that spin. What changes comes from what the
//! server sends of the block entity (`native_block_entity_update_tag`: its
//! `tanks` with their capacities, its `energy` with its `energy_cap`), which
//! the server sends again when a drawing would change (`display_signature`).
use super::{Game, native_block_entity_draw::{BlockEntityDraw, CubeFace}};
use horizon_client::containers::ModBlockEntity;
use horizon_mods::{
    block_render::{bounce_offset, fluid_box, fluid_sprite, fluid_tint, sixteenths, spin_degrees, turn_degrees, volume_fluid_box},
    content::{BlockRenderer, RenderElement, parse_color},
};
use horizon_world::nbt::{JavaString, Tag};
use std::{collections::HashMap, sync::OnceLock};

/// The mods' renderers by block id, set once with the menu types.
pub(super) static BLOCK_RENDERERS: OnceLock<HashMap<String, BlockRenderer>> = OnceLock::new();

/// The renderer of block `id`, if it has one.
pub(super) fn renderer_of(id: &str) -> Option<&'static BlockRenderer> {
    BLOCK_RENDERERS.get()?.get(id)
}

/// A tank as the server sent it.
struct ShownTank {
    substance: String,
    amount: u64,
    capacity: u64,
}

fn key(s: &str) -> JavaString {
    JavaString::from(s)
}
fn long(tag: Option<&Tag>) -> u64 {
    match tag {
        Some(Tag::Long(v)) => (*v).max(0) as u64,
        Some(Tag::Int(v)) => (*v).max(0) as u64,
        _ => 0,
    }
}

/// The tanks in a block entity's `nbt`.
fn tanks_of(entity: &ModBlockEntity) -> Vec<ShownTank> {
    let Some(Tag::List(list)) = entity.nbt.get(&key("tanks")) else {
        return Vec::new();
    };
    list.iter()
        .map(|t| ShownTank {
            substance: match t.get("substance") {
                Some(Tag::String(s)) => s.to_string_lossy(),
                _ => String::new(),
            },
            amount: long(t.get("amount")),
            capacity: long(t.get("capacity")),
        })
        .collect()
}

/// What a client draws of a block entity's `nbt`, in sixteenths, folded into
/// one number: the server sends the block entity again only when it changes.
pub(super) fn display_signature(entity: &ModBlockEntity) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for t in tanks_of(entity) {
        (t.substance, sixteenths(t.amount, t.capacity)).hash(&mut h);
    }
    let energy = long(entity.nbt.get(&key("energy")));
    let cap = long(entity.nbt.get(&key("energy_cap")));
    sixteenths(energy, cap).hash(&mut h);
    if renderer_of(&entity.id).is_some_and(BlockRenderer::uses_data) {
        data_of(entity).hash(&mut h);
    }
    h.finish()
}

/// The bytes the mod saved with its last update (what a part's test reads).
fn data_of(entity: &ModBlockEntity) -> &[u8] {
    match entity.nbt.get(&key("data")) {
        Some(Tag::ByteArray(b)) => b.as_slice(),
        _ => &[],
    }
}

/// The part of a block entity's `nbt` the clients are sent.
pub(super) fn display_tag(entity: &ModBlockEntity) -> Option<horizon_world::nbt::Compound> {
    let mut tag = horizon_world::nbt::Compound::new();
    for k in ["tanks", "energy", "energy_cap"] {
        if let Some(v) = entity.nbt.get(&key(k)) {
            tag.insert(key(k), v.clone());
        }
    }
    // The data, for the parts a renderer draws by it (a few bytes: the first 64).
    if renderer_of(&entity.id).is_some_and(BlockRenderer::uses_data) {
        let data = data_of(entity);
        let n = data.len().min(64);
        tag.insert(key("data"), Tag::ByteArray(data[..n].to_vec().into()));
    }
    (!tag.is_empty()).then_some(tag)
}

/// The same sprite on all six faces.
fn all(sprite: &str) -> [Option<CubeFace<'_>>; 6] {
    [Some(CubeFace { sprite, uv: None }); 6]
}

impl Game<'_> {
    /// Draws block `block`'s declared renderer for `entity`; false when it has none.
    pub(super) fn draw_declared_renderer(
        &self,
        block: &str,
        entity: &ModBlockEntity,
        draw: &mut BlockEntityDraw,
    ) -> bool {
        let Some(renderer) = renderer_of(block) else {
            return false;
        };
        let tanks = tanks_of(entity);
        for element in &renderer.elements {
            match element {
                RenderElement::Cube { from, to, sprite, tint } => {
                    let tint = tint.as_deref().and_then(parse_color).map_or(0xFFFFFF, |c| c & 0xFF_FFFF);
                    self.native_draw_cube(draw, *from, *to, all(sprite), tint);
                }
                RenderElement::Fluid { tank, from, to, sprites, default_sprite, tints } => {
                    let Some(t) = tanks.get(usize::from(*tank)).filter(|t| t.capacity > 0 && t.amount > 0)
                    else {
                        continue;
                    };
                    let fraction = t.amount as f32 / t.capacity as f32;
                    let Some((lo, hi)) = fluid_box(*from, *to, fraction) else {
                        continue;
                    };
                    let Some(sprite) = fluid_sprite(sprites, default_sprite.as_ref(), &t.substance) else {
                        continue;
                    };
                    let tint = fluid_tint(tints, &t.substance);
                    self.native_draw_cube(draw, lo, hi, all(sprite), tint);
                }
                RenderElement::FluidVolume { tank, box_at, draw_at, fill_at, sprites, default_sprite, tints } => {
                    let Some(t) = tanks.get(usize::from(*tank)).filter(|t| t.amount > 0) else {
                        continue;
                    };
                    let Some((lo, hi)) = volume_fluid_box(
                        data_of(entity),
                        usize::from(*box_at),
                        usize::from(*draw_at),
                        usize::from(*fill_at),
                        draw.block_pos(),
                    ) else {
                        continue;
                    };
                    let Some(sprite) = fluid_sprite(sprites, default_sprite.as_ref(), &t.substance) else {
                        continue;
                    };
                    let tint = fluid_tint(tints, &t.substance);
                    self.native_draw_cube(draw, lo, hi, all(sprite), tint);
                }
                RenderElement::Part { block, when } => {
                    if when.as_ref().is_none_or(|w| w.matches(data_of(entity))) {
                        self.native_draw_block(draw, block, &[]);
                    }
                }
                RenderElement::Animated {
                    from,
                    to,
                    sprite,
                    scale,
                    center,
                    bounce,
                    rotations,
                    fullbright,
                    only_with_energy,
                    energy_fade,
                } => {
                    if *only_with_energy && long(entity.nbt.get(&key("energy"))) == 0 {
                        continue;
                    }
                    let time = draw.game_time as f32 + draw.partial;
                    let middle = center.unwrap_or_else(|| [0, 1, 2].map(|i| (from[i] + to[i]) / 2.0));
                    let at = middle.map(|v| v / 16.0);
                    let size = scale.unwrap_or(1.0);
                    draw.push();
                    draw.translate(at);
                    draw.scale([size; 3]);
                    if let Some(b) = bounce {
                        draw.translate([0.0, bounce_offset(time, b.amplitude, b.degrees_per_tick), 0.0]);
                    }
                    for r in rotations {
                        let len = r.axis.iter().map(|v| v * v).sum::<f32>().sqrt();
                        if len > 0.0 {
                            let axis = r.axis.map(|v| v / len);
                            draw.rotate_about(axis, turn_degrees(time, r.degrees_per_tick, r.offset), [0.0; 3]);
                        }
                    }
                    let lit = draw.light;
                    if *fullbright {
                        // 16 times sky light in the low byte, block light in the next.
                        draw.light = 0xF0F0;
                    }
                    // The box keeps its own texel coordinates (they set its texture mapping);
                    // the pose moves its centre back to the origin it turns about.
                    draw.translate(at.map(|v| -v));
                    let tint = if *energy_fade {
                        // Not below a fifth, so a nearly empty cube still shows its core.
                        let full = sixteenths(long(entity.nbt.get(&key("energy"))), long(entity.nbt.get(&key("energy_cap"))));
                        let level = (u32::from(full.max(3)) * 255 / 16).min(255);
                        level << 16 | level << 8 | level
                    } else {
                        0xFFFFFF
                    };
                    self.native_draw_cube(draw, *from, *to, all(sprite), tint);
                    draw.light = lit;
                    draw.pop();
                }
                RenderElement::Spin { from, to, sprite, axis, degrees_per_tick, pivot } => {
                    let degrees = spin_degrees(draw.game_time, draw.partial, *degrees_per_tick);
                    let centre = pivot.unwrap_or_else(|| {
                        [0, 1, 2].map(|i| (from[i] + to[i]) / 2.0)
                    });
                    let axis = match axis.as_str() {
                        "x" => [1.0, 0.0, 0.0],
                        "y" => [0.0, 1.0, 0.0],
                        _ => [0.0, 0.0, 1.0],
                    };
                    draw.push();
                    // The pivot is in texels; the pose is in blocks.
                    draw.rotate_about(axis, degrees, centre.map(|v| v / 16.0));
                    self.native_draw_cube(draw, *from, *to, all(sprite), 0xFFFFFF);
                    draw.pop();
                }
            }
        }
        true
    }

    /// Tells the clients of the declared-renderer block entities of this
    /// level whose drawing changed since they were last told.
    #[cfg(feature = "mods")]
    pub(super) fn sync_declared_displays(&mut self) {
        if BLOCK_RENDERERS.get().is_none_or(HashMap::is_empty) {
            return;
        }
        let mut changed: Vec<[i32; 3]> = Vec::new();
        let mut seen: Vec<([i32; 3], u64)> = Vec::new();
        for (pos, e) in self.world.block_entities.iter() {
            let horizon_client_render::containers::BlockEntity::Mod(m) = e else {
                continue;
            };
            if renderer_of(&m.id).is_none() {
                continue;
            }
            // A change of the block's state (a side's look) makes the clients build its entity again,
            // empty: what they are shown is sent again with it.
            let state = u64::from(self.world.block(pos.0, pos.1, pos.2).0);
            seen.push(([pos.0, pos.1, pos.2], display_signature(m) ^ state.wrapping_mul(0x9E37_79B9_7F4A_7C15)));
        }
        for (at, sig) in seen {
            if self.mods.shown_displays.insert(at, sig) != Some(sig) {
                changed.push(at);
            }
        }
        for pos in changed {
            self.native_block_entity_sync(pos);
        }
    }
}
