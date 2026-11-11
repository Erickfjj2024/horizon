//! Server mods in the game (`docs/mods.md`, "Server mods"): the game that
//! simulates the world (in-frame single player, the server thread, a LAN
//! host, the dedicated server) runs them. At the end of each tick it tells
//! them who joined and left, then what players said, broke and placed since
//! the last one, then ticks them; a player's command reaches them at once.
//! Their lines go to the players they name. With no server mod on, each hook
//! here is one check.
use super::{Game, lan::LanHost};
#[cfg(not(feature = "mods"))]
use horizon_world::registry::BlockStateId;

/// Who did something a server mod hears about, resolved at the tick's end.
#[cfg_attr(not(feature = "mods"), allow(dead_code))]
pub(super) enum Who {
    /// The window's own player, in-frame.
    Local,
    /// A connection's player (the server thread's owner is one).
    Conn(u32),
    Uuid(u128),
}

/// What players did during a tick, for the server mods at its end.
#[cfg_attr(not(feature = "mods"), allow(dead_code))]
pub(super) enum ModEvent {
    Chat(Who, String),
    Block {
        who: Who,
        pos: [i32; 3],
        state: String,
        placed: bool,
    },
    /// A chunk came into a level (`new`: just generated).
    Chunk {
        dimension: String,
        chunk: [i32; 2],
        new: bool,
    },
    /// A player's client half sent bytes on a mod channel.
    Net {
        who: Who,
        channel: String,
        data: Vec<u8>,
    },
    /// A player pressed menu button `id` of the mod menu at `pos`.
    MenuButton {
        who: Who,
        pos: [i32; 3],
        id: i32,
    },
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn tick_server_mods(&mut self) {}
    pub(super) fn mod_block_event(&mut self, _pos: [i32; 3], _state: BlockStateId, _placed: bool) {}
    pub(super) fn mod_player_chat(&mut self, _who: Who, _text: &str) {}
    pub(super) fn local_server_mod_command(&mut self, _line: &str) -> bool {
        false
    }
    pub(super) fn guest_server_mod_command(
        &mut self,
        _host: &mut LanHost,
        _conn: u32,
        _line: &str,
    ) -> bool {
        false
    }
    pub(super) fn apply_mod_block_writes(&mut self) {}
    pub(super) fn mod_incoming_damage(&mut self, _damage_type: &str, amount: f32, _causing: i32) -> Option<f32> {
        Some(amount)
    }
    pub(super) fn mod_redirect(&self, _pos: [i32; 3]) -> Option<[i32; 3]> {
        None
    }
    pub(super) fn mod_can_place(&mut self, _pos: [i32; 3], _state: BlockStateId) -> bool {
        true
    }
    pub(super) fn mod_hit_damage(&mut self, _kind: &str, damage: f32) -> f32 {
        damage
    }
}

#[cfg(feature = "mods")]
mod run {
    use super::{Game, LanHost, ModEvent, Who};
    use horizon_client::chunk_map::ChunkMap;
    use horizon_client::{
        command::args::GameMode as Mode,
        lan::data::component_nbt,
        text::{self, Text},
    };
    use crate::game::commands::Target;
    use horizon_core::geometry::Axis;
    use horizon_mods::{
        ArgumentKind, BlockPos, CollisionBox, GameMode, Motion, Output, ServerCommand,
        ServerPlayer, WorldView,
    };
    use std::sync::Arc;
    use horizon_protocol::packets::play::{self, Clientbound as C};
    use horizon_world::registry::{BlockRegistry, BlockStateId};
    use serde_json::{Value, json};

    /// Blocks queued for levels that don't tick (another dimension, in-frame)
    /// are kept up to this many; the oldest go first.
    const MAX_QUEUED_WRITES: usize = 1 << 16;

    /// Capability `world`'s view during a call: the levels' chunks, moved out
    /// of the game for the call and back after it (no copy).
    struct LentWorld {
        registry: &'static BlockRegistry,
        biomes: std::sync::Arc<horizon_client::biome::Biomes>,
        dims: Vec<(&'static str, ChunkMap)>,
        /// Collision boxes by block state id (`None`: a shape that depends on
        /// its surroundings, taken as empty).
        shapes: Arc<Vec<Option<Arc<[CollisionBox]>>>>,
        /// The overworld's time of day and weather when it was lent.
        day_time: i64,
        rain: f32,
        thunder: f32,
        /// The redstone signal at each mod block entity of the dimension in place, read as the tick began.
        signals: std::collections::HashMap<[i32; 3], u8>,
        dimension: &'static str,
        /// The items lying in the level in place, as the mods may look for them (`items-in-box`), at most 4096.
        dropped: Vec<(u32, horizon_mods::ModItemStack, [f64; 3])>,
        /// The game's recipes and items, for `crafting-find`, `crafting-matches` and `item-max-stack-size`.
        res: Arc<crate::game::Resources>,
    }
    impl LentWorld {
        /// A mod's stack as the game's (`game_stack` without the game): none for an item it does not know.
        fn stack(&self, s: &horizon_mods::ModItemStack) -> Option<horizon_client::items::ItemStack> {
            let max = self.res.items().get(&s.item).map(|i| i.max_stack)?;
            if s.count == 0 {
                return None;
            }
            let mut entries = vec![
                ("id", horizon_world::nbt::Tag::String(s.item.as_str().into())),
                ("count", horizon_world::nbt::Tag::Int(s.count.min(max) as i32)),
            ];
            if !s.components.is_empty() {
                let (components, _) = horizon_world::nbt::parse_snbt(&s.components).ok()?;
                entries.push(("components", components));
            }
            horizon_client::save::item_from_nbt(&horizon_world::nbt::Tag::compound(entries))
        }
        /// The grid as the game's stacks: none when a cell holds an item the game does not know.
        fn grid(&self, grid: &[Option<horizon_mods::ModItemStack>]) -> Option<Vec<Option<horizon_client::items::ItemStack>>> {
            grid.iter()
                .map(|c| match c {
                    None => Some(None),
                    Some(s) => self.stack(s).map(Some),
                })
                .collect()
        }
    }
    impl WorldView for LentWorld {
        fn crafting_find(&self, grid: &[Option<horizon_mods::ModItemStack>]) -> Option<horizon_mods::CraftMatch> {
            let stacks = self.grid(grid)?;
            let recipes = self.res.recipes();
            let (id, output) = recipes.find_with(&stacks, 3, &|_| None)?;
            let remaining = recipes
                .remainders(id, &stacks, 3)
                .iter()
                .map(|s| s.as_ref().map(Game::mod_stack))
                .collect();
            Some(horizon_mods::CraftMatch { id: id.to_owned(), output: Game::mod_stack(&output), remaining })
        }
        fn crafting_matches(&self, id: &str, grid: &[Option<horizon_mods::ModItemStack>]) -> bool {
            let Some(stacks) = self.grid(grid) else { return false };
            self.res.recipes().matches_recipe(id, &stacks, 3, &|_| None)
        }
        fn item_max_stack(&self, item: &str) -> Option<u32> {
            self.res.items().get(item).map(|i| i.max_stack)
        }
        fn redstone(&self, dimension: &str, x: i32, y: i32, z: i32) -> u8 {
            if dimension != self.dimension {
                return 0;
            }
            self.signals.get(&[x, y, z]).copied().unwrap_or(0)
        }
        fn environment(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<horizon_mods::Environment> {
            let (_, chunks) = self.dims.iter().find(|(d, _)| *d == dimension)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            let biome = self.biomes.get(chunk.biome(x, y, z));
            let overworld = dimension == "minecraft:overworld";
            Some(horizon_mods::Environment {
                day_time: self.day_time.rem_euclid(24_000) as u64,
                rain: if overworld { self.rain } else { 0.0 },
                thunder: if overworld { self.thunder } else { 0.0 },
                temperature: biome.temperature,
                downfall: biome.downfall,
                precipitation: biome.precipitation,
                has_skylight: overworld,
            })
        }
        fn collision(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<Vec<CollisionBox>> {
            let (_, chunks) = self.dims.iter().find(|(d, _)| *d == dimension)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            let id = chunk.block(x & 15, y, z & 15);
            Some(
                self.shapes
                    .get(id.0 as usize)
                    .and_then(Option::as_deref)
                    .map(<[CollisionBox]>::to_vec)
                    .unwrap_or_default(),
            )
        }
        fn breakable(&self, dimension: &str, x: i32, y: i32, z: i32) -> bool {
            let Some((_, chunks)) = self.dims.iter().find(|(d, _)| *d == dimension) else { return false };
            let Some(chunk) = chunks.get(&(x >> 4, z >> 4)) else { return false };
            let f = self.res.facts().get(chunk.block(x & 15, y, z & 15));
            let base = &*f.name;
            !base.ends_with(":air") && !matches!(base, "minecraft:water" | "minecraft:lava") && f.hardness >= 0.0
        }
        fn dropped_items(&self, dimension: &str, min: [f64; 3], max: [f64; 3]) -> Vec<(u32, horizon_mods::ModItemStack, [f64; 3])> {
            if dimension != self.dimension {
                return Vec::new();
            }
            self.dropped
                .iter()
                .filter(|(_, _, p)| (0..3).all(|i| p[i] >= min[i] && p[i] <= max[i]))
                .cloned()
                .collect()
        }
        fn block_hardness(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<f32> {
            let (_, chunks) = self.dims.iter().find(|(d, _)| *d == dimension)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            Some(self.res.facts().get(chunk.block(x & 15, y, z & 15)).hardness)
        }
        fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
            let (_, chunks) = self.dims.iter().find(|(d, _)| *d == dimension)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            Some(state_string(self.registry, chunk.block(x & 15, y, z & 15)))
        }
        fn biome(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
            let (_, chunks) = self.dims.iter().find(|(d, _)| *d == dimension)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            Some(self.biomes.get(chunk.biome(x, y, z)).name.clone())
        }
        fn has_dimension(&self, dimension: &str) -> bool {
            matches!(
                dimension,
                "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
            )
        }
        fn check_state(&self, state: &str) -> Result<(), String> {
            crate::game::render_control::parse_state(self.registry, state)
                .map(drop)
                .map_err(|e| format!("{state}: {e}"))
        }
    }

    /// Server mods' commands as a root of the commands report
    /// (`lan::commands`): a literal per command, then its arguments in a
    /// chain, each node runnable where only optional arguments follow.
    fn command_tree<'a>(commands: impl Iterator<Item = &'a ServerCommand>) -> Value {
        let mut root = serde_json::Map::new();
        for c in commands {
            let mut next: Option<(String, Value)> = None;
            for (i, a) in c.arguments.iter().enumerate().rev() {
                let executable = c.arguments.get(i + 1).is_none_or(|b| b.optional);
                let (parser, properties) = match a.kind {
                    ArgumentKind::Word => ("brigadier:string", json!({"type": "word"})),
                    ArgumentKind::Text => ("brigadier:string", json!({"type": "greedy"})),
                    ArgumentKind::Integer((min, max)) => {
                        let mut p = serde_json::Map::new();
                        if let Some(min) = min {
                            p.insert("min".into(), min.into());
                        }
                        if let Some(max) = max {
                            p.insert("max".into(), max.into());
                        }
                        ("brigadier:integer", Value::Object(p))
                    }
                    ArgumentKind::Number => ("brigadier:double", Value::Null),
                    ArgumentKind::Boolean => ("brigadier:bool", Value::Null),
                    ArgumentKind::Player => (
                        "minecraft:entity",
                        json!({"amount": "single", "type": "players"}),
                    ),
                    ArgumentKind::BlockPos => ("minecraft:block_pos", Value::Null),
                };
                let mut node = json!({
                    "type": "argument",
                    "parser": parser,
                    "properties": properties,
                    "executable": executable,
                });
                if let Some((name, child)) = next.take() {
                    node["children"] = json!({ name: child });
                }
                next = Some((a.name.clone(), node));
            }
            let executable = c.arguments.first().is_none_or(|a| a.optional);
            let mut literal = json!({"type": "literal", "executable": executable});
            if let Some((name, child)) = next {
                literal["children"] = json!({ name: child });
            }
            root.insert(c.name.clone(), literal);
        }
        if root.is_empty() {
            return Value::Null;
        }
        json!({"type": "root", "children": root})
    }

    /// `minecraft:oak_log[axis=y]`.
    fn state_string(registry: &BlockRegistry, state: BlockStateId) -> String {
        let Some(s) = registry.state(state) else {
            return "minecraft:air".into();
        };
        if s.properties.is_empty() {
            return s.name.clone();
        }
        let props: Vec<String> = s
            .properties
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        format!("{}[{}]", s.name, props.join(","))
    }

    /// `UUID.toString()`.
    /// An effect id as the game keeps one: vanilla's without `minecraft:`, a
    /// content mod's as `<ns>:<path>`; kept for the life of the process (a mod
    /// uses few).
    fn effect_id(effect: &str) -> &'static str {
        static IDS: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
        let id = effect.strip_prefix("minecraft:").unwrap_or(effect);
        let Ok(mut ids) = IDS.lock() else {
            return "";
        };
        if let Some(known) = ids.iter().find(|k| **k == id) {
            return known;
        }
        let leaked: &'static str = Box::leak(id.to_owned().into_boxed_str());
        ids.push(leaked);
        leaked
    }

    fn uuid_string(u: u128) -> String {
        let h = format!("{u:032x}");
        format!(
            "{}-{}-{}-{}-{}",
            &h[..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..]
        )
    }

    fn mode(id: i32) -> GameMode {
        match id {
            1 => GameMode::Creative,
            2 => GameMode::Adventure,
            3 => GameMode::Spectator,
            _ => GameMode::Survival,
        }
    }

    impl Game<'_> {
        /// The players on this game's server: the window's own player
        /// in-frame, then every connection's. `host` is the hub when it is
        /// out of `self.lan`.
        pub(in crate::game) fn server_players(&self, host: Option<&LanHost>) -> Vec<ServerPlayer> {
            let mut out = Vec::new();
            if self.options.execution_role.is_client()
                && self.remote.is_none()
                && let Some(p) = &self.player
            {
                out.push(ServerPlayer {
                    id: uuid_string(self.player_uuid()),
                    name: crate::game::chat::PLAYER_NAME.to_owned(),
                    x: p.position.x,
                    y: p.position.y,
                    z: p.position.z,
                    dimension: self.dim.id().to_owned(),
                    game_mode: match self.mode {
                        Mode::Survival => GameMode::Survival,
                        Mode::Creative => GameMode::Creative,
                        Mode::Adventure => GameMode::Adventure,
                        Mode::Spectator => GameMode::Spectator,
                    },
                    health: p.health.health(),
                });
            }
            let guests = host
                .or(self.lan.as_ref())
                .into_iter()
                .flat_map(|h| &h.hub.guests);
            out.extend(guests.filter(|g| g.placed).map(|g| ServerPlayer {
                id: uuid_string(g.uuid),
                name: g.name.clone(),
                x: g.position[0],
                y: g.position[1],
                z: g.position[2],
                dimension: g.dimension.clone(),
                game_mode: mode(g.game_mode),
                health: g.health,
            }));
            out
        }

        /// Whether what players do now reaches server mods: one is running
        /// and this game simulates the world.
        fn hears_players(&self) -> bool {
            self.mods.server && self.remote.is_none()
        }

