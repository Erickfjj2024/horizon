//! A mod run with no game: `cargo horizon-mod test` (docs/mods.md, "Testing a mod").
//!
//! [`TestGame`] loads one `.hmod` from a scratch copy (its `storage` lives and
//! dies with the run) into the same sandbox the game uses, and plays the game's
//! part: the snapshot, ticks, frames, chat lines and commands, and for a
//! server mod the players joining, leaving, chatting and breaking or placing
//! blocks. [`Script`] is a
//! test written in TOML, `tests/<name>.toml` in the mod's folder: steps, each
//! saying what happens and what the mod must have shown by then.
use crate::{
    BlockPos, Clock, CollisionBox, Corner, GameMode, Mods, Motion, Output, Panel, Player,
    ServerPlayer, Snapshot, Status,
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

/// One mod in the sandbox, driven by a test.
pub struct TestGame {
    mods: Mods,
    dir: PathBuf,
    hmod: PathBuf,
    /// What the mod reads with `game-read`; ticks move the clock as the game does.
    pub player: Option<Player>,
    pub clock: Option<Clock>,
    /// The players on the server, for a server mod, in join order.
    pub players: Vec<ServerPlayer>,
    /// What each of those players is doing, by player id (capability
    /// `player-control`); a player with no entry is standing still with no experience.
    pub motions: Vec<(String, Motion)>,
    /// The body of each player, by player id (`player-control.get-state`); none listed is the standing still default.
    pub states: Vec<(String, crate::PlayerState)>,
    /// Where each player looks, `(yaw, pitch)`, by player id; none listed looks south, level.
    pub looks: Vec<(String, (f32, f32))>,
    /// Every name that joined, for their id.
    names: Vec<String>,
    tick: u64,
    /// Capability `world`'s Overworld: the blocks a script set or the mod
    /// changed; every other block, and the other dimensions, are air. Every
    /// biome is the dimension's plainest: plains, nether wastes, the end.
    pub world: std::collections::BTreeMap<(i32, i32, i32), String>,
    /// The saved bytes of the Overworld's block entities, by position: one
    /// is made when a script sets a block a mod gave a block entity, and
    /// dropped when the block changes to another.
    pub block_entities: std::collections::BTreeMap<(i32, i32, i32), Vec<u8>>,
    /// Their slots, by position, as `"minecraft:stone*3"` (empty: no stack).
    pub block_items: std::collections::BTreeMap<(i32, i32, i32), Vec<String>>,
    /// Their stored energy and the sides of their faces, by position.
    pub block_energy: std::collections::BTreeMap<(i32, i32, i32), (u64, [crate::FlowSide; 6])>,
    /// Their tanks, by position: what each holds (`"minecraft:water*500"`,
    /// empty for none) and the sides of its faces.
    pub block_tanks: std::collections::BTreeMap<(i32, i32, i32), Vec<(String, [crate::FlowSide; 6])>>,
    /// The redstone signal at block entities' positions, for `get-redstone`.
    pub power: std::collections::BTreeMap<(i32, i32, i32), u8>,
    /// Their menus' data slots, by position.
    pub block_menu_data: std::collections::BTreeMap<(i32, i32, i32), Vec<i32>>,
    /// The faces each transmitter connects through, by position.
    pub block_transmitters: std::collections::BTreeMap<(i32, i32, i32), [bool; 6]>,
    /// What each player carries, by player id (capability `player-items`).
    pub inventories: Vec<(String, Vec<Option<crate::ModItemStack>>)>,
    /// What the last `item_used` answered.
    pub claimed: Option<bool>,
    /// What the last `block_used` answered (`default`, `pass` or `consume`).
    pub block_use: Option<&'static str>,
    /// What the last `player_damaged` and `item_hit_damage` answered.
    pub damage_result: Option<f32>,
    pub hit_damage_result: Option<f32>,
    /// What the last `item_released` answered.
    pub release_result: Option<bool>,
    /// What the last `mod_entity_used` answered, and the damage the last `mod_entity_damaged` applied.
    pub entity_used_result: Option<bool>,
    pub entity_damage_result: Option<f32>,
    /// The living entities of the mod's own kinds (FORK-10) and the kinds its content declares.
    pub mod_entities: Vec<crate::ModEntity>,
    pub entity_kinds: Vec<crate::content::EntityKind>,
    next_mod_entity: u32,
    /// The items lying in the world of the script (FORK-10 f).
    pub dropped: Vec<DroppedSet>,
    entity_goals: std::collections::BTreeMap<u32, ([f64; 3], f32)>,
    /// What the last `can_place` answered.
    pub can_place_result: Option<bool>,
    /// The blocks packed into boxes (`world.pack-block`), by the box's position.
    pub packed: std::collections::BTreeMap<(i32, i32, i32), String>,
    /// The crafting recipes and stack sizes a script gave.
    pub crafts: Vec<CraftSet>,
    pub stack_sizes: std::collections::BTreeMap<String, u32>,
    /// The hardness of blocks a script gave.
    pub hardness: std::collections::BTreeMap<String, f32>,
    /// The mobs of the Overworld a script set, for capability `entities`.
    pub mobs: Vec<crate::MobInfo>,
    /// The structures last told to the mod, by dimension and group.
    multiblock_seen: crate::multiblock::Seen,
}

/// The world a script's mod reads: [`TestGame::world`], copied for a step.
struct ScriptWorld(
    std::collections::BTreeMap<(i32, i32, i32), String>,
    std::collections::BTreeMap<(i32, i32, i32), u8>,
    Vec<CraftSet>,
    std::collections::BTreeMap<String, u32>,
    std::collections::BTreeMap<String, f32>,
    Vec<DroppedSet>,
);

/// An item lying in the world of a script (`dropped = [...]` of a step): its entity id, stack and position.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DroppedSet {
    id: u32,
    stack: String,
    x: f64,
    y: f64,
    z: f64,
}

/// A crafting recipe of a script (`[[craft]]`): the nine cells it takes, what it makes, what stays in the cells.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CraftSet {
    id: String,
    grid: Vec<String>,
    output: String,
    #[serde(default)]
    remaining: Vec<String>,
}

impl ScriptWorld {
    fn cells(list: &[String]) -> Vec<Option<crate::ModItemStack>> {
        list.iter().map(|t| parse_stack(t)).collect()
    }
}

impl crate::WorldView for ScriptWorld {
    fn crafting_find(&self, grid: &[Option<crate::ModItemStack>]) -> Option<crate::CraftMatch> {
        let r = self.2.iter().find(|c| Self::cells(&c.grid) == grid)?;
        let mut remaining = Self::cells(&r.remaining);
        remaining.resize(9, None);
        Some(crate::CraftMatch { id: r.id.clone(), output: parse_stack(&r.output)?, remaining })
    }
    fn crafting_matches(&self, id: &str, grid: &[Option<crate::ModItemStack>]) -> bool {
        self.2.iter().any(|c| c.id == id && Self::cells(&c.grid) == grid)
    }
    fn item_max_stack(&self, item: &str) -> Option<u32> {
        self.3.get(item).copied()
    }
    fn dropped_items(&self, dimension: &str, min: [f64; 3], max: [f64; 3]) -> Vec<(u32, crate::ModItemStack, [f64; 3])> {
        if dimension != "minecraft:overworld" {
            return Vec::new();
        }
        self.5
            .iter()
            .filter(|d| (0..3).all(|i| [d.x, d.y, d.z][i] >= min[i] && [d.x, d.y, d.z][i] <= max[i]))
            .filter_map(|d| Some((d.id, parse_stack(&d.stack)?, [d.x, d.y, d.z])))
            .collect()
    }
    fn block_hardness(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<f32> {
        let name = self.block(dimension, x, y, z)?;
        Some(self.4.get(name.split('[').next().unwrap_or("")).copied().unwrap_or(0.0))
    }
    fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
        let here = (dimension == "minecraft:overworld")
            .then(|| self.0.get(&(x, y, z)).cloned())
            .flatten();
        Some(here.unwrap_or_else(|| "minecraft:air".into()))
    }
    fn biome(&self, dimension: &str, _x: i32, _y: i32, _z: i32) -> Option<String> {
        let biome = match dimension {
            "minecraft:overworld" => "minecraft:plains",
            "minecraft:the_nether" => "minecraft:nether_wastes",
            "minecraft:the_end" => "minecraft:the_end",
            _ => return None,
        };
        Some(biome.into())
    }
    fn redstone(&self, dimension: &str, x: i32, y: i32, z: i32) -> u8 {
        if dimension == "minecraft:overworld" { self.1.get(&(x, y, z)).copied().unwrap_or(0) } else { 0 }
    }
    fn environment(&self, dimension: &str, _x: i32, _y: i32, _z: i32) -> Option<crate::Environment> {
        // Noon in the plains, clear.
        let overworld = dimension == "minecraft:overworld";
        (overworld || dimension == "minecraft:the_nether" || dimension == "minecraft:the_end").then_some(crate::Environment {
            day_time: 6000,
            rain: 0.0,
            thunder: 0.0,
            temperature: 0.8,
            downfall: 0.4,
            precipitation: true,
            has_skylight: overworld,
        })
    }
    fn collision(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<Vec<CollisionBox>> {
        let state = self.block(dimension, x, y, z)?;
        let name = state.split('[').next().unwrap_or_default();
        // No shapes table here: air, plants and carpets have none, every other
        // block is a full cube.
        let none = name == "minecraft:air" || name.ends_with("grass") || name.ends_with("_torch");
        // A carpet is a thin slab, a sixteenth of a block high (collision, not a full cube).
        Some(if name.ends_with("_carpet") {
            vec![CollisionBox {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 1.0,
                max_y: 0.0625,
                max_z: 1.0,
            }]
        } else if none {
            Vec::new()
        } else {
            vec![CollisionBox {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 1.0,
                max_y: 1.0,
                max_z: 1.0,
            }]
        })
    }
    fn has_dimension(&self, dimension: &str) -> bool {
        matches!(
            dimension,
            "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
        )
    }
    fn check_state(&self, state: &str) -> Result<(), String> {
        // No registry here: the shape only (the game checks the names).
        let name = state.split('[').next().unwrap_or_default();
        let plain = |t: &str| {
            !t.is_empty()
                && t.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-/".contains(&b))
        };
        let ok = match name.split_once(':') {
            Some((ns, path)) => plain(ns) && plain(path),
            None => plain(name),
        };
        let closed = !state.contains('[') || state.ends_with(']');
        (ok && closed)
            .then_some(())
            .ok_or_else(|| format!("{state} is not a block state"))
    }
}

/// Where a script starts: standing at 0.5 64 0.5 in the Overworld, facing
/// south, full health and food, at the first tick of a new world's first morning.
pub fn default_player() -> Player {
    Player {
        x: 0.5,
        y: 64.0,
        z: 0.5,
        yaw: 0.0,
        pitch: 0.0,
        health: 20.0,
        food: 20,
        dimension: "minecraft:overworld".into(),
    }
}

impl TestGame {
    /// Starts the mod at `hmod`, as the game would with it turned on. Errs
    /// when it doesn't start, with the game's reason.
    pub fn load(hmod: &Path) -> Result<TestGame, String> {
        static RUN: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "horizon-mod-test-{}-{}",
            std::process::id(),
            RUN.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let copy = dir.join(hmod.file_name().unwrap_or("mod.hmod".as_ref()));
        let mut game = TestGame {
            mods: Mods::default(),
            hmod: copy.clone(),
            dir,
            player: Some(default_player()),
            clock: Some(Clock {
                game_time: 0,
                day_time: 0,
            }),
            players: Vec::new(),
            motions: Vec::new(),
            states: Vec::new(),
            looks: Vec::new(),
            names: Vec::new(),
            tick: 0,
            world: Default::default(),
            block_entities: Default::default(),
            block_items: Default::default(),
            block_energy: Default::default(),
            block_tanks: Default::default(),
            power: Default::default(),
            block_menu_data: Default::default(),
            block_transmitters: Default::default(),
            multiblock_seen: Default::default(),
            mobs: Vec::new(),
            inventories: Vec::new(),
            claimed: None,
            block_use: None,
            damage_result: None,
            hit_damage_result: None,
            release_result: None,
            entity_used_result: None,
            entity_damage_result: None,
            mod_entities: Vec::new(),
            entity_kinds: Vec::new(),
            next_mod_entity: 1000,
            dropped: Vec::new(),
            entity_goals: Default::default(),
            can_place_result: None,
            packed: Default::default(),
            crafts: Vec::new(),
            stack_sizes: Default::default(),
            hardness: Default::default(),
        };
        copy_mod(hmod, &copy)?;
        game.start()?;
        Ok(game)
    }

