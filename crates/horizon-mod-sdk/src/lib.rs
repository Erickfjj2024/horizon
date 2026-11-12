//! Write Horizon mods in Rust (`docs/mods.md`).
//!
//! A mod is a type implementing [`Mod`], exported with [`export_mod!`] from
//! a `cdylib` crate built for `wasm32-unknown-unknown`;
//! `cargo horizon-mod build` turns it into the component the game loads.
//!
//! ```ignore
//! use horizon_mod_sdk::{Mod, Registration, chat};
//!
//! struct Hello;
//! impl Mod for Hello {
//!     fn init() -> Result<(Self, Registration), String> {
//!         Ok((Hello, Registration::new().command("hello")))
//!     }
//!     fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
//!         chat::show("Hello from a mod!");
//!         Ok(None)
//!     }
//! }
//! horizon_mod_sdk::export_mod!(Hello);
//! ```
//!
//! Every interface but `log` is a capability: list the ones the mod uses in
//! `horizon-mod.toml` (`game-read`, `chat`, `hud`, `storage`, `command`, `ui`;
//! a server mod `storage` and `players`). Server mods: [`server`].
//!
//! Widgets (capability `ui`): a mod shows a screen it owns with
//! [`ui::open_screen`] and a page in Options > Mods with
//! [`ui::set_settings`]; what the player does with them comes back as
//! [`Mod::ui_select`], [`Mod::ui_closed`] and [`Mod::ui_settings_opened`].

wit_bindgen::generate!({
    path: "wit",
    world: "horizon-ui-mod",
    pub_export_macro: true,
    export_macro_name: "__export_horizon_mod",
    default_bindings_module: "horizon_mod_sdk",
});

pub use exports::horizon::mods::events::{Guest as __Guest, Registration};
pub use exports::horizon::mods::net_events::Guest as __NetEventsGuest;
pub use exports::horizon::mods::ui_events::Guest as __UiEventsGuest;
pub use horizon::mods::{
    browser, chat, command, game, hud, input, log, net_client, recipes, render, storage, ui,
};

/// The game's calls into a mod. Only the hooks [`Registration`] asks for
/// are called; the others may stay as their defaults.
pub trait Mod: Sized + 'static {
    /// Once, after loading: the mod's state and the hooks it wants. An error
    /// keeps the mod off and is shown to the player.
    fn init() -> Result<(Self, Registration), String>;
    /// Every game tick, when [`Registration::tick`] is set.
    fn tick(&mut self, _tick: u64) {}
    /// Every rendered frame, when [`Registration::frame`] is set.
    fn frame(&mut self, _seconds: f32) {}
    /// `/name args` for a command the mod registered. `Ok(Some(text))` is
    /// shown as feedback, `Err(text)` in red.
    fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
        Err("This command does nothing".into())
    }
    /// A chat line the player received, when [`Registration::chat`] is set.
    fn chat(&mut self, _text: &str) {}
    /// A row of one of the mod's `list` widgets was picked (capability
    /// `ui`): the widget's `id` and the row.
    fn ui_select(&mut self, _id: u32, _row: u32) {}
    /// The player left the mod's screen (Esc or Done; capability `ui`).
    fn ui_closed(&mut self) {}
    /// The player opened the mod's settings page (capability `ui`): call
    /// [`ui::set_settings`] for fresh widgets.
    fn ui_settings_opened(&mut self) {}
    /// Bytes the server half of the mod sent on `channel` (capability
    /// `net-client`).
    fn net_message(&mut self, _channel: &str, _data: &[u8]) {}
}

impl Registration {
    /// No commands and no hooks.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            tick: false,
            frame: false,
            chat: false,
        }
    }
    /// Adds `/name`.
    pub fn command(mut self, name: &str) -> Self {
        self.commands.push(name.into());
        self
    }
    pub fn on_tick(mut self) -> Self {
        self.tick = true;
        self
    }
    pub fn on_frame(mut self) -> Self {
        self.frame = true;
        self
    }
    pub fn on_chat(mut self) -> Self {
        self.chat = true;
        self
    }
}
impl Default for Registration {
    fn default() -> Self {
        Self::new()
    }
}

/// `log::write` at info level.
pub fn info(message: &str) {
    log::write(log::Level::Info, message);
}
/// `log::write` at warn level.
pub fn warn(message: &str) {
    log::write(log::Level::Warn, message);
}