        /// The tick's end: joins and leaves, the tick's events, the tick hook.
        pub(in crate::game) fn tick_server_mods(&mut self) {
            if !self.options.execution_role.is_client() {
                // A server has no frames to check its dev mods in.
                self.reload_dev_mods();
            }
            self.send_server_mod_commands();
            if !self.mods.server {
                return;
            }
            let players = self.server_players(None);
            let mut snapshot = self.mod_snapshot_with(players.clone());
            if self.mods.world {
                snapshot.motions = self.server_motions();
                snapshot.states = self.server_states();
                snapshot.looks = self.server_looks();
            }
            if self.mods.mods.wants_player_items() {
                snapshot.inventories = self.server_inventories(&players);
            }
            if self.mods.mods.wants_entities() {
                let dim = self.dim.id();
                snapshot.mobs = self
                    .native_mobs(dim)
                    .into_iter()
                    .map(|m| horizon_mods::MobInfo {
                        id: m.id,
                        kind: m.kind,
                        pos: m.pos,
                        yaw: m.yaw,
                        health: m.health,
                        max_health: m.max_health,
                    })
                    .collect();
                snapshot.mobs_dimension = dim.to_owned();
            }
            self.mods.mods.set_snapshot(snapshot);
            self.lend_recipe_types();
            let entities = self.collect_mod_entities();
            let structures = self.changed_structures();
            // Menu buttons pressed this tick get their entity as it is now,
            // before the levels' chunks are lent out.
            let kinds = self.mods.mods.block_entity_kinds();
            let pressed_at: Vec<[i32; 3]> = self
                .mods
                .events
                .iter()
                .filter_map(|e| match e {
                    ModEvent::MenuButton { pos, .. } => Some(*pos),
                    _ => None,
                })
                .collect();
            let pressed: Vec<([i32; 3], horizon_mods::BlockEntity)> = pressed_at
                .into_iter()
                .filter_map(|p| Some((p, self.snapshot_entity(p, &kinds, false)?)))
                .collect();
            let mut menu_updates = Vec::new();
            self.lend_world();
            for (id, ticket, entity) in std::mem::take(&mut self.mods.spawn_answers) {
                self.mods.mods.entity_spawned(&id, ticket, entity);
            }
            let before = std::mem::take(&mut self.mods.players);
            for p in before
                .iter()
                .filter(|p| !players.iter().any(|q| q.id == p.id))
            {
                self.mods.mods.player_left(p);
            }
            for p in players
                .iter()
                .filter(|p| !before.iter().any(|q| q.id == p.id))
            {
                self.mods.mods.player_joined(p);
            }
            for event in std::mem::take(&mut self.mods.events) {
                let who = |w: &Who| {
                    let id = match w {
                        Who::Local => uuid_string(self.player_uuid()),
                        Who::Uuid(u) => uuid_string(*u),
                        Who::Conn(c) => {
                            let guests = self.lan.iter().flat_map(|h| &h.hub.guests);
                            let g = guests.into_iter().find(|g| g.conn == *c);
                            g.map_or_else(String::new, |g| uuid_string(g.uuid))
                        }
                    };
                    players
                        .iter()
                        .chain(&before)
                        .find(|p| p.id == id)
                        .cloned()
                        // With one player on the server there is no one else it can be.
                        .or_else(|| (players.len() == 1).then(|| players[0].clone()))
                };
                match event {
                    ModEvent::Chat(w, text) => {
                        if let Some(p) = who(&w) {
                            self.mods.mods.player_chat(&p, &text);
                        }
                    }
                    ModEvent::Chunk { dimension, chunk, new } => {
                        self.mods.mods.chunk_loaded(&dimension, chunk, new);
                    }
                    ModEvent::Net { who: w, channel, data } => {
                        if let Some(p) = who(&w) {
                            self.mods.mods.net_message(&p, &channel, &data);
                        }
                    }
                    ModEvent::MenuButton { who: w, pos, id } => {
                        let Some(p) = who(&w) else { continue };
                        if let Some((_, entity)) = pressed.iter().find(|(q, _)| *q == pos)
                            && let Some(u) = self.mods.mods.menu_button(&p, id, entity)
                        {
                            menu_updates.push(u);
                        }
                    }
                    ModEvent::Block {
                        who: w,
                        pos,
                        state,
                        placed,
                    } => {
                        let Some(p) = who(&w) else { continue };
                        let pos = BlockPos {
                            x: pos[0],
                            y: pos[1],
                            z: pos[2],
                        };
                        if placed {
                            self.mods.mods.block_placed(&p, pos, &state);
                        } else {
                            self.mods.mods.block_broken(&p, pos, &state);
                        }
                    }
                }
            }
            self.mods.mods.server_tick(self.ticks);
            let dim_id = self.dim.id();
            for (group, found) in &structures {
                menu_updates.extend(self.mods.mods.multiblock_changed(dim_id, group, found));
            }
            let updates = if entities.is_empty() {
                Vec::new()
            } else {
                self.mods.mods.block_entities_tick(self.ticks, &entities)
            };
            // An open menu's slots are what the entity holds now (the player may have moved items), before the mod sees it.
            let menus: Vec<(i32, i32, i32)> = self
                .world
                .block_entities
                .keys()
                .filter(|k| k.0 == i32::MAX - 16 && k.1 == crate::game::minecarts::CART_KEY_Y)
                .copied()
                .collect();
            for key in menus {
                self.store_mod_entity_menu(key);
            }
            let kept = if self.mods.mods.hears_mod_entities() { self.mod_entities_view() } else { Vec::new() };
            let kept = if kept.is_empty() { Vec::new() } else { self.mods.mods.entities_tick(self.ticks, &kept) };
            self.return_world();
            self.apply_mod_entity_ticks(kept);
            self.mirror_mod_entity_menus();
            self.walk_mod_entities();
            self.mod_entity_hazards();
            self.sync_mod_entity_carriers();
            self.apply_mod_entity_updates(updates);
            self.apply_mod_entity_updates(menu_updates);
            self.transfer_mod_energy();
            self.transfer_mod_tanks();
            self.transfer_mod_items();
            self.sync_declared_displays();
            self.mods.players = players;
            self.show_mod_output();
            self.apply_mod_block_writes();
        }

        /// Players' command trees list the running server mods' commands
        /// after vanilla's, whenever they change (or a hub opens).
        fn send_server_mod_commands(&mut self) {
            let key = self.mods.mods.generation();
            let Some(host) = self.lan.as_mut() else {
                return;
            };
            if host.hub.mod_commands_key() == Some(key) {
                return;
            }
            let tree = command_tree(self.mods.mods.server_commands());
            host.hub.set_mod_commands(key, tree);
        }

        /// Lends the levels' chunks to capability `world` for the calls up to
        /// [`Game::return_world`], when a running server mod has it.
        fn lend_world(&mut self) {
            if !self.mods.world {
                return;
            }
            // The redstone signals of the mod block entities, read while the world is still in place.
            let dimension = self.dim.id();
            let at: Vec<[i32; 3]> = self
                .world
                .block_entities
                .iter_mut()
                .filter_map(|(pos, e)| {
                    matches!(e, horizon_client_render::containers::BlockEntity::Mod(_))
                        .then_some([pos.0, pos.1, pos.2])
                })
                .take(horizon_mods::BLOCK_ENTITIES_PER_CALL)
                .collect();
            let signals: std::collections::HashMap<[i32; 3], u8> = at
                .into_iter()
                .map(|p| (p, self.native_neighbor_signal(dimension, p).clamp(0, 15) as u8))
                .collect();
            let mut dims = vec![(self.dim.id(), std::mem::take(&mut self.world.chunks))];
            for (dim, chunks) in self.levels.parked_chunks() {
                dims.push((dim.id(), std::mem::take(chunks)));
            }
            let dropped: Vec<(u32, horizon_mods::ModItemStack, [f64; 3])> = self
                .dropped
                .iter()
                .take(4096)
                .map(|d| (d.id, Self::mod_stack(&d.stack), [d.position.x, d.position.y, d.position.z]))
                .collect();
            let registry = self.pack.assets.registry;
            let biomes = self.res.biomes().clone();
            let shapes = self.collision_table();
            let (rain, thunder) = self.world.weather.levels(1.0);
            let day_time = self.world.day_time;
            self.mods.mods.lend_world(Box::new(LentWorld {
                registry,
                biomes,
                dims,
                shapes,
                day_time,
                rain,
                thunder,
                signals,
                dimension,
                dropped,
                res: Arc::clone(&self.res),
            }));
        }

        /// Every state's collision boxes, made once. A shape the game works out
        /// from the blocks around it is left out (`None`).
        fn collision_table(&mut self) -> Arc<Vec<Option<Arc<[CollisionBox]>>>> {
            if let Some(table) = &self.mods.collision {
                return table.clone();
            }
            let cached = self.collision.cached_shapes();
            let count = self.pack.assets.registry.len() as u32;
            let table: Vec<Option<Arc<[CollisionBox]>>> = (0..count)
                .map(|i| {
                    let shape = cached.cached_shape(BlockStateId(i)).ok().flatten()?;
                    let boxes: Vec<CollisionBox> = shape
                        .boxes()
                        .iter()
                        .map(|b| CollisionBox {
                            min_x: b.min(Axis::X),
                            min_y: b.min(Axis::Y),
                            min_z: b.min(Axis::Z),
                            max_x: b.max(Axis::X),
                            max_y: b.max(Axis::Y),
                            max_z: b.max(Axis::Z),
                        })
                        .collect();
                    Some(Arc::from(boxes))
                })
                .collect();
            let table = Arc::new(table);
            self.mods.collision = Some(table.clone());
            table
        }

        /// What the window's own player is doing, as capability `player-control` reads it.
        /// Where each player looks, `(yaw, pitch)`: the window's own player in-frame and the guests.
        fn server_looks(&mut self) -> Vec<(String, (f32, f32))> {
            let mut out = Vec::new();
            if self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player.is_some()
            {
                out.push((uuid_string(self.player_uuid()), (self.yaw, self.pitch)));
            }
            let guests: Vec<(u32, u128)> = self
                .lan
                .iter()
                .flat_map(|h| &h.hub.guests)
                .filter(|g| g.placed)
                .map(|g| (g.conn, g.uuid))
                .collect();
            for (conn, uuid) in guests {
                if let Some(look) = self.for_player(Target::Guest(conn), |g| (g.yaw, g.pitch)) {
                    out.push((uuid_string(uuid), look));
                }
            }
            out
        }

        fn own_motion(&self) -> Motion {
            let p = self.player.as_ref();
            Motion {
                // The sneak key, not the pose: a player standing with their feet inside a carpet
                // or another thin block is put in the crawling pose whatever they press, and
                // never reads as crouching.
                sneaking: p.is_some_and(|p| p.shift_key_down),
                on_ground: p.is_some_and(|p| p.ground.on_ground),
                jumps: self.stats.get("custom", "minecraft:jump").max(0) as u32,
                xp_level: self.experience.level,
                xp_progress: self.experience.progress,
            }
        }

        /// This player's body as `player-control.get-state` reads it.
        fn own_state(&self) -> horizon_mods::PlayerState {
            let effects = self
                .potions
                .effects
                .saved()
                .into_iter()
                .map(|(id, duration, amplifier, _, _)| horizon_mods::EffectState {
                    id: if id.contains(':') { id } else { format!("minecraft:{id}") },
                    duration,
                    amplifier,
                })
                .collect();
            let Some(p) = self.player.as_ref() else {
                return horizon_mods::PlayerState {
                    velocity_x: 0.0, velocity_y: 0.0, velocity_z: 0.0, jumping: false, sprinting: false, flying: false, may_fly: false,
                    fall_flying: false, swimming: false, in_water: false, eyes_in_water: false, in_lava: false, air: 300, max_air: 300,
                    fire_ticks: 0, passenger: false, sleeping: false, food: 20, saturation: 5.0, effects, blocking: false,
                };
            };
            horizon_mods::PlayerState {
                velocity_x: p.velocity.x,
                velocity_y: p.velocity.y,
                velocity_z: p.velocity.z,
                jumping: p.jumping,
                sprinting: p.sprinting,
                flying: p.flying,
                may_fly: p.may_fly,
                fall_flying: p.fall_flying,
                swimming: p.swimming,
                in_water: p.fluids.was_touching_water,
                eyes_in_water: p.is_under_water(),
                in_lava: p.is_in_lava(),
                air: p.air,
                max_air: 300,
                fire_ticks: p.fire_ticks.max(0),
                passenger: p.is_passenger,
                sleeping: p.sleeping,
                food: p.health.food.food.clamp(0, 20) as u32,
                saturation: p.health.food.saturation,
                effects,
                blocking: self.player_blocking(None, "minecraft:generic", false).is_some(),
            }
        }

        /// What every player's body is doing, as `player-control.get-state` reads it (as `server_motions`).
        fn server_states(&mut self) -> Vec<(String, horizon_mods::PlayerState)> {
            let mut out = Vec::new();
            if self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player.is_some()
            {
                out.push((uuid_string(self.player_uuid()), self.own_state()));
            }
            let guests: Vec<(u32, u128, bool, bool)> = self
                .lan
                .iter()
                .flat_map(|h| &h.hub.guests)
                .filter(|g| g.placed)
                .map(|g| (g.conn, g.uuid, g.sneaking, g.mod_may_fly))
                .collect();
            for (conn, uuid, _, _) in guests {
                if let Some(s) = self.for_player(Target::Guest(conn), |g| g.own_state()) {
                    out.push((uuid_string(uuid), s));
                }
            }
            out
        }

        /// Who a server mod's player id names: the window's own player in-frame,
        /// or a guest by connection (the owner of a single-player world is one
        /// when the server runs on its own thread, the default). Also the
        /// dimension they are in.
        fn mod_player(&self, id: &str) -> Option<(Target, String)> {
            if self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player.is_some()
                && id == uuid_string(self.player_uuid())
            {
                return Some((Target::Player, self.dim.id().to_owned()));
            }
            self.lan
                .iter()
                .flat_map(|h| &h.hub.guests)
                .find(|g| g.placed && uuid_string(g.uuid) == id)
                .map(|g| (Target::Guest(g.conn), g.dimension.clone()))
        }

        /// What each player carries as capability `player-items` reads it
        /// (slots 0 to 35, then feet, legs, chest, head and the off hand).
        fn server_inventories(
            &mut self,
            players: &[horizon_mods::ServerPlayer],
        ) -> Vec<(String, Vec<Option<horizon_mods::ModItemStack>>)> {
            let mut out = Vec::new();
            for p in players {
                let Some((target, _)) = self.mod_player(&p.id) else {
                    continue;
                };
                if let Some(slots) = self.for_player(target, |g| g.inventory_for_mods()) {
                    out.push((p.id.clone(), slots));
                }
            }
            out
        }

        /// This player's carried stacks, in the order of `player-items`.
        fn inventory_for_mods(&self) -> Vec<Option<horizon_mods::ModItemStack>> {
            let stack = |s: Option<&Option<horizon_client::items::ItemStack>>| {
                s.and_then(|s| s.as_ref()).map(Self::mod_stack)
            };
            let mut out: Vec<_> = (0..36).map(|i| stack(self.inventory.get(i))).collect();
            // `armor` holds the head first: feet is its last.
            for i in 0..4 {
                out.push(stack(self.armor.get(3 - i)));
            }
            out.push(self.offhand.as_ref().map(Self::mod_stack));
            out
        }