    fn start(&mut self) -> Result<(), String> {
        if let Ok(package) = crate::manifest::Package::read(&self.hmod)
            && let Ok(content) = crate::Content::read(&self.hmod, &package.manifest)
        {
            self.entity_kinds = content.entity_kinds.clone();
        }
        self.mods = Mods::load(std::slice::from_ref(&self.hmod), |_| true, |_| false);
        match &self.mods.entries()[0].status {
            Status::Failed(reason) => Err(format!("the mod didn't start: {reason}")),
            _ if !self.mods.is_running() => Err("the mod has no code to run".into()),
            _ => Ok(()),
        }
    }

    /// Stops the mod and starts it again, as a rebuild does in the game: its
    /// memory starts over, its `storage` stays.
    pub fn reload(&mut self) -> Result<(), String> {
        self.mods = Mods::default();
        self.start()
    }

    fn snapshot(&mut self) {
        self.mods.set_snapshot(Snapshot {
            player: self.player.clone(),
            clock: self.clock,
            players: self.players.clone(),
            motions: self.motions.clone(),
            states: self.states.clone(),
            looks: self.looks.clone(),
            mobs: self.mobs.clone(),
            mobs_dimension: "minecraft:overworld".into(),
            inventories: self.inventories.clone(),
            // The GUI of a 1280 x 720 window at scale 3, as the trial run's.
            gui_size: (427.0, 240.0),
            ..Snapshot::default()
        });
    }
    /// The id of the (first) mod under test.
    pub fn mod_id(&self) -> String {
        self.mods.entries()[0]
            .manifest
            .as_ref()
            .map_or_else(String::new, |m| m.id.clone())
    }
    /// Whether the mod is a server mod.
    pub fn server(&self) -> bool {
        self.mods.runs_server()
    }