/// Exports a [`Mod`] as the component's `events` interface. The state lives
/// in a thread local: a mod instance is single-threaded.
#[macro_export]
macro_rules! export_mod {
    ($t:ty) => {
        const _: () = {
            use ::std::cell::RefCell;
            ::std::thread_local! {
                static STATE: RefCell<Option<$t>> = const { RefCell::new(None) };
            }
            fn with(f: impl FnOnce(&mut $t)) {
                STATE.with(|s| {
                    if let Some(m) = s.borrow_mut().as_mut() {
                        f(m)
                    }
                })
            }
            struct Export;
            impl $crate::__Guest for Export {
                fn init() -> Result<$crate::Registration, String> {
                    let (m, registration) = <$t as $crate::Mod>::init()?;
                    STATE.with(|s| *s.borrow_mut() = Some(m));
                    Ok(registration)
                }
                fn tick(tick: u64) {
                    with(|m| $crate::Mod::tick(m, tick))
                }
                fn frame(seconds: f32) {
                    with(|m| $crate::Mod::frame(m, seconds))
                }
                fn command(name: String, args: String) -> Result<Option<String>, String> {
                    let mut out = Err("The mod is not running".to_string());
                    with(|m| out = $crate::Mod::command(m, &name, &args));
                    out
                }
                fn chat(text: String) {
                    with(|m| $crate::Mod::chat(m, &text))
                }
            }
            impl $crate::__UiEventsGuest for Export {
                fn select(id: u32, row: u32) {
                    with(|m| $crate::Mod::ui_select(m, id, row))
                }
                fn closed() {
                    with(|m| $crate::Mod::ui_closed(m))
                }
                fn settings_opened() {
                    with(|m| $crate::Mod::ui_settings_opened(m))
                }
            }
            impl $crate::__NetEventsGuest for Export {
                fn message(channel: String, data: Vec<u8>) {
                    with(|m| $crate::Mod::net_message(m, &channel, &data))
                }
            }
            // Only WebAssembly has the component's exports: a host build (cargo test or
            // clippy on the workspace) checks the mod without them, which macOS
            // couldn't link (its exported-symbols list cuts names at the `#`).
            #[cfg(target_arch = "wasm32")]
            $crate::__export_horizon_mod!(Export);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (
                <Export as $crate::__Guest>::init,
                <Export as $crate::__UiEventsGuest>::select,
            );
        };
    };
}

/// Server mods (manifest `side = "server"`, `api = "0.3"`, experimental): a
/// type implementing [`ServerMod`](server::ServerMod), exported with
/// [`export_server_mod!`]. It runs where the world is simulated, and reads
/// and messages the players with capability `players`.
pub mod components;
pub mod config;

pub mod server {
    wit_bindgen::generate!({
        path: "wit",
        world: "horizon-server-mod",
        pub_export_macro: true,
        export_macro_name: "__export_horizon_server_mod",
        default_bindings_module: "horizon_mod_sdk::server",
        additional_derives: [PartialEq],
        with: {
            "horizon:mods/log@0.1.0": crate::log,
            "horizon:mods/storage@0.1.0": crate::storage,
        },
    });

    /// What a mod screen's `textfield` sends the server when the player confirms it (FORK-20): a mod message on channel
    /// `<mod id>:menu_text`, which [`menu_input::decode`] reads. Needs [`Registration::on_net`].
    pub mod menu_input {
        use super::BlockPos;
        /// The block whose menu has the box, the box's widget `id` and the confirmed text.
        #[derive(Clone, Debug, PartialEq)]
        pub struct MenuInput {
            pub pos: BlockPos,
            pub id: i32,
            pub text: String,
        }
        /// The channel of the mod `mod_id`.
        pub fn channel(mod_id: &str) -> String {
            format!("{mod_id}:menu_text")
        }
        /// The message's position, widget id and text; None for bytes that are not one.
        pub fn decode(data: &[u8]) -> Option<MenuInput> {
            let int = |at: usize| data.get(at..at + 4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            Some(MenuInput {
                pos: BlockPos { x: int(0)?, y: int(4)?, z: int(8)? },
                id: int(12)?,
                text: String::from_utf8(data.get(16..)?.to_vec()).ok()?,
            })
        }
    }

