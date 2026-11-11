//! The game's part of the living entities of the mods' own kinds (FORK-10): spawning, removing, ticking by the server mods that asked
//! for them (`entities-tick`) and keeping them in the world's folder. Their state and its save are `horizon_mods::entity_state`.

use super::Game;
use horizon_mods::{
    content::EntityKind,
    entity_state::{CARRIER_TAG, FILE, ModEntityState, apply, carrier_snbt, decode, encode},
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
            .filter_map(|e| e.carrier)
            .collect();
        for c in carriers {
            self.others.retain(|o| o.id != c);
            self.persist.raw.remove(&c);
        }
        let before = self.mod_entities.list.len();
        self.mod_entities.list.retain(|e| !(e.id == id && e.dimension == dimension));
        self.mod_entities.list.len() != before
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
                .filter(|o| o.kind == "minecraft:item_display")
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

    /// The entities of the level in place, as the mods see them.
    #[cfg(feature = "mods")]
    pub(super) fn mod_entities_view(&self) -> Vec<horizon_mods::ModEntity> {
        let dim = self.dim.id();
        self.mod_entities
            .list
            .iter()
            .filter(|e| e.dimension == dim)
            .map(|e| horizon_mods::ModEntity {
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
            })
            .collect()
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
            }
        }
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