    /// `n` game ticks; each moves the clock one tick on, as the game does.
    pub fn tick(&mut self, n: u64) {
        for _ in 0..n {
            self.tick += 1;
            if let Some(clock) = &mut self.clock {
                clock.game_time += 1;
                clock.day_time += 1;
            }
            self.snapshot();
            self.mods.tick(self.tick);
            self.mods.server_tick(self.tick);
            self.tick_block_entities();
            self.tick_mod_entities();
        }
    }
    /// `entities-tick` with the mod's entities, and what it answers applied (as the game does: data, slots, energy, skin, name, a teleport, a removal).
    fn tick_mod_entities(&mut self) {
        if self.mod_entities.is_empty() {
            return;
        }
        if !self.mods.hears_mod_entities() {
            return;
        }
        let list = self.mod_entities.clone();
        for u in self.mods.entities_tick(self.tick, &list) {
            let Some(i) = self.mod_entities.iter().position(|e| e.id == u.id && e.dimension == u.dimension) else {
                continue;
            };
            if u.remove {
                self.mod_entities.remove(i);
                continue;
            }
            let slots = self.entity_kinds.iter().find(|k| k.id == self.mod_entities[i].kind).map_or(0, |k| usize::from(k.container_slots));
            let skins: Vec<String> = self
                .entity_kinds
                .iter()
                .find(|k| k.id == self.mod_entities[i].kind)
                .map(|k| k.skins.iter().map(|s| s.id.clone()).collect())
                .unwrap_or_default();
            let e = &mut self.mod_entities[i];
            if let Some(d) = u.data {
                e.data = d;
            }
            if let Some(mut items) = u.items {
                items.resize(slots, None);
                e.items = items;
            }
            if let Some(energy) = u.energy {
                e.energy = energy;
            }
            if let Some(skin) = u.skin.filter(|s| skins.contains(s)) {
                e.skin = skin;
            }
            if let Some(name) = u.name {
                e.name = name;
            }
            if let Some((x, y, z)) = u.teleport {
                (e.x, e.y, e.z) = (x, y, z);
                self.entity_goals.remove(&u.id);
            }
            if let Some((x, y, z, speed)) = u.path_to {
                self.entity_goals.insert(u.id, ([x, y, z], speed));
            }
            if u.stop {
                self.entity_goals.remove(&u.id);
            }
        }
        // Walking, over the script's blocks (a cell with a block that is not air stops it), as the game does.
        let world = self.world.clone();
        let solid = |x: i32, y: i32, z: i32| world.get(&(x, y, z)).is_some_and(|s| !s.starts_with("minecraft:air"));
        for e in &mut self.mod_entities {
            let Some((goal, mult)) = self.entity_goals.get(&e.id).copied() else { continue };
            let Some(kind) = self.entity_kinds.iter().find(|k| k.id == e.kind) else { continue };
            let (mut pos, mut yaw) = ([e.x, e.y, e.z], e.yaw);
            let step = f64::from(kind.speed) * f64::from(mult) * crate::entity_state::WALK_FACTOR;
            let r = crate::entity_state::walk(&mut pos, &mut yaw, goal, step, f64::from(kind.height), &solid);
            (e.x, e.y, e.z, e.yaw) = (pos[0], pos[1], pos[2], yaw);
            if r != crate::entity_state::Walk::Moving {
                self.entity_goals.remove(&e.id);
            }
        }
    }
    /// Makes and drops block entities to match `world`, then runs
    /// `block-entities-tick` and keeps the bytes the mod returns.
    fn tick_block_entities(&mut self) {
        let kinds = self.mods.block_entity_kinds();
        if kinds.is_empty() {
            return;
        }
        let of = |state: &str| {
            let block = state.split('[').next().unwrap_or("");
            kinds.iter().any(|k| k.block == block)
        };
        self.block_entities
            .retain(|pos, _| self.world.get(pos).is_some_and(|s| of(s)));
        self.block_items
            .retain(|pos, _| self.world.get(pos).is_some_and(|s| of(s)));
        self.block_energy
            .retain(|pos, _| self.world.get(pos).is_some_and(|s| of(s)));
        let spec_of = |state: &str| {
            let block = state.split('[').next().unwrap_or("");
            kinds
                .iter()
                .find(|k| k.block == block)
                .and_then(|k| k.energy.clone())
        };
        self.block_tanks
            .retain(|pos, _| self.world.get(pos).is_some_and(|s| of(s)));
        self.block_menu_data
            .retain(|pos, _| self.world.get(pos).is_some_and(|s| of(s)));
        self.block_transmitters
            .retain(|pos, _| self.world.get(pos).is_some_and(|s| of(s)));
        let transmitter_of = |state: &str| {
            let block = state.split('[').next().unwrap_or("");
            kinds
                .iter()
                .find(|k| k.block == block)
                .and_then(|k| k.transmitter.clone())
        };
        for (pos, state) in &self.world {
            if transmitter_of(state).is_some() {
                self.block_transmitters.entry(*pos).or_insert([true; 6]);
            }
        }
        let tanks_of = |state: &str| {
            let block = state.split('[').next().unwrap_or("");
            kinds
                .iter()
                .find(|k| k.block == block)
                .map(|k| k.tanks.clone())
                .unwrap_or_default()
        };
        let six = |faces: &[crate::FlowSide]| {
            let mut f = [crate::FlowSide::None; 6];
            for (to, from) in f.iter_mut().zip(faces) {
                *to = *from;
            }
            f
        };
        for (pos, state) in &self.world {
            let specs = tanks_of(state);
            if !specs.is_empty() {
                self.block_tanks.entry(*pos).or_insert_with(|| {
                    specs.iter().map(|s| (String::new(), six(&s.faces))).collect()
                });
            }
        }
        let faces_of = |spec: &crate::EnergySpec| {
            let mut f = [crate::FlowSide::None; 6];
            for (to, from) in f.iter_mut().zip(&spec.faces) {
                *to = *from;
            }
            f
        };
        for (pos, state) in &self.world {
            if let Some(spec) = spec_of(state) {
                self.block_energy
                    .entry(*pos)
                    .or_insert((0, faces_of(&spec)));
            }
        }
        for (pos, state) in &self.world {
            if of(state) {
                self.block_entities.entry(*pos).or_default();
            }
        }
        let slots_of = |state: &str| {
            let block = state.split('[').next().unwrap_or("");
            kinds
                .iter()
                .find(|k| k.block == block)
                .and_then(|k| k.container.as_ref())
                .map_or(0, |c| c.slots as usize)
        };
        let mut list = Vec::new();
        for (&(x, y, z), data) in &self.block_entities {
            let state = self.world[&(x, y, z)].clone();
            let n = slots_of(&state);
            let held = self.block_items.entry((x, y, z)).or_default();
            held.resize(n, String::new());
            list.push(crate::BlockEntity {
                dimension: "minecraft:overworld".into(),
                pos: BlockPos { x, y, z },
                state,
                data: data.clone(),
                items: held.iter().map(|s| parse_stack(s)).collect(),
                menu_data: self.block_menu_data.get(&(x, y, z)).cloned().unwrap_or_default(),
                transmitter_faces: self
                    .block_transmitters
                    .get(&(x, y, z))
                    .map_or_else(Vec::new, |f| f.to_vec()),
                tanks: self
                    .block_tanks
                    .get(&(x, y, z))
                    .map_or_else(Vec::new, |v| v.iter().map(|(s, _)| parse_tank(s)).collect()),
                tank_faces: self
                    .block_tanks
                    .get(&(x, y, z))
                    .map_or_else(Vec::new, |v| v.iter().map(|(_, f)| f.to_vec()).collect()),
                energy: self.block_energy.get(&(x, y, z)).map_or(0, |e| e.0),
                energy_faces: self
                    .block_energy
                    .get(&(x, y, z))
                    .map_or_else(Vec::new, |e| e.1.to_vec()),
            });
        }
        let mut updates = self.mods.block_entities_tick(self.tick, &list);
        for (group, blocks) in self.mods.multiblock_groups() {
            let members: Vec<crate::multiblock::Member> = self
                .world
                .iter()
                .filter(|(_, s)| blocks.iter().any(|b| s.split('[').next() == Some(b.as_str())))
                .map(|(&(x, y, z), s)| crate::multiblock::Member {
                    pos: [x, y, z],
                    state: s.clone(),
                })
                .collect();
            let found = crate::multiblock::structures(&members);
            if self.multiblock_seen.changed("minecraft:overworld", &group, &found) {
                self.snapshot();
                updates.extend(self.mods.multiblock_changed("minecraft:overworld", &group, &found));
            }
        }
        for u in updates {
            let key = (u.pos.x, u.pos.y, u.pos.z);
            self.block_entities.insert(key, u.data);
            if let (Some(spec), Some(entry)) = (
                self.world.get(&key).and_then(|s| spec_of(s)),
                self.block_energy.get_mut(&key),
            ) {
                if let Some(e) = u.energy {
                    entry.0 = e.min(spec.capacity);
                }
                if let Some(f) = &u.energy_faces {
                    for (to, from) in entry.1.iter_mut().zip(f) {
                        *to = *from;
                    }
                }
            }
            if let (Some(tanks), Some(held)) = (&u.tanks, self.block_tanks.get_mut(&key)) {
                let specs = self.world.get(&key).map(|s| tanks_of(s)).unwrap_or_default();
                for (i, tank) in tanks.iter().enumerate().take(held.len()) {
                    let cap = specs.get(i).map_or(0, |s| s.capacity);
                    let amount = if tank.substance.is_empty() { 0 } else { tank.amount.min(cap) };
                    held[i].0 = if amount == 0 {
                        String::new()
                    } else {
                        format!("{}*{amount}", tank.substance)
                    };
                }
            }
            if let Some(d) = &u.menu_data {
                self.block_menu_data.insert(key, d.clone());
            }
            if let (Some(f), Some(entry)) = (&u.transmitter_faces, self.block_transmitters.get_mut(&key)) {
                for (to, from) in entry.iter_mut().zip(f) {
                    *to = *from;
                }
            }
            if let Some(items) = u.items {
                let n = self.block_items.get(&key).map_or(0, Vec::len);
                let mut text: Vec<String> = items.iter().map(|s| show_stack(s)).collect();
                text.resize(n, String::new());
                self.block_items.insert(key, text);
            }
        }
        let mut nodes: Vec<crate::energy::EnergyNode> = self
            .block_energy
            .iter()
            .filter_map(|(&(x, y, z), &(energy, faces))| {
                let spec = spec_of(self.world.get(&(x, y, z))?)?;
                Some(crate::energy::EnergyNode {
                    pos: [x, y, z],
                    energy: energy.min(spec.capacity),
                    capacity: spec.capacity,
                    max_receive: spec.max_receive,
                    max_extract: spec.max_extract,
                    faces,
                })
            })
            .collect();
        let mut tank_nodes: Vec<crate::flow::TankNode> = self
            .block_tanks
            .iter()
            .filter_map(|(&(x, y, z), held)| {
                let specs = tanks_of(self.world.get(&(x, y, z))?);
                let tanks = specs
                    .iter()
                    .zip(held)
                    .map(|(s, (text, faces))| {
                        let (substance, amount) = split_tank(text);
                        crate::flow::TankState {
                            kind: s.kind.clone(),
                            capacity: s.capacity,
                            max_rate: s.max_rate,
                            faces: *faces,
                            substance,
                            amount,
                            accepts: Vec::new(),
                        }
                    })
                    .collect();
                Some(crate::flow::TankNode { pos: [x, y, z], tanks })
            })
            .collect();
        let transmitters: Vec<crate::network::Transmitter> = self
            .block_transmitters
            .iter()
            .filter_map(|(&(x, y, z), &connect)| {
                let spec = transmitter_of(self.world.get(&(x, y, z))?)?;
                Some(crate::network::Transmitter {
                    pos: [x, y, z],
                    medium: match spec.medium {
                        crate::TransmitterMedium::Energy => crate::network::Medium::Energy,
                        crate::TransmitterMedium::Tank(kind) => crate::network::Medium::Tank(kind),
                        crate::TransmitterMedium::Items => crate::network::Medium::Items,
                    },
                    throughput: spec.throughput,
                    connect,
                })
            })
            .collect();
        crate::network::transfer_tanks(&transmitters, &mut tank_nodes);
        crate::flow::transfer(&mut tank_nodes);
        for n in tank_nodes {
            if let Some(held) = self.block_tanks.get_mut(&(n.pos[0], n.pos[1], n.pos[2])) {
                for (slot, t) in held.iter_mut().zip(&n.tanks) {
                    slot.0 = if t.amount == 0 {
                        String::new()
                    } else {
                        format!("{}*{}", t.substance, t.amount)
                    };
                }
            }
        }
        crate::network::transfer_energy(&transmitters, &mut nodes);
        crate::energy::transfer(&mut nodes);
        for n in nodes {
            if let Some(e) = self.block_energy.get_mut(&(n.pos[0], n.pos[1], n.pos[2])) {
                e.0 = n.energy;
            }
        }
    }
    /// `player` presses menu button `id` of the block entity at `x y z`, and
    /// the mod's update is kept as a tick's is. False when the mod answers
    /// with none or there is no entity there.
    pub fn press_button(&mut self, player: &ServerPlayer, pos: (i32, i32, i32), id: i32) -> bool {
        let Some(state) = self.world.get(&pos).cloned() else {
            return false;
        };
        if !self.block_entities.contains_key(&pos) {
            return false;
        }
        let entity = crate::BlockEntity {
            dimension: "minecraft:overworld".into(),
            pos: BlockPos { x: pos.0, y: pos.1, z: pos.2 },
            state,
            data: self.block_entities[&pos].clone(),
            items: self
                .block_items
                .get(&pos)
                .map_or_else(Vec::new, |v| v.iter().map(|s| parse_stack(s)).collect()),
            energy: self.block_energy.get(&pos).map_or(0, |e| e.0),
            energy_faces: self.block_energy.get(&pos).map_or_else(Vec::new, |e| e.1.to_vec()),
            tanks: self
                .block_tanks
                .get(&pos)
                .map_or_else(Vec::new, |v| v.iter().map(|(s, _)| parse_tank(s)).collect()),
            tank_faces: self
                .block_tanks
                .get(&pos)
                .map_or_else(Vec::new, |v| v.iter().map(|(_, f)| f.to_vec()).collect()),
            menu_data: self.block_menu_data.get(&pos).cloned().unwrap_or_default(),
            transmitter_faces: self
                .block_transmitters
                .get(&pos)
                .map_or_else(Vec::new, |f| f.to_vec()),
        };
        self.snapshot();
        let Some(u) = self.mods.menu_button(player, id, &entity) else {
            return false;
        };
        self.block_entities.insert(pos, u.data);
        if let Some(d) = u.menu_data {
            self.block_menu_data.insert(pos, d);
        }
        if let Some(items) = u.items {
            let text: Vec<String> = items.iter().map(|s| show_stack(s)).collect();
            self.block_items.insert(pos, text);
        }
        if let (Some(e), Some(entry)) = (u.energy, self.block_energy.get_mut(&pos)) {
            entry.0 = e;
        }
        // The tanks and their faces, as a tick's update sets them.
        let block = self.world.get(&pos).map(|s| s.split('[').next().unwrap_or("").to_owned()).unwrap_or_default();
        let specs = self
            .mods
            .block_entity_kinds()
            .iter()
            .find(|k| k.block == block)
            .map(|k| k.tanks.clone())
            .unwrap_or_default();
        if let Some(held) = self.block_tanks.get_mut(&pos) {
            if let Some(tanks) = &u.tanks {
                for (i, tank) in tanks.iter().enumerate().take(held.len()) {
                    let cap = specs.get(i).map_or(0, |s| s.capacity);
                    let amount = if tank.substance.is_empty() { 0 } else { tank.amount.min(cap) };
                    held[i].0 = if amount == 0 { String::new() } else { format!("{}*{amount}", tank.substance) };
                }
            }
            if let Some(faces) = &u.tank_faces {
                for (slot, f) in held.iter_mut().zip(faces) {
                    for (to, from) in slot.1.iter_mut().zip(f) {
                        *to = *from;
                    }
                }
            }
        }
        true
    }
    /// `n` frames, 1/60 s apart.
    pub fn frames(&mut self, n: u64) {
        self.snapshot();
        for _ in 0..n {
            self.mods.frame(1.0 / 60.0);
        }
    }
    /// A chat line the player receives.
    pub fn chat(&mut self, line: &str) {
        self.snapshot();
        self.mods.chat(line);
    }
    /// `/line` typed by the player (without the slash); false when the mod
    /// owns no such command.
    pub fn command(&mut self, line: &str) -> bool {
        self.snapshot();
        self.mods.command(line)
    }

    /// A player named `name` joins the server, standing where a script starts
    /// in survival. Their id is `00000000-0000-4000-8000-000000000001` for
    /// the first name that joined, `...0002` for the second, and so on.
    pub fn join(&mut self, name: &str) -> ServerPlayer {
        let n = match self.names.iter().position(|n| n == name) {
            Some(i) => i + 1,
            None => {
                self.names.push(name.to_owned());
                self.names.len()
            }
        };
        let p = default_player();
        let player = ServerPlayer {
            id: format!("00000000-0000-4000-8000-{n:012x}"),
            name: name.to_owned(),
            x: p.x,
            y: p.y,
            z: p.z,
            dimension: p.dimension,
            game_mode: GameMode::Survival,
            health: p.health,
        };
        self.players.push(player.clone());
        self.snapshot();
        self.mods.player_joined(&player);
        player
    }
    /// The player named `name` leaves; false when no such player is on.
    pub fn leave(&mut self, name: &str) -> bool {
        let Some(i) = self.players.iter().position(|p| p.name == name) else {
            return false;
        };
        let player = self.players.remove(i);
        self.snapshot();
        self.mods.player_left(&player);
        true
    }
    /// A chat message `player` sends, for a server mod.
    pub fn player_chat(&mut self, player: &ServerPlayer, line: &str) {
        self.snapshot();
        self.mods.player_chat(player, line);
    }
    /// `/line` typed by `player`, for a server mod; false when the mod owns
    /// no such command.
    pub fn player_command(&mut self, player: &ServerPlayer, line: &str) -> bool {
        self.snapshot();
        self.mods.server_command(player, line)
    }
    /// `player` breaks (or places) `state` at `pos`.
    pub fn block(&mut self, player: &ServerPlayer, pos: BlockPos, state: &str, placed: bool) {
        self.snapshot();
        if placed {
            self.mods.block_placed(player, pos, state);
        } else {
            self.mods.block_broken(player, pos, state);
        }
    }

    /// The player's motion, made standing still with no experience if the
    /// player had none yet.
    pub fn motion_of(&mut self, id: &str) -> &mut Motion {
        let i = match self.motions.iter().position(|(p, _)| p == id) {
            Some(i) => i,
            None => {
                let still = Motion {
                    sneaking: false,
                    on_ground: true,
                    jumps: 0,
                    xp_level: 0,
                    xp_progress: 0.0,
                };
                self.motions.push((id.to_owned(), still));
                self.motions.len() - 1
            }
        };
        &mut self.motions[i].1
    }