    pub use exports::horizon::mods::server_events::{
        Argument, ArgumentKind, ArgumentValue, BlockEntity, BlockEntityKind, BlockEntityUpdate,
        BlockPos, BlockUse, GridAnswer, GridEntry, GridTarget, ItemMenu, ItemMenuUpdate, ModEntity, ModEntityUpdate, DamageSource, RedirectSpec, ContainerSpec, FlowSide, EnergySpec, ItemStack, Tank, TankSpec, TransmitterMedium, TransmitterSpec, MultiblockMember, MultiblockSpec, Structure, Guest as __Guest, Registration, ServerCommand,
    };
    pub use horizon::mods::players::{self, GameMode, Player};
    pub use horizon::mods::player_control::{self, Motion};
    pub use horizon::mods::world::{self, BlockChange, CollisionBox};
    pub use horizon::mods::entities::{self, Display, Mob, MobOptions};
    pub use horizon::mods::net;
    pub use horizon::mods::{chunks, player_items, world_data};
    /// The `index` of a `menu_grid_click` on a cell with no entry (past the last one): the grid itself, where Mekanism's QIO
    /// Dashboard stores the cursor's stack.
    pub const GRID_EMPTY: u32 = u32::MAX;
    pub use horizon::mods::recipe_types::{self, CookingRecipe, CustomRecipe};
    /// For reading the recipes' JSON.
    pub use serde_json;

