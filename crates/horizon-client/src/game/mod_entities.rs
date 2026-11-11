//! The game's part of the living entities of the mods' own kinds (FORK-10): spawning, removing, ticking by the server mods that asked
//! for them (`entities-tick`) and keeping them in the world's folder. Their state and its save are `horizon_mods::entity_state`.

use super::Game;
use horizon_mods::{
    content::EntityKind,
    entity_state::{CARRIER_TAG, FILE, ModEntityState, WALK_FACTOR, Walk, apply, carrier_snbt, decode, encode, follow, hitbox_snbt, needs_path},
};
use std::{collections::HashMap, sync::OnceLock};

/// The mods' entity kinds by id, set once with the other content.
pub(super) static MOD_KINDS: OnceLock<HashMap<String, EntityKind>> = OnceLock::new();

use horizon_core::random::RandomSource as _;

impl Game<'_> {
    fn mod_entities_path(&self) -> std::path::PathBuf {
        self.world_root.join(FILE)
    }

    /// Reads the saved entities once, on the server's side.
    pub(super) fn load_mod_entities(&mut self) {
        if self.mod_entities.loaded || self.remote.is_some() {
            return;
        }
        self.mod_entities.loaded = true;
        if let Ok(text) = std::fs::read_to_string(self.mod_entities_path()) {
            self.mod_entities.list = decode(&text);
            // The id counter goes past every id kept.
            if let Some(top) = self.mod_entities.list.iter().map(|e| e.id).max() {
                self.next_entity = self.next_entity.max(top);
            }
        }
    }

    /// Writes the entities (nothing for a world that never had one).
    pub(super) fn save_mod_entities(&self) {
        if !self.mod_entities.loaded || self.remote.is_some() {
            return;
        }
        let path = self.mod_entities_path();
        if self.mod_entities.list.is_empty() && !path.exists() {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, encode(&self.mod_entities.list)) {
            log::warn!("the mods' entities were not written: {e}");
        }
    }

    /// A mod's `spawn-mod-entity`: a new entity of its kind in the level in place; its id. None for another level, a kind no mod declared or
    /// a position that is not on a loaded chunk.
    pub(super) fn native_spawn_mod_entity(&mut self, dimension: &str, kind: &str, pos: [f64; 3], data: Vec<u8>) -> Option<u32> {
        if self.remote.is_some() || dimension != self.dim.id() {
            return None;
        }
        let kind = MOD_KINDS.get()?.get(kind)?;
        self.load_mod_entities();
        self.next_entity += 1;
        let id = self.next_entity;
        let mut e = ModEntityState::new(id, dimension, kind, pos, data);
        e.yaw = self.world.random.next_float() * 360.0;
        self.mod_entities.list.push(e);
        Some(id)
    }

    /// Removes the mod entity `id` with no death and no drops (its carrier goes too); false for none.
    pub(super) fn native_remove_mod_entity(&mut self, dimension: &str, id: u32) -> bool {
        let carriers: Vec<u32> = self
            .mod_entities
            .list
            .iter()
            .filter(|e| e.id == id && e.dimension == dimension)
            .flat_map(|e| [e.carrier, e.hitbox])
            .flatten()
            .collect();
        for c in carriers {
            self.others.retain(|o| o.id != c);
            self.persist.raw.remove(&c);
        }
        let before = self.mod_entities.list.len();
        self.mod_entities.list.retain(|e| !(e.id == id && e.dimension == dimension));
        self.mod_entities.list.len() != before
    }

    /// One tick of walking for the entities of the level in place that have a goal (FORK-10 c, 5c): a path found by vanilla's search
    /// (`mod_entity_path`) followed waypoint by waypoint (`follow`), at the kind's `speed` times the goal's multiplier; arriving ends
    /// the goal, and so do nothing reachable and three blocked steps in a row.
    pub(super) fn walk_mod_entities(&mut self) {
        use horizon_client::mobs::path::PathType;
        let Some(kinds) = MOD_KINDS.get() else {
            return;
        };
        let dim = self.dim.id().to_owned();
        let mut list = std::mem::take(&mut self.mod_entities.list);
        {
            let w = self.mob_world_base(&[]);
            // A cell's collision top for walking: 0 where it walks through (air, water, an open door, a plant).
            let top = |x: i32, y: i32, z: i32| {
                let st = w.data.state(w.world.block(x, y, z));
                if matches!(st.path, PathType::Open | PathType::DoorOpen | PathType::Water | PathType::Lava) {
                    0.0
                } else {
                    f64::from(st.top)
                }
            };
            for e in &mut list {
                let (Some((goal, mult)), true) = (e.goal, e.dimension == dim) else {
                    continue;
                };
                let Some(kind) = kinds.get(&e.kind) else {
                    continue;
                };
                // A chunk that is not loaded stops it where it is (its blocks read as air).
                if !self.world.chunks.contains_key(&((e.pos[0].floor() as i32) >> 4, (e.pos[2].floor() as i32) >> 4)) {
                    continue;
                }
                // FORK-10 5c: vanilla's path search (`GroundPathNavigation`) for a new goal, at most every 10 ticks for a moving one;
                // nothing reachable stops it, as a mob's failed `moveTo` does.
                if needs_path(goal, e.path_goal, e.path_age) {
                    let from = horizon_core::geometry::Vec3::new(e.pos[0], e.pos[1], e.pos[2]);
                    match horizon_client::mobs::nav::mod_entity_path(&w, from, kind.width, kind.height, goal) {
                        Some(path) => {
                            e.path = path;
                            e.path_goal = Some(goal);
                            e.path_age = 0;
                        }
                        None => {
                            e.goal = None;
                            e.path.clear();
                            e.path_goal = None;
                            continue;
                        }
                    }
                }
                e.path_age = e.path_age.saturating_add(1);
                let step = f64::from(kind.speed) * f64::from(mult) * WALK_FACTOR;
                let (mut pos, mut yaw) = (e.pos, e.yaw);
                let result = follow(&mut pos, &mut yaw, &mut e.path, step, f64::from(kind.width), f64::from(kind.height), &top);
                e.pos = pos;
                e.yaw = yaw;
                match result {
                    Walk::Moving => e.stuck = 0,
                    Walk::Arrived => {
                        e.goal = None;
                        e.path_goal = None;
                        e.stuck = 0;
                    }
                    // A step it cannot make: a new path next tick, and after 3 in a row it gives up (vanilla's stuck check).
                    Walk::Blocked => {
                        e.stuck += 1;
                        e.path.clear();
                        e.path_goal = None;
                        if e.stuck >= 3 {
                            e.goal = None;
                            e.stuck = 0;
                        }
                    }
                }
            }
        }
        self.mod_entities.list = list;
    }

    /// Makes each entity of the level in place stand for itself on the clients (FORK-10 b): an `item_display` of the skin's model (see
    /// `carrier_snbt`) summoned beside the entity, moved with it each tick, made again when the skin or the name changes, and again when
    /// its chunk comes back. Carriers a save kept from an earlier run (the display is saved with the world, the entity's state is the
    /// mod's) are taken out before the first new one is made.
    pub(super) fn sync_mod_entity_carriers(&mut self) {
        if self.remote.is_some() || self.mod_entities.list.is_empty() && !self.mod_entities.swept {
            return;
        }
        let Some(kinds) = MOD_KINDS.get() else {
            return;
        };
        if !self.mod_entities.swept {
            self.mod_entities.swept = true;
            let stale: Vec<u32> = self
                .others
                .iter()
                .filter(|o| matches!(o.kind.as_str(), "minecraft:item_display" | "minecraft:interaction"))
                .filter(|o| self.persist.raw.get(&o.id).is_some_and(|t| has_carrier_tag(t)))
                .map(|o| o.id)
                .collect();
            for id in stale {
                self.others.retain(|o| o.id != id);
                self.persist.raw.remove(&id);
            }
        }
        let dim = self.dim.id().to_owned();
        for i in 0..self.mod_entities.list.len() {
            if self.mod_entities.list[i].dimension != dim {
                continue;
            }
            let Some(kind) = kinds.get(&self.mod_entities.list[i].kind) else {
                continue;
            };
            let e = self.mod_entities.list[i].clone();
            let alive = e.carrier.is_some_and(|c| self.others.iter().any(|o| o.id == c));
            let current = Some((e.skin.clone(), e.name.clone())) == e.shown;
            if alive && !current {
                if let Some(c) = e.carrier {
                    self.others.retain(|o| o.id != c);
                    self.persist.raw.remove(&c);
                }
            }
            // Its box: an interaction of the kind's size, made again when its chunk comes back, moved with it.
            let box_alive = e.hitbox.is_some_and(|h| self.others.iter().any(|o| o.id == h));
            if !box_alive {
                let hitbox_tag = hitbox_snbt(kind);
                if let Ok((horizon_world::nbt::Tag::Compound(compound), _)) = horizon_world::nbt::parse_snbt(&hitbox_tag) {
                    let made = self.native_summon(&dim, "minecraft:interaction", e.pos, compound);
                    self.mod_entities.list[i].hitbox = made;
                }
            } else if let Some(h) = e.hitbox {
                let _ = self.native_move_entity(&dim, h, e.pos, e.yaw, 0.0);
            }
            if alive && current {
                if let Some(c) = e.carrier {
                    let _ = self.native_move_entity(&dim, c, e.pos, e.yaw, 0.0);
                }
                continue;
            }
            let text = carrier_snbt(kind, &e.skin, e.name.as_deref());
            let Ok((horizon_world::nbt::Tag::Compound(compound), _)) = horizon_world::nbt::parse_snbt(&text) else {
                continue;
            };
            let carrier = self.native_summon(&dim, "minecraft:item_display", e.pos, compound);
            let entry = &mut self.mod_entities.list[i];
            entry.carrier = carrier;
            entry.shown = carrier.map(|_| (e.skin, e.name));
        }
    }

    /// Takes `amount` of health from the mod entity `id`: whether it died of it.
    pub(super) fn damage_mod_entity(&mut self, id: u32, amount: f32) -> bool {
        let Some(e) = self.mod_entities.list.iter_mut().find(|e| e.id == id) else {
            return false;
        };
        let (health, dead) = horizon_mods::entity_state::after_damage(e.health, amount);
        e.health = health;
        dead
    }

    /// A mod's `take-dropped-item`: `count` items out of the item entity `id` (it goes when none are left; mods hear it leave). False
    /// for none, another level or a client's copy of the world.
    pub(super) fn native_take_dropped(&mut self, dimension: &str, id: u32, count: u32) -> bool {
        if self.remote.is_some() || dimension != self.dim.id() {
            return false;
        }
        let Some(i) = self.dropped.iter().position(|d| d.id == id) else {
            return false;
        };
        let have = self.dropped[i].stack.count;
        if count >= have {
            self.remove_dropped(i);
        } else {
            self.dropped[i].stack.count = have - count;
            self.dropped[i].needs_sync = true;
        }
        true
    }

    /// The entities of the level in place, as the mods see them.
    #[cfg(feature = "mods")]
    pub(super) fn mod_entities_view(&self) -> Vec<horizon_mods::ModEntity> {
        let dim = self.dim.id();
        self.mod_entities
            .list
            .iter()
            .filter(|e| e.dimension == dim)
            .map(wire)
            .collect()
    }

    /// The mod entity whose box (`interaction` entity) is entity `id`, as the mods see it.
    #[cfg(feature = "mods")]
    pub(super) fn mod_entity_of_hitbox(&self, id: u32) -> Option<horizon_mods::ModEntity> {
        self.mod_entities.list.iter().find(|e| e.hitbox == Some(id)).map(wire)
    }

    /// Applies what the mods answered to `entities-tick`.
    #[cfg(feature = "mods")]
    pub(super) fn apply_mod_entity_ticks(&mut self, updates: Vec<horizon_mods::ModEntityUpdate>) {
        let Some(kinds) = MOD_KINDS.get() else {
            return;
        };
        for u in updates {
            let Some(i) = self.mod_entities.list.iter().position(|e| e.id == u.id && e.dimension == u.dimension) else {
                continue;
            };
            let Some(kind) = kinds.get(&self.mod_entities.list[i].kind) else {
                continue;
            };
            if apply(&mut self.mod_entities.list[i], kind, &u) {
                self.mod_entities.list.remove(i);
            } else if u.menu_text.is_some() {
                self.send_mod_entity_menu_text(u.id);
            }
        }
    }
}