    /// The scratch copy the mod runs from; its `storage` is beside it.
    pub fn hmod(&self) -> &Path {
        &self.hmod
    }
    /// The commands the mod registered.
    pub fn commands(&self) -> Vec<String> {
        let server = self.mods.server_commands().map(|c| c.name.as_str());
        self.mods
            .commands()
            .chain(server)
            .map(str::to_owned)
            .collect()
    }
    pub fn panel(&self) -> Option<&Panel> {
        self.mods.panels().next()
    }
    pub fn status(&self) -> &Status {
        &self.mods.entries()[0].status
    }
    /// What the mod showed since the last call.
    pub fn drain(&mut self) -> Vec<Output> {
        self.mods.drain()
    }
}

impl Drop for TestGame {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A `.hmod` file, or a mod folder (`horizon-mod.toml` + `mod.wasm`).
fn copy_mod(from: &Path, to: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", from.display());
    if !from.is_dir() {
        return std::fs::copy(from, to).map(drop).map_err(err);
    }
    std::fs::create_dir_all(to).map_err(err)?;
    for entry in std::fs::read_dir(from).map_err(err)? {
        let entry = entry.map_err(err)?;
        if entry.file_type().map_err(err)?.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(err)?;
        }
    }
    Ok(())
}

/// A test in TOML: where a client mod's player starts, then steps run in order.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Script {
    #[serde(default)]
    pub player: PlayerSet,
    #[serde(default)]
    pub clock: ClockSet,
    /// Capability `recipe-types`: recipes of mods' own types (`[[recipe]]`
    /// with an `id` and the file's `json`; the type is the JSON's `type`),
    /// the item tags that resolve their ingredients (`[tags]`, each one
    /// `"c:dusts/redstone" = ["minecraft:redstone"]`) and the items the game
    /// knows (`items`; none listed: all are).
    #[serde(default, rename = "recipe")]
    pub recipes: Vec<RecipeSet>,
    /// Cooking recipes (`[[cooking]]`: `kind`, `inputs`, `output`, `count`, `time`).
    #[serde(default, rename = "cooking")]
    pub cooking: Vec<CookingSet>,
    #[serde(default)]
    pub tags: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub items: Vec<String>,
    /// The ticks items burn in a furnace (`[burn_times]`, `"minecraft:coal" = 1600`).
    #[serde(default)]
    pub burn_times: std::collections::BTreeMap<String, u32>,
    /// Crafting recipes (`[[craft]]`: `id`, the nine cells of `grid`, `output`, `remaining`) the world's crafting answers from.
    #[serde(default, rename = "craft")]
    pub crafts: Vec<CraftSet>,
    /// Stack sizes (`[stack-sizes]`, `"minecraft:stick" = 64`).
    #[serde(default)]
    pub stack_sizes: std::collections::BTreeMap<String, u32>,
    /// The destroy time of blocks (`[hardness]`, `"minecraft:stone" = 1.5`); a block not listed is 0.
    #[serde(default)]
    pub hardness: std::collections::BTreeMap<String, f32>,
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
}

/// A cooking recipe in a test.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookingSet {
    #[serde(default = "smelting")]
    kind: String,
    inputs: Vec<String>,
    output: String,
    #[serde(default = "one")]
    count: u32,
    #[serde(default = "two_hundred")]
    time: u32,
}
fn smelting() -> String {
    "smelting".into()
}
fn one() -> u32 {
    1
}
fn two_hundred() -> u32 {
    200
}

/// A recipe file of a mod's own type in a test.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeSet {
    id: String,
    json: String,
}

/// Fields of the player to change; the others keep their values. A server
/// mod's players have no `food` (their `yaw` and `pitch` are what `get-look` answers), and a client mod's player
/// no `game-mode`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PlayerSet {
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    yaw: Option<f32>,
    pitch: Option<f32>,
    health: Option<f32>,
    food: Option<u32>,
    dimension: Option<String>,
    game_mode: Option<Mode>,
}

/// `players.game-mode` as a script spells it.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}
impl From<Mode> for GameMode {
    fn from(m: Mode) -> GameMode {
        match m {
            Mode::Survival => GameMode::Survival,
            Mode::Creative => GameMode::Creative,
            Mode::Adventure => GameMode::Adventure,
            Mode::Spectator => GameMode::Spectator,
        }
    }
}

/// Fields of what a player is doing (capability `player-control`) to change;
/// the others keep their values.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct StateSet {
    velocity: Option<[f64; 3]>,
    jumping: Option<bool>,
    flying: Option<bool>,
    fall_flying: Option<bool>,
    in_water: Option<bool>,
    in_lava: Option<bool>,
    air: Option<i32>,
    fire_ticks: Option<i32>,
    food: Option<u32>,
    saturation: Option<f32>,
    /// `"minecraft:poison 100 0"`: id, ticks, amplifier.
    effects: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionSet {
    sneaking: Option<bool>,
    on_ground: Option<bool>,
    jumps: Option<u32>,
    xp_level: Option<i32>,
    xp_progress: Option<f32>,
}

fn split_tank(text: &str) -> (String, u64) {
    match text.split_once('*') {
        Some((s, n)) => (s.to_owned(), n.parse().unwrap_or(0)),
        None => (String::new(), 0),
    }
}
fn parse_tank(text: &str) -> crate::Tank {
    let (substance, amount) = split_tank(text);
    crate::Tank { substance, amount }
}
/// `"minecraft:stick*2"`, or with components `"minecraft:stick*2{\"mymod:energy\":50L}"`.
fn parse_stack(text: &str) -> Option<crate::ModItemStack> {
    let (item, rest) = text.split_once('*')?;
    let split = rest.find('{').unwrap_or(rest.len());
    Some(crate::ModItemStack {
        item: item.into(),
        count: rest[..split].parse().ok()?,
        components: rest[split..].to_owned(),
    })
}
fn show_stack(s: &Option<crate::ModItemStack>) -> String {
    s.as_ref().map_or(String::new(), |s| {
        format!("{}*{}{}", s.item, s.count, s.components)
    })
}

/// The slots of a block entity a step sets or expects.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockItemsSet {
    x: i32,
    y: i32,
    z: i32,
    /// `"minecraft:stone*3"`, or `""` for an empty slot.
    slots: Vec<String>,
}

/// Bytes sent on a mod channel in a step, or expected.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetSet {
    channel: String,
    #[serde(default)]
    data: Vec<u8>,
}

/// A chunk coming into the Overworld in a step.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkSet {
    x: i32,
    z: i32,
    #[serde(default)]
    new: bool,
}

/// `state` (`mymod:machine[facing=north,lit=false]`) with `property` set to `value`
/// (added when it had none).
fn with_property(state: &str, property: &str, value: &str) -> String {
    let (name, props) = match state.split_once('[') {
        Some((n, p)) => (n, p.trim_end_matches(']')),
        None => (state, ""),
    };
    let mut list: Vec<(String, String)> = props
        .split(',')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    match list.iter_mut().find(|(k, _)| k == property) {
        Some(e) => e.1 = value.to_owned(),
        None => list.push((property.to_owned(), value.to_owned())),
    }
    let body: Vec<String> = list.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{name}[{}]", body.join(","))
}

/// A stack used or hit with in a step: `"minecraft:stick*1"`, components after the count.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceSet {
    pos: [i32; 3],
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageSet {
    #[serde(rename = "type")]
    damage_type: String,
    amount: f32,
    causing: Option<u32>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HitDamageSet {
    stack: String,
    kind: String,
    damage: f32,
}

/// A right click on one of the mod's entities.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityUseSet {
    id: u32,
    #[serde(default)]
    sneaking: bool,
}

/// A blow on one of the mod's entities (FORK-10 e): the damage the weapon makes, before the mod's say.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityDamageSet {
    id: u32,
    amount: f32,
}

/// A bow-like item let go: `item-released`, and when the mod lets the shot stand, `item-shot` with the arrows' ids.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSet {
    stack: String,
    ticks: u32,
    #[serde(default)]
    hand: Option<String>,
    /// The entity ids of the arrows the game made.
    #[serde(default)]
    projectiles: Vec<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UseBlockSet {
    pos: [i32; 3],
    stack: Option<String>,
    #[serde(default)]
    sneaking: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UseSet {
    stack: String,
    /// The block clicked; none: in the air.
    pos: Option<[i32; 3]>,
    /// For `hit`: the entity and its kind.
    entity: Option<u32>,
    kind: Option<String>,
}

/// A mob a step puts in the Overworld (replacing the last list).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MobSet {
    id: u32,
    kind: String,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    #[serde(default)]
    z: f64,
    #[serde(default = "twenty")]
    health: f32,
}
fn twenty() -> f32 {
    20.0
}

/// The answer to a spawn ticket a step gives the mod (no `id`: it failed).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnedSet {
    ticket: u64,
    id: Option<u32>,
}

/// A menu button a step presses.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressSet {
    x: i32,
    y: i32,
    z: i32,
    id: i32,
}

/// The data slots of a block entity's menu a step expects.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuDataSet {
    x: i32,
    y: i32,
    z: i32,
    data: Vec<i32>,
}

/// The tanks of a block entity a step sets or expects.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockTanksSet {
    x: i32,
    y: i32,
    z: i32,
    /// `"minecraft:water*500"` for each tank, `""` for an empty one.
    tanks: Vec<String>,
}

/// The redstone signal a step puts at a block.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerSet {
    x: i32,
    y: i32,
    z: i32,
    level: u8,
}

/// The stored energy of a block entity a step sets or expects.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockEnergySet {
    x: i32,
    y: i32,
    z: i32,
    energy: u64,
}

/// A block entity's bytes a step expects.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockEntityExpect {
    x: i32,
    y: i32,
    z: i32,
    data: Vec<u8>,
}

/// A block a player breaks or places.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockSet {
    x: i32,
    y: i32,
    z: i32,
    state: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ClockSet {
    game_time: Option<u64>,
    day_time: Option<u64>,
}