    /// The server's calls into a mod. Only the hooks [`Registration`] asks
    /// for are called; the others may stay as their defaults.
    pub trait ServerMod: Sized + 'static {
        /// Once, after loading: the mod's state and the hooks it wants. An
        /// error keeps the mod off and is logged.
        fn init() -> Result<(Self, Registration), String>;
        /// Every server tick, when [`Registration::tick`] is set.
        fn tick(&mut self, _tick: u64) {}
        /// When [`Registration::joins`] is set.
        fn player_joined(&mut self, _player: &Player) {}
        fn player_left(&mut self, _player: &Player) {}
        /// A chat message a player sent, when [`Registration::chat`] is set.
        fn chat(&mut self, _player: &Player, _text: &str) {}
        /// `/name args` typed by `player`, parsed as the command's
        /// arguments say ([`Registration::command_with`]). `Ok(Some(text))`
        /// is their feedback, `Err(text)` their error in red.
        fn command(
            &mut self,
            _player: &Player,
            _name: &str,
            _args: &Args,
        ) -> Result<Option<String>, String> {
            Err("This command does nothing".into())
        }
        /// When [`Registration::blocks`] is set: `state` is what was there.
        fn block_broken(&mut self, _player: &Player, _pos: BlockPos, _state: &str) {}
        fn block_placed(&mut self, _player: &Player, _pos: BlockPos, _state: &str) {}
        /// A chunk came into the world (`new`: just generated), when
        /// [`Registration::on_chunks`] is set: where a mod adds its ores.
        fn chunk_loaded(&mut self, _dimension: &str, _chunk_x: i32, _chunk_z: i32, _new: bool) {}
        /// A player used `stack` (right click), on a block (`pos`) or in the air.
        /// Answer true to take the use. When [`Registration::on_items`] is set.
        fn item_used(&mut self, _player: &Player, _stack: ItemStack, _pos: Option<BlockPos>) -> bool {
            false
        }
        /// A right click on one of the mod's blocks, asked before the block's own use, when [`Registration::blocks`]
        /// is set: [`BlockUse::Default`] leaves it to the game, [`BlockUse::Pass`] skips the block's use and goes
        /// on to the held item (a block in hand is placed against it), [`BlockUse::Consume`] takes the click.
        fn block_used(&mut self, _player: &Player, _pos: BlockPos, _sneaking: bool, _stack: Option<ItemStack>) -> BlockUse {
            BlockUse::Default
        }
        /// A player is about to be hurt, before armour and enchantments (NeoForge's `LivingIncomingDamageEvent`): the damage to go on
        /// with (0 cancels). When [`Registration::on_damage`] is set.
        fn player_damaged(&mut self, _player: &Player, _source: DamageSource, amount: f32) -> f32 {
            amount
        }
        /// The attack damage of a player's melee blow against an entity of `kind`, before the attack strength scales it: the damage to
        /// use. When [`Registration::on_damage`] is set.
        fn item_hit_damage(&mut self, _player: &Player, _stack: ItemStack, _kind: &str, damage: f32) -> f32 {
            damage
        }
        /// A player let go of a bow-like item of the mod: false cancels the shot. When [`Registration::on_item_release`] is set.
        fn item_released(&mut self, _player: &Player, _stack: ItemStack, _hand: &str, _ticks_held: u32) -> bool {
            true
        }
        /// The arrows of a shot exist, with these entity ids. When [`Registration::on_item_release`] is set.
        fn item_shot(&mut self, _player: &Player, _stack: ItemStack, _projectiles: Vec<u32>) {}
        /// A player is breaking the block `state` at `pos` holding `stack`, an item of the mod: true cancels the normal break. When
        /// [`Registration::on_block_break_start`] is set.
        fn block_break_start(&mut self, _player: &Player, _stack: ItemStack, _pos: BlockPos, _state: &str) -> bool {
            false
        }
        /// A player right-clicked one of the mod's own entities: true takes the click. When [`Registration::on_mod_entities`] is set.
        fn mod_entity_used(&mut self, _player: &Player, _entity: ModEntity, _sneaking: bool) -> bool {
            false
        }
        /// One of the mod's own entities is about to take `amount` damage from `source`: the damage to apply. When
        /// [`Registration::on_mod_entities`] is set.
        fn mod_entity_damaged(&mut self, _entity: ModEntity, _source: DamageSource, amount: f32) -> f32 {
            amount
        }
        /// One of the mod's own entities died of damage. When [`Registration::on_mod_entities`] is set.
        fn mod_entity_died(&mut self, _entity: ModEntity) {}
        /// A player pressed entry `index` of the item grid of one of the mod's menus (`target`), holding `carried`: the cursor's
        /// stack after and stacks for their inventory; none leaves the cursor.
        fn menu_grid_click(
            &mut self,
            _player: &Player,
            _target: GridTarget,
            _index: u32,
            _button: u8,
            _shift: bool,
            _carried: Option<ItemStack>,
        ) -> Option<GridAnswer> {
            None
        }
        /// A player pressed one of the mod's keys (`data/<mod>/horizon/key/<name>.json`, `key` its id) while playing: true when the
        /// mod used the press.
        fn key_pressed(&mut self, _player: &Player, _key: &str) -> bool {
            false
        }
        /// Each tick while a player has one of the mod's item menus open (`players::open_item_menu`): what to change in it.
        fn item_menu_tick(&mut self, _player: &Player, _menu: ItemMenu) -> Option<ItemMenuUpdate> {
            None
        }
        /// A player pressed button `id` of one of the mod's item menus.
        fn item_menu_button(&mut self, _player: &Player, _menu: ItemMenu, _id: i32) -> Option<ItemMenuUpdate> {
            None
        }
        /// A player took `stack` from result slot `slot` of one of the mod's item menus: use up what made it.
        fn item_menu_taken(&mut self, _player: &Player, _menu: ItemMenu, _slot: u32, _stack: ItemStack) -> Option<ItemMenuUpdate> {
            None
        }
        /// A player pressed button `id` of the declared menu of one of the mod's own entities (FORK-10 5b): what to change in it.
        /// When [`Registration::on_mod_entities`] is set.
        fn entity_menu_button(&mut self, _player: &Player, _entity: ModEntity, _id: i32) -> Option<ModEntityUpdate> {
            None
        }
        /// A player took `stack` from result slot `slot` of the declared menu of one of the mod's own entities (FORK-10 5b): use up
        /// what made it. When [`Registration::on_mod_entities`] is set.
        fn entity_menu_taken(&mut self, _player: &Player, _entity: ModEntity, _slot: u32, _stack: ItemStack) -> Option<ModEntityUpdate> {
            None
        }
        /// The living entities of the mod's own kinds, each tick (FORK-10): what to do with them. When [`Registration::on_mod_entities`] is set.
        fn entities_tick(&mut self, _tick: u64, _entities: Vec<ModEntity>) -> Vec<ModEntityUpdate> {
            Vec::new()
        }
        /// A mod's block was placed at `pos`: false takes it away again (`BlockItem.canPlace`). When [`Registration::blocks`] is set.
        fn can_place(&mut self, _player: &Player, _pos: BlockPos, _state: &str) -> bool {
            true
        }
        /// A player hit an entity holding `stack`. When [`Registration::on_items`] is set.
        fn item_hit(&mut self, _player: &Player, _stack: ItemStack, _entity: u32, _kind: &str) {}
        /// The answer to `entities::spawn_mob` or `summon`: the new entity's
        /// id, or none when it could not be made.
        fn entity_spawned(&mut self, _ticket: u64, _id: Option<u32>) {}
        /// The structure candidates of a multiblock `group` in `dimension` changed
        /// (all of them now); returns what to change in member blocks.
        fn multiblock_changed(
            &mut self,
            _group: &str,
            _dimension: &str,
            _structures: Vec<Structure>,
        ) -> Vec<BlockEntityUpdate> {
            Vec::new()
        }
        /// What `player`'s client half sent on `channel`, when
        /// [`Registration::on_net`] is set. The bytes are the player's own.
        fn net_message(&mut self, _player: &Player, _channel: &str, _data: &[u8]) {}
        /// A player pressed a menu button of a block entity: what to change
        /// in it, if anything.
        fn menu_button(
            &mut self,
            _player: &Player,
            _pos: BlockPos,
            _id: i32,
            _entity: BlockEntity,
        ) -> Option<BlockEntityUpdate> {
            None
        }
        /// Each tick, for the block entities of blocks added with
        /// [`Registration::block_entity`] that tick. Return the ones whose
        /// bytes changed.
        fn block_entities_tick(
            &mut self,
            _tick: u64,
            _entities: Vec<BlockEntity>,
        ) -> Vec<BlockEntityUpdate> {
            Vec::new()
        }
    }