        /// Puts `stack` in slot `slot` of `player-items`' numbering.
        fn put_slot_for_mods(&mut self, slot: u32, stack: Option<horizon_client::items::ItemStack>) {
            let cell = match slot {
                0..=35 => self.inventory.get_mut(slot as usize),
                36..=39 => self.armor.get_mut(3 - (slot as usize - 36)),
                40 => Some(&mut self.offhand),
                _ => None,
            };
            if let Some(cell) = cell {
                *cell = stack;
            }
        }

        /// What every player is doing, as capability `player-control` reads it:
        /// the window's own player in-frame, then each guest (their game state
        /// swapped in for the read).
        fn server_motions(&mut self) -> Vec<(String, Motion)> {
            let mut out = Vec::new();
            if self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player.is_some()
            {
                out.push((uuid_string(self.player_uuid()), self.own_motion()));
            }
            let guests: Vec<(u32, u128, bool)> = self
                .lan
                .iter()
                .flat_map(|h| &h.hub.guests)
                .filter(|g| g.placed)
                .map(|g| (g.conn, g.uuid, g.sneaking))
                .collect();
            for (conn, uuid, sneaking) in guests {
                if let Some(mut m) = self.for_player(Target::Guest(conn), |g| g.own_motion()) {
                    // What the guest's client says about the sneak key.
                    m.sneaking = sneaking;
                    out.push((uuid_string(uuid), m));
                }
            }
            out
        }

        /// The sides of a block entity's faces: what its mod last set
        /// (`energy_faces`), else the kind's own.
        fn energy_faces(
            m: &horizon_client_render::containers::ModBlockEntity,
            spec: &horizon_mods::EnergySpec,
        ) -> [horizon_mods::FlowSide; 6] {
            use horizon_mods::FlowSide as S;
            let from_byte = |b: u8| match b {
                1 => S::Input,
                2 => S::Output,
                3 => S::Both,
                _ => S::None,
            };
            let mut out = [S::None; 6];
            match m.nbt.get(&horizon_world::nbt::JavaString::from("energy_faces")) {
                Some(horizon_world::nbt::Tag::ByteArray(b)) => {
                    for (to, from) in out.iter_mut().zip(b) {
                        *to = from_byte(*from);
                    }
                }
                _ => {
                    for (to, from) in out.iter_mut().zip(&spec.faces) {
                        *to = *from;
                    }
                }
            }
            out
        }
        fn side_byte(s: horizon_mods::FlowSide) -> u8 {
            use horizon_mods::FlowSide as S;
            match s {
                S::None => 0,
                S::Input => 1,
                S::Output => 2,
                S::Both => 3,
            }
        }
        fn jname(s: &str) -> horizon_world::nbt::JavaString {
            horizon_world::nbt::JavaString::from(s)
        }
        /// A mod's `world.mine-block`: the block broken as a player with the tool would, its drops into the container at `to`.
        pub(in crate::game) fn native_mine_block(&mut self, pos: [i32; 3], tool: &str, silk_touch: bool, to: [i32; 3], face: u32) {
            use horizon_core::geometry::Direction;
            let state = self.world.block(pos[0], pos[1], pos[2]);
            let name = self.data_block_name(pos);
            let base = name.split('[').next().unwrap_or("");
            if base.ends_with(":air") || matches!(base, "minecraft:water" | "minecraft:lava" | "minecraft:bedrock" | "minecraft:barrier" | "minecraft:end_portal_frame" | "minecraft:end_portal" | "minecraft:nether_portal" | "minecraft:command_block" | "minecraft:structure_block" | "minecraft:jigsaw") {
                return;
            }
            let components = if silk_touch { "{\"minecraft:enchantments\":{\"minecraft:silk_touch\":1}}".to_owned() } else { String::new() };
            let held = self.game_stack(&horizon_mods::ModItemStack { item: tool.to_owned(), count: 1, components });
            let drops = self.block_loot_at(pos, state, held.as_ref());
            let air = match crate::game::render_control::parse_state(self.pack.assets.registry, "minecraft:air") {
                Ok(s) => s,
                Err(_) => return,
            };
            if self.place_state(pos, air, false).is_err() {
                return;
            }
            let dir = match face {
                0 => Direction::Down,
                1 => Direction::Up,
                2 => Direction::North,
                3 => Direction::South,
                4 => Direction::West,
                _ => Direction::East,
            };
            for stack in drops {
                if let Some(rest) = self.give_to_container(to, dir, stack) {
                    self.pop_resource(pos, rest);
                }
            }
        }
        /// A mod's `world.break-blocks`: each block that is loaded and breakable (not air, a fluid, bedrock-hard) becomes air and its drops,
        /// worked out with the tool (silk touch, fortune), fall at its place.
        pub(in crate::game) fn native_break_blocks(&mut self, positions: &[[i32; 3]], tool: &str, silk_touch: bool, fortune: u32) {
            let mut enchants = Vec::new();
            if silk_touch {
                enchants.push("\"minecraft:silk_touch\":1".to_owned());
            }
            if fortune > 0 {
                enchants.push(format!("\"minecraft:fortune\":{fortune}"));
            }
            let components = if enchants.is_empty() { String::new() } else { format!("{{\"minecraft:enchantments\":{{{}}}}}", enchants.join(",")) };
            let held = self.game_stack(&horizon_mods::ModItemStack { item: tool.to_owned(), count: 1, components });
            let Ok(air) = crate::game::render_control::parse_state(self.pack.assets.registry, "minecraft:air") else {
                return;
            };
            for pos in positions.iter().take(4096) {
                let state = self.world.block(pos[0], pos[1], pos[2]);
                let facts = self.res.facts().get(state);
                let base = &*facts.name;
                if !self.world.chunks.contains_key(&(pos[0] >> 4, pos[2] >> 4))
                    || base.ends_with(":air")
                    || matches!(base, "minecraft:water" | "minecraft:lava")
                    || facts.hardness < 0.0
                {
                    continue;
                }
                let drops = self.block_loot_at(*pos, state, held.as_ref());
                if self.place_state(*pos, air, false).is_err() {
                    continue;
                }
                for stack in drops {
                    self.pop_resource(*pos, stack);
                }
            }
        }
        /// A mod's `world.set-item-faces`: the slots each face of the entity's container reaches, kept in it.
        pub(in crate::game) fn native_set_item_faces(&mut self, pos: [i32; 3], faces: &[Vec<u32>]) {
            let key = (pos[0], pos[1], pos[2]);
            if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                self.world.block_entities.get_mut(&key)
            {
                let masks: Vec<i32> = faces.iter().map(|f| f.iter().fold(0u32, |a, s| a | (1 << (*s & 31))) as i32).collect();
                m.nbt.insert(Self::jname("item_faces"), horizon_world::nbt::Tag::IntArray(masks));
                self.world.modified.insert((key.0 >> 4, key.2 >> 4));
            }
        }
        /// A mod's `world.set-comparator-output`: the signal is kept in the block entity (`comparator` of its saved compound) and the
        /// comparators around hear it change.
        pub(in crate::game) fn native_set_comparator(&mut self, pos: [i32; 3], level: u8) {
            let key = (pos[0], pos[1], pos[2]);
            let Some(horizon_client_render::containers::BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&key) else {
                return;
            };
            let name = Self::jname("comparator");
            let new = horizon_world::nbt::Tag::Byte(level.min(15) as i8);
            if m.nbt.get(&name) == Some(&new) {
                return;
            }
            m.nbt.insert(name, new);
            self.container_changed(key);
        }

        /// A mod's `world.set-menu-text` (FORK-20): the strings are kept in the block entity (`menu_text` of its saved compound) and go
        /// to the clients within reach, which show them in the menu's `text` widgets.
        pub(in crate::game) fn native_set_menu_text(&mut self, pos: [i32; 3], texts: &[String]) {
            let key = (pos[0], pos[1], pos[2]);
            let bytes = horizon_mods::screen::encode_menu_text(pos, texts);
            let Some(horizon_client_render::containers::BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&key) else {
                return;
            };
            m.nbt.insert(Self::jname("menu_text"), horizon_world::nbt::Tag::ByteArray(bytes.clone().into()));
            self.world.modified.insert((key.0 >> 4, key.2 >> 4));
            let near: Vec<u128> = self
                .lan
                .as_ref()
                .map(|h| {
                    h.hub
                        .guests
                        .iter()
                        .filter(|g| g.placed && (0..3).map(|i| (g.position[i] - f64::from(pos[i])).powi(2)).sum::<f64>() < 16.0 * 16.0)
                        .map(|g| g.uuid)
                        .collect()
                })
                .unwrap_or_default();
            for uuid in near {
                let _ = self.native_net_send_to_player(uuid, horizon_mods::screen::MENU_TEXT_CHANNEL, &bytes);
            }
        }

        /// The strings of the mod block at `key`, to the acting player: its menu just opened.
        pub(in crate::game) fn send_menu_text_to_actor(&mut self, key: (i32, i32, i32)) {
            let bytes = match self.world.block_entities.get(&key) {
                Some(horizon_client_render::containers::BlockEntity::Mod(m)) => match m.nbt.get(&Self::jname("menu_text")) {
                    Some(horizon_world::nbt::Tag::ByteArray(b)) => b.iter().map(|v| *v as u8).collect::<Vec<u8>>(),
                    _ => return,
                },
                _ => return,
            };
            let ints = self.acting_player_uuid();
            let uuid = ints.iter().fold(0u128, |acc, i| (acc << 32) | u128::from(*i as u32));
            let _ = self.native_net_send_to_player(uuid, horizon_mods::screen::MENU_TEXT_CHANNEL, &bytes);
        }