/// One step. Its parts happen in this order: `reload`, `title-screen`,
/// `blocks`, `join`, `player`, `motion`, `clock`, `ticks`, `frames`, `chat`, `command`,
/// `break`, `place`, `leave`; then the blocks the mod set are made, and
/// `expect` is checked against what the mod showed and changed during the
/// step. For a server mod, `player`, `chat`, `command`, `break`
/// and `place` are the doing of the player named by `as`, or of the first
/// player on.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Step {
    #[serde(default)]
    reload: bool,
    /// Leave the world: the mod reads no player and no clock until a step sets them.
    #[serde(default)]
    title_screen: bool,
    player: Option<PlayerSet>,
    /// Server mods: what the player is doing, for `player-control`.
    motion: Option<MotionSet>,
    /// Server mods (`player-control.get-state`): what the actor's body is doing, from now on.
    state: Option<StateSet>,
    clock: Option<ClockSet>,
    #[serde(default)]
    ticks: u64,
    #[serde(default)]
    frames: u64,
    chat: Option<String>,
    command: Option<String>,
    /// Server mods: a player joins, leaves, breaks or places a block.
    join: Option<String>,
    leave: Option<String>,
    #[serde(rename = "as")]
    actor: Option<String>,
    #[serde(rename = "break")]
    broken: Option<BlockSet>,
    place: Option<BlockSet>,
    /// Blocks of the Overworld before the rest of the step (capability
    /// `world`); every other block is air.
    #[serde(default)]
    blocks: Vec<BlockSet>,
    /// Slots of block entities before the ticks (the entity must exist: set
    /// its block in an earlier step).
    #[serde(default)]
    set_items: Vec<BlockItemsSet>,
    /// Server mods, capability `player-items`: what the actor carries from now
    /// on, slot 0 first (`"minecraft:stone*3"`, `""` for an empty slot).
    inventory: Option<Vec<String>>,
    /// Server mods, capability `chunks`: a chunk comes into the Overworld after the ticks.
    chunk: Option<ChunkSet>,
    /// Server mods: the actor uses a stack (right click) after the ticks.
    use_item: Option<UseSet>,
    /// Server mods: the actor hits an entity holding a stack after the ticks.
    hit_with: Option<UseSet>,
    /// Server mods (FORK-29): the actor right-clicks the block at `pos` after the ticks, holding `stack` (none: an empty hand).
    use_block: Option<UseBlockSet>,
    /// Server mods: the actor is about to be hurt.
    damage: Option<DamageSet>,
    /// Server mods (FORK-35): the actor placed a block of the mod at a position (`pos` and `state`).
    place_mod_block: Option<PlaceSet>,
    /// Server mods: the actor's melee blow is worked out.
    hit_damage: Option<HitDamageSet>,
    /// Server mods: the actor let go of a bow-like item.
    release_item: Option<ReleaseSet>,
    /// Server mods (FORK-10 d): the actor right-clicks one of the mod's entities.
    use_mod_entity: Option<EntityUseSet>,
    /// Server mods (FORK-10 f): the items lying in the world from this step on (they replace the earlier ones).
    dropped: Option<Vec<DroppedSet>>,
    /// Server mods (FORK-10 e): a blow on one of the mod's entities.
    damage_mod_entity: Option<EntityDamageSet>,
    /// Server mods, capability `entities`: the mobs of the Overworld from now on.
    mobs: Option<Vec<MobSet>>,
    /// Server mods: a spawn ticket is answered after the ticks.
    spawned: Option<SpawnedSet>,
    /// Server mods: the actor's client half sends bytes after the ticks.
    net: Option<NetSet>,
    /// Client mods: the server half sends bytes after the ticks.
    net_from_server: Option<NetSet>,
    /// Server mods: the actor presses a menu button after the ticks.
    press: Option<PressSet>,
    /// Tanks of block entities before the ticks (as `set-items`).
    #[serde(default)]
    set_tanks: Vec<BlockTanksSet>,
    /// Redstone signals to put at positions (0 takes it off).
    #[serde(default)]
    power: Vec<PowerSet>,
    /// Energy of block entities before the ticks (as `set-items`).
    #[serde(default)]
    set_energy: Vec<BlockEnergySet>,
    #[serde(default)]
    expect: Expect,
}

/// What a step checks; a part left out is not checked.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Expect {
    /// The commands the mod owns.
    commands: Option<Vec<String>>,
    /// Every chat line shown during the step, in order.
    chat: Option<Vec<String>>,
    /// The command's feedback, and its red error.
    feedback: Option<String>,
    error: Option<String>,
    /// Server mods: every `players.tell` as `"Name: text"`, and every
    /// `players.broadcast`, in order.
    told: Option<Vec<String>>,
    broadcast: Option<Vec<String>>,
    /// The panel's lines (`[]`: no panel) and its corner.
    panel: Option<Vec<String>>,
    corner: Option<String>,
    /// Blocks of the Overworld after the step, the mod's changes made
    /// (capability `world`).
    blocks: Option<Vec<BlockSet>>,
    /// Server mods, capability `chunks`: every `set-forced` as `"dimension x z true"`.
    chunk_forced: Option<Vec<String>>,
    /// Server mods: whether the mod took the step's `use-item`.
    claimed: Option<bool>,
    /// Server mods (FORK-29): what the mod answered to the step's `use-block`: `default`, `pass` or `consume`.
    block_use: Option<String>,
    /// Server mods: what the mod answered to the step's `damage` (the damage to go on with).
    damage_result: Option<f32>,
    /// Server mods (FORK-35): whether the mod let the step's `place` stand.
    can_place: Option<bool>,
    /// Server mods (FORK-35): the redirecting blocks the mods declared, as `"block flag-at pos-at"`.
    redirects: Option<Vec<String>>,
    /// Server mods: what the mod answered to the step's `hit-damage`.
    hit_damage_result: Option<f32>,
    /// Server mods: whether the mod let the step's `release-item` shoot.
    release_result: Option<bool>,
    /// Server mods (FORK-10 d): whether the mod took the step's `use-mod-entity` click.
    mod_entity_used: Option<bool>,
    /// Server mods (FORK-10 f): the items still lying in the world after the step, as `"id item*count"`.
    dropped_left: Option<Vec<String>>,
    /// Server mods (FORK-10 f): the slots of the mod's entities after the step, as `"id slot=item*count,slot=..."`.
    mod_entity_items: Option<Vec<String>>,
    /// Server mods (FORK-10 e): the damage the mod let the step's `damage-mod-entity` apply.
    mod_entity_damage: Option<f32>,
    /// Server mods, capability `player-items`: every request, as `"set-slot Name 3 stack"`
    /// (`-` for an empty slot), `"hurt Name type amount"` and `"effect Name id ticks level"`.
    player_items: Option<Vec<String>>,
    /// Server mods, capability `entities`: every request, as `"spawn-mob kind x y z #ticket"`,
    /// `"summon kind x y z #ticket nbt"`, `"move id x y z yaw pitch"`, `"display id"`,
    /// `"remove-entity id"`, `"teleport-mob id x y z"`, `"push-mob id x y z"`,
    /// `"health id value"`, `"kill-mob id"` and `"remove-mob id"`.
    entity_commands: Option<Vec<String>>,
    /// Server mods, capability `net`: every `net.send` as `"Name: channel [bytes]"`.
    net_sent: Option<Vec<String>>,
    /// Client mods, capability `net-client`: every `send-to-server` as `"channel [bytes]"`.
    net_to_server: Option<Vec<String>>,
    /// Server mods: the data slots of block entities' menus after the step.
    block_menu_data: Option<Vec<MenuDataSet>>,
    /// Server mods: the tanks of block entities after the step.
    block_tanks: Option<Vec<BlockTanksSet>>,
    /// Server mods: the stored energy of block entities after the step.
    block_energy: Option<Vec<BlockEnergySet>>,
    /// Server mods: the slots of block entities after the step.
    block_items: Option<Vec<BlockItemsSet>>,
    /// Server mods: the saved bytes of block entities after the step.
    block_entities: Option<Vec<BlockEntityExpect>>,
    /// Server mods, capability `player-control`: every `teleport` as
    /// `"Name: dimension x y z yaw pitch"` (`~` for a yaw or pitch left as it is),
    /// every `take-xp` as `"Name: points"` and every `play-sound` as
    /// `"dimension sound x y z"`, in order.
    teleports: Option<Vec<String>>,
    xp_taken: Option<Vec<String>>,
    sounds: Option<Vec<String>>,
    /// Server mods (FORK-10): the mod's entities after the step, as `"id kind x y z energy=E data=[bytes] skin=S name=N"` (the coordinates with 3 decimals, name `-` for none).
    mod_entities: Option<Vec<String>>,
    /// Server mods (FORK-20): every `set-menu-text`, as `"dimension x y z text|text"`.
    menu_text: Option<Vec<String>>,
    /// Server mods (FORK-34): every `set-capabilities-enabled`, as `"dimension x y z items=true fluids=true energy=false"`.
    capabilities: Option<Vec<String>>,
    /// Server mods (FORK-30): every `spawn-particles`, as `"dimension particle x y z count"`.
    particles: Option<Vec<String>>,
    /// The mod is turned off during this step, for a reason holding this text.
    /// Without it, a mod that stops fails the test.
    stopped: Option<String>,
}

impl Script {
    pub fn parse(text: &str) -> Result<Script, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Runs every step on `game`; the error names the first step that failed.
    pub fn run(&self, game: &mut TestGame) -> Result<(), String> {
        game.crafts = self.crafts.clone();
        game.stack_sizes = self.stack_sizes.clone();
        game.hardness = self.hardness.clone();
        if let Some(p) = &mut game.player {
            self.player.apply(p);
        }
        if let Some(c) = &mut game.clock {
            self.clock.apply(c);
        }
        if !self.recipes.is_empty() || !self.cooking.is_empty() || !self.tags.is_empty() || !self.items.is_empty() {
            let mut data = crate::recipe_types::RecipeData::default();
            for r in &self.recipes {
                let kind = serde_json::from_str::<serde_json::Value>(&r.json)
                    .ok()
                    .and_then(|v| v["type"].as_str().map(str::to_owned))
                    .ok_or_else(|| format!("recipe {}: its json has no type", r.id))?;
                data.recipes
                    .entry(kind)
                    .or_default()
                    .push((r.id.clone(), r.json.clone()));
            }
            data.cooking = self
                .cooking
                .iter()
                .map(|c| crate::recipe_types::Cooking {
                    kind: c.kind.clone(),
                    inputs: c.inputs.clone(),
                    output: c.output.clone(),
                    count: c.count,
                    experience: 0.0,
                    time: c.time,
                })
                .collect();
            data.tags = self.tags.clone().into_iter().collect();
            data.items = self.items.iter().cloned().collect();
            data.burn_times = self.burn_times.iter().map(|(k, v)| (k.clone(), *v)).collect();
            game.mods.set_recipe_data(std::sync::Arc::new(data));
        }
        for (i, step) in self.steps.iter().enumerate() {
            step.run(game)
                .map_err(|e| format!("step {} ({}): {e}", i + 1, step.describe()))?;
        }
        Ok(())
    }
}

impl PlayerSet {
    fn apply(&self, p: &mut Player) {
        let set = |to: &mut f64, v: Option<f64>| *to = v.unwrap_or(*to);
        set(&mut p.x, self.x);
        set(&mut p.y, self.y);
        set(&mut p.z, self.z);
        p.yaw = self.yaw.unwrap_or(p.yaw);
        p.pitch = self.pitch.unwrap_or(p.pitch);
        p.health = self.health.unwrap_or(p.health);
        p.food = self.food.unwrap_or(p.food);
        if let Some(d) = &self.dimension {
            p.dimension.clone_from(d);
        }
    }
    fn apply_server(&self, p: &mut ServerPlayer) {
        let set = |to: &mut f64, v: Option<f64>| *to = v.unwrap_or(*to);
        set(&mut p.x, self.x);
        set(&mut p.y, self.y);
        set(&mut p.z, self.z);
        p.health = self.health.unwrap_or(p.health);
        p.game_mode = self.game_mode.map_or(p.game_mode, GameMode::from);
        if let Some(d) = &self.dimension {
            p.dimension.clone_from(d);
        }
    }
}