    impl ContainerSpec {
        /// `slots` slots that every face reaches and hoppers may fill and empty.
        pub fn open(slots: u32) -> Self {
            let all: Vec<u32> = (0..slots).collect();
            Self {
                slots,
                faces: vec![all.clone(); 6],
                inputs: all.clone(),
                outputs: all,
                menu: false,
            }
        }
    }
    impl EnergySpec {
        /// A store of `capacity` that takes and gives `rate` a tick through every face.
        pub fn open(capacity: u64, rate: u64, side: FlowSide) -> Self {
            Self {
                capacity,
                max_receive: rate,
                max_extract: rate,
                faces: vec![side; 6],
            }
        }
    }
    impl ItemStack {
        pub fn new(item: &str, count: u32) -> Self {
            Self {
                item: item.into(),
                count,
                components: String::new(),
            }
        }
    }

    impl Registration {
        /// No commands and no hooks.
        pub fn new() -> Self {
            Self {
                commands: Vec::new(),
                tick: false,
                joins: false,
                chat: false,
                blocks: false,
                block_entities: Vec::new(),
                net: false,
                items: false,
                chunks: false,
                damage: false,
                item_release: false,
                mod_entities: false,
                block_break: false,
                redirects: Vec::new(),
            }
        }
        /// `block` (`mymod:machine`) carries a block entity; `ticks` asks
        /// for it in [`ServerMod::block_entities_tick`].
        pub fn block_entity(mut self, block: &str, ticks: bool) -> Self {
            self.block_entities.push(BlockEntityKind {
                block: block.into(),
                ticks,
                container: None,
                energy: None,
                tanks: Vec::new(),
                keep_data: false,
                transmitter: None,
                multiblock: None,
            });
            self
        }
        /// Adds a block entity kind with everything set (slots, energy).
        pub fn block_entity_kind(mut self, kind: BlockEntityKind) -> Self {
            self.block_entities.push(kind);
            self
        }
        /// As [`Registration::block_entity`], with item slots.
        pub fn block_entity_with_slots(
            mut self,
            block: &str,
            ticks: bool,
            container: ContainerSpec,
        ) -> Self {
            self.block_entities.push(BlockEntityKind {
                block: block.into(),
                ticks,
                container: Some(container),
                energy: None,
                tanks: Vec::new(),
                keep_data: false,
                transmitter: None,
                multiblock: None,
            });
            self
        }
        /// Adds `/name`, taking free text ([`Args::text`]): one optional
        /// `text` argument named `args`.
        pub fn command(self, name: &str) -> Self {
            self.command_with(name, [Argument::text("args").optional()])
        }
        /// Adds `/name` with typed arguments (none: nothing after the name):
        /// players' chat boxes complete and check them, and the server
        /// parses them into [`Args`].
        pub fn command_with(
            mut self,
            name: &str,
            arguments: impl IntoIterator<Item = Argument>,
        ) -> Self {
            self.commands.push(ServerCommand {
                name: name.into(),
                arguments: arguments.into_iter().collect(),
            });
            self
        }
        pub fn on_tick(mut self) -> Self {
            self.tick = true;
            self
        }
        pub fn on_joins(mut self) -> Self {
            self.joins = true;
            self
        }
        pub fn on_chat(mut self) -> Self {
            self.chat = true;
            self
        }
        /// `block` stands for another block while byte `flag_at` of its entity's data is 1: the three little-endian whole numbers at `pos_at`
        /// are that block's position (FORK-35), so a click on it or breaking it reaches that block.
        pub fn redirect(mut self, block: &str, flag_at: u32, pos_at: u32) -> Self {
            self.redirects.push(RedirectSpec { block: block.into(), flag_at, pos_at });
            self
        }
        /// Call [`ServerMod::player_damaged`] and [`ServerMod::item_hit_damage`].
        pub fn on_damage(mut self) -> Self {
            self.damage = true;
            self
        }
        /// Call [`ServerMod::item_released`] and [`ServerMod::item_shot`] for bow-like items.
        pub fn on_item_release(mut self) -> Self {
            self.item_release = true;
            self
        }
        /// Call [`ServerMod::block_break_start`] when a player breaks a block holding an item of the mod.
        pub fn on_block_break_start(mut self) -> Self {
            self.block_break = true;
            self
        }
        /// Call [`ServerMod::entities_tick`] for the mod's own entity kinds.
        pub fn on_mod_entities(mut self) -> Self {
            self.mod_entities = true;
            self
        }
        /// Call [`ServerMod::chunk_loaded`].
        pub fn on_chunks(mut self) -> Self {
            self.chunks = true;
            self
        }
        /// Call [`ServerMod::item_used`] and [`ServerMod::item_hit`].
        pub fn on_items(mut self) -> Self {
            self.items = true;
            self
        }
        /// Call [`ServerMod::net_message`].
        pub fn on_net(mut self) -> Self {
            self.net = true;
            self
        }
        pub fn on_blocks(mut self) -> Self {
            self.blocks = true;
            self
        }
    }
    impl Default for Registration {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Argument {
        fn of(name: &str, kind: ArgumentKind) -> Self {
            Self {
                name: name.into(),
                kind,
                optional: false,
            }
        }
        /// One word: letters, digits and `_ - . +`.
        pub fn word(name: &str) -> Self {
            Self::of(name, ArgumentKind::Word)
        }
        /// The rest of the line: only last.
        pub fn text(name: &str) -> Self {
            Self::of(name, ArgumentKind::Text)
        }
        /// A whole number, at least `min` and at most `max` when given.
        pub fn integer(name: &str, min: Option<i32>, max: Option<i32>) -> Self {
            Self::of(name, ArgumentKind::Integer((min, max)))
        }
        pub fn number(name: &str) -> Self {
            Self::of(name, ArgumentKind::Number)
        }
        pub fn boolean(name: &str) -> Self {
            Self::of(name, ArgumentKind::Boolean)
        }
        /// One online player: a name, `@s` or `@p`.
        pub fn player(name: &str) -> Self {
            Self::of(name, ArgumentKind::Player)
        }
        /// `x y z`, each one also `~` or `~n`.
        pub fn block_pos(name: &str) -> Self {
            Self::of(name, ArgumentKind::BlockPos)
        }
        /// May be left out; optional arguments come after the required ones.
        pub fn optional(mut self) -> Self {
            self.optional = true;
            self
        }
    }