        /// A mod's `world.set-capabilities-enabled` (FORK-34): which of the entity's capabilities the outside can use, kept in it.
        pub(in crate::game) fn native_set_capabilities_enabled(&mut self, pos: [i32; 3], items: bool, fluids: bool, energy: bool) {
            let key = (pos[0], pos[1], pos[2]);
            if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                self.world.block_entities.get_mut(&key)
            {
                let closed = u8::from(!items) | u8::from(!fluids) << 1 | u8::from(!energy) << 2;
                if closed == 0 {
                    m.nbt.remove(&Self::jname("closed"));
                } else {
                    m.nbt.insert(Self::jname("closed"), horizon_world::nbt::Tag::Byte(closed as i8));
                }
                self.world.modified.insert((key.0 >> 4, key.2 >> 4));
            }
        }
        /// A mod's `world.pack-block` (FORK-23 of the cardboard box): the block at `pos`, with its block entity, kept in the mod's block
        /// entity at `into` (under `captured`) and taken away without a drop or a spill. False when it cannot.
        pub(in crate::game) fn native_pack_block(&mut self, pos: [i32; 3], into: [i32; 3], replacement: Option<&str>) -> bool {
            use horizon_world::nbt::{Compound, Tag};
            let (key, into_key) = ((pos[0], pos[1], pos[2]), (into[0], into[1], into[2]));
            let state = self.world.block(pos[0], pos[1], pos[2]);
            let name = self.res.facts().get(state).name.clone();
            let air = matches!(&*name, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air");
            // The box may be the replacement itself, at the packed block's own place.
            let in_place = key == into_key;
            let replacement_state = match replacement {
                Some(text) => match crate::game::render_control::parse_state(self.pack.assets.registry, text) {
                    Ok(s) => Some(s),
                    Err(_) => return false,
                },
                None => None,
            };
            if in_place && replacement_state.is_none() {
                return false;
            }
            if !in_place {
                let holds = match self.world.block_entities.get(&into_key) {
                    Some(horizon_client_render::containers::BlockEntity::Mod(m)) => m.nbt.get(&Self::jname("captured")).is_some(),
                    _ => return false,
                };
                if holds {
                    return false;
                }
            }
            if air {
                return false;
            }
            let mut captured = Compound::default();
            captured.insert(Self::jname("state"), Tag::String(state_string(self.pack.assets.registry, state).as_str().into()));
            if let Some(entity) = self.saved_block_entity(key) {
                captured.insert(Self::jname("entity"), entity);
            }
            // Taken away as `removeBlockEntity` then `removeBlock`: no drop, no spill.
            self.world.block_entities.remove(&key);
            self.world.block_names.remove(&key);
            let _ = self.place_state(pos, replacement_state.unwrap_or(BlockStateId(0)), false);
            if let Some(state) = replacement_state {
                let name = self.res.facts().get(state).name.clone();
                if let Some(entity) = self.new_block_entity(&name) {
                    self.world.block_entities.insert(key, entity);
                }
            }
            match self.world.block_entities.get_mut(&into_key) {
                Some(horizon_client_render::containers::BlockEntity::Mod(m)) => {
                    m.nbt.insert(Self::jname("captured"), Tag::Compound(captured));
                    self.world.modified.insert((into[0] >> 4, into[2] >> 4));
                    true
                }
                _ => false,
            }
        }
        /// A mod's `world.unpack-block`: what the box at `box_pos` holds, put back at `pos`. False when it holds nothing.
        pub(in crate::game) fn native_unpack_block(&mut self, box_pos: [i32; 3], pos: [i32; 3]) -> bool {
            use horizon_world::nbt::Tag;
            let box_key = (box_pos[0], box_pos[1], box_pos[2]);
            let captured = match self.world.block_entities.get(&box_key) {
                Some(horizon_client_render::containers::BlockEntity::Mod(m)) => match m.nbt.get(&Self::jname("captured")) {
                    Some(Tag::Compound(c)) => c.clone(),
                    _ => return false,
                },
                _ => return false,
            };
            let Some(Tag::String(text)) = captured.get(&Self::jname("state")) else { return false };
            let Ok(state) = crate::game::render_control::parse_state(self.pack.assets.registry, &text.to_string_lossy()) else {
                return false;
            };
            let key = (pos[0], pos[1], pos[2]);
            let _ = self.place_state(pos, state, false);
            if let Some(Tag::Compound(entity)) = captured.get(&Self::jname("entity")) {
                let mut tag = entity.clone();
                tag.insert(Self::jname("x"), Tag::Int(pos[0]));
                tag.insert(Self::jname("y"), Tag::Int(pos[1]));
                tag.insert(Self::jname("z"), Tag::Int(pos[2]));
                let tag = Tag::Compound(tag);
                if let Some((_, entity)) = horizon_client::save::decode_block_entity(&tag) {
                    if let Some(name) = horizon_client::save::text(tag.get("CustomName")) {
                        self.world.block_names.insert(key, name);
                    }
                    self.world.block_entities.insert(key, entity);
                }
            }
            if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&box_key) {
                m.nbt.remove(&Self::jname("captured"));
            }
            self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
            true
        }
        /// A mod's `world.set-item-eject`: whether the entity pushes its output items out.
        pub(in crate::game) fn native_set_item_eject(&mut self, pos: [i32; 3], on: bool) {
            let key = (pos[0], pos[1], pos[2]);
            if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                self.world.block_entities.get_mut(&key)
            {
                m.nbt.insert(Self::jname("eject_items"), horizon_world::nbt::Tag::Byte(i8::from(on)));
                self.world.modified.insert((key.0 >> 4, key.2 >> 4));
            }
        }
        /// A mod's `world.set-tank-filter`: what tank `tank` takes in (kept as `tank_accepts`, a compound by tank number
        /// of lists of substances; an empty list removes the filter).
        pub(in crate::game) fn native_set_tank_filter(&mut self, pos: [i32; 3], tank: u32, accepts: &[String]) {
            use horizon_world::nbt::Tag;
            let key = (pos[0], pos[1], pos[2]);
            if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                self.world.block_entities.get_mut(&key)
            {
                let name = Self::jname("tank_accepts");
                let mut all: Vec<(String, Tag)> = match m.nbt.get(&name) {
                    Some(Tag::Compound(c)) => c.iter().map(|(k, v)| (k.to_string_lossy(), v.clone())).collect(),
                    _ => Vec::new(),
                };
                all.retain(|(k, _)| *k != tank.to_string());
                if !accepts.is_empty() {
                    all.push((tank.to_string(), Tag::List(accepts.iter().map(|a| Tag::String(a.as_str().into())).collect())));
                }
                m.nbt.insert(name, Tag::compound(all.iter().map(|(k, v)| (k.as_str(), v.clone()))));
                self.world.modified.insert((key.0 >> 4, key.2 >> 4));
            }
        }
        /// A mod's `world.set-energy-capacity`: the most the block entity holds (kept as `energy_limit`; 0 removes it).
        pub(in crate::game) fn native_set_energy_capacity(&mut self, pos: [i32; 3], capacity: u64) {
            use horizon_world::nbt::Tag;
            let key = (pos[0], pos[1], pos[2]);
            if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&key) {
                let name = Self::jname("energy_limit");
                if capacity == 0 {
                    m.nbt.remove(&name);
                } else {
                    m.nbt.insert(name, Tag::Long(capacity.min(i64::MAX as u64) as i64));
                    // What it holds above the new limit is lost.
                    let held = Self::stored_energy(m);
                    if held > capacity {
                        m.nbt.insert(Self::jname("energy"), Tag::Long(capacity.min(i64::MAX as u64) as i64));
                    }
                }
                self.world.modified.insert((key.0 >> 4, key.2 >> 4));
            }
        }
        /// How much energy a block entity holds at most: its mod's `energy_limit` or its kind's capacity.
        fn energy_capacity(m: &horizon_client_render::containers::ModBlockEntity, spec: &horizon_mods::EnergySpec) -> u64 {
            match m.nbt.get(&Self::jname("energy_limit")) {
                Some(horizon_world::nbt::Tag::Long(v)) if *v > 0 => *v as u64,
                _ => spec.capacity,
            }
        }
        /// What each tank of a block entity takes in (empty: anything of its kind).
        fn tank_accepts(
            m: &horizon_client_render::containers::ModBlockEntity,
            count: usize,
        ) -> Vec<Vec<String>> {
            use horizon_world::nbt::Tag;
            let saved = match m.nbt.get(&Self::jname("tank_accepts")) {
                Some(Tag::Compound(c)) => Some(c),
                _ => None,
            };
            (0..count)
                .map(|i| match saved.and_then(|c| c.get(&Self::jname(&i.to_string()))) {
                    Some(Tag::List(l)) => l
                        .iter()
                        .filter_map(|t| match t {
                            Tag::String(s) => Some(s.to_string_lossy()),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                })
                .collect()
        }
        /// The faces a transmitter connects through: what its mod last set
        /// (`transmitter_faces`), else all of them.
        fn transmitter_connect(
            m: &horizon_client_render::containers::ModBlockEntity,
        ) -> [bool; 6] {
            let mut out = [true; 6];
            if let Some(horizon_world::nbt::Tag::ByteArray(b)) =
                m.nbt.get(&Self::jname("transmitter_faces"))
            {
                for (to, from) in out.iter_mut().zip(b) {
                    *to = *from != 0;
                }
            }
            out
        }

        /// The loaded transmitters (cables and pipes) of this level.
        fn collect_transmitters(
            &mut self,
            kinds: &[horizon_mods::BlockEntityKind],
        ) -> Vec<horizon_mods::network::Transmitter> {
            use horizon_mods::network::{Medium, Transmitter};
            // A cable's entity outlives its block (the template has no entity to remove
            // with it): drop the ones whose block is another now.
            let marks: Vec<(i32, i32, i32)> = self
                .world
                .block_entities
                .iter_mut()
                .filter_map(|(pos, e)| {
                    let horizon_client_render::containers::BlockEntity::Mod(m) = e else {
                        return None;
                    };
                    kinds
                        .iter()
                        .any(|k| k.block == m.id && k.transmitter.is_some())
                        .then_some((pos.0, pos.1, pos.2))
                })
                .collect();
            for pos in marks {
                let id = match self.world.block_entities.get(&pos) {
                    Some(horizon_client_render::containers::BlockEntity::Mod(m)) => m.id.clone(),
                    _ => continue,
                };
                let at = self.world.block(pos.0, pos.1, pos.2);
                if self.res.facts().get(at).name.as_ref() != id.as_str() {
                    self.world.block_entities.remove(&pos);
                    self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                }
            }
            self.world
                .block_entities
                .iter_mut()
                .filter_map(|(pos, e)| {
                    let horizon_client_render::containers::BlockEntity::Mod(m) = e else {
                        return None;
                    };
                    let spec = kinds.iter().find(|k| k.block == m.id)?.transmitter.as_ref()?;
                    Some(Transmitter {
                        pos: [pos.0, pos.1, pos.2],
                        medium: match &spec.medium {
                            horizon_mods::TransmitterMedium::Energy => Medium::Energy,
                            horizon_mods::TransmitterMedium::Tank(kind) => Medium::Tank(kind.clone()),
                            horizon_mods::TransmitterMedium::Items => Medium::Items,
                        },
                        throughput: spec.throughput,
                        connect: Self::transmitter_connect(m),
                    })
                })
                .collect()
        }

        /// What a block entity's tanks hold, one per tank the kind declares.
        fn stored_tanks(
            m: &horizon_client_render::containers::ModBlockEntity,
            specs: &[horizon_mods::TankSpec],
        ) -> Vec<horizon_mods::Tank> {
            use horizon_world::nbt::Tag;
            let saved = match m.nbt.get(&Self::jname("tanks")) {
                Some(Tag::List(l)) => l.as_slice(),
                _ => &[],
            };
            specs
                .iter()
                .enumerate()
                .map(|(i, spec)| {
                    let substance = match saved.get(i).and_then(|t| t.get("substance")) {
                        Some(Tag::String(s)) => s.to_string_lossy(),
                        _ => String::new(),
                    };
                    let amount = match saved.get(i).and_then(|t| t.get("amount")) {
                        Some(Tag::Long(v)) => (*v).max(0) as u64,
                        _ => 0,
                    };
                    let amount = if substance.is_empty() { 0 } else { amount.min(spec.capacity) };
                    horizon_mods::Tank { substance, amount }
                })
                .collect()
        }
        fn tank_sides(
            m: &horizon_client_render::containers::ModBlockEntity,
            specs: &[horizon_mods::TankSpec],
        ) -> Vec<[horizon_mods::FlowSide; 6]> {
            use horizon_mods::FlowSide as S;
            use horizon_world::nbt::Tag;
            let saved = match m.nbt.get(&Self::jname("tank_faces")) {
                Some(Tag::ByteArray(b)) => b.as_slice(),
                _ => &[],
            };
            specs
                .iter()
                .enumerate()
                .map(|(i, spec)| {
                    let mut out = [S::None; 6];
                    for (f, to) in out.iter_mut().enumerate() {
                        *to = match saved.get(i * 6 + f) {
                            Some(b) => match b {
                                1 => S::Input,
                                2 => S::Output,
                                3 => S::Both,
                                _ => S::None,
                            },
                            None => spec.faces.get(f).copied().unwrap_or(S::None),
                        };
                    }
                    out
                })
                .collect()
        }
        fn put_tanks(
            m: &mut horizon_client_render::containers::ModBlockEntity,
            tanks: &[horizon_mods::Tank],
            capacities: &[u64],
        ) {
            use horizon_world::nbt::Tag;
            let list = tanks
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let cap = capacities.get(i).copied().unwrap_or(0);
                    Tag::compound([
                        ("substance", Tag::String(t.substance.as_str().into())),
                        ("amount", Tag::Long(t.amount.min(i64::MAX as u64) as i64)),
                        // For a client drawing the tank (the server alone knows it).
                        ("capacity", Tag::Long(cap.min(i64::MAX as u64) as i64)),
                    ])
                })
                .collect();
            m.nbt.insert(Self::jname("tanks"), Tag::List(list));
        }
        fn stored_energy(m: &horizon_client_render::containers::ModBlockEntity) -> u64 {
            match m.nbt.get(&horizon_world::nbt::JavaString::from("energy")) {
                Some(horizon_world::nbt::Tag::Long(v)) => (*v).max(0) as u64,
                _ => 0,
            }
        }

        /// The recipes of mods' own types and the item tags, for capability
        /// `recipe-types`: made once the world's tables are loaded, when a
        /// running server mod asks for it.
        fn lend_recipe_types(&mut self) {
            if !self.mods.mods.wants_recipe_types()
                || self.mods.mods.has_recipe_data()
                || self.res.rest.get().is_none()
            {
                return;
            }
            let recipes = &self.recipes().custom;
            let items = self.items();
            let data = horizon_mods::recipe_types::RecipeData {
                recipes: recipes
                    .iter()
                    .map(|(kind, list)| {
                        let list = list.iter().map(|r| (r.id.clone(), r.json.to_string()));
                        (kind.clone(), list.collect())
                    })
                    .collect(),
                tags: items.item_tag_order.clone(),
                items: items.items.keys().map(|k| k.to_string()).collect(),
                burn_times: items
                    .items
                    .keys()
                    .filter_map(|k| {
                        let stack = horizon_client::items::ItemStack::new(k, 1);
                        let ticks = self.res.providers().burn_time(items, &stack, false);
                        (ticks > 0).then(|| (k.to_string(), ticks as u32))
                    })
                    .collect(),
                cooking: self
                    .recipes()
                    .cooking
                    .iter()
                    .map(|c| {
                        let mut inputs: Vec<String> = c.ingredient.1.clone();
                        if inputs.is_empty() {
                            inputs = c.ingredient.0.iter().cloned().collect();
                        }
                        horizon_mods::recipe_types::Cooking {
                            kind: c.kind.trim_start_matches("minecraft:").to_owned(),
                            inputs,
                            output: c.result.item.to_string(),
                            count: c.result.count,
                            experience: c.experience,
                            time: c.time,
                        }
                    })
                    .collect(),
            };
            self.mods.mods.set_recipe_data(Arc::new(data));
        }

        /// A slot as the mod sees it: id, count and the SNBT of its components.
        fn mod_stack(stack: &horizon_client::items::ItemStack) -> horizon_mods::ModItemStack {
            let tag = horizon_client::save::item_to_nbt(stack);
            let components = tag
                .get("components")
                .map(horizon_world::snbt_text::to_snbt)
                .unwrap_or_default();
            horizon_mods::ModItemStack {
                item: stack.item.to_string(),
                count: stack.count,
                components,
            }
        }

        /// A slot from the mod: none for an item the game doesn't know, an
        /// empty stack or components that don't parse; the count is kept
        /// within the item's stack size.
        fn game_stack(
            &self,
            s: &horizon_mods::ModItemStack,
        ) -> Option<horizon_client::items::ItemStack> {
            let max = self.items().get(&s.item).map(|i| i.max_stack)?;
            if s.count == 0 {
                return None;
            }
            let mut entries = vec![
                ("id", horizon_world::nbt::Tag::String(s.item.as_str().into())),
                (
                    "count",
                    horizon_world::nbt::Tag::Int(s.count.min(max) as i32),
                ),
            ];
            if !s.components.is_empty() {
                let (components, _) = horizon_world::nbt::parse_snbt(&s.components).ok()?;
                entries.push(("components", components));
            }
            horizon_client::save::item_from_nbt(&horizon_world::nbt::Tag::compound(entries))
        }

        /// The loaded block entities (this level's) of blocks server mods
        /// gave one and asked to tick, with their saved bytes.
        fn collect_mod_entities(&mut self) -> Vec<horizon_mods::BlockEntity> {
            let kinds = self.mods.mods.block_entity_kinds();
            if !kinds.iter().any(|k| k.ticks) {
                return Vec::new();
            }
            let positions: Vec<[i32; 3]> = self
                .world
                .block_entities
                .iter_mut()
                .filter_map(|(pos, e)| {
                    let horizon_client_render::containers::BlockEntity::Mod(m) = e else {
                        return None;
                    };
                    kinds
                        .iter()
                        .any(|k| k.ticks && k.block == m.id)
                        .then_some([pos.0, pos.1, pos.2])
                })
                .take(horizon_mods::BLOCK_ENTITIES_PER_CALL)
                .collect();
            positions
                .into_iter()
                .filter_map(|p| self.snapshot_entity(p, &kinds, true))
                .collect()
        }

        /// The multiblock groups whose structure candidates in this level
        /// changed since the mods were last told, with all their structures.
        fn changed_structures(
            &mut self,
        ) -> Vec<(String, Vec<horizon_mods::multiblock::Structure>)> {
            use horizon_mods::multiblock::{Member, structures};
            let groups = self.mods.mods.multiblock_groups();
            if groups.is_empty() {
                return Vec::new();
            }
            let registry = self.pack.assets.registry;
            let dim = self.dim.id();
            let mut found: Vec<([i32; 3], String)> = Vec::new();
            for (pos, e) in self.world.block_entities.iter_mut() {
                if let horizon_client_render::containers::BlockEntity::Mod(m) = e {
                    found.push(([pos.0, pos.1, pos.2], m.id.clone()));
                }
            }
            let mut out = Vec::new();
            for (group, blocks) in groups {
                let members: Vec<Member> = found
                    .iter()
                    .filter(|(_, id)| blocks.contains(id))
                    .map(|(p, _)| Member {
                        pos: *p,
                        state: state_string(registry, self.world.block(p[0], p[1], p[2])),
                    })
                    .collect();
                let now = structures(&members);
                if self.mods.multiblock_seen.changed(dim, &group, &now) {
                    out.push((group, now));
                }
            }
            out
        }