impl MotionSet {
    fn apply(&self, m: &mut Motion) {
        m.sneaking = self.sneaking.unwrap_or(m.sneaking);
        m.on_ground = self.on_ground.unwrap_or(m.on_ground);
        m.jumps = self.jumps.unwrap_or(m.jumps);
        m.xp_level = self.xp_level.unwrap_or(m.xp_level);
        m.xp_progress = self.xp_progress.unwrap_or(m.xp_progress);
    }
}

/// Experience points needed to leave `level` (`Player.getXpNeededForNextLevel`).
fn xp_needed(level: i32) -> i32 {
    if level >= 30 {
        112 + (level - 30) * 9
    } else if level >= 15 {
        37 + (level - 15) * 5
    } else {
        7 + level * 2
    }
}

/// `Player.giveExperiencePoints(-points)`, as the game does it.
fn take_xp(m: &mut Motion, points: u32) {
    let amount = -i64::from(points);
    m.xp_progress += amount as f32 / xp_needed(m.xp_level) as f32;
    while m.xp_progress < 0.0 {
        let f = m.xp_progress * xp_needed(m.xp_level) as f32;
        if m.xp_level > 0 {
            m.xp_level -= 1;
            m.xp_progress = 1.0 + f / xp_needed(m.xp_level) as f32;
        } else {
            m.xp_level = 0;
            m.xp_progress = 0.0;
        }
    }
}

impl ClockSet {
    fn apply(&self, c: &mut Clock) {
        c.game_time = self.game_time.unwrap_or(c.game_time);
        c.day_time = self.day_time.unwrap_or(c.day_time);
    }
}