/// An entity as the mods get it.
#[cfg(feature = "mods")]
fn wire(e: &ModEntityState) -> horizon_mods::ModEntity {
    horizon_mods::ModEntity {
        id: e.id,
        dimension: e.dimension.clone(),
        kind: e.kind.clone(),
        x: e.pos[0],
        y: e.pos[1],
        z: e.pos[2],
        yaw: e.yaw,
        health: e.health,
        data: e.data.clone(),
        items: e.items.clone(),
        energy: e.energy,
        skin: e.skin.clone(),
        name: e.name.clone(),
        menu_data: e.menu_data.clone(),
    }
}

/// Whether an entity's memory carries the tag of a mod entity's carrier.
fn has_carrier_tag(tag: &horizon_world::nbt::Tag) -> bool {
    let horizon_world::nbt::Tag::Compound(c) = tag else {
        return false;
    };
    matches!(c.get(&horizon_world::nbt::JavaString::from("Tags")), Some(horizon_world::nbt::Tag::List(l))
        if l.iter().any(|t| matches!(t, horizon_world::nbt::Tag::String(s) if s.to_string().is_ok_and(|t| t == CARRIER_TAG))))
}

/// The mods' entity kinds, set once.
pub(super) fn register_kinds(mods: &[horizon_client::mod_content::ContentMod]) {
    let kinds = mods
        .iter()
        .flat_map(|m| m.content.entity_kinds.iter())
        .map(|k| (k.id.clone(), k.clone()))
        .collect();
    let _ = MOD_KINDS.set(kinds);
}