        /// The block entity at `p` as a server mod sees it (`need_ticks`: only
        /// if its kind ticks); none unless it is a mod's of a registered kind.
        fn snapshot_entity(
            &mut self,
            p: [i32; 3],
            kinds: &[horizon_mods::BlockEntityKind],
            need_ticks: bool,
        ) -> Option<horizon_mods::BlockEntity> {
            let registry = self.pack.assets.registry;
            let dim = self.dim.id();
            let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                self.world.block_entities.get_mut(&(p[0], p[1], p[2]))
            else {
                return None;
            };
            let kind = kinds
                .iter()
                .find(|k| k.block == m.id && (k.ticks || !need_ticks))?;
            // Grown only, as the engine's own tick does.
            let slots = kind.container.as_ref().map_or(0, |c| c.slots.min(256) as usize);
            if m.items.len() < slots {
                m.items.resize(slots, None);
            }
            let data = match m.nbt.get(&horizon_world::nbt::JavaString::from("data")) {
                Some(horizon_world::nbt::Tag::ByteArray(b)) => b.clone(),
                _ => Vec::new(),
            };
            let items = m.items.iter().map(|s| s.as_ref().map(Self::mod_stack)).collect();
            let (energy, energy_faces) = match &kind.energy {
                Some(spec) => (
                    Self::stored_energy(m).min(Self::energy_capacity(m, spec)),
                    Self::energy_faces(m, spec).to_vec(),
                ),
                None => (0, Vec::new()),
            };
            let tanks = Self::stored_tanks(m, &kind.tanks);
            let tank_faces = Self::tank_sides(m, &kind.tanks)
                .into_iter()
                .map(|f| f.to_vec())
                .collect();
            let menu_data = m.data.clone();
            let transmitter_faces = if kind.transmitter.is_some() {
                Self::transmitter_connect(m).to_vec()
            } else {
                Vec::new()
            };
            Some(horizon_mods::BlockEntity {
                dimension: dim.into(),
                pos: BlockPos {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                },
                state: state_string(registry, self.world.block(p[0], p[1], p[2])),
                data,
                items,
                energy,
                energy_faces,
                tanks,
                tank_faces,
                menu_data,
                transmitter_faces,
            })
        }

        /// Keeps the bytes server mods returned for this level's block
        /// entities, and has their chunks saved.
        fn apply_mod_entity_updates(&mut self, updates: Vec<horizon_mods::BlockEntityUpdate>) {
            let dim = self.dim.id();
            for u in updates {
                if u.dimension != dim {
                    continue;
                }
                let key = (u.pos.x, u.pos.y, u.pos.z);
                if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                    self.world.block_entities.get_mut(&key)
                {
                    let same = matches!(m.nbt.get(&horizon_world::nbt::JavaString::from("data")),
                        Some(horizon_world::nbt::Tag::ByteArray(b)) if *b == u.data);
                    if !same {
                        m.nbt.insert("data".into(), horizon_world::nbt::Tag::ByteArray(u.data));
                        self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                    }
                }
                if u.energy.is_some() || u.energy_faces.is_some() {
                    let kinds = self.mods.mods.block_entity_kinds();
                    if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                        self.world.block_entities.get_mut(&key)
                        && let Some(spec) = kinds
                            .iter()
                            .find(|k| k.block == m.id)
                            .and_then(|k| k.energy.as_ref())
                    {
                        let name = |s: &str| horizon_world::nbt::JavaString::from(s);
                        if let Some(e) = u.energy {
                            let limit = Self::energy_capacity(m, spec);
                            let e = e.min(limit).min(i64::MAX as u64) as i64;
                            m.nbt.insert(name("energy"), horizon_world::nbt::Tag::Long(e));
                            m.nbt.insert(
                                name("energy_cap"),
                                horizon_world::nbt::Tag::Long(limit.min(i64::MAX as u64) as i64),
                            );
                        }
                        if let Some(f) = &u.energy_faces {
                            let bytes = f.iter().map(|s| Self::side_byte(*s)).collect();
                            m.nbt.insert(name("energy_faces"), horizon_world::nbt::Tag::ByteArray(bytes));
                        }
                        self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                    }
                }
                if u.tanks.is_some() || u.tank_faces.is_some() {
                    let kinds = self.mods.mods.block_entity_kinds();
                    if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                        self.world.block_entities.get_mut(&key)
                        && let Some(specs) = kinds.iter().find(|k| k.block == m.id).map(|k| &k.tanks)
                        && !specs.is_empty()
                    {
                        if let Some(tanks) = &u.tanks {
                            let mut now = Self::stored_tanks(m, specs);
                            for (to, from) in now.iter_mut().zip(tanks) {
                                to.substance.clone_from(&from.substance);
                                to.amount = if from.substance.is_empty() { 0 } else { from.amount };
                            }
                            for (t, spec) in now.iter_mut().zip(specs) {
                                t.amount = t.amount.min(spec.capacity);
                                if t.amount == 0 {
                                    t.substance.clear();
                                }
                            }
                            let caps: Vec<u64> = specs.iter().map(|s| s.capacity).collect();
                            Self::put_tanks(m, &now, &caps);
                        }
                        if let Some(faces) = &u.tank_faces {
                            let mut bytes = Vec::new();
                            for (i, _) in specs.iter().enumerate() {
                                let current = Self::tank_sides(m, specs)[i];
                                for f in 0..6 {
                                    let s = faces.get(i).and_then(|l| l.get(f)).copied().unwrap_or(current[f]);
                                    bytes.push(Self::side_byte(s));
                                }
                            }
                            m.nbt.insert(
                                Self::jname("tank_faces"),
                                horizon_world::nbt::Tag::ByteArray(bytes),
                            );
                        }
                        self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                    }
                }
                if let Some(faces) = &u.transmitter_faces {
                    let kinds = self.mods.mods.block_entity_kinds();
                    if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                        self.world.block_entities.get_mut(&key)
                        && kinds.iter().any(|k| k.block == m.id && k.transmitter.is_some())
                    {
                        let mut now = Self::transmitter_connect(m);
                        for (to, from) in now.iter_mut().zip(faces) {
                            *to = *from;
                        }
                        let bytes = now.iter().map(|b| u8::from(*b)).collect();
                        m.nbt.insert(
                            Self::jname("transmitter_faces"),
                            horizon_world::nbt::Tag::ByteArray(bytes),
                        );
                        self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                    }
                }
                if let Some(data) = &u.menu_data {
                    if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                        self.world.block_entities.get_mut(&key)
                        && m.data != *data
                    {
                        m.data.clone_from(data);
                        self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                    }
                }
                if let Some(items) = u.items {
                    let slots = match self.world.block_entities.get_mut(&key) {
                        Some(horizon_client_render::containers::BlockEntity::Mod(m)) => m.items.len(),
                        _ => continue,
                    };
                    let mut new: Vec<_> = items.iter().map(|s| s.as_ref().and_then(|s| self.game_stack(s))).collect();
                    new.resize(slots, None);
                    if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                        self.world.block_entities.get_mut(&key)
                        && m.items != new
                    {
                        m.items = new;
                        self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                        self.container_changed(key);
                    }
                }
            }
        }

        /// One tick of energy moving between neighbouring block entities
        /// of this level (`horizon_mods::energy`).
        fn transfer_mod_energy(&mut self) {
            let kinds = self.mods.mods.block_entity_kinds();
            if !kinds.iter().any(|k| k.energy.is_some()) {
                return;
            }
            let transmitters = self.collect_transmitters(&kinds);
            let mut nodes: Vec<horizon_mods::energy::EnergyNode> = self
                .world
                .block_entities
                .iter_mut()
                .filter_map(|(pos, e)| {
                    let horizon_client_render::containers::BlockEntity::Mod(m) = e else {
                        return None;
                    };
                    let spec = kinds.iter().find(|k| k.block == m.id)?.energy.as_ref()?;
                    Some(horizon_mods::energy::EnergyNode {
                        pos: [pos.0, pos.1, pos.2],
                        energy: Self::stored_energy(m).min(Self::energy_capacity(m, spec)),
                        capacity: Self::energy_capacity(m, spec),
                        max_receive: spec.max_receive,
                        max_extract: spec.max_extract,
                        faces: if crate::game::machines::closed_capabilities(m) & 4 != 0 { [horizon_mods::FlowSide::None; 6] } else { Self::energy_faces(m, spec) },
                    })
                })
                .collect();
            if nodes.len() < 2 {
                return;
            }
            let before: Vec<u64> = nodes.iter().map(|n| n.energy).collect();
            horizon_mods::network::transfer_energy(&transmitters, &mut nodes);
            horizon_mods::energy::transfer(&mut nodes);
            for (n, was) in nodes.iter().zip(before) {
                if n.energy == was {
                    continue;
                }
                let key = (n.pos[0], n.pos[1], n.pos[2]);
                if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                    self.world.block_entities.get_mut(&key)
                {
                    m.nbt.insert(
                        horizon_world::nbt::JavaString::from("energy"),
                        horizon_world::nbt::Tag::Long(n.energy.min(i64::MAX as u64) as i64),
                    );
                    m.nbt.insert(
                        horizon_world::nbt::JavaString::from("energy_cap"),
                        horizon_world::nbt::Tag::Long(n.capacity.min(i64::MAX as u64) as i64),
                    );
                    self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                }
            }
        }

        /// One tick of items moving along the item networks: from the output slots of mod blocks beside a network into the
        /// containers beside it that take them, up to the network's throughput, round robin over the sinks.
        fn transfer_mod_items(&mut self) {
            let kinds = self.mods.mods.block_entity_kinds();
            if !kinds
                .iter()
                .any(|k| k.transmitter.as_ref().is_some_and(|t| matches!(t.medium, horizon_mods::TransmitterMedium::Items)))
            {
                return;
            }
            // Items move every ten ticks, `throughput` of them (Mekanism's transporters pull in bursts).
            let tick = self.mods.item_turn;
            self.mods.item_turn = tick.wrapping_add(1);
            if tick % 10 != 0 {
                return;
            }
            let transmitters = self.collect_transmitters(&kinds);
            for (ends, throughput) in horizon_mods::network::item_networks(&transmitters) {
                self.run_item_network(&ends, throughput, tick / 10);
            }
        }

        /// One network's tick.
        fn run_item_network(&mut self, ends: &[([i32; 3], usize)], throughput: u64, round: usize) {
            use horizon_core::geometry::Direction;
            let face_of = |f: usize| match f {
                0 => Direction::Down,
                1 => Direction::Up,
                2 => Direction::North,
                3 => Direction::South,
                4 => Direction::West,
                _ => Direction::East,
            };
            let mut left = throughput;
            let mut turn = round;
            for &(src, src_face) in ends {
                if left == 0 {
                    break;
                }
                // The neighbour's own face toward the network.
                let from_dir = face_of(src_face ^ 1);
                if !self.mod_container_gives(src) {
                    continue;
                }
                while left > 0 {
                    let Some((slot, one)) = self.take_from_mod_container(src, from_dir) else { break };
                    let mut rest = Some(one);
                    let n = ends.len();
                    for k in 0..n {
                        let (dst, dst_face) = ends[(turn + k) % n];
                        if dst == src {
                            continue;
                        }
                        let Some(stack) = rest.take() else { break };
                        rest = self.give_to_container(dst, face_of(dst_face ^ 1), stack);
                    }
                    turn = turn.wrapping_add(1);
                    if let Some(back) = rest {
                        self.put_back_mod_container(src, slot, back);
                        break;
                    }
                    left -= 1;
                }
            }
        }

        /// One tick of fluids and chemicals moving between neighbouring
        /// block entities' tanks of this level (`horizon_mods::flow`).
        fn transfer_mod_tanks(&mut self) {
            let kinds = self.mods.mods.block_entity_kinds();
            if !kinds.iter().any(|k| !k.tanks.is_empty()) {
                return;
            }
            let transmitters = self.collect_transmitters(&kinds);
            let mut nodes: Vec<horizon_mods::flow::TankNode> = self
                .world
                .block_entities
                .iter_mut()
                .filter_map(|(pos, e)| {
                    let horizon_client_render::containers::BlockEntity::Mod(m) = e else {
                        return None;
                    };
                    let specs = &kinds.iter().find(|k| k.block == m.id)?.tanks;
                    if specs.is_empty() {
                        return None;
                    }
                    let held = Self::stored_tanks(m, specs);
                    let sides = if crate::game::machines::closed_capabilities(m) & 2 != 0 {
                        vec![[horizon_mods::FlowSide::None; 6]; specs.len()]
                    } else {
                        Self::tank_sides(m, specs)
                    };
                    let accepts = Self::tank_accepts(m, specs.len());
                    let tanks = specs
                        .iter()
                        .zip(held)
                        .zip(sides)
                        .zip(accepts)
                        .map(|(((s, h), faces), accepts)| horizon_mods::flow::TankState {
                            kind: s.kind.clone(),
                            capacity: s.capacity,
                            max_rate: s.max_rate,
                            faces,
                            substance: h.substance,
                            amount: h.amount,
                            accepts,
                        })
                        .collect();
                    Some(horizon_mods::flow::TankNode {
                        pos: [pos.0, pos.1, pos.2],
                        tanks,
                    })
                })
                .collect();
            if nodes.is_empty() {
                return;
            }
            let before = nodes.clone();
            if nodes.len() >= 2 {
                horizon_mods::network::transfer_tanks(&transmitters, &mut nodes);
                horizon_mods::flow::transfer(&mut nodes);
            }
            // A tank's output face fills an empty cauldron beside it with a bucket of water or lava (the one fluid handler vanilla
            // blocks have, NeoForge's cauldron wrapper).
            let mut cauldrons: Vec<([i32; 3], &'static str)> = Vec::new();
            for n in &mut nodes {
                for t in &mut n.tanks {
                    for (face, d) in horizon_mods::energy::FACES.iter().enumerate() {
                        let at = [n.pos[0] + d[0], n.pos[1] + d[1], n.pos[2] + d[2]];
                        if cauldrons.iter().any(|(p, _)| *p == at) {
                            continue;
                        }
                        let name = &self.res.facts().get(self.world.block(at[0], at[1], at[2])).name;
                        if let Some(full) = horizon_mods::flow::fill_cauldron(t, face, name) {
                            cauldrons.push((at, full));
                        }
                    }
                }
            }
            for (at, full) in cauldrons {
                if let Ok(state) = crate::game::render_control::parse_state(self.pack.assets.registry, full) {
                    let _ = self.place_state(at, state, false);
                }
            }
            for (n, was) in nodes.iter().zip(before) {
                if *n == was {
                    continue;
                }
                let key = (n.pos[0], n.pos[1], n.pos[2]);
                if let Some(horizon_client_render::containers::BlockEntity::Mod(m)) =
                    self.world.block_entities.get_mut(&key)
                {
                    let held: Vec<_> = n
                        .tanks
                        .iter()
                        .map(|t| horizon_mods::Tank {
                            substance: t.substance.clone(),
                            amount: t.amount,
                        })
                        .collect();
                    let caps: Vec<u64> = n.tanks.iter().map(|t| t.capacity).collect();
                    Self::put_tanks(m, &held, &caps);
                    self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                }
            }
        }

        /// Puts back the chunks [`Game::lend_world`] lent.
        fn return_world(&mut self) {
            let Some(view) = self.mods.mods.take_world() else {
                return;
            };
            let view: Box<dyn std::any::Any> = view;
            let Ok(lent) = view.downcast::<LentWorld>() else {
                return;
            };
            let mut dims = lent.dims;
            if let Some(i) = dims.iter().position(|(d, _)| *d == self.dim.id()) {
                self.world.chunks = dims.swap_remove(i).1;
            }
            for (dim, chunks) in self.levels.parked_chunks() {
                if let Some(i) = dims.iter().position(|(d, _)| *d == dim.id()) {
                    *chunks = dims.swap_remove(i).1;
                }
            }
        }

        /// Makes the blocks server mods set in the level in place, in order,
        /// as `/setblock` does. Those of other dimensions wait for their level.
        pub(in crate::game) fn apply_mod_block_writes(&mut self) {
            if self.mods.block_writes.is_empty() {
                return;
            }
            let dim = self.dim.id();
            let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.mods.block_writes)
                .into_iter()
                .partition(|(d, _)| d == dim);
            self.mods.block_writes = later;
            let registry = self.pack.assets.registry;
            for (_, b) in now {
                let state = match crate::game::render_control::parse_state(registry, &b.state) {
                    Ok(state) => state,
                    Err(e) => {
                        log::warn!("Horizon: a server mod set {}: {e}", b.state);
                        continue;
                    }
                };
                if let Err(e) = self.place_state([b.x, b.y, b.z], state, false) {
                    log::debug!(
                        "Horizon: a server mod's block at {} {} {}: {e:?}",
                        b.x,
                        b.y,
                        b.z
                    );
                }
            }
        }