    /// A command's arguments: the line as typed, and the values the server
    /// parsed from it, by argument name.
    #[derive(Clone, Debug, Default)]
    pub struct Args {
        text: String,
        values: Vec<(String, ArgumentValue)>,
    }
    impl Args {
        pub fn new(text: String, values: Vec<(String, ArgumentValue)>) -> Self {
            Self { text, values }
        }
        /// The line after the command's name, as typed.
        pub fn text(&self) -> &str {
            &self.text
        }
        /// The value of argument `name`; `None` when it was left out.
        pub fn get(&self, name: &str) -> Option<&ArgumentValue> {
            self.values.iter().find(|(n, _)| n == name).map(|(_, v)| v)
        }
        pub fn integer(&self, name: &str) -> Option<i32> {
            match self.get(name)? {
                ArgumentValue::Integer(n) => Some(*n),
                _ => None,
            }
        }
        pub fn number(&self, name: &str) -> Option<f64> {
            match self.get(name)? {
                ArgumentValue::Number(n) => Some(*n),
                _ => None,
            }
        }
        /// A `word` or `text` argument.
        pub fn string(&self, name: &str) -> Option<&str> {
            match self.get(name)? {
                ArgumentValue::Word(s) | ArgumentValue::Text(s) => Some(s),
                _ => None,
            }
        }
        pub fn boolean(&self, name: &str) -> Option<bool> {
            match self.get(name)? {
                ArgumentValue::Boolean(b) => Some(*b),
                _ => None,
            }
        }
        pub fn player(&self, name: &str) -> Option<&Player> {
            match self.get(name)? {
                ArgumentValue::Player(p) => Some(p),
                _ => None,
            }
        }
        pub fn block_pos(&self, name: &str) -> Option<BlockPos> {
            match self.get(name)? {
                ArgumentValue::BlockPos(p) => Some(*p),
                _ => None,
            }
        }
    }
}

/// Exports a [`ServerMod`](server::ServerMod) as the component's
/// `server-events` interface, as [`export_mod!`] does for a client mod.
#[macro_export]
macro_rules! export_server_mod {
    ($t:ty) => {
        const _: () = {
            use ::std::cell::RefCell;
            use $crate::server::{
                Args, ArgumentValue, BlockEntity, BlockEntityUpdate, BlockPos, BlockUse, DamageSource, Player, Registration,
                GridAnswer, GridTarget, ItemMenu, ItemMenuUpdate, ItemStack, ModEntity, ModEntityUpdate, ServerMod, Structure,
            };
            ::std::thread_local! {
                static STATE: RefCell<Option<$t>> = const { RefCell::new(None) };
            }
            fn with(f: impl FnOnce(&mut $t)) {
                STATE.with(|s| {
                    if let Some(m) = s.borrow_mut().as_mut() {
                        f(m)
                    }
                })
            }
            struct Export;
            impl $crate::server::__Guest for Export {
                fn init() -> Result<Registration, String> {
                    let (m, registration) = <$t as ServerMod>::init()?;
                    STATE.with(|s| *s.borrow_mut() = Some(m));
                    Ok(registration)
                }
                fn tick(tick: u64) {
                    with(|m| m.tick(tick))
                }
                fn player_joined(player: Player) {
                    with(|m| m.player_joined(&player))
                }
                fn player_left(player: Player) {
                    with(|m| m.player_left(&player))
                }
                fn chat(player: Player, text: String) {
                    with(|m| m.chat(&player, &text))
                }
                fn command(
                    player: Player,
                    name: String,
                    args: String,
                    values: Vec<(String, ArgumentValue)>,
                ) -> Result<Option<String>, String> {
                    let mut out = Err("The mod is not running".to_string());
                    let args = Args::new(args, values);
                    with(|m| out = m.command(&player, &name, &args));
                    out
                }
                fn block_broken(player: Player, pos: BlockPos, state: String) {
                    with(|m| m.block_broken(&player, pos, &state))
                }
                fn block_placed(player: Player, pos: BlockPos, state: String) {
                    with(|m| m.block_placed(&player, pos, &state))
                }
                fn chunk_loaded(dimension: String, chunk_x: i32, chunk_z: i32, new: bool) {
                    with(|m| m.chunk_loaded(&dimension, chunk_x, chunk_z, new))
                }
                fn item_used(player: Player, stack: ItemStack, pos: Option<BlockPos>) -> bool {
                    let mut out = false;
                    with(|m| out = m.item_used(&player, stack, pos));
                    out
                }
                fn block_used(player: Player, pos: BlockPos, sneaking: bool, stack: Option<ItemStack>) -> BlockUse {
                    let mut out = BlockUse::Default;
                    with(|m| out = m.block_used(&player, pos, sneaking, stack));
                    out
                }
                fn player_damaged(player: Player, source: DamageSource, amount: f32) -> f32 {
                    let mut out = amount;
                    with(|m| out = m.player_damaged(&player, source, amount));
                    out
                }
                fn item_hit_damage(player: Player, stack: ItemStack, kind: String, damage: f32) -> f32 {
                    let mut out = damage;
                    with(|m| out = m.item_hit_damage(&player, stack, &kind, damage));
                    out
                }
                fn item_released(player: Player, stack: ItemStack, hand: String, ticks_held: u32) -> bool {
                    let mut out = true;
                    with(|m| out = m.item_released(&player, stack, &hand, ticks_held));
                    out
                }
                fn mod_entity_damaged(entity: ModEntity, source: DamageSource, amount: f32) -> f32 {
                    let mut out = amount;
                    with(|m| out = m.mod_entity_damaged(entity, source, amount));
                    out
                }
                fn mod_entity_died(entity: ModEntity) {
                    with(|m| m.mod_entity_died(entity));
                }
                fn menu_grid_click(
                    player: Player,
                    target: GridTarget,
                    index: u32,
                    button: u8,
                    shift: bool,
                    carried: Option<ItemStack>,
                ) -> Option<GridAnswer> {
                    let mut out = None;
                    with(|m| out = m.menu_grid_click(&player, target, index, button, shift, carried));
                    out
                }
                fn key_pressed(player: Player, key: String) -> bool {
                    let mut out = false;
                    with(|m| out = m.key_pressed(&player, &key));
                    out
                }
                fn item_menu_tick(player: Player, menu: ItemMenu) -> Option<ItemMenuUpdate> {
                    let mut out = None;
                    with(|m| out = m.item_menu_tick(&player, menu));
                    out
                }
                fn item_menu_button(player: Player, menu: ItemMenu, id: i32) -> Option<ItemMenuUpdate> {
                    let mut out = None;
                    with(|m| out = m.item_menu_button(&player, menu, id));
                    out
                }
                fn item_menu_taken(player: Player, menu: ItemMenu, slot: u32, stack: ItemStack) -> Option<ItemMenuUpdate> {
                    let mut out = None;
                    with(|m| out = m.item_menu_taken(&player, menu, slot, stack));
                    out
                }
                fn entity_menu_button(player: Player, entity: ModEntity, id: i32) -> Option<ModEntityUpdate> {
                    let mut out = None;
                    with(|m| out = m.entity_menu_button(&player, entity, id));
                    out
                }
                fn entity_menu_taken(player: Player, entity: ModEntity, slot: u32, stack: ItemStack) -> Option<ModEntityUpdate> {
                    let mut out = None;
                    with(|m| out = m.entity_menu_taken(&player, entity, slot, stack));
                    out
                }
                fn block_break_start(player: Player, stack: ItemStack, pos: BlockPos, state: String) -> bool {
                    let mut out = false;
                    with(|m| out = m.block_break_start(&player, stack, pos, &state));
                    out
                }
                fn mod_entity_used(player: Player, entity: ModEntity, sneaking: bool) -> bool {
                    let mut out = false;
                    with(|m| out = m.mod_entity_used(&player, entity, sneaking));
                    out
                }
                fn entities_tick(tick: u64, entities: Vec<ModEntity>) -> Vec<ModEntityUpdate> {
                    let mut out = Vec::new();
                    with(|m| out = m.entities_tick(tick, entities));
                    out
                }
                fn item_shot(player: Player, stack: ItemStack, projectiles: Vec<u32>) {
                    with(|m| m.item_shot(&player, stack, projectiles));
                }
                fn can_place(player: Player, pos: BlockPos, state: String) -> bool {
                    let mut out = true;
                    with(|m| out = m.can_place(&player, pos, &state));
                    out
                }
                fn item_hit(player: Player, stack: ItemStack, entity: u32, kind: String) {
                    with(|m| m.item_hit(&player, stack, entity, &kind))
                }
                fn entity_spawned(ticket: u64, id: Option<u32>) {
                    with(|m| m.entity_spawned(ticket, id))
                }
                fn multiblock_changed(
                    group: String,
                    dimension: String,
                    structures: Vec<Structure>,
                ) -> Vec<BlockEntityUpdate> {
                    let mut out = Vec::new();
                    with(|m| out = m.multiblock_changed(&group, &dimension, structures));
                    out
                }
                fn net_message(player: Player, channel: String, data: Vec<u8>) {
                    with(|m| m.net_message(&player, &channel, &data))
                }
                fn menu_button(
                    player: Player,
                    _dimension: String,
                    pos: BlockPos,
                    id: i32,
                    entity: BlockEntity,
                ) -> Option<BlockEntityUpdate> {
                    let mut out = None;
                    with(|m| out = m.menu_button(&player, pos, id, entity));
                    out
                }
                fn block_entities_tick(
                    tick: u64,
                    entities: Vec<BlockEntity>,
                ) -> Vec<BlockEntityUpdate> {
                    let mut out = Vec::new();
                    with(|m| out = m.block_entities_tick(tick, entities));
                    out
                }
            }
            #[cfg(target_arch = "wasm32")]
            $crate::server::__export_horizon_server_mod!(Export);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = <Export as $crate::server::__Guest>::init;
        };
    };
}