impl Step {
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.reload {
            parts.push("reload".to_string());
        }
        let count = |n: u64, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
        if self.ticks > 0 {
            parts.push(count(self.ticks, "tick"));
        }
        if self.frames > 0 {
            parts.push(count(self.frames, "frame"));
        }
        if let Some(line) = &self.chat {
            parts.push(format!("chat {line:?}"));
        }
        if let Some(line) = &self.command {
            parts.push(format!("/{line}"));
        }
        if let Some(name) = &self.join {
            parts.insert(0, format!("{name} joins"));
        }
        for (what, b) in [("breaks", &self.broken), ("places", &self.place)] {
            if let Some(b) = b {
                parts.push(format!("{what} {} at {} {} {}", b.state, b.x, b.y, b.z));
            }
        }
        if let Some(name) = &self.leave {
            parts.push(format!("{name} leaves"));
        }
        if parts.is_empty() {
            parts.push("checks".into());
        }
        parts.join(", ")
    }

    fn run(&self, game: &mut TestGame) -> Result<(), String> {
        if self.reload {
            game.reload()?;
        }
        if self.title_screen {
            game.player = None;
            game.clock = None;
        }
        for b in &self.blocks {
            game.world.insert((b.x, b.y, b.z), b.state.clone());
        }
        for p in &self.power {
            game.power.insert((p.x, p.y, p.z), p.level);
        }
        if let Some(list) = &self.dropped {
            game.dropped.clone_from(list);
        }
        for s in &self.set_tanks {
            if let Some(held) = game.block_tanks.get_mut(&(s.x, s.y, s.z)) {
                for (slot, text) in held.iter_mut().zip(&s.tanks) {
                    slot.0.clone_from(text);
                }
            }
        }
        for s in &self.set_energy {
            if let Some(e) = game.block_energy.get_mut(&(s.x, s.y, s.z)) {
                e.0 = s.energy;
            }
        }
        for s in &self.set_items {
            game.block_items.insert((s.x, s.y, s.z), s.slots.clone());
        }
        game.mods
            .lend_world(Box::new(ScriptWorld(game.world.clone(), game.power.clone(), game.crafts.clone(), game.stack_sizes.clone(), game.hardness.clone(), game.dropped.clone())));
        if let Some(name) = &self.join {
            game.join(name);
        }
        let server = game.server();
        let actor = |game: &TestGame| match &self.actor {
            Some(name) => game
                .players
                .iter()
                .position(|p| p.name == *name)
                .ok_or_else(|| format!("{name} is not on the server")),
            None => (!game.players.is_empty())
                .then_some(0)
                .ok_or_else(|| "no player is on the server: add join = \"Name\"".to_string()),
        };
        let acts = self.player.is_some()
            || self.inventory.is_some()
            || self.use_item.is_some()
            || self.use_block.is_some()
            || self.damage.is_some()
            || self.place_mod_block.is_some()
            || self.hit_damage.is_some()
            || self.release_item.is_some()
            || self.use_mod_entity.is_some()
            || self.damage_mod_entity.is_some()
            || self.hit_with.is_some()
            || self.press.is_some()
            || self.net.is_some()
            || self.motion.is_some()
            || self.state.is_some()
            || self.chat.is_some()
            || self.command.is_some()
            || self.broken.is_some()
            || self.place.is_some();
        let actor = if server && acts {
            Some(actor(game)?)
        } else {
            None
        };
        if let Some(set) = &self.player {
            match actor {
                Some(i) => {
                    set.apply_server(&mut game.players[i]);
                    if set.yaw.is_some() || set.pitch.is_some() {
                        let id = game.players[i].id.clone();
                        let at = match game.looks.iter().position(|(p, _)| *p == id) {
                            Some(at) => at,
                            None => {
                                game.looks.push((id, (0.0, 0.0)));
                                game.looks.len() - 1
                            }
                        };
                        let look = &mut game.looks[at].1;
                        look.0 = set.yaw.unwrap_or(look.0);
                        look.1 = set.pitch.unwrap_or(look.1);
                    }
                }
                None => set.apply(game.player.get_or_insert_with(default_player)),
            }
        }
        if let (Some(set), Some(i)) = (&self.state, actor) {
            let id = game.players[i].id.clone();
            let at = match game.states.iter().position(|(p, _)| *p == id) {
                Some(at) => at,
                None => {
                    game.states.push((id, crate::PlayerState {
                        velocity_x: 0.0, velocity_y: 0.0, velocity_z: 0.0, jumping: false, sprinting: false, flying: false, may_fly: false,
                        fall_flying: false, swimming: false, in_water: false, eyes_in_water: false, in_lava: false, air: 300, max_air: 300,
                        fire_ticks: 0, passenger: false, sleeping: false, food: 20, saturation: 5.0, effects: Vec::new(),
                    }));
                    game.states.len() - 1
                }
            };
            let s = &mut game.states[at].1;
            if let Some(v) = set.velocity {
                (s.velocity_x, s.velocity_y, s.velocity_z) = (v[0], v[1], v[2]);
            }
            s.jumping = set.jumping.unwrap_or(s.jumping);
            s.flying = set.flying.unwrap_or(s.flying);
            s.fall_flying = set.fall_flying.unwrap_or(s.fall_flying);
            s.in_water = set.in_water.unwrap_or(s.in_water);
            s.in_lava = set.in_lava.unwrap_or(s.in_lava);
            s.air = set.air.unwrap_or(s.air);
            s.fire_ticks = set.fire_ticks.unwrap_or(s.fire_ticks);
            s.food = set.food.unwrap_or(s.food);
            s.saturation = set.saturation.unwrap_or(s.saturation);
            if let Some(list) = &set.effects {
                s.effects = list
                    .iter()
                    .filter_map(|t| {
                        let mut p = t.split_whitespace();
                        Some(crate::EffectState { id: p.next()?.into(), duration: p.next()?.parse().ok()?, amplifier: p.next()?.parse().ok()? })
                    })
                    .collect();
            }
        }
        if let (Some(set), Some(i)) = (&self.motion, actor) {
            let id = game.players[i].id.clone();
            set.apply(game.motion_of(&id));
        }
        if let (Some(list), Some(i)) = (&self.inventory, actor) {
            let id = game.players[i].id.clone();
            let slots = list.iter().map(|s| parse_stack(s)).collect();
            game.inventories.retain(|(p, _)| *p != id);
            game.inventories.push((id, slots));
        }
        if let Some(set) = &self.clock {
            set.apply(game.clock.get_or_insert(Clock {
                game_time: 0,
                day_time: 0,
            }));
        }
        if let Some(mobs) = &self.mobs {
            game.mobs = mobs
                .iter()
                .map(|m| crate::MobInfo {
                    id: m.id,
                    kind: m.kind.clone(),
                    pos: [m.x, m.y, m.z],
                    yaw: 0.0,
                    health: m.health,
                    max_health: 20.0,
                })
                .collect();
        }
        game.tick(self.ticks);
        game.frames(self.frames);
        let who = actor.map(|i| game.players[i].clone());
        if let Some(c) = &self.chunk {
            game.snapshot();
            game.mods.chunk_loaded("minecraft:overworld", [c.x, c.z], c.new);
        }
        if let (Some(u), Some(who)) = (&self.use_item, &who) {
            game.snapshot();
            let stack = parse_stack(&u.stack).ok_or("use-item: a stack is item*count")?;
            game.claimed = Some(game.mods.item_used(who, &stack, u.pos));
        }
        if let (Some(d), Some(who)) = (&self.damage, &who) {
            game.snapshot();
            let t = d.damage_type.as_str();
            let has = |tag: &str| d.tags.iter().any(|x| x == tag);
            let source = crate::DamageSource {
                damage_type: t.to_owned(),
                causing: d.causing,
                is_magic: matches!(t, "minecraft:magic" | "minecraft:indirect_magic"),
                is_fire: has("minecraft:is_fire"),
                is_fall: has("minecraft:is_fall"),
                is_explosion: has("minecraft:is_explosion"),
                tags: d.tags.clone(),
            };
            game.damage_result = Some(game.mods.player_damaged(who, &source, d.amount));
        }
        if let (Some(p), Some(who)) = (&self.place_mod_block, &who) {
            game.snapshot();
            game.can_place_result = Some(game.mods.can_place(who, p.pos, &p.state));
        }
        if let (Some(h), Some(who)) = (&self.hit_damage, &who) {
            game.snapshot();
            let stack = parse_stack(&h.stack).ok_or("hit-damage: a stack is item*count")?;
            game.hit_damage_result = Some(game.mods.item_hit_damage(who, &stack, &h.kind, h.damage));
        }
        if let Some(d) = &self.damage_mod_entity {
            game.snapshot();
            let entity = game.mod_entities.iter().find(|e| e.id == d.id).cloned().ok_or("damage-mod-entity: no such entity")?;
            let source = crate::DamageSource {
                damage_type: "minecraft:player_attack".into(),
                causing: None,
                is_magic: false,
                is_fire: false,
                is_fall: false,
                is_explosion: false,
                tags: vec!["minecraft:is_player_attack".into()],
            };
            let applied = game.mods.mod_entity_damaged(&entity, &source, d.amount);
            game.entity_damage_result = Some(applied);
            let (health, dead) = crate::entity_state::after_damage(entity.health, applied);
            if let Some(m) = game.mod_entities.iter_mut().find(|m| m.id == d.id) {
                m.health = health;
            }
            if dead {
                let mut last = entity;
                last.health = 0.0;
                game.mods.mod_entity_died(&last);
                game.mod_entities.retain(|m| m.id != d.id);
                game.entity_goals.remove(&d.id);
            }
        }
        if let (Some(u), Some(who)) = (&self.use_mod_entity, &who) {
            game.snapshot();
            let entity = game.mod_entities.iter().find(|e| e.id == u.id).cloned().ok_or("use-mod-entity: no such entity")?;
            game.entity_used_result = Some(game.mods.mod_entity_used(who, &entity, u.sneaking));
        }
        if let (Some(r), Some(who)) = (&self.release_item, &who) {
            game.snapshot();
            let stack = parse_stack(&r.stack).ok_or("release-item: a stack is item*count")?;
            let hand = r.hand.as_deref().unwrap_or("main");
            let allowed = game.mods.item_released(who, &stack, hand, r.ticks);
            if allowed {
                game.mods.item_shot(who, &stack, &r.projectiles);
            }
            game.release_result = Some(allowed);
        }
        if let (Some(u), Some(who)) = (&self.use_block, &who) {
            game.snapshot();
            let stack = match &u.stack {
                Some(text) => Some(parse_stack(text).ok_or("use-block: a stack is item*count")?),
                None => None,
            };
            game.block_use = Some(match game.mods.block_used(who, u.pos, u.sneaking, stack.as_ref()) {
                crate::BlockUse::Default => "default",
                crate::BlockUse::Pass => "pass",
                crate::BlockUse::Consume => "consume",
            });
        }
        if let (Some(u), Some(who)) = (&self.hit_with, &who) {
            game.snapshot();
            let stack = parse_stack(&u.stack).ok_or("hit-with: a stack is item*count")?;
            let kind = u.kind.clone().unwrap_or_default();
            game.mods.item_hit(who, &stack, u.entity.unwrap_or(0), &kind);
        }
        if let Some(s) = &self.spawned {
            game.snapshot();
            game.mods.entity_spawned(&game.mod_id(), s.ticket, s.id);
        }
        if let (Some(n), Some(who)) = (&self.net, &who) {
            game.snapshot();
            game.mods.net_message(who, &n.channel, &n.data);
        }
        if let Some(n) = &self.net_from_server {
            game.snapshot();
            game.mods.net_from_server(&n.channel, &n.data);
        }
        if let (Some(p), Some(who)) = (&self.press, &who) {
            game.press_button(who, (p.x, p.y, p.z), p.id);
        }
        if let Some(line) = &self.chat {
            match &who {
                Some(p) => game.player_chat(p, line),
                None => game.chat(line),
            }
        }
        let owned = self.command.as_ref().map(|line| match &who {
            Some(p) => game.player_command(p, line),
            None => game.command(line),
        });
        for (b, placed) in [(&self.broken, false), (&self.place, true)] {
            if let (Some(b), Some(p)) = (b, &who) {
                let pos = BlockPos {
                    x: b.x,
                    y: b.y,
                    z: b.z,
                };
                game.block(p, pos, &b.state, placed);
            }
        }
        if let Some(name) = &self.leave
            && !game.leave(name)
        {
            return Err(format!("{name} is not on the server"));
        }
        game.mods.take_world();
        let out = game.drain();
        for o in &out {
            if let Output::SetBlocks {
                dimension, blocks, ..
            } = o
                && dimension == "minecraft:overworld"
            {
                for b in blocks {
                    game.world.insert((b.x, b.y, b.z), b.state.clone());
                }
            }
        }
        // A mod entity spawned or removed (FORK-10): the spawn is answered with its id, as the game does.
        for o in &out {
            let Output::Entity { dimension, command, .. } = o else { continue };
            match command {
                crate::EntityCommand::SpawnModEntity { ticket, kind, pos, data } => {
                    let Some(k) = game.entity_kinds.iter().find(|k| &k.id == kind).cloned() else {
                        game.mods.entity_spawned(&game.mod_id(), *ticket, None);
                        continue;
                    };
                    game.next_mod_entity += 1;
                    let id = game.next_mod_entity;
                    game.mod_entities.push(crate::ModEntity {
                        id,
                        dimension: dimension.clone(),
                        kind: kind.clone(),
                        x: pos[0],
                        y: pos[1],
                        z: pos[2],
                        yaw: 0.0,
                        health: k.max_health,
                        data: data.clone(),
                        items: vec![None; usize::from(k.container_slots)],
                        energy: 0,
                        skin: k.skins.first().map(|s| s.id.clone()).unwrap_or_default(),
                        name: None,
                    });
                    game.snapshot();
                    game.mods.entity_spawned(&game.mod_id(), *ticket, Some(id));
                }
                crate::EntityCommand::TakeDropped { id, count } => {
                    if let Some(i) = game.dropped.iter().position(|d| d.id == *id)
                        && let Some(mut stack) = parse_stack(&game.dropped[i].stack)
                    {
                        stack.count = stack.count.saturating_sub(*count);
                        if stack.count == 0 {
                            game.dropped.remove(i);
                        } else {
                            game.dropped[i].stack = show_stack(&Some(stack));
                        }
                    }
                }
                crate::EntityCommand::RemoveEntity { id } => {
                    game.mod_entities.retain(|e| e.id != *id);
                    game.entity_goals.remove(id);
                }
                _ => {}
            }
        }
        // A block packed goes into its box (the scripts keep its state), and comes out at the place it is unpacked.
        for o in &out {
            match o {
                Output::PackBlock { dimension, pos, into, replacement, .. } if dimension == "minecraft:overworld" => {
                    let put = replacement.clone().unwrap_or_else(|| "minecraft:air".to_owned());
                    if let Some(state) = game.world.insert((pos[0], pos[1], pos[2]), put) {
                        game.packed.insert((into[0], into[1], into[2]), state);
                    }
                }
                Output::UnpackBlock { dimension, box_pos, pos, .. } if dimension == "minecraft:overworld" => {
                    if let Some(state) = game.packed.remove(&(box_pos[0], box_pos[1], box_pos[2])) {
                        game.world.insert((pos[0], pos[1], pos[2]), state);
                    }
                }
                _ => {}
            }
        }
        // A block mined is gone (its drops are not simulated).
        for o in &out {
            if let Output::MineBlock { dimension, pos, .. } = o
                && dimension == "minecraft:overworld"
            {
                game.world.insert((pos[0], pos[1], pos[2]), "minecraft:air".to_owned());
            }
        }
        // A property set keeps the block and changes the one property in its state.
        for o in &out {
            if let Output::SetBlockProperty { dimension, pos, property, value, .. } = o
                && dimension == "minecraft:overworld"
                && let Some(state) = game.world.get_mut(&(pos[0], pos[1], pos[2]))
            {
                *state = with_property(state, property, value);
            }
        }
        // Teleports and experience taken happen as the game makes them.
        for o in &out {
            match o {
                Output::Teleport {
                    player,
                    dimension,
                    pos,
                    ..
                } => {
                    if let Some(p) = game.players.iter_mut().find(|p| p.id == *player) {
                        (p.x, p.y, p.z) = (pos[0], pos[1], pos[2]);
                        p.dimension.clone_from(dimension);
                    }
                }
                Output::TakeXp { player, points, .. } => {
                    let m = game.motion_of(player);
                    take_xp(m, *points);
                }
                _ => {}
            }
        }
        let stopped = out.iter().find_map(|o| match o {
            Output::Stopped { reason, .. } => Some(reason.as_str()),
            _ => None,
        });
        let e = &self.expect;
        match (stopped, &e.stopped) {
            (Some(reason), Some(want)) if !reason.contains(want.as_str()) => {
                return Err(format!("the mod stopped ({reason}), expected {want:?}"));
            }
            (Some(_), Some(_)) => return Ok(()),
            (Some(reason), None) => return Err(format!("the mod stopped: {reason}")),
            (None, Some(want)) => {
                return Err(format!(
                    "the mod kept running, expected it to stop ({want:?})"
                ));
            }
            (None, None) => {}
        }
        if owned == Some(false) {
            let line = self.command.as_deref().unwrap_or_default();
            let name = line.split(' ').next().unwrap_or_default();
            return Err(format!("the mod owns no command /{name}"));
        }
        if let Some(want) = &e.commands {
            same("commands", want, &game.commands())?;
        }
        if let Some(want) = &e.chat {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Chat { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            same("chat", want, &got)?;
        }
        let feedback = |error: bool| {
            out.iter().rev().find_map(|o| match o {
                Output::Feedback { text, error: e } | Output::Reply { text, error: e, .. }
                    if *e == error =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
        };
        if let Some(want) = &e.told {
            let name = |id: &str| {
                let p = game.players.iter().find(|p| p.id == id);
                p.map_or(id.to_owned(), |p| p.name.clone())
            };
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Tell { player, text, .. } => Some(format!("{}: {text}", name(player))),
                    Output::TellOverlay { player, text, .. } => Some(format!("{} (action bar): {text}", name(player))),
                    _ => None,
                })
                .collect();
            same("told", want, &got)?;
        }
        if let Some(want) = &e.broadcast {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Broadcast { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            same("broadcast", want, &got)?;
        }
        if let Some(want) = &e.feedback {
            same("feedback", &Some(want.clone()), &feedback(false))?;
        }
        if let Some(want) = &e.error {
            same("error", &Some(want.clone()), &feedback(true))?;
        }
        if let Some(want) = &e.panel {
            let got: Vec<String> = game
                .panel()
                .map(|p| p.lines.iter().map(|l| l.text.clone()).collect())
                .unwrap_or_default();
            same("panel", want, &got)?;
        }
        if let Some(want) = &e.blocks {
            for b in want {
                let got = game.world.get(&(b.x, b.y, b.z));
                let got = got.map_or("minecraft:air", String::as_str);
                if got != b.state {
                    return Err(format!(
                        "block at {} {} {}: expected {}, got {got}",
                        b.x, b.y, b.z, b.state
                    ));
                }
            }
        }
        if let Some(want) = &e.block_menu_data {
            for b in want {
                let got = game.block_menu_data.get(&(b.x, b.y, b.z));
                if got != Some(&b.data) {
                    return Err(format!(
                        "menu data at {} {} {}: expected {:?}, got {got:?}",
                        b.x, b.y, b.z, b.data
                    ));
                }
            }
        }
        if let Some(want) = &e.block_tanks {
            for b in want {
                let got: Option<Vec<String>> = game
                    .block_tanks
                    .get(&(b.x, b.y, b.z))
                    .map(|v| v.iter().map(|(s, _)| s.clone()).collect());
                if got.as_ref() != Some(&b.tanks) {
                    return Err(format!(
                        "tanks at {} {} {}: expected {:?}, got {got:?}",
                        b.x, b.y, b.z, b.tanks
                    ));
                }
            }
        }
        if let Some(want) = &e.block_energy {
            for b in want {
                let got = game.block_energy.get(&(b.x, b.y, b.z)).map(|e| e.0);
                if got != Some(b.energy) {
                    return Err(format!(
                        "energy at {} {} {}: expected {}, got {got:?}",
                        b.x, b.y, b.z, b.energy
                    ));
                }
            }
        }
        if let Some(want) = &e.block_items {
            for b in want {
                let got = game.block_items.get(&(b.x, b.y, b.z));
                if got != Some(&b.slots) {
                    return Err(format!(
                        "slots at {} {} {}: expected {:?}, got {got:?}",
                        b.x, b.y, b.z, b.slots
                    ));
                }
            }
        }
        if let Some(want) = &e.block_entities {
            for b in want {
                let got = game.block_entities.get(&(b.x, b.y, b.z));
                if got != Some(&b.data) {
                    return Err(format!(
                        "block entity at {} {} {}: expected {:?}, got {got:?}",
                        b.x, b.y, b.z, b.data
                    ));
                }
            }
        }
        if let Some(want) = &e.chunk_forced {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Chunk { dimension, chunk, forced, .. } => {
                        Some(format!("{dimension} {} {} {forced}", chunk[0], chunk[1]))
                    }
                    _ => None,
                })
                .collect();
            same("chunk-forced", want, &got)?;
        }
        if let Some(want) = e.can_place {
            if game.can_place_result != Some(want) {
                return Err(format!("can-place: expected {want}, got {:?}", game.can_place_result));
            }
        }
        if let Some(want) = &e.redirects {
            let got: Vec<String> = game.mods.redirects().iter().map(|r| format!("{} {} {}", r.block, r.flag_at, r.pos_at)).collect();
            same("redirects", want, &got)?;
        }
        if let Some(want) = e.damage_result {
            if game.damage_result != Some(want) {
                return Err(format!("damage-result: expected {want}, got {:?}", game.damage_result));
            }
        }
        if let Some(want) = e.hit_damage_result {
            if game.hit_damage_result != Some(want) {
                return Err(format!("hit-damage-result: expected {want}, got {:?}", game.hit_damage_result));
            }
        }
        if let Some(want) = &e.dropped_left {
            let got: Vec<String> = game.dropped.iter().map(|d| format!("{} {}", d.id, d.stack)).collect();
            same("dropped-left", want, &got)?;
        }
        if let Some(want) = &e.mod_entity_items {
            let got: Vec<String> = game
                .mod_entities
                .iter()
                .filter(|m| m.items.iter().any(Option::is_some))
                .map(|m| {
                    let slots: Vec<String> = m
                        .items
                        .iter()
                        .enumerate()
                        .filter_map(|(i, s)| s.as_ref().map(|s| format!("{i}={}", show_stack(&Some(s.clone())))))
                        .collect();
                    format!("{} {}", m.id, slots.join(","))
                })
                .collect();
            same("mod-entity-items", want, &got)?;
        }
        if let Some(want) = e.mod_entity_damage {
            if game.entity_damage_result != Some(want) {
                return Err(format!("mod-entity-damage: expected {want}, got {:?}", game.entity_damage_result));
            }
        }
        if let Some(want) = e.mod_entity_used {
            if game.entity_used_result != Some(want) {
                return Err(format!("mod-entity-used: expected {want}, got {:?}", game.entity_used_result));
            }
        }
        if let Some(want) = e.release_result {
            if game.release_result != Some(want) {
                return Err(format!("release-result: expected {want}, got {:?}", game.release_result));
            }
        }
        if let Some(want) = &e.block_use {
            if game.block_use != Some(want.as_str()) {
                return Err(format!("block-use: expected {want}, got {:?}", game.block_use));
            }
        }
        if let Some(want) = e.claimed {
            if game.claimed != Some(want) {
                return Err(format!("claimed: expected {want}, got {:?}", game.claimed));
            }
        }
        if let Some(want) = &e.player_items {
            use crate::PlayerItemsCommand as P;
            let name = |id: &str| {
                let p = game.players.iter().find(|p| p.id == id);
                p.map_or(id.to_owned(), |p| p.name.clone())
            };
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::PlayerBody { player, change, .. } => {
                        use crate::BodyChange as B;
                        Some(match change {
                            B::Air(a) => format!("set-air {} {a}", name(player)),
                            B::ResetFall => format!("reset-fall {}", name(player)),
                            B::ReduceEffect { effect, ticks } => format!("reduce-effect {} {effect} {ticks}", name(player)),
                            B::Modifier { attribute, id, amount, operation, present: true } => {
                                format!("modifier {} {attribute} {id} {amount} {operation}", name(player))
                            }
                            B::Modifier { attribute, id, present: false, .. } => format!("remove-modifier {} {attribute} {id}", name(player)),
                        })
                    }
                    Output::SetFood { player, food, saturation, .. } => Some(format!("set-food {} {food} {saturation}", name(player))),
                    Output::SetMotion { player, velocity, .. } => {
                        Some(format!("set-motion {} {} {} {}", name(player), velocity[0], velocity[1], velocity[2]))
                    }
                    Output::SetAllowFlight { player, allowed, .. } => Some(format!("allow-flight {} {allowed}", name(player))),
                    Output::PlayerItems { player, command, .. } => Some(match command {
                        P::SetSlot { slot, stack } => format!(
                            "set-slot {} {slot} {}",
                            name(player),
                            stack.as_ref().map_or("-".to_owned(), |s| show_stack(&Some(s.clone())))
                        ),
                        P::CompareAndSet { slot, expected, replacement } => format!(
                            "compare-and-set {} {slot} {} {}",
                            name(player),
                            show_stack(expected),
                            show_stack(replacement)
                        ),
                        P::Hurt { damage_type, amount } => {
                            format!("hurt {} {damage_type} {amount}", name(player))
                        }
                        P::Effect { effect, duration, amplifier } => {
                            format!("effect {} {effect} {duration} {amplifier}", name(player))
                        }
                    }),
                    _ => None,
                })
                .collect();
            same("player-items", want, &got)?;
        }
        if let Some(want) = &e.entity_commands {
            use crate::EntityCommand as C;
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Entity { command, .. } => Some(match command {
                        C::SpawnMob { ticket, kind, pos, .. } => {
                            format!("spawn-mob {kind} {} {} {} #{ticket}", pos[0], pos[1], pos[2])
                        }
                        C::Summon { ticket, kind, pos, nbt } => {
                            format!("summon {kind} {} {} {} #{ticket} {nbt}", pos[0], pos[1], pos[2])
                        }
                        C::Move { id, pos, yaw, pitch } => {
                            format!("move {id} {} {} {} {yaw} {pitch}", pos[0], pos[1], pos[2])
                        }
                        C::SetDisplay { id, .. } => format!("display {id}"),
                        C::RemoveEntity { id } => format!("remove-entity {id}"),
                        C::TeleportMob { id, pos } => {
                            format!("teleport-mob {id} {} {} {}", pos[0], pos[1], pos[2])
                        }
                        C::PushMob { id, velocity } => {
                            format!("push-mob {id} {} {} {}", velocity[0], velocity[1], velocity[2])
                        }
                        C::SetMobHealth { id, health } => format!("health {id} {health}"),
                        C::KillMob { id } => format!("kill-mob {id}"),
                        C::RemoveMob { id } => format!("remove-mob {id}"),
                        C::SetFire { id, ticks } => format!("fire {id} {ticks}"),
                        C::TakeDropped { id, count } => format!("take-dropped-item {id} {count}"),
                        C::SpawnModEntity { ticket, kind, pos, data } => {
                            format!("spawn-mod-entity {kind} {} {} {} {} byte(s) #{ticket}", pos[0], pos[1], pos[2], data.len())
                        }
                    }),
                    _ => None,
                })
                .collect();
            same("entity-commands", want, &got)?;
        }
        if let Some(want) = &e.net_sent {
            let name = |id: &str| {
                let p = game.players.iter().find(|p| p.id == id);
                p.map_or(id.to_owned(), |p| p.name.clone())
            };
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::NetSend { player, channel, data, .. } => {
                        Some(format!("{}: {channel} {data:?}", name(player)))
                    }
                    _ => None,
                })
                .collect();
            same("net-sent", want, &got)?;
        }
        if let Some(want) = &e.net_to_server {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::NetToServer { channel, data, .. } => Some(format!("{channel} {data:?}")),
                    _ => None,
                })
                .collect();
            same("net-to-server", want, &got)?;
        }
        if let Some(want) = &e.teleports {
            let name = |id: &str| {
                let p = game.players.iter().find(|p| p.id == id);
                p.map_or(id.to_owned(), |p| p.name.clone())
            };
            let angle = |a: &Option<f32>| a.map_or("~".to_owned(), |v| format!("{v}"));
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Teleport {
                        player,
                        dimension,
                        pos,
                        yaw,
                        pitch,
                        ..
                    } => Some(format!(
                        "{}: {dimension} {} {} {} {} {}",
                        name(player),
                        pos[0],
                        pos[1],
                        pos[2],
                        angle(yaw),
                        angle(pitch)
                    )),
                    _ => None,
                })
                .collect();
            same("teleports", want, &got)?;
        }
        if let Some(want) = &e.xp_taken {
            let name = |id: &str| {
                let p = game.players.iter().find(|p| p.id == id);
                p.map_or(id.to_owned(), |p| p.name.clone())
            };
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::TakeXp { player, points, .. } => {
                        Some(format!("{}: {points}", name(player)))
                    }
                    _ => None,
                })
                .collect();
            same("xp-taken", want, &got)?;
        }
        if let Some(want) = &e.sounds {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::PlaySound {
                        dimension,
                        sound,
                        pos,
                        ..
                    } => Some(format!("{dimension} {sound} {} {} {}", pos[0], pos[1], pos[2])),
                    _ => None,
                })
                .collect();
            same("sounds", want, &got)?;
        }
        if let Some(want) = &e.particles {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::SpawnParticles { dimension, particle, pos, count, .. } => {
                        Some(format!("{dimension} {particle} {} {} {} {count}", pos[0], pos[1], pos[2]))
                    }
                    _ => None,
                })
                .collect();
            same("particles", want, &got)?;
        }
        if let Some(want) = &e.mod_entities {
            let got: Vec<String> = game
                .mod_entities
                .iter()
                .map(|m| {
                    format!(
                        "{} {} {:.3} {:.3} {:.3} energy={} data={:?} skin={} name={}",
                        m.id, m.kind, m.x, m.y, m.z, m.energy, m.data, m.skin, m.name.as_deref().unwrap_or("-")
                    )
                })
                .collect();
            same("mod-entities", want, &got)?;
        }
        if let Some(want) = &e.menu_text {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::SetMenuText { dimension, pos, texts, .. } => {
                        Some(format!("{dimension} {} {} {} {}", pos[0], pos[1], pos[2], texts.join("|")))
                    }
                    _ => None,
                })
                .collect();
            same("menu-text", want, &got)?;
        }
        if let Some(want) = &e.capabilities {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::SetCapabilitiesEnabled { dimension, pos, items, fluids, energy, .. } => Some(format!(
                        "{dimension} {} {} {} items={items} fluids={fluids} energy={energy}",
                        pos[0], pos[1], pos[2]
                    )),
                    _ => None,
                })
                .collect();
            same("capabilities", want, &got)?;
        }
        if let Some(want) = &e.corner {
            let got = game.panel().map(|p| corner_name(p.corner).to_string());
            same("corner", &Some(want.clone()), &got)?;
        }
        Ok(())
    }
}

fn corner_name(c: Corner) -> &'static str {
    match c {
        Corner::TopLeft => "top-left",
        Corner::TopRight => "top-right",
        Corner::BottomLeft => "bottom-left",
        Corner::BottomRight => "bottom-right",
    }
}

fn same<T: PartialEq + std::fmt::Debug>(what: &str, want: &T, got: &T) -> Result<(), String> {
    if want == got {
        return Ok(());
    }
    Err(format!(
        "{what}\n    expected: {want:?}\n    got:      {got:?}"
    ))
}