        /// The acting player as a server mod sees them, if the mods heard
        /// them join.
        fn acting_server_player(&mut self) -> Option<horizon_mods::ServerPlayer> {
            let id = uuid_string(self.native_acting_id());
            self.mods.players.iter().find(|p| p.id == id).cloned()
        }

        /// The acting player used `stack` (on the block at `pos`, or in the
        /// air): true when a server mod took the use.
        pub(in crate::game) fn mod_item_use(
            &mut self,
            stack: &horizon_client::items::ItemStack,
            pos: Option<[i32; 3]>,
        ) -> bool {
            if !self.mods.server || !self.mods.mods.hears_items() {
                return false;
            }
            let Some(player) = self.acting_server_player() else {
                return false;
            };
            let stack = Self::mod_stack(stack);
            // The world is lent for the call, as it is for a tick or a command: the mod reads the block it
            // was used on.
            self.lend_world();
            let claimed = self.mods.mods.item_used(&player, &stack, pos);
            self.return_world();
            self.apply_mod_block_writes();
            claimed
        }

        /// FORK-29: the acting player right-clicked the block at `pos` holding `stack`; what a server mod that owns
        /// the block decides for the block's own use. Only blocks of a mod are asked.
        pub(in crate::game) fn mod_block_use(
            &mut self,
            pos: [i32; 3],
            stack: Option<&horizon_client::items::ItemStack>,
            sneaking: bool,
        ) -> horizon_mods::BlockUse {
            if !self.mods.server || !self.mods.mods.hears_blocks() {
                return horizon_mods::BlockUse::Default;
            }
            let name = self
                .res
                .facts()
                .get(self.world.block(pos[0], pos[1], pos[2]))
                .name
                .clone();
            // The blocks the mods declared are the ones their ids name: vanilla blocks are not theirs to decide.
            if name.starts_with("minecraft:") {
                return horizon_mods::BlockUse::Default;
            }
            let Some(player) = self.acting_server_player() else {
                return horizon_mods::BlockUse::Default;
            };
            let stack = stack.map(Self::mod_stack);
            self.lend_world();
            let answer = self.mods.mods.block_used(&player, pos, sneaking, stack.as_ref());
            self.return_world();
            self.apply_mod_block_writes();
            answer
        }

        /// `LivingIncomingDamageEvent` for the acting player: the damage a server mod lets through of `amount` of `damage_type`, before
        /// the armour and the enchantments; none when it cancels the hit. Past the guards the caller has (creative, spectator).
        pub(in crate::game) fn mod_incoming_damage(&mut self, damage_type: &str, amount: f32, causing: i32) -> Option<f32> {
            if amount <= 0.0 || !self.mods.server || !self.mods.mods.hears_damage() {
                return Some(amount);
            }
            let Some(player) = self.acting_server_player() else {
                return Some(amount);
            };
            let tags: Vec<String> = self
                .res
                .stations()
                .enchantments
                .damage_tags
                .iter()
                .filter(|(_, types)| types.contains(damage_type))
                .map(|(tag, _)| tag.to_string())
                .collect();
            let has = |t: &str| tags.iter().any(|x| x == t);
            let source = horizon_mods::DamageSource {
                damage_type: damage_type.to_owned(),
                causing: u32::try_from(causing).ok().filter(|c| *c != 0),
                is_magic: matches!(damage_type, "minecraft:magic" | "minecraft:indirect_magic"),
                is_fire: has("minecraft:is_fire"),
                is_fall: has("minecraft:is_fall"),
                is_explosion: has("minecraft:is_explosion"),
                tags,
            };
            self.lend_world();
            let left = self.mods.mods.player_damaged(&player, &source, amount);
            self.return_world();
            self.apply_mod_block_writes();
            (left > 0.0).then_some(left)
        }

        /// FORK-35: the block that the one at `pos` stands for, when a server mod says it does (its bounding blocks stand for the machine).
        pub(in crate::game) fn mod_redirect(&self, pos: [i32; 3]) -> Option<[i32; 3]> {
            if !self.mods.server {
                return None;
            }
            let horizon_client_render::containers::BlockEntity::Mod(m) =
                self.world.block_entities.get(&(pos[0], pos[1], pos[2]))?
            else {
                return None;
            };
            let spec = self.mods.mods.redirects().into_iter().find(|r| r.block == m.id)?;
            let data = match m.nbt.get(&horizon_world::nbt::JavaString::from("data")) {
                Some(horizon_world::nbt::Tag::ByteArray(b)) => b.clone(),
                _ => return None,
            };
            let target = horizon_mods::block_render::redirect_target(&data, spec.flag_at as usize, spec.pos_at as usize)?;
            // Never to itself, and never far (a corrupted position): a machine's bounding blocks are within a few blocks.
            let near = (0..3).all(|a| (target[a] - pos[a]).abs() <= 16);
            (target != pos && near).then_some(target)
        }

        /// FORK-35: whether the mods allow the mod's block `state` just placed at `pos` to stay (`BlockItem.canPlace`).
        pub(in crate::game) fn mod_can_place(&mut self, pos: [i32; 3], state: BlockStateId) -> bool {
            if !self.mods.server || !self.mods.mods.hears_blocks() {
                return true;
            }
            let name = self.res.facts().get(state).name.clone();
            if name.starts_with("minecraft:") {
                return true;
            }
            let Some(player) = self.acting_server_player() else {
                return true;
            };
            let text = state_string(self.pack.assets.registry, state);
            self.lend_world();
            let allowed = self.mods.mods.can_place(&player, pos, &text);
            self.return_world();
            allowed
        }

        /// The attack damage of the acting player's melee blow against `kind`, as a server mod makes it.
        pub(in crate::game) fn mod_hit_damage(&mut self, kind: &str, damage: f32) -> f32 {
            if !self.mods.server || !self.mods.mods.hears_damage() {
                return damage;
            }
            let Some(player) = self.acting_server_player() else {
                return damage;
            };
            let Some(stack) = self.held().cloned() else {
                return damage;
            };
            let stack = Self::mod_stack(&stack);
            self.mods.mods.item_hit_damage(&player, &stack, kind, damage)
        }

        /// A bow-like item of a mod was let go after `ticks` of drawing: false when a server mod cancels the shot.
        pub(in crate::game) fn mod_item_released(&mut self, stack: &horizon_client::items::ItemStack, ticks: u32) -> bool {
            if !self.mods.server || !self.mods.mods.hears_item_release() {
                return true;
            }
            let Some(player) = self.acting_server_player() else {
                return true;
            };
            let hand = if self.used_hand() == Some(horizon_client::first_person::Hand::Off) { "off" } else { "main" };
            let stack = Self::mod_stack(stack);
            self.mods.mods.item_released(&player, &stack, hand, ticks)
        }

        /// The arrows `ids` of the acting player's shot with `stack` exist.
        pub(in crate::game) fn mod_item_shot(&mut self, stack: &horizon_client::items::ItemStack, ids: &[u32]) {
            if !self.mods.server || !self.mods.mods.hears_item_release() {
                return;
            }
            let Some(player) = self.acting_server_player() else {
                return;
            };
            let stack = Self::mod_stack(stack);
            self.mods.mods.item_shot(&player, &stack, ids);
        }

        /// The key the open menu of mod entity `id` keeps its slots under (a vehicle's placeholder kind, FORK-10 d2).
        pub(in crate::game) fn mod_entity_menu_key(id: u32) -> (i32, i32, i32) {
            (i32::MAX - 16, crate::game::minecarts::CART_KEY_Y, id as i32)
        }

        /// A mod's `open-entity-menu`: the entity's first rows of 9 slots open as a chest for the player, under a placeholder the menu edits.
        pub(in crate::game) fn open_mod_entity_menu(&mut self, mod_id: &str, player: &str, dimension: &str, id: u32) -> bool {
            if self.remote.is_some() || dimension != self.dim.id() {
                return false;
            }
            let Some(e) = self.mod_entities.list.iter().find(|e| e.id == id && e.dimension == dimension).cloned() else {
                return false;
            };
            if e.kind.split_once(':').map(|(ns, _)| ns) != Some(mod_id) {
                return false;
            }
            let rows = (e.items.len() / 9).min(6);
            if rows == 0 {
                return false;
            }
            let uuid = u128::from_str_radix(&player.replace('-', ""), 16).unwrap_or(0);
            let Some(conn) = self.native_conn(uuid) else {
                return false;
            };
            let key = Self::mod_entity_menu_key(id);
            let items: Vec<Option<horizon_client::items::ItemStack>> =
                e.items.iter().take(rows * 9).map(|s| s.as_ref().and_then(|s| self.game_stack(s))).collect();
            self.world.block_entities.insert(key, horizon_client_render::containers::BlockEntity::Chest(items.clone()));
            let title = match &e.name {
                Some(n) => horizon_protocol::text::literal(n),
                None => horizon_protocol::text::translatable(&format!("entity.{}", e.kind.replace(':', ".")), &[]),
            };
            let Some(mut host) = self.lan.take() else {
                return false;
            };
            self.guest_close_all(&mut host, conn);
            host.hub.open_container(
                conn,
                &format!("minecraft:generic_9x{rows}"),
                title,
                key,
                horizon_client::lan::menu::Slots::Block(key),
                items,
            );
            self.lan = Some(host);
            true
        }

        /// The open menu of a mod entity went (closed, or its entity is gone): what it holds goes back to the entity's first slots.
        pub(in crate::game) fn store_mod_entity_menu(&mut self, key: (i32, i32, i32)) {
            if key.0 != i32::MAX - 16 {
                return;
            }
            let Some(horizon_client_render::containers::BlockEntity::Chest(items)) = self.world.block_entities.get(&key) else {
                return;
            };
            let shown: Vec<Option<horizon_mods::ModItemStack>> = items.iter().map(|s| s.as_ref().map(Self::mod_stack)).collect();
            if let Some(e) = self.mod_entities.list.iter_mut().find(|e| e.id as i32 == key.2) {
                for (slot, s) in e.items.iter_mut().zip(shown) {
                    *slot = s;
                }
            }
        }

        /// While a mod entity's menu is open, what the mod's tick put in the entity's slots shows in the menu.
        fn mirror_mod_entity_menus(&mut self) {
            let open: Vec<u32> = self
                .world
                .block_entities
                .keys()
                .filter(|k| k.0 == i32::MAX - 16 && k.1 == crate::game::minecarts::CART_KEY_Y)
                .map(|k| k.2 as u32)
                .collect();
            for id in open {
                let key = Self::mod_entity_menu_key(id);
                let Some(e) = self.mod_entities.list.iter().find(|e| e.id == id) else {
                    continue;
                };
                let items: Vec<Option<horizon_client::items::ItemStack>> =
                    e.items.iter().map(|s| s.as_ref().and_then(|s| self.game_stack(s))).collect();
                if let Some(horizon_client_render::containers::BlockEntity::Chest(open)) = self.world.block_entities.get_mut(&key) {
                    let changed = open.iter().zip(&items).any(|(a, b)| a != b);
                    if changed {
                        for (to, from) in open.iter_mut().zip(items) {
                            *to = from;
                        }
                        self.container_changed(key);
                    }
                }
            }
        }

        /// The box of mod entity `id` (its kind's size at its place), for the reach of its menu.
        pub(in crate::game) fn mod_entity_bounds(&self, id: i32) -> Option<horizon_core::geometry::Aabb> {
            use horizon_core::geometry::{Aabb, Vec3};
            let e = self.mod_entities.list.iter().find(|e| e.id as i32 == id)?;
            let kind = crate::game::mod_entities::MOD_KINDS.get()?.get(&e.kind)?;
            let half = f64::from(kind.width) / 2.0;
            Some(Aabb::new(
                Vec3::new(e.pos[0] - half, e.pos[1], e.pos[2] - half),
                Vec3::new(e.pos[0] + half, e.pos[1] + f64::from(kind.height), e.pos[2] + half),
            ))
        }

        /// The acting player is breaking the block `state` at `pos`: whether the mod of the held item cancels the normal break.
        pub(in crate::game) fn mod_block_break_start(&mut self, pos: [i32; 3], state: horizon_world::registry::BlockStateId) -> bool {
            if !self.mods.server || !self.mods.mods.hears_block_break() {
                return false;
            }
            let Some(stack) = self.held().cloned().filter(|s| !s.item.starts_with("minecraft:")) else {
                return false;
            };
            let Some(player) = self.acting_server_player() else {
                return false;
            };
            let text = state_string(self.pack.assets.registry, state);
            let stack = Self::mod_stack(&stack);
            self.lend_world();
            let cancel = self.mods.mods.block_break_start(&player, &stack, pos, &text);
            self.return_world();
            self.apply_mod_block_writes();
            cancel
        }

        /// The acting player right-clicked entity `entity`: when it is the box of one of a mod's own entities (FORK-10 d), whether the
        /// mod that owns the kind takes the click.
        pub(in crate::game) fn mod_entity_click(&mut self, entity: u32) -> bool {
            if !self.mods.server || !self.mods.mods.hears_mod_entities() {
                return false;
            }
            let Some(view) = self.mod_entity_of_hitbox(entity) else {
                return false;
            };
            let Some(player) = self.acting_server_player() else {
                return false;
            };
            let sneaking = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
            self.lend_world();
            let took = self.mods.mods.mod_entity_used(&player, &view, sneaking);
            self.return_world();
            self.apply_mod_block_writes();
            took
        }

        /// The acting player attacked entity `entity`: when it is the box of one of a mod's own entities (FORK-10 e), the blow is the
        /// player's weapon damage as `melee_damage` works it out, the mod may change it (`mod-entity-damaged`), the entity loses that much
        /// health and dies at 0 (`mod-entity-died`, then removed). True when the blow was for a mod entity (so the game goes no further).
        pub(in crate::game) fn mod_entity_attacked(&mut self, entity: u32) -> bool {
            if !self.mods.server || !self.mods.mods.hears_mod_entities() {
                return false;
            }
            let Some(view) = self.mod_entity_of_hitbox(entity) else {
                return false;
            };
            let (amount, ..) = self.melee_damage(&view.kind, true);
            self.reset_attack_strength();
            self.hurt_mod_entity(view.id, "minecraft:player_attack", amount);
            true
        }

        /// Mod entity `id` takes `amount` of `damage_type` (FORK-10 5a): the mod that owns its kind says how much lands
        /// (`mod-entity-damaged`), its health goes down, and at 0 the mod hears `mod-entity-died` and the entity goes. False when it
        /// is not there.
        pub(in crate::game) fn hurt_mod_entity(&mut self, id: u32, damage_type: &str, amount: f32) -> bool {
            if !self.mods.server || !self.mods.mods.hears_mod_entities() {
                return false;
            }
            let Some(view) = self.mod_entities_view().into_iter().find(|e| e.id == id) else {
                return false;
            };
            let tag = |t: &str| format!("minecraft:{t}");
            let is_fire = matches!(damage_type, "minecraft:in_fire" | "minecraft:on_fire" | "minecraft:lava" | "minecraft:hot_floor");
            let is_explosion = matches!(damage_type, "minecraft:explosion" | "minecraft:player_explosion");
            let mut tags = Vec::new();
            if is_fire {
                tags.push(tag("is_fire"));
            }
            if is_explosion {
                tags.push(tag("is_explosion"));
            }
            if damage_type == "minecraft:fall" {
                tags.push(tag("is_fall"));
            }
            if damage_type == "minecraft:player_attack" {
                tags.push(tag("is_player_attack"));
            }
            let source = horizon_mods::DamageSource {
                damage_type: damage_type.to_owned(),
                causing: None,
                is_magic: false,
                is_fire,
                is_fall: damage_type == "minecraft:fall",
                is_explosion,
                tags,
            };
            self.lend_world();
            let applied = self.mods.mods.mod_entity_damaged(&view, &source, amount);
            self.return_world();
            let dead = self.damage_mod_entity(view.id, applied);
            if dead {
                let mut last = view.clone();
                last.health = 0.0;
                self.mods.mods.mod_entity_died(&last);
                self.native_remove_mod_entity(&view.dimension, view.id);
            }
            self.apply_mod_block_writes();
            true
        }

        /// The world's own harm to the mod entities of the level in place, once a tick (FORK-10 5a): they fall (vanilla's gravity) and a
        /// fall past 3 blocks hurts (`minecraft:fall`); lava (`minecraft:lava`, 4) and fire (`minecraft:in_fire`, 1) hurt every 10 ticks
        /// and set them burning (15 and 8 seconds); burning hurts 1 a second (`minecraft:on_fire`) and water puts it out. A fire-immune
        /// kind skips the fire.
        pub(in crate::game) fn mod_entity_hazards(&mut self) {
            let Some(kinds) = crate::game::mod_entities::MOD_KINDS.get() else {
                return;
            };
            let dim = self.dim.id().to_owned();
            let mut hurts: Vec<(u32, &'static str, f32)> = Vec::new();
            for i in 0..self.mod_entities.list.len() {
                let e = &self.mod_entities.list[i];
                if e.dimension != dim || !self.world.chunks.contains_key(&((e.pos[0].floor() as i32) >> 4, (e.pos[2].floor() as i32) >> 4)) {
                    continue;
                }
                let Some(kind) = kinds.get(&e.kind) else { continue };
                let (x, z) = (e.pos[0].floor() as i32, e.pos[2].floor() as i32);
                let facts = self.res.facts();
                let world = &self.world;
                let name_at = |y: i32| -> &str { &facts.get(world.block(x, y, z)).name };
                // The floor: the top of the highest full block at or below the feet, within 64 blocks.
                let feet = (e.pos[1] - 1.0e-6).floor() as i32;
                let floor_top = (feet - 64..=feet).rev().find(|y| facts.get(world.block(x, *y, z)).full_collision).map(|y| f64::from(y + 1));
                let (y, v, fallen, landed) = horizon_mods::entity_state::fall_tick(e.pos[1], e.fall_velocity, floor_top);
                let rooms = f64::from(kind.height).ceil().max(1.0) as i32;
                let base = y.floor() as i32;
                let touching: Vec<String> = (base..base + rooms).map(|cy| name_at(cy).to_string()).collect();
                let in_lava = touching.iter().any(|n| n == "minecraft:lava");
                let in_fire = touching.iter().any(|n| matches!(n.as_str(), "minecraft:fire" | "minecraft:soul_fire"));
                let in_water = touching.iter().any(|n| n == "minecraft:water" || n == "minecraft:bubble_column");
                let fall_distance = if landed { e.fall_distance + fallen } else { e.fall_distance + fallen };
                let e = &mut self.mod_entities.list[i];
                e.pos[1] = y;
                e.fall_velocity = v;
                if landed {
                    let damage = horizon_mods::entity_state::fall_damage(fall_distance);
                    if damage > 0.0 && !in_water {
                        hurts.push((e.id, "minecraft:fall", damage));
                    }
                    e.fall_distance = 0.0;
                } else {
                    e.fall_distance = if in_water { 0.0 } else { fall_distance };
                }
                e.hurt_cooldown = e.hurt_cooldown.saturating_sub(1);
                if in_water {
                    e.burning = 0;
                } else if !kind.fire_immune {
                    if in_lava {
                        e.burning = e.burning.max(300);
                        if e.hurt_cooldown == 0 {
                            hurts.push((e.id, "minecraft:lava", 4.0));
                            e.hurt_cooldown = 10;
                        }
                    } else if in_fire {
                        e.burning = e.burning.max(160);
                        if e.hurt_cooldown == 0 {
                            hurts.push((e.id, "minecraft:in_fire", 1.0));
                            e.hurt_cooldown = 10;
                        }
                    }
                    if e.burning > 0 {
                        e.burning -= 1;
                        if e.burning % 20 == 0 && !in_lava && !in_fire {
                            hurts.push((e.id, "minecraft:on_fire", 1.0));
                        }
                    }
                }
            }
            for (id, kind, amount) in hurts {
                self.hurt_mod_entity(id, kind, amount);
            }
        }

        /// The acting player hit entity `entity` of `kind` holding `stack`.
        pub(in crate::game) fn mod_item_hit(
            &mut self,
            stack: &horizon_client::items::ItemStack,
            entity: u32,
            kind: &str,
        ) {
            if !self.mods.server || !self.mods.mods.hears_items() {
                return;
            }
            let Some(player) = self.acting_server_player() else {
                return;
            };
            let stack = Self::mod_stack(stack);
            self.mods.mods.item_hit(&player, &stack, entity, kind);
        }

        /// A chunk came into `dimension`; the mods hear it at the tick's end.
        pub(in crate::game) fn mod_chunk_event(&mut self, dimension: &str, chunk: [i32; 2], new: bool) {
            /// Chunks waiting for the tick's end at most (a big first load).
            const MAX_WAITING: usize = 4096;
            if !self.mods.server || !self.mods.mods.hears_chunks() {
                return;
            }
            if self.mods.events.len() < MAX_WAITING {
                self.mods.events.push(ModEvent::Chunk {
                    dimension: dimension.to_owned(),
                    chunk,
                    new,
                });
            }
        }

        /// A player's client half sent `data` on `channel`.
        pub(in crate::game) fn mod_net_event(&mut self, player: u128, channel: &str, data: &[u8]) {
            if !self.hears_players() {
                return;
            }
            self.mods.events.push(ModEvent::Net {
                who: Who::Uuid(player),
                channel: channel.to_owned(),
                data: data.to_vec(),
            });
        }

        /// A player pressed menu button `id` of the mod menu at `pos`: the
        /// player acting now.
        pub(in crate::game) fn mod_menu_button_event(&mut self, pos: [i32; 3], id: i32) {
            if !self.hears_players() {
                log::warn!("Horizon: a menu button reached a game that does not hear players");
                return;
            }
            // The player by their id, as an item use finds them (`acting_server_player`): the connection's guest is not
            // always in the list the tick looks it up in.
            let who = Who::Uuid(match self.mods.acting_guest {
                Some(uuid) => uuid,
                None => self.native_acting_id(),
            });
            self.mods.events.push(ModEvent::MenuButton { who, pos, id });
        }

        /// A player broke (or placed) `state` at `pos`: the player acting now.
        pub(in crate::game) fn mod_block_event(
            &mut self,
            pos: [i32; 3],
            state: BlockStateId,
            placed: bool,
        ) {
            if !self.hears_players() {
                return;
            }
            let who = Who::Uuid(self.native_acting_id());
            let state = self.state_string(state);
            self.mods.events.push(ModEvent::Block {
                who,
                pos,
                state,
                placed,
            });
        }

        /// A chat message a player sent.
        pub(in crate::game) fn mod_player_chat(&mut self, who: Who, text: &str) {
            if self.hears_players() {
                self.mods.events.push(ModEvent::Chat(who, text.to_owned()));
            }
        }

        /// `/line` typed by the window's own player in-frame: true when a
        /// server mod owns it.
        pub(in crate::game) fn local_server_mod_command(&mut self, line: &str) -> bool {
            if !self.hears_players() {
                return false;
            }
            let players = self.server_players(None);
            let Some(me) = players.first().cloned() else {
                return false;
            };
            let snapshot = self.mod_snapshot_with(players);
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world();
            let owned = self.mods.mods.server_command(&me, line);
            self.return_world();
            self.show_mod_output();
            self.apply_mod_block_writes();
            owned
        }

        /// `/line` typed by a connection's player: true when a server mod
        /// owns it, whatever the player's permission level.
        pub(in crate::game) fn guest_server_mod_command(
            &mut self,
            host: &mut LanHost,
            conn: u32,
            line: &str,
        ) -> bool {
            if !self.hears_players() {
                return false;
            }
            let Some(g) = host.hub.guest(conn) else {
                return false;
            };
            let id = uuid_string(g.uuid);
            let players = self.server_players(Some(host));
            let Some(me) = players.iter().find(|p| p.id == id).cloned() else {
                return false;
            };
            let snapshot = self.mod_snapshot_with(players);
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world();
            let owned = self.mods.mods.server_command(&me, line);
            self.return_world();
            if !owned {
                return false;
            }
            self.route_mod_output(Some(host));
            self.apply_mod_block_writes();
            true
        }

        /// Sends a server mod's line to the player with this id: the
        /// window's own chat for the local player, a system message for a
        /// connection's. `host` is the hub when it is out of `self.lan`.
        pub(in crate::game) fn tell_server_player(
            &mut self,
            host: Option<&mut LanHost>,
            id: &str,
            line: Text,
        ) {
            self.tell_server_player_as(host, id, line, false);
        }

        /// `tell_server_player`, over the hotbar (the action bar) when `overlay`.
        pub(in crate::game) fn tell_server_player_as(
            &mut self,
            host: Option<&mut LanHost>,
            id: &str,
            line: Text,
            overlay: bool,
        ) {
            let local = self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player.is_some();
            if local && uuid_string(self.player_uuid()) == id {
                self.add_mod_chat_line(line);
                return;
            }
            let Some(host) = host.or(self.lan.as_mut()) else {
                return;
            };
            let to = host
                .hub
                .guests
                .iter()
                .find(|g| g.placed && uuid_string(g.uuid) == id);
            if let Some(conn) = to.map(|g| g.conn) {
                host.hub.send_to(
                    conn,
                    &C::SystemChat(play::SystemChat {
                        content: component_nbt(&line.component()),
                        overlay,
                    }),
                );
            }
        }

        /// Routes a server mod's output: `Tell` and `Reply` to their player,
        /// `Broadcast` to everyone and the log.
        pub(in crate::game) fn route_server_output(
            &mut self,
            mut host: Option<&mut LanHost>,
            output: Output,
        ) {
            let named = |name: &str, text: String| {
                let mut line = Text::literal(format!("[{name}] ")).with_color(text::GRAY);
                line.push(Text::literal(text));
                line
            };
            match output {
                Output::Tell { name, player, text } => {
                    self.tell_server_player(host, &player, named(&name, text));
                }
                Output::Reply {
                    player,
                    text,
                    error,
                } => {
                    let line = Text::literal(text);
                    let line = if error {
                        line.with_color(text::RED)
                    } else {
                        line
                    };
                    self.tell_server_player(host, &player, line);
                }
                Output::PackBlock { name, dimension, pos, into, replacement } => {
                    if dimension != self.dim.id() || !self.native_pack_block(pos, into, replacement.as_deref()) {
                        log::debug!("Horizon: mod {name} could not pack the block at {pos:?}");
                    }
                }
                Output::UnpackBlock { name, dimension, box_pos, pos } => {
                    if dimension != self.dim.id() || !self.native_unpack_block(box_pos, pos) {
                        log::debug!("Horizon: mod {name} could not unpack into {pos:?}");
                    }
                }
                Output::EntityMenu { name, mod_id, player, dimension, id } => {
                    if !self.open_mod_entity_menu(&mod_id, &player, &dimension, id) {
                        log::debug!("Horizon: mod {name} could not open the menu of entity {id} for {player}");
                    }
                }
                Output::Radial { name, mod_id, player, entries, current } => {
                    let uuid = u128::from_str_radix(&player.replace('-', ""), 16).unwrap_or(0);
                    let data = horizon_mods::screen::encode_radial(&mod_id, &entries, current);
                    if let Err(e) = self.native_net_send_to_player(uuid, horizon_mods::screen::RADIAL_CHANNEL, &data) {
                        log::debug!("Horizon: mod {name}'s radial menu for {player}: {e}");
                    }
                }
                Output::HudLine { name, player, id, icon, text, fraction } => {
                    let uuid = u128::from_str_radix(&player.replace('-', ""), 16).unwrap_or(0);
                    let data = horizon_mods::screen::encode_hud_line(&id, &icon, &text, fraction);
                    if let Err(e) = self.native_net_send_to_player(uuid, horizon_mods::screen::HUD_CHANNEL, &data) {
                        log::debug!("Horizon: mod {name}'s HUD line for {player}: {e}");
                    }
                }
                Output::TellOverlay { name: _, player, text } => {
                    self.tell_server_player_as(host, &player, Text::literal(text), true);
                }
                Output::PlayerBody { name, player, change } => {
                    use horizon_mods::BodyChange as B;
                    let Some((target, _)) = self.mod_player(&player) else {
                        return;
                    };
                    let done = self
                        .for_player(target, |g| match change {
                            B::Air(air) => {
                                if let Some(p) = g.player.as_mut() {
                                    p.air = air;
                                }
                            }
                            B::ResetFall => {
                                if let Some(p) = g.player.as_mut() {
                                    p.fall_distance = 0.0;
                                }
                            }
                            B::ReduceEffect { effect, ticks } => {
                                let id = effect.strip_prefix("minecraft:").unwrap_or(&effect).to_owned();
                                if g.potions.effects.reduce(&id, ticks) {
                                    g.apply_effect_status();
                                }
                            }
                            B::Modifier { attribute, id, amount, operation, present } => {
                                if let Some(p) = g.player.as_mut() {
                                    let _ = p.set_modifier(&attribute, &id, amount, &operation, present);
                                }
                            }
                        })
                        .is_some();
                    if !done {
                        log::debug!("Horizon: mod {name} changed the body of a player that is not here");
                    }
                }
                Output::Broadcast { name, text } => {
                    let plain = format!("[{name}] {text}");
                    if self.options.dedicated.is_some() {
                        super::super::dedicated::log("INFO", &plain);
                    } else {
                        log::info!("{plain}");
                    }
                    for p in self.server_players(host.as_deref()) {
                        let line = named(&name, text.clone());
                        self.tell_server_player(host.as_deref_mut(), &p.id, line);
                    }
                }
                Output::SetBlocks {
                    name,
                    dimension,
                    blocks,
                } => {
                    let queued = &mut self.mods.block_writes;
                    queued.extend(blocks.into_iter().map(|b| (dimension.clone(), b)));
                    if queued.len() > MAX_QUEUED_WRITES {
                        let over = queued.len() - MAX_QUEUED_WRITES;
                        queued.drain(..over);
                        log::warn!(
                            "Horizon: mod {name} set more blocks than wait for their level: {over} dropped"
                        );
                    }
                }
                Output::Teleport {
                    name,
                    player,
                    dimension,
                    pos,
                    yaw,
                    pitch,
                } => {
                    let Some((target, now)) = self.mod_player(&player) else {
                        return;
                    };
                    if now != dimension || (host.is_some() && target != Target::Player) {
                        log::warn!(
                            "Horizon: mod {name} can move a player only within the dimension they are in"
                        );
                        return;
                    }
                    let rotation = (yaw.is_some() || pitch.is_some()).then(|| {
                        let (cy, cp) = match target {
                            Target::Player => (self.yaw, self.pitch),
                            _ => (0.0, 0.0),
                        };
                        [yaw.unwrap_or(cy), pitch.unwrap_or(cp)]
                    });
                    self.teleport_to_in_level(target, pos, rotation);
                }
                Output::SetFood { name, player, food, saturation } => {
                    let Some((target, _)) = self.mod_player(&player) else {
                        return;
                    };
                    let done = self
                        .for_player(target, |g| {
                            if let Some(p) = g.player.as_mut() {
                                p.health.food.food = food.min(20) as i32;
                                // FoodData.setSaturation: within the food level.
                                p.health.food.saturation = saturation.clamp(0.0, p.health.food.food as f32);
                            }
                        })
                        .is_some();
                    if !done {
                        log::debug!("Horizon: mod {name} set the food of a player that is not here");
                    }
                }
                Output::SetMotion { name, player, velocity } => {
                    let Some((Target::Guest(conn), _)) = self.mod_player(&player) else {
                        log::debug!("Horizon: mod {name} set the motion of a player that is not a guest");
                        return;
                    };
                    if let Some(host) = self.lan.as_mut() {
                        host.hub.set_motion(conn, velocity);
                    }
                }
                Output::SetAllowFlight { name, player, allowed } => {
                    let Some((Target::Guest(conn), _)) = self.mod_player(&player) else {
                        log::debug!("Horizon: mod {name} changed the flight of a player that is not a guest");
                        return;
                    };
                    if let Some(host) = self.lan.as_mut() {
                        host.hub.set_may_fly(conn, allowed);
                    }
                }
                Output::TakeXp {
                    name,
                    player,
                    points,
                } => {
                    let Some((target, _)) = self.mod_player(&player) else {
                        return;
                    };
                    let points = i32::try_from(points).unwrap_or(i32::MAX);
                    if self
                        .for_player(target, |g| g.change_experience(true, false, -points))
                        .is_none()
                    {
                        log::warn!("Horizon: mod {name} could not take experience from {player}");
                    }
                }
                Output::PlaySound {
                    dimension,
                    sound,
                    pos,
                    volume,
                    pitch,
                    ..
                } => {
                    if dimension != self.dim.id() {
                        return;
                    }
                    // The window's own game plays it when it simulates the world...
                    if self.options.execution_role.is_client() && self.remote.is_none() {
                        self.audio.play(&sound, Some(pos), volume, pitch);
                    }
                    // ...and the guests' games are sent it when the host is free.
                    if self.lan.is_some() || host.is_some() {
                        self.creatures.lan_sounds.push(super::super::creatures::NativeSound {
                            dimension: self.dim.id(),
                            sound,
                            at: pos,
                            source: 4, // SoundSource.BLOCKS
                            volume,
                            pitch,
                        });
                    }
                }
                Output::SetBlockProperty {
                    name,
                    dimension,
                    pos,
                    property,
                    value,
                } => {
                    if dimension == self.dim.id() {
                        self.native_set_block_property(pos, &property, &value);
                    } else {
                        log::debug!("Horizon: mod {name} set a property in another level");
                    }
                }
                Output::SetItemFaces { name, dimension, pos, faces } => {
                    if dimension == self.dim.id() {
                        self.native_set_item_faces(pos, &faces);
                    } else {
                        log::debug!("Horizon: mod {name} set item faces in another level");
                    }
                }
                Output::SpawnParticles { name, dimension, particle, pos, spread, speed, count, color, scale, max_distance } => {
                    if !self.lan_send_mod_particles(&dimension, &particle, pos, spread, speed, count, color, scale, max_distance) {
                        log::debug!("Horizon: mod {name} asked for the particle {particle}, which was not shown");
                    }
                }
                Output::BreakBlocks { name, dimension, positions, tool, silk_touch, fortune } => {
                    if dimension == self.dim.id() {
                        self.native_break_blocks(&positions, &tool, silk_touch, fortune);
                    } else {
                        log::debug!("Horizon: mod {name} broke blocks in another level");
                    }
                }
                Output::SetComparator { name, dimension, pos, level } => {
                    if dimension == self.dim.id() {
                        self.native_set_comparator(pos, level);
                    } else {
                        log::debug!("Horizon: mod {name} set a comparator output in another level");
                    }
                }
                Output::SetMenuText { name, dimension, pos, texts } => {
                    if dimension == self.dim.id() {
                        self.native_set_menu_text(pos, &texts);
                    } else {
                        log::debug!("Horizon: mod {name} set menu text in another level");
                    }
                }
                Output::SetCapabilitiesEnabled { name, dimension, pos, items, fluids, energy } => {
                    if dimension == self.dim.id() {
                        self.native_set_capabilities_enabled(pos, items, fluids, energy);
                    } else {
                        log::debug!("Horizon: mod {name} changed capabilities in another level");
                    }
                }
                Output::SetEnergyCapacity { name, dimension, pos, capacity } => {
                    if dimension == self.dim.id() {
                        self.native_set_energy_capacity(pos, capacity);
                    } else {
                        log::debug!("Horizon: mod {name} set an energy capacity in another level");
                    }
                }
                Output::SetTankFilter { name, dimension, pos, tank, accepts } => {
                    if dimension == self.dim.id() {
                        self.native_set_tank_filter(pos, tank, &accepts);
                    } else {
                        log::debug!("Horizon: mod {name} set a tank filter in another level");
                    }
                }
                Output::MineBlock { name, dimension, pos, tool, silk_touch, to, face } => {
                    if dimension == self.dim.id() {
                        self.native_mine_block(pos, &tool, silk_touch, to, face);
                    } else {
                        log::debug!("Horizon: mod {name} mined a block in another level");
                    }
                }
                Output::PullItems { name, dimension, pos, face, items, max } => {
                    if dimension == self.dim.id() {
                        self.native_pull_items(pos, face, &items, max);
                    } else {
                        log::debug!("Horizon: mod {name} pulled items in another level");
                    }
                }
                Output::SetItemEject { name, dimension, pos, on } => {
                    if dimension == self.dim.id() {
                        self.native_set_item_eject(pos, on);
                    } else {
                        log::debug!("Horizon: mod {name} set item eject in another level");
                    }
                }
                Output::Chunk {
                    name,
                    id,
                    dimension,
                    chunk,
                    forced,
                } => {
                    /// Chunks one mod may keep loaded.
                    const PER_MOD: usize = 256;
                    let key = (chunk[0], chunk[1]);
                    if dimension != self.dim.id() {
                        log::debug!("Horizon: mod {name} kept a chunk of another level");
                        return;
                    }
                    if forced {
                        let held = self
                            .mods
                            .mod_forced
                            .values()
                            .filter(|(_, mods)| mods.contains(&id))
                            .count();
                        let was_player = self.streaming.forced.contains(&key);
                        let entry = self
                            .mods
                            .mod_forced
                            .entry(key)
                            .or_insert_with(|| (was_player, Vec::new()));
                        if !entry.1.contains(&id) {
                            if held >= PER_MOD {
                                log::warn!("Horizon: mod {name} keeps {PER_MOD} chunks loaded already");
                                return;
                            }
                            entry.1.push(id);
                        }
                        self.streaming.forced.insert(key);
                    } else if let Some((was_player, mods)) = self.mods.mod_forced.get_mut(&key) {
                        mods.retain(|m| *m != id);
                        if mods.is_empty() {
                            let was_player = *was_player;
                            self.mods.mod_forced.remove(&key);
                            // A chunk a player forced stays forced.
                            if !was_player {
                                self.streaming.forced.remove(&key);
                            }
                        }
                    }
                }
                Output::PlayerItems {
                    name,
                    player,
                    command,
                } => {
                    use horizon_mods::PlayerItemsCommand as P;
                    let Some((target, _)) = self.mod_player(&player) else {
                        return;
                    };
                    let done = match command {
                        P::SetSlot { slot, stack } => self
                            .for_player(target, |g| {
                                let made = stack.as_ref().and_then(|s| g.game_stack(s));
                                if stack.is_some() && made.is_none() {
                                    return false;
                                }
                                g.put_slot_for_mods(slot, made);
                                true
                            })
                            .unwrap_or(false),
                        P::CompareAndSet { slot, expected, replacement } => self
                            .for_player(target, |g| {
                                let now = g.inventory_for_mods();
                                if now.get(slot as usize).cloned().flatten() != expected {
                                    return false;
                                }
                                let made = replacement.as_ref().and_then(|s| g.game_stack(s));
                                if replacement.is_some() && made.is_none() {
                                    return false;
                                }
                                g.put_slot_for_mods(slot, made);
                                true
                            })
                            .unwrap_or(false),
                        P::Hurt { damage_type, amount } => self
                            .for_player(target, |g| g.hurt_player_by(&damage_type, amount))
                            .is_some(),
                        P::Effect { effect, duration, amplifier } => {
                            let id = effect_id(&effect);
                            self.for_player(target, |g| {
                                g.add_player_effect(horizon_client::effects::EffectInstance::new(
                                    id, duration, amplifier,
                                ))
                            })
                            .is_some()
                        }
                    };
                    if !done {
                        log::debug!("Horizon: mod {name}'s player request was refused");
                    }
                }
                Output::Entity {
                    name,
                    id: mod_id,
                    dimension,
                    command,
                } => {
                    use crate::game::{native_entities::NativeDisplay, native_mobs::NativeSpawn};
                    use horizon_mods::EntityCommand as C;
                    let done = match command {
                        C::SpawnMob { ticket, kind, pos, options } => {
                            let spawn = NativeSpawn {
                                name: options.name,
                                health: options.health,
                                no_ai: options.no_ai,
                                invisible: options.invisible,
                                silent: options.silent,
                                invulnerable: options.invulnerable,
                                no_gravity: options.no_gravity,
                            };
                            let entity = self.native_spawn_mob(&dimension, &kind, pos, &spawn);
                            self.mods.spawn_answers.push((mod_id, ticket, entity));
                            entity.is_some()
                        }
                        C::Summon { ticket, kind, pos, nbt } => {
                            let text = if nbt.trim().is_empty() { "{}" } else { nbt.as_str() };
                            let compound = match horizon_world::nbt::parse_snbt(text) {
                                Ok((horizon_world::nbt::Tag::Compound(c), _)) => Some(c),
                                _ => None,
                            };
                            let entity = compound
                                .and_then(|c| self.native_summon(&dimension, &kind, pos, c));
                            self.mods.spawn_answers.push((mod_id, ticket, entity));
                            entity.is_some()
                        }
                        C::Move { id, pos, yaw, pitch } => {
                            self.native_move_entity(&dimension, id, pos, yaw, pitch)
                        }
                        C::SetDisplay { id, display } => {
                            let d = NativeDisplay {
                                translation: [display.translation.0, display.translation.1, display.translation.2],
                                left_rotation: [display.left_rotation.0, display.left_rotation.1, display.left_rotation.2, display.left_rotation.3],
                                scale: [display.scale.0, display.scale.1, display.scale.2],
                                right_rotation: [display.right_rotation.0, display.right_rotation.1, display.right_rotation.2, display.right_rotation.3],
                                interpolation: display.interpolation,
                                teleport: display.teleport,
                            };
                            self.native_set_display(&dimension, id, &d)
                        }
                        C::TakeDropped { id, count } => self.native_take_dropped(&dimension, id, count),
                        C::Damage { ids, players, damage_type, amount, source, area } => {
                            let mobs = self.native_damage_mobs(&dimension, &ids, amount);
                            // The mods' own entities in the box (FORK-10 5a), as the laser hits any living entity.
                            let inside: Vec<u32> = self
                                .mod_entities
                                .list
                                .iter()
                                .filter(|e| e.dimension == dimension && (0..3).all(|i| e.pos[i] >= area.0[i] && e.pos[i] <= area.1[i]))
                                .map(|e| e.id)
                                .collect();
                            for id in inside {
                                self.hurt_mod_entity(id, &damage_type, amount);
                            }
                            let mut any = false;
                            for id in players {
                                let Some((target, dim)) = self.mod_player(&id) else { continue };
                                if dim != dimension {
                                    continue;
                                }
                                // The game's own hurt: a shield raised toward `source` blocks it and wears, armour counts.
                                let hurt = self.for_player(target, |g| {
                                    let hurt = g.hurt_player_from(amount, source, &damage_type, Some(None));
                                    if hurt && let Some(p) = g.player.as_mut() {
                                        p.fire_ticks = p.fire_ticks.max(amount as i32);
                                    }
                                    hurt
                                });
                                any |= hurt.unwrap_or(false);
                            }
                            mobs || any
                        }
                        C::SpawnModEntity { ticket, kind, pos, data } => {
                            let entity = self.native_spawn_mod_entity(&dimension, &kind, pos, data);
                            self.mods.spawn_answers.push((mod_id, ticket, entity));
                            entity.is_some()
                        }
                        C::RemoveEntity { id } => self.native_remove_entity(&dimension, id) || self.native_remove_mod_entity(&dimension, id),
                        C::TeleportMob { id, pos } => self.native_teleport_mob(&dimension, id, pos),
                        C::PushMob { id, velocity } => self.native_push_mob(&dimension, id, velocity),
                        C::SetMobHealth { id, health } => self.native_set_mob_health(&dimension, id, health),
                        C::KillMob { id } => self.native_kill_mob(&dimension, id),
                        C::RemoveMob { id } => self.native_remove_mob(&dimension, id),
                        C::SetFire { id, ticks } => self.native_set_fire(&dimension, id, ticks),
                    };
                    if !done {
                        log::debug!("Horizon: mod {name}'s entity request was refused");
                    }
                }
                Output::NetSend {
                    name,
                    player,
                    channel,
                    data,
                } => {
                    let id = u128::from_str_radix(&player.replace('-', ""), 16).unwrap_or(0);
                    if let Err(e) = self.native_net_send_to_player(id, &channel, &data) {
                        log::debug!("Horizon: mod {name}'s message to {player}: {e}");
                    }
                }
                Output::NetToServer {
                    name,
                    channel,
                    data,
                } => {
                    if let Err(e) = self.native_net_send_to_server(&channel, &data) {
                        log::debug!("Horizon: mod {name}'s message to the server: {e}");
                    }
                }
                _ => {}
            }
        }
    }
}
