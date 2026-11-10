//! Loading and calling mods with wasmtime's component model.
use crate::manifest::{Manifest, Package, Side};
use server_bindings::{
    HorizonServerMod,
    exports::horizon::mods::server_events::Registration as ServerRegistration,
    horizon::mods::{chunks, entities, net, player_control, player_items, players, recipe_types, world as mod_world},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap,
    component::{Component, HasSelf, Linker},
};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "horizon-mod",
    });
}
/// The server mods' world: `log` and `storage` are the client world's.
mod server_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "horizon-server-mod",
        additional_derives: [PartialEq],
        with: {
            "horizon:mods/log": super::bindings::horizon::mods::log,
            "horizon:mods/storage": super::bindings::horizon::mods::storage,
        },
    });
}
/// Capability `ui`'s world beyond `horizon-mod` (docs/mods.md): a second
/// world, so mods built before it (no `ui-events` export) still instantiate
/// as `horizon-mod` and simply hear nothing of their widgets.
mod ui_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "ui-host",
    });
}
pub use bindings::horizon::mods::{
    game::{Clock, Hit, Player},
    hud::{Corner, Line},
};
use bindings::{
    HorizonMod,
    exports::horizon::mods::events::Registration,
    horizon::mods::{chat, command, game, hud, log as mod_log, storage},
};
pub use server_bindings::{
    exports::horizon::mods::server_events::{
        Argument, ArgumentKind, ArgumentValue, BlockEntity, BlockEntityKind, BlockEntityUpdate,
        BlockPos, BlockUse, DamageSource, RedirectSpec, ContainerSpec, FlowSide, EnergySpec, ItemStack as ModItemStack, Tank, TankSpec, TransmitterMedium, TransmitterSpec, MultiblockMember, MultiblockSpec, Structure as ModStructure, ServerCommand,
    },
    horizon::mods::player_control::{EffectState, Motion, PlayerState},
    horizon::mods::players::{GameMode, Player as ServerPlayer},
    horizon::mods::world::{BlockChange, CollisionBox, Environment},
};
/// Block entities handed to one mod in one tick, and the most bytes one keeps.
pub const BLOCK_ENTITIES_PER_CALL: usize = 4096;
pub const BLOCK_ENTITY_BYTES: usize = 65536;
type ClientEvents = bindings::exports::horizon::mods::events::Guest;
type ServerEvents = server_bindings::exports::horizon::mods::server_events::Guest;
/// The widget model a `ui` screen or settings page shows (WIT `interface
/// ui`), for the game to draw with its menus' own widgets.
pub use ui_bindings::horizon::mods::ui::{
    ButtonWidget, ItemWidget, ListWidget, Screen as UiScreen, TextWidget, Widget as UiWidget,
};
use ui_bindings::{UiHost, horizon::mods::ui};
/// Capability `net-client`'s world beyond `horizon-mod`: an optional export, so
/// mods built before it still instantiate and simply hear nothing.
mod net_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "net-host",
    });
}
use net_bindings::{NetHost, horizon::mods::net_client};
/// Capability `render`'s imports (docs/mods.md): the game draws the shapes.
mod render_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "render-host",
    });
}
use render_bindings::horizon::mods::input;
/// Capability `input`'s records (WIT `interface input`).
pub use render_bindings::horizon::mods::input::{Event as InputEvent, HeldItem, ItemUse};
/// What capability `render` asks the game to draw (WIT `interface render`).
pub use render_bindings::horizon::mods::render::{
    Corner as RenderCorner, FarTerrain as ModFarTerrain, FillShape, FrameRate, ItemShape,
    Minimap as ModMinimap, Point as RenderPoint, QuadShape, Rect as RenderRect, Shape, SpriteShape,
    TextShape, Zoom as ModZoom,
};
use render_bindings::{RenderHost, horizon::mods::render};
/// Capability `recipes`'s imports (docs/mods.md): the game's recipes and items.
mod recipes_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "recipes-host",
    });
}
/// Capability `ui`'s recipe viewer panel (WIT `interface browser`).
pub use recipes_bindings::horizon::mods::browser::{
    Click as BrowserClick, Event as BrowserEvent, ItemKey as BrowserItemKey,
    Layout as BrowserLayout, RecipeView as BrowserRecipe, SlotClick as BrowserSlotClick,
    Stack as BrowserStack,
};
/// What capability `recipes` reads (WIT `interface recipes`).
pub use recipes_bindings::horizon::mods::recipes::{
    Item as RecipeItem, Kind as RecipeKind, Recipe, Stack as RecipeStack,
};
use recipes_bindings::{RecipesHost, horizon::mods::browser, horizon::mods::recipes};

/// A mod's linear memory cap.
const MEMORY: usize = 64 << 20;
/// Fuel (about one unit per WebAssembly instruction) per call: a bound on
/// runaway loops, far above the hooks' time budgets.
const FUEL_INIT: u64 = 2_000_000_000;
const FUEL_TICK: u64 = 50_000_000;
const FUEL_FRAME: u64 = 10_000_000;
const FUEL_EVENT: u64 = 100_000_000;
/// Blocks a mod may set per `world.set-blocks` call.
pub const BLOCKS_PER_CALL: usize = 4096;
/// Chat lines a mod may show per call, HUD lines and their length.
const CHAT_PER_CALL: usize = 8;
/// `command.run-quiet` lines one call of a mod may queue (a quiet payload's
/// cap, `horizon_protocol::quiet::MAX_LINES`).
const QUIET_PER_CALL: usize = 256;
/// How far `game.raycast` sees, in blocks.
pub const RAYCAST_MAX: f32 = 1024.0;
const HUD_LINES: usize = 16;
const LINE_CHARS: usize = 256;
/// Total bytes a mod's `storage` may hold on disk, across all its keys.
const STORAGE_QUOTA: u64 = 4 << 20;
/// Distinct keys a mod's `storage` may hold: `STORAGE_QUOTA` alone doesn't
/// bound a flood of zero- or near-zero-byte files (each still a file name,
/// a directory entry and an inode).
const STORAGE_MAX_KEYS: usize = 64;
/// Bytes `storage::set` may write across one hook call: fuel bounds a
/// mod's CPU time per call, not the time a synchronous disk write takes, so
/// a cheap-in-fuel loop replacing one key with large payloads needs its own
/// cap, separate from the on-disk quota it would otherwise stay under.
const STORAGE_WRITE_BUDGET: u64 = 1 << 20;

/// A mob as capability `entities` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct MobInfo {
    pub id: u32,
    pub kind: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub health: f32,
    pub max_health: f32,
}

/// What a mod changed in a player's body (`player-control`), made after its call.
#[derive(Clone, Debug, PartialEq)]
pub enum BodyChange {
    Air(i32),
    ReduceEffect { effect: String, ticks: i32 },
    ResetFall,
    Modifier { attribute: String, id: String, amount: f64, operation: String, present: bool },
}

/// What a mod asked of a player (capability `player-items`), made after its call.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerItemsCommand {
    SetSlot {
        slot: u32,
        stack: Option<ModItemStack>,
    },
    Hurt {
        damage_type: String,
        amount: f32,
    },
    /// `compare-and-set-slot`: the slot changes only if it holds `expected` when this is made.
    CompareAndSet {
        slot: u32,
        expected: Option<ModItemStack>,
        replacement: Option<ModItemStack>,
    },
    Effect {
        effect: String,
        duration: i32,
        amplifier: i32,
    },
}

/// What a mod asked of an entity (capability `entities`), made after its call.
#[derive(Clone, Debug, PartialEq)]
pub enum EntityCommand {
    SpawnMob {
        ticket: u64,
        kind: String,
        pos: [f64; 3],
        options: entities::MobOptions,
    },
    Summon {
        ticket: u64,
        kind: String,
        pos: [f64; 3],
        nbt: String,
    },
    Move {
        id: u32,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
    },
    SetDisplay {
        id: u32,
        display: entities::Display,
    },
    RemoveEntity {
        id: u32,
    },
    TeleportMob {
        id: u32,
        pos: [f64; 3],
    },
    PushMob {
        id: u32,
        velocity: [f64; 3],
    },
    SetMobHealth {
        id: u32,
        health: f32,
    },
    KillMob {
        id: u32,
    },
    RemoveMob {
        id: u32,
    },
}

/// What mods may read of the game, copied once per tick by the game.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub player: Option<Player>,
    pub clock: Option<Clock>,
    /// The screen in GUI pixels (capability `render`'s `gui-size`).
    pub gui_size: (f32, f32),
    /// The main hand's stack (capability `input`'s `held`).
    pub held: Option<HeldItem>,
    /// The game's frame counter: fps, min, 1% low (`render.frame-stats`).
    pub frame: (u32, u32, u32),
    /// The movement and click mappings `input.mapping` answers for:
    /// (mapping, the bound key's label, held).
    pub mappings: Vec<(&'static str, String, bool)>,
    /// Attack and use clicks in the last second.
    pub cps: (u32, u32),
    /// The HUD font's advance per character from U+0000
    /// (`render.text-width`); empty: 6 each.
    pub font_widths: Arc<[f32]>,
    /// The players on the server, for server mods.
    pub players: Vec<ServerPlayer>,
    /// What each of those players is doing, by player id, for capability
    /// `player-control`'s `motion`.
    pub motions: Vec<(String, Motion)>,
    /// The body of each of those players, by player id, for `player-control`'s `get-state`.
    pub states: Vec<(String, PlayerState)>,
    /// Where each of those players looks, `(yaw, pitch)`, by player id.
    pub looks: Vec<(String, (f32, f32))>,
    /// The living mobs of the level in place (`mobs_dimension`), for
    /// capability `entities`.
    pub mobs: Vec<MobInfo>,
    pub mobs_dimension: String,
    /// What each player carries (slots 0 to 40, see `player-items`), by player
    /// id, for capability `player-items`.
    pub inventories: Vec<(String, Vec<Option<ModItemStack>>)>,
    /// The first block along the player's view within [`RAYCAST_MAX`] and
    /// its distance from the eye, for `game.raycast`; the game works it out
    /// only while a mod calls it ([`Mods::wants_raycast`]).
    pub view_hit: Option<(f32, Hit)>,
    /// Every item and recipe, for capability `recipes`; the game makes them
    /// once, and only while a mod reads them ([`Mods::wants_recipes`]).
    pub recipes: Option<Arc<Recipes>>,
    /// The `book-id`s of the recipes the player's book holds.
    pub known_recipes: Arc<[i32]>,
    /// The open screen takes recipes (`recipes.place`).
    pub placing: bool,
    /// The player is in creative mode (`recipes.give`).
    pub creative: bool,
}

/// Capability `recipes`'s tables: `items` in the registry's order, which
/// each [`RecipeStack::item`] indexes.
#[derive(Clone, Debug, Default)]
pub struct Recipes {
    pub items: Vec<RecipeItem>,
    pub recipes: Vec<Recipe>,
}

/// Which mods a game starts: the window's game the client mods, the game
/// simulating the world the server mods, single player in the frame both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Runs {
    Client,
    Server,
    Both,
}
impl Runs {
    fn starts(self, side: Side) -> bool {
        match self {
            Runs::Both => true,
            Runs::Client => side != Side::Server,
            Runs::Server => side == Side::Server,
        }
    }
}

/// What mods asked the game to show since the last [`Mods::drain`].
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// `chat.show`: a line after the mod's name.
    Chat { name: String, text: String },
    /// A mod command's feedback, red when `error`.
    Feedback { text: String, error: bool },
    /// `command.run`: a vanilla command line, without its slash, to run as
    /// the player.
    Command { line: String },
    /// `command.run-quiet`: vanilla command lines to run as the player with
    /// nothing in the chat (`horizon_protocol::quiet` on a server).
    QuietCommands { mod_id: String, lines: Vec<String> },
    /// The mod was turned off: it trapped, overran or failed to start.
    Stopped { name: String, reason: String },
    /// `players.tell`: a line after the mod's name, to the player with this id.
    Tell {
        name: String,
        player: String,
        text: String,
    },
    /// `players.tell-overlay`: a line over the hotbar, to the player with this id.
    TellOverlay {
        name: String,
        player: String,
        text: String,
    },
    /// `players.broadcast`: a line after the mod's name, to every player.
    Broadcast { name: String, text: String },
    /// `player-control.set-air`, `reduce-effect`, `reset-fall-distance` and the attribute modifiers.
    PlayerBody {
        name: String,
        player: String,
        change: BodyChange,
    },
    /// A server mod command's feedback, to the player who typed it.
    Reply {
        player: String,
        text: String,
        error: bool,
    },
    /// `world.set-blocks`: changes to make in `dimension`, in order, each
    /// state checked by the [`WorldView`] lent for the call.
    SetBlocks {
        name: String,
        dimension: String,
        blocks: Vec<BlockChange>,
    },
    /// `player-control.teleport`: move the player as `/tp` does, in
    /// `dimension`. `yaw` and `pitch` stay as they are when `None`.
    Teleport {
        name: String,
        player: String,
        dimension: String,
        pos: [f64; 3],
        yaw: Option<f32>,
        pitch: Option<f32>,
    },
    /// `player-control.set-food`: the player's food level and saturation.
    SetFood {
        name: String,
        player: String,
        food: u32,
        saturation: f32,
    },
    /// `player-control.set-motion`: the player's velocity, in blocks a tick.
    SetMotion {
        name: String,
        player: String,
        velocity: [f64; 3],
    },
    /// `player-control.set-allow-flight`: `Abilities.mayfly` whatever the game mode.
    SetAllowFlight {
        name: String,
        player: String,
        allowed: bool,
    },
    /// `player-control.take-xp`: `Player.giveExperiencePoints(-points)`.
    TakeXp {
        name: String,
        player: String,
        points: u32,
    },
    /// `player-control.play-sound`: a sound event for the players near
    /// `pos` in `dimension`, in the blocks category.
    PlaySound {
        name: String,
        dimension: String,
        sound: String,
        pos: [f64; 3],
        volume: f32,
        pitch: f32,
    },
    /// `world.set-block-property`: one property of a block, its entity kept.
    SetBlockProperty {
        name: String,
        dimension: String,
        pos: [i32; 3],
        property: String,
        value: String,
    },
    /// `world.set-item-faces`: the slots each face of a block entity's container reaches.
    SetItemFaces {
        name: String,
        dimension: String,
        pos: [i32; 3],
        faces: Vec<Vec<u32>>,
    },
    /// `world.spawn-particles`: a particle shown to the players near a position.
    SpawnParticles {
        name: String,
        dimension: String,
        particle: String,
        pos: [f64; 3],
        spread: [f32; 3],
        speed: f32,
        count: u32,
        color: Option<u32>,
        scale: f32,
        max_distance: f32,
    },
    /// `world.pack-block`: the block at `pos` into the mod's block entity at `into`.
    PackBlock { name: String, dimension: String, pos: [i32; 3], into: [i32; 3] },
    /// `world.unpack-block`: what the box at `box_pos` holds, back at `pos`.
    UnpackBlock { name: String, dimension: String, box_pos: [i32; 3], pos: [i32; 3] },
    /// `world.set-capabilities-enabled`: which of a block entity's capabilities the outside can use.
    SetCapabilitiesEnabled {
        name: String,
        dimension: String,
        pos: [i32; 3],
        items: bool,
        fluids: bool,
        energy: bool,
    },
    /// `world.mine-block`: a block broken with a tool, its drops into a container.
    MineBlock {
        name: String,
        dimension: String,
        pos: [i32; 3],
        tool: String,
        silk_touch: bool,
        to: [i32; 3],
        face: u32,
    },
    /// `world.pull-items`: items from the container beside a face of a block entity into its own.
    PullItems {
        name: String,
        dimension: String,
        pos: [i32; 3],
        face: u32,
        items: Vec<String>,
        max: u32,
    },
    /// `world.set-item-eject`: a block entity pushing its output items out, or not.
    SetItemEject {
        name: String,
        dimension: String,
        pos: [i32; 3],
        on: bool,
    },
    /// `world.set-tank-filter`: what one tank of a block entity takes in.
    SetTankFilter {
        name: String,
        dimension: String,
        pos: [i32; 3],
        tank: u32,
        accepts: Vec<String>,
    },
    /// `world.set-energy-capacity`: how much one block entity holds at most (0: its kind's).
    SetEnergyCapacity {
        name: String,
        dimension: String,
        pos: [i32; 3],
        capacity: u64,
    },
    /// `chunks.set-forced` of mod `id`.
    Chunk {
        name: String,
        id: String,
        dimension: String,
        chunk: [i32; 2],
        forced: bool,
    },
    /// A `player-items` request for a player.
    PlayerItems {
        name: String,
        player: String,
        command: PlayerItemsCommand,
    },
    /// An `entities` request of mod `id`, for `dimension`.
    Entity {
        name: String,
        id: String,
        dimension: String,
        command: EntityCommand,
    },
    /// `net.send`: bytes for a player's client half, on a channel of the mod.
    NetSend {
        name: String,
        player: String,
        channel: String,
        data: Vec<u8>,
    },
    /// `net-client.send-to-server`: bytes for the server half.
    NetToServer {
        name: String,
        channel: String,
        data: Vec<u8>,
    },
    /// `ui.open-screen`: the mod's entry in `entries()`; what it shows is
    /// [`Mods::screen`].
    OpenScreen { owner: usize },
    /// `ui.close-screen`: the mod closed its own screen (no `closed` back).
    CloseScreen { owner: usize },
    /// `render.shake`: strength 0 to 1, fading out over `seconds` (≤ 10).
    Shake { strength: f32, seconds: f32 },
    /// `recipes.place`: the recipe book's click on this display id, all of
    /// it when `all`.
    PlaceRecipe { book_id: i32, all: bool },
    /// `recipes.give`: a stack for a creative player.
    Give {
        item: String,
        count: u32,
        components: String,
    },
}

/// The game's world as capability `world` reads it during a call: the
/// game lends one with [`Mods::lend_world`] and takes it back after.
pub trait WorldView: std::any::Any + Send {
    /// The block state at a position, `minecraft:oak_log[axis=y]`; `None`
    /// where no chunk of that dimension is loaded.
    fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String>;
    /// The biome at a position, `minecraft:plains`; `None` as for blocks.
    fn biome(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String>;
    /// The sky and weather at a position; `None` as for blocks.
    fn environment(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<Environment>;
    /// The redstone signal at a mod block entity's position, 0 to 15.
    fn redstone(&self, dimension: &str, x: i32, y: i32, z: i32) -> u8;
    /// The collision boxes of the block at a position, relative to the
    /// block's corner; `None` where no chunk of that dimension is loaded.
    fn collision(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<Vec<CollisionBox>>;
    /// Whether `dimension` is one the server has.
    fn has_dimension(&self, dimension: &str) -> bool;
    /// `Err` (why) for a state string that names no block state.
    fn check_state(&self, state: &str) -> Result<(), String>;
    /// The first crafting recipe the 3 by 3 grid makes (`crafting-find`).
    fn crafting_find(&self, _grid: &[Option<ModItemStack>]) -> Option<CraftMatch> {
        None
    }
    /// Whether crafting recipe `id` accepts the grid (`crafting-matches`).
    fn crafting_matches(&self, _id: &str, _grid: &[Option<ModItemStack>]) -> bool {
        false
    }
    /// An item's stack size (`item-max-stack-size`).
    fn item_max_stack(&self, _item: &str) -> Option<u32> {
        None
    }
}

/// A crafting recipe a grid makes (`recipe-types.craft-match`).
pub use server_bindings::horizon::mods::recipe_types::CraftMatch;

/// The view lent for the current calls, shared by every mod's store.
type WorldSlot = Arc<Mutex<Option<Box<dyn WorldView>>>>;
/// The recipes of mods' own types and the tags that resolve their ingredients.
type RecipeSlot = Arc<Mutex<Option<Arc<crate::recipe_types::RecipeData>>>>;

/// A mod's text panel, drawn every frame until it changes.
#[derive(Clone, Debug)]
pub struct Panel {
    pub corner: Corner,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Installed and turned off by the player.
    Off,
    Running,
    /// Turned on, and run by another game: a server mod in the window while
    /// the world runs on its server thread.
    Elsewhere,
    /// Could not load or start, or stopped while running.
    Failed(String),
}

/// One installed mod, for the Mods page.
pub struct Entry {
    pub path: PathBuf,
    /// None when the manifest could not be read (`status` says why).
    pub manifest: Option<Manifest>,
    pub status: Status,
    /// The mod's average time per call of its busiest hook.
    pub cost: Duration,
    run: Option<Running>,
    modified: Option<SystemTime>,
}
impl Entry {
    /// The running mod has a settings page (capability `ui` set widgets
    /// with `ui.set-settings`, or `input` registered keys the player may
    /// bind there): its row in Options > Mods gains a button.
    pub fn has_ui_settings(&self) -> bool {
        self.run.as_ref().is_some_and(|r| {
            let d = r.store.data();
            !d.ui.settings.is_empty() || !d.input.keys.is_empty()
        })
    }
}

struct Running {
    store: Store<State>,
    /// The mod's `ui-events` export; none for a mod built before `ui`, or a
    /// server mod.
    ui: Option<UiHost>,
    /// The mod's `net-events` export; none for a mod built before `net-client`.
    net: Option<NetHost>,
    hooks: Hooks,
    /// It imports `game.raycast`.
    raycasts: bool,
    /// It imports capability `recipes`.
    reads_recipes: bool,
}

/// A running mod's exports and the hooks its `init` asked for.
enum Hooks {
    Client(HorizonMod, Registration),
    Server(HorizonServerMod, ServerRegistration),
}
impl Hooks {
    fn client(&self) -> Option<&Registration> {
        match self {
            Hooks::Client(_, r) => Some(r),
            Hooks::Server(..) => None,
        }
    }
    fn server(&self) -> Option<&ServerRegistration> {
        match self {
            Hooks::Server(_, r) => Some(r),
            Hooks::Client(..) => None,
        }
    }
    fn commands(&self) -> Vec<&str> {
        match self {
            Hooks::Client(_, r) => r.commands.iter().map(String::as_str).collect(),
            Hooks::Server(_, r) => r.commands.iter().map(|c| c.name.as_str()).collect(),
        }
    }
}

/// What capability `ui` pushed since the hooks last ran.
#[derive(Default)]
struct UiState {
    /// The screen `ui.open-screen` last pushed, until `ui.close-screen`.
    screen: Option<UiScreen>,
    /// The widgets `ui.set-settings` last pushed (the mod's page in
    /// Options > Mods).
    settings: Vec<UiWidget>,
    /// The calls since the last hook ended, for [`Output`].
    open: bool,
    close: bool,
}

/// Capability `ui`'s panel beside container screens (`browser`).
#[derive(Default)]
struct BrowserState {
    items: Vec<BrowserStack>,
    /// Which `set-items` or `set-recipes` call the panel is from, unique
    /// across mods and reloads, so the game converts them once.
    version: u64,
    /// `set-recipes`' title and recipes, shown instead of the items.
    recipes: Option<RecipePage>,
    events: std::collections::VecDeque<BrowserEvent>,
}
/// What `browser.set-recipes` shows: a title, tabs (`tab` the selected
/// one) and the recipes.
#[derive(Clone, Debug)]
pub struct RecipePage {
    pub title: String,
    pub tabs: Vec<BrowserStack>,
    pub tab: u32,
    pub recipes: Vec<BrowserRecipe>,
}
static BROWSER_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// Items a panel may hold, and events kept for `browser.poll`.
const BROWSER_ITEMS: usize = 8192;
/// `recipes.place` and `give` calls a hook call may make.
const RECIPE_ACTIONS: usize = 36;
/// Recipes `set-recipes` may show, slots in one, and stacks a slot cycles.
const BROWSER_RECIPES: usize = 256;
const RECIPE_SLOTS: usize = 9;
const RECIPE_TABS: usize = 16;
const SLOT_STACKS: usize = 64;
const BROWSER_EVENTS: usize = 256;
impl BrowserState {
    fn push(&mut self, event: BrowserEvent) {
        if self.events.len() == BROWSER_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

struct State {
    name: String,
    id: String,
    snapshot: Arc<Snapshot>,
    out: Vec<Output>,
    panel: Option<Panel>,
    ui: UiState,
    /// Capability `render`'s HUD shapes, drawn until replaced.
    shapes: Vec<Shape>,
    /// Capability `render`'s corner panel.
    corner: Option<CornerPanel>,
    /// Capability `render`'s zoom, when the mod zooms.
    zoom: Option<ModZoom>,
    /// Capability `render`'s minimap, when the mod shows one.
    minimap: Option<ModMinimap>,
    /// Capability `render`'s far terrain, when the mod draws one.
    far_terrain: Option<ModFarTerrain>,
    input: InputState,
    browser: BrowserState,
    /// Capability `storage`'s folder; made on first `set`.
    data_dir: PathBuf,
    /// Bytes `storage::set` may still write before this hook call ends
    /// (reset per call in [`Mods::call`]): fuel bounds a mod's CPU time but
    /// not disk I/O time, so a loop of small-fuel, large-payload `set`s
    /// needs its own budget.
    storage_budget: u64,
    limits: StoreLimits,
    world: WorldSlot,
    recipe_data: RecipeSlot,
    /// The next ticket `entities.spawn-mob` and `summon` hand out.
    next_ticket: u64,
}

/// The installed mods and the engine running them. `Mods::default()` is
/// the state with none: no engine, and every hook a no-op.
#[derive(Default)]
pub struct Mods {
    engine: Option<(Engine, Linker<State>)>,
    entries: Vec<Entry>,
    snapshot: Arc<Snapshot>,
    out: Vec<Output>,
    world: WorldSlot,
    recipe_data: RecipeSlot,
    /// Moves on whenever a mod starts or stops: the game resends the server
    /// mods' commands to players when it changes.
    generation: u64,
}

impl Mods {
    /// Changes whenever the running mods (so their commands) change.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Whether a running mod may use capability `world`: only then is a
    /// view worth lending.
    pub fn wants_world(&self) -> bool {
        self.entries.iter().any(|e| {
            e.status == Status::Running
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| {
                    m.capabilities.contains(&crate::Capability::World)
                        || m.capabilities.contains(&crate::Capability::PlayerControl)
                })
        })
    }
    /// Whether a running mod may use capability `recipe-types`: only then is
    /// its table worth building.
    pub fn wants_recipe_types(&self) -> bool {
        self.entries.iter().any(|e| {
            e.status == Status::Running
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&crate::Capability::RecipeTypes))
        })
    }
    /// Whether a running mod may use capability `player-items`: only then are
    /// the players' inventories worth reading each tick.
    pub fn wants_player_items(&self) -> bool {
        self.entries.iter().any(|e| {
            e.status == Status::Running
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&crate::Capability::PlayerItems))
        })
    }
    /// Whether a running server mod asked for `chunk-loaded`.
    pub fn hears_chunks(&self) -> bool {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .any(|r| r.chunks)
    }
    /// A chunk came into `dimension` (`new`: just generated).
    pub fn chunk_loaded(&mut self, dimension: &str, chunk: [i32; 2], new: bool) {
        self.each_server(
            FUEL_EVENT,
            |h| h.chunks,
            |m, s| m.call_chunk_loaded(s, dimension, chunk[0], chunk[1], new),
        );
    }
    /// Whether a running server mod asked for `item-used` and `item-hit`.
    pub fn hears_items(&self) -> bool {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .any(|r| r.items)
    }
    /// `player` used `stack` (right click), on the block at `pos` or in the
    /// air: true when a mod took the use.
    pub fn item_used(
        &mut self,
        player: &ServerPlayer,
        stack: &ModItemStack,
        pos: Option<[i32; 3]>,
    ) -> bool {
        let at = pos.map(|p| BlockPos { x: p[0], y: p[1], z: p[2] });
        let mut claimed = false;
        for i in 0..self.entries.len() {
            if !self.entries[i]
                .run
                .as_ref()
                .and_then(|r| r.hooks.server())
                .is_some_and(|r| r.items)
            {
                continue;
            }
            let answer = self.call(i, FUEL_EVENT, |run| match &run.hooks {
                Hooks::Server(m, _) => m
                    .horizon_mods_server_events()
                    .call_item_used(&mut run.store, player, stack, at),
                Hooks::Client(..) => Ok(false),
            });
            if answer == Some(true) {
                claimed = true;
                break;
            }
        }
        claimed
    }
    /// `player` right-clicked the block at `pos` (a block of a mod that asked for `blocks`), `stack` in hand: what the mod
    /// decides for the block's own use (FORK-29). The first mod that does not answer `default` decides.
    pub fn block_used(
        &mut self,
        player: &ServerPlayer,
        pos: [i32; 3],
        sneaking: bool,
        stack: Option<&ModItemStack>,
    ) -> BlockUse {
        let at = BlockPos { x: pos[0], y: pos[1], z: pos[2] };
        for i in 0..self.entries.len() {
            if !self.entries[i]
                .run
                .as_ref()
                .and_then(|r| r.hooks.server())
                .is_some_and(|r| r.blocks)
            {
                continue;
            }
            let answer = self.call(i, FUEL_EVENT, |run| match &run.hooks {
                Hooks::Server(m, _) => m
                    .horizon_mods_server_events()
                    .call_block_used(&mut run.store, player, at, sneaking, stack),
                Hooks::Client(..) => Ok(BlockUse::Default),
            });
            match answer {
                Some(BlockUse::Default) | None => {}
                Some(other) => return other,
            }
        }
        BlockUse::Default
    }
    /// The blocks the running server mods say stand for another block (FORK-35).
    pub fn redirects(&self) -> Vec<RedirectSpec> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .flat_map(|r| r.redirects.iter().cloned())
            .collect()
    }
    /// `player` placed the mod's block `state` at `pos`: false when a mod refuses it (the first refusal decides).
    pub fn can_place(&mut self, player: &ServerPlayer, pos: [i32; 3], state: &str) -> bool {
        let at = BlockPos { x: pos[0], y: pos[1], z: pos[2] };
        for i in 0..self.entries.len() {
            if !self.entries[i].run.as_ref().and_then(|r| r.hooks.server()).is_some_and(|r| r.blocks) {
                continue;
            }
            let answer = self.call(i, FUEL_EVENT, |run| match &run.hooks {
                Hooks::Server(m, _) => m.horizon_mods_server_events().call_can_place(&mut run.store, player, at, state),
                Hooks::Client(..) => Ok(true),
            });
            if answer == Some(false) {
                return false;
            }
        }
        true
    }
    /// Whether a running server mod asked for `damage`.
    pub fn hears_damage(&self) -> bool {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .any(|r| r.damage)
    }
    /// `player` is about to be hurt by `source` for `amount`: the damage to go on with (the first mod that changes it decides).
    pub fn player_damaged(&mut self, player: &ServerPlayer, source: &DamageSource, amount: f32) -> f32 {
        for i in 0..self.entries.len() {
            if !self.entries[i].run.as_ref().and_then(|r| r.hooks.server()).is_some_and(|r| r.damage) {
                continue;
            }
            let answer = self.call(i, FUEL_EVENT, |run| match &run.hooks {
                Hooks::Server(m, _) => m
                    .horizon_mods_server_events()
                    .call_player_damaged(&mut run.store, player, source, amount),
                Hooks::Client(..) => Ok(amount),
            });
            if let Some(a) = answer.filter(|a| a.is_finite() && (*a - amount).abs() > f32::EPSILON) {
                return a.min(amount.max(0.0) * 1000.0 + 1000.0);
            }
        }
        amount
    }
    /// `player`'s melee blow with `stack` against `kind` has `damage` attack damage: what to use.
    pub fn item_hit_damage(&mut self, player: &ServerPlayer, stack: &ModItemStack, kind: &str, damage: f32) -> f32 {
        for i in 0..self.entries.len() {
            if !self.entries[i].run.as_ref().and_then(|r| r.hooks.server()).is_some_and(|r| r.damage) {
                continue;
            }
            let answer = self.call(i, FUEL_EVENT, |run| match &run.hooks {
                Hooks::Server(m, _) => m
                    .horizon_mods_server_events()
                    .call_item_hit_damage(&mut run.store, player, stack, kind, damage),
                Hooks::Client(..) => Ok(damage),
            });
            if let Some(a) = answer.filter(|a| a.is_finite() && (*a - damage).abs() > f32::EPSILON) {
                return a.clamp(0.0, 1024.0);
            }
        }
        damage
    }
    /// Whether a running server mod asked for `blocks` (and so hears `block-used`).
    pub fn hears_blocks(&self) -> bool {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .any(|r| r.blocks)
    }
    /// `player` hit entity `entity` of `kind` holding `stack`.
    pub fn item_hit(&mut self, player: &ServerPlayer, stack: &ModItemStack, entity: u32, kind: &str) {
        self.each_server(
            FUEL_EVENT,
            |h| h.items,
            |m, s| m.call_item_hit(s, player, stack, entity, kind),
        );
    }
    /// Whether a running mod may use capability `entities`: only then is the
    /// level's mob list worth making each tick.
    pub fn wants_entities(&self) -> bool {
        self.entries.iter().any(|e| {
            e.status == Status::Running
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&crate::Capability::Entities))
        })
    }
    /// Whether [`Mods::set_recipe_data`] has been called.
    pub fn has_recipe_data(&self) -> bool {
        self.recipe_data.lock().is_ok_and(|s| s.is_some())
    }
    /// The tables capability `recipe-types` answers from; replaces the last.
    pub fn set_recipe_data(&mut self, data: Arc<crate::recipe_types::RecipeData>) {
        if let Ok(mut slot) = self.recipe_data.lock() {
            *slot = Some(data);
        }
    }
    /// Lends the game's world to capability `world` for the calls until
    /// [`Mods::take_world`]; with none lent, `get-block` finds nothing and
    /// `set-blocks` checks only its count.
    pub fn lend_world(&mut self, view: Box<dyn WorldView>) {
        if let Ok(mut slot) = self.world.lock() {
            *slot = Some(view);
        }
    }
    /// The view [`Mods::lend_world`] lent, back to the game.
    pub fn take_world(&mut self) -> Option<Box<dyn WorldView>> {
        self.world.lock().ok()?.take()
    }

    /// Reads every mod at `paths` and starts those `enabled` says are on,
    /// client and server mods alike. A command that `reserved` (vanilla's
    /// commands) or an earlier mod already uses is refused.
    pub fn load(
        paths: &[PathBuf],
        enabled: impl Fn(&str) -> bool,
        reserved: impl Fn(&str) -> bool,
    ) -> Mods {
        Mods::load_for(Runs::Both, paths, enabled, reserved)
    }
    /// [`Mods::load`], starting only the mods `runs` says this game runs; the
    /// others that are on are [`Status::Elsewhere`].
    pub fn load_for(
        runs: Runs,
        paths: &[PathBuf],
        enabled: impl Fn(&str) -> bool,
        reserved: impl Fn(&str) -> bool,
    ) -> Mods {
        let mut mods = Mods::default();
        for path in paths {
            let modified = modified(path);
            let mut entry = Entry {
                path: path.clone(),
                manifest: None,
                status: Status::Off,
                cost: Duration::ZERO,
                run: None,
                modified,
            };
            match Package::read(path) {
                Err(e) => entry.status = Status::Failed(e),
                Ok(package) => {
                    let on = enabled(&package.manifest.id);
                    entry.manifest = Some(package.manifest.clone());
                    if on && !runs.starts(package.manifest.side) {
                        entry.status = Status::Elsewhere;
                    } else if on {
                        // Content is read before the world loads; a mod whose
                        // content is refused stays off, with the reason.
                        let content = crate::Content::read(path, &package.manifest);
                        entry.status =
                            match content.and_then(|_| mods.start(path, &package, &reserved)) {
                                Ok(run) => {
                                    entry.run = run;
                                    Status::Running
                                }
                                Err(e) => Status::Failed(e),
                            };
                    }
                }
            }
            if let (Status::Failed(reason), Some(m)) = (&entry.status, &entry.manifest) {
                log::warn!("Horizon: mod {} did not start: {reason}", m.id);
            }
            mods.entries.push(entry);
        }
        mods
    }

    fn engine(&mut self) -> Result<&(Engine, Linker<State>), String> {
        if self.engine.is_none() {
            let mut config = Config::new();
            config.wasm_component_model(true).consume_fuel(true);
            let engine = Engine::new(&config).map_err(|e| e.to_string())?;
            let mut linker = Linker::new(&engine);
            HorizonMod::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            players::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            mod_world::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            player_control::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            recipe_types::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            net::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            entities::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            player_items::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            chunks::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            NetHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            UiHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            RenderHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            RecipesHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            self.engine = Some((engine, linker));
        }
        Ok(self.engine.as_ref().expect("made above"))
    }

    /// Starts a mod's code; `Ok(None)` for a content-only mod, which has
    /// nothing to run (the game reads its content before the world loads).
    /// `path` is the mod's own `.hmod` or folder, for capability `storage`'s
    /// per-mod directory.
    fn start(
        &mut self,
        path: &Path,
        package: &Package,
        reserved: &dyn Fn(&str) -> bool,
    ) -> Result<Option<Running>, String> {
        let m = &package.manifest;
        let Some(wasm) = &package.wasm else {
            return Ok(None);
        };
        // Two running mods with the same id would share one `data_dir`
        // (capability `storage`): the first loaded keeps the id.
        if self
            .entries
            .iter()
            .any(|e| e.run.is_some() && e.manifest.as_ref().is_some_and(|other| other.id == m.id))
        {
            return Err(format!("another running mod already uses id {}", m.id));
        }
        let server = m.side == Side::Server;
        let taken: Vec<String> = self
            .entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .filter(|r| r.hooks.server().is_some() == server)
            .flat_map(|r| r.hooks.commands())
            .map(str::to_owned)
            .collect();
        let snapshot = self.snapshot.clone();
        let world = self.world.clone();
        let recipe_data = self.recipe_data.clone();
        let (engine, linker) = self.engine()?;
        let component =
            Component::new(engine, wasm).map_err(|e| format!("not a component: {e}"))?;
        let ty = component.component_type();
        let imports: Vec<String> = ty
            .imports(engine)
            .map(|(name, _)| name.to_string())
            .collect();
        m.check_imports(imports.iter().map(String::as_str))?;
        // A function this Horizon doesn't have: say so, not wasmtime's linking error.
        let functions = crate::api_level::imports(wasm)?;
        let (level, function) = crate::api_level::needed(functions.iter().map(String::as_str));
        if let Some(why) = crate::api_level::too_new(level, function) {
            return Err(why);
        }
        let data_dir = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(crate::manifest::STORAGE_DIR)
            .join(&m.id);
        let state = State {
            name: m.name.clone(),
            id: m.id.clone(),
            snapshot,
            out: Vec::new(),
            panel: None,
            ui: UiState::default(),
            shapes: Vec::new(),
            corner: None,
            zoom: None,
            minimap: None,
            far_terrain: None,
            input: InputState::default(),
            browser: BrowserState::default(),
            data_dir,
            storage_budget: STORAGE_WRITE_BUDGET,
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY)
                .instances(16)
                .build(),
            world,
            recipe_data,
            next_ticket: 1,
        };
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(FUEL_INIT).map_err(|e| e.to_string())?;
        let instance = linker
            .instantiate(&mut store, &component)
            .map_err(|e| reason(&e))?;
        let net_events = if server {
            None
        } else {
            NetHost::new(&mut store, &instance).ok()
        };
        let (mut hooks, ui) = if server {
            let exports = HorizonServerMod::new(&mut store, &instance).map_err(|e| reason(&e))?;
            let r = exports
                .horizon_mods_server_events()
                .call_init(&mut store)
                .map_err(|e| reason(&e))??;
            (Hooks::Server(exports, r), None)
        } else {
            let exports = HorizonMod::new(&mut store, &instance).map_err(|e| reason(&e))?;
            // `ui-events` is a mod built since `ui`'s; older mods have no export
            // and stay callable through `events` alone.
            let ui = UiHost::new(&mut store, &instance).ok();
            let r = exports
                .horizon_mods_events()
                .call_init(&mut store)
                .map_err(|e| reason(&e))??;
            (Hooks::Client(exports, r), ui)
        };
        let free = |c: &str| {
            let free = !reserved(c) && !taken.iter().any(|t| t == c);
            if !free {
                log::warn!("Horizon: mod {} cannot take /{c}: already a command", m.id);
            }
            free
        };
        match &mut hooks {
            Hooks::Client(_, r) => r.commands.retain(|c| free(c)),
            Hooks::Server(_, r) => {
                r.commands.retain(|c| free(&c.name));
                for c in &mut r.commands {
                    if let Err(e) = check_arguments(&c.arguments) {
                        log::warn!(
                            "Horizon: mod {}'s /{}: {e}; it takes free text",
                            m.id,
                            c.name
                        );
                        c.arguments.clear();
                    }
                }
            }
        }
        self.generation += 1;
        self.out.append(&mut store.data_mut().out);
        Ok(Some(Running {
            store,
            ui,
            net: net_events,
            hooks,
            raycasts: functions.iter().any(|f| f == "game.raycast"),
            reads_recipes: functions.iter().any(|f| f.starts_with("recipes.")),
        }))
    }

    /// True when at least one mod runs: the game skips its mod work otherwise.
    pub fn is_running(&self) -> bool {
        self.entries.iter().any(|e| e.run.is_some())
    }
    /// True when a server mod runs: the server skips its mod work otherwise.
    pub fn runs_server(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.hooks.server().is_some()))
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    /// What mods read until the next snapshot.
    pub fn set_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = Arc::new(snapshot);
    }

    /// Calls `f` on every running mod whose hooks `wants`, with `fuel`.
    fn each(
        &mut self,
        fuel: u64,
        wants: impl Fn(&Hooks) -> bool,
        mut f: impl FnMut(&mut Running) -> wasmtime::Result<()>,
    ) {
        for i in 0..self.entries.len() {
            if self.entries[i]
                .run
                .as_ref()
                .is_some_and(|r| wants(&r.hooks))
            {
                self.call(i, fuel, &mut f);
            }
        }
    }
    /// [`Mods::each`] over the client mods.
    fn each_client(
        &mut self,
        fuel: u64,
        wants: impl Fn(&Registration) -> bool,
        mut f: impl FnMut(&ClientEvents, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        self.each(
            fuel,
            |h| h.client().is_some_and(&wants),
            |run| match &run.hooks {
                Hooks::Client(m, _) => f(m.horizon_mods_events(), &mut run.store),
                Hooks::Server(..) => Ok(()),
            },
        );
    }
    /// [`Mods::each`] over the server mods.
    fn each_server(
        &mut self,
        fuel: u64,
        wants: impl Fn(&ServerRegistration) -> bool,
        mut f: impl FnMut(&ServerEvents, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        self.each(
            fuel,
            |h| h.server().is_some_and(&wants),
            |run| match &run.hooks {
                Hooks::Server(m, _) => f(m.horizon_mods_server_events(), &mut run.store),
                Hooks::Client(..) => Ok(()),
            },
        );
    }

    /// One call into mod `i`; a trap or an overrun turns the mod off.
    fn call<R>(
        &mut self,
        i: usize,
        fuel: u64,
        f: impl FnOnce(&mut Running) -> wasmtime::Result<R>,
    ) -> Option<R> {
        let entry = &mut self.entries[i];
        let run = entry.run.as_mut()?;
        run.store.data_mut().snapshot = self.snapshot.clone();
        run.store.data_mut().storage_budget = STORAGE_WRITE_BUDGET;
        let started = Instant::now();
        let result = run.store.set_fuel(fuel).and_then(|()| f(run));
        // An average over about the last 64 calls.
        entry.cost = (entry.cost * 63 + started.elapsed()) / 64;
        let data = run.store.data_mut();
        let (open, close) = (
            std::mem::take(&mut data.ui.open),
            std::mem::take(&mut data.ui.close),
        );
        self.out.append(&mut data.out);
        if open {
            self.out.push(Output::OpenScreen { owner: i });
        }
        if close {
            self.out.push(Output::CloseScreen { owner: i });
        }
        match result {
            Ok(r) => Some(r),
            Err(e) => {
                let reason = reason(&e);
                log::warn!("Horizon: mod {} stopped: {reason}", run.store.data().id);
                let name = run.store.data().name.clone();
                self.out.push(Output::Stopped {
                    name,
                    reason: reason.clone(),
                });
                entry.status = Status::Failed(reason);
                entry.run = None;
                self.generation += 1;
                None
            }
        }
    }

    /// A game tick.
    pub fn tick(&mut self, tick: u64) {
        self.each_client(FUEL_TICK, |h| h.tick, |m, s| m.call_tick(s, tick));
    }
    /// A rendered frame, `seconds` after the last.
    pub fn frame(&mut self, seconds: f32) {
        self.each_client(FUEL_FRAME, |h| h.frame, |m, s| m.call_frame(s, seconds));
    }
    /// A chat line the player received, as plain text.
    pub fn chat(&mut self, text: &str) {
        self.each_client(FUEL_EVENT, |h| h.chat, |m, s| m.call_chat(s, text));
    }
    /// Whether a running client mod owns `/name`.
    pub fn has_command(&self, name: &str) -> bool {
        self.command_owner(false, name).is_some()
    }
    fn command_owner(&self, server: bool, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.run.as_ref().is_some_and(|r| {
                r.hooks.server().is_some() == server && r.hooks.commands().contains(&name)
            })
        })
    }
    /// Runs `line` (without its slash) if a client mod owns its command;
    /// false when none does.
    pub fn command(&mut self, line: &str) -> bool {
        let (name, args) = line.split_once(' ').unwrap_or((line, ""));
        let Some(i) = self.command_owner(false, name) else {
            return false;
        };
        let result = self.call(i, FUEL_EVENT, |run| match &run.hooks {
            Hooks::Client(m, _) => m
                .horizon_mods_events()
                .call_command(&mut run.store, name, args),
            Hooks::Server(..) => Ok(Ok(None)),
        });
        match result {
            Some(Ok(Some(text))) => self.out.push(Output::Feedback { text, error: false }),
            Some(Err(text)) => self.out.push(Output::Feedback { text, error: true }),
            Some(Ok(None)) | None => {}
        }
        true
    }
    /// The commands running client mods own, for suggestions.
    pub fn commands(&self) -> impl Iterator<Item = &str> {
        self.owned_commands(false)
    }
    fn owned_commands(&self, server: bool) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .filter(move |r| r.hooks.server().is_some() == server)
            .flat_map(|r| r.hooks.commands())
    }

    /// A server tick.
    pub fn server_tick(&mut self, tick: u64) {
        self.each_server(FUEL_TICK, |h| h.tick, |m, s| m.call_tick(s, tick));
    }
    /// The blocks running server mods gave a block entity
    /// (`registration.block-entities`).
    pub fn block_entity_kinds(&self) -> Vec<BlockEntityKind> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .flat_map(|r| r.block_entities.iter().cloned())
            .collect()
    }
    /// A tick's `block-entities-tick`: each running server mod gets the
    /// entities of its own ticking blocks (at most [`BLOCK_ENTITIES_PER_CALL`]
    /// a call) and answers with the ones whose bytes changed; an update for
    /// an entity it was not given, or with too many bytes, is dropped.
    pub fn block_entities_tick(
        &mut self,
        tick: u64,
        entities: &[BlockEntity],
    ) -> Vec<BlockEntityUpdate> {
        let mut out = Vec::new();
        for i in 0..self.entries.len() {
            let Some(reg) = self.entries[i]
                .run
                .as_ref()
                .and_then(|r| r.hooks.server())
            else {
                continue;
            };
            let ticking: Vec<&str> = reg
                .block_entities
                .iter()
                .filter(|k| k.ticks)
                .map(|k| k.block.as_str())
                .collect();
            if ticking.is_empty() {
                continue;
            }
            let mine: Vec<BlockEntity> = entities
                .iter()
                .filter(|e| {
                    let block = e.state.split('[').next().unwrap_or("");
                    ticking.contains(&block)
                })
                .take(BLOCK_ENTITIES_PER_CALL)
                .cloned()
                .collect();
            if mine.is_empty() {
                continue;
            }
            let updates = self.call(i, FUEL_TICK, |run| match &run.hooks {
                Hooks::Server(m, _) => m
                    .horizon_mods_server_events()
                    .call_block_entities_tick(&mut run.store, tick, &mine),
                Hooks::Client(..) => Ok(Vec::new()),
            });
            for u in updates.into_iter().flatten() {
                let given = mine
                    .iter()
                    .any(|e| e.dimension == u.dimension && e.pos == u.pos);
                if given
                    && u.data.len() <= BLOCK_ENTITY_BYTES
                    && u.items.as_ref().is_none_or(|s| s.len() <= 256)
                    && u.tanks.as_ref().is_none_or(|s| s.len() <= 16)
                    && u.menu_data.as_ref().is_none_or(|s| s.len() <= 256)
                {
                    out.push(u);
                }
            }
        }
        out
    }
    /// `player` pressed menu button `id` of the menu of `entity` (a block
    /// entity of a running server mod's block, as the game holds it): the
    /// update the mod answers with, checked as a tick's.
    pub fn menu_button(
        &mut self,
        player: &ServerPlayer,
        id: i32,
        entity: &BlockEntity,
    ) -> Option<BlockEntityUpdate> {
        let block = entity.state.split('[').next().unwrap_or("");
        let owner = self.entries.iter().position(|e| {
            e.run
                .as_ref()
                .and_then(|r| r.hooks.server())
                .is_some_and(|r| r.block_entities.iter().any(|k| k.block == block))
        })?;
        let update = self
            .call(owner, FUEL_EVENT, |run| match &run.hooks {
                Hooks::Server(m, _) => m.horizon_mods_server_events().call_menu_button(
                    &mut run.store,
                    player,
                    &entity.dimension,
                    entity.pos,
                    id,
                    entity,
                ),
                Hooks::Client(..) => Ok(None),
            })
            .flatten()?;
        (update.dimension == entity.dimension
            && update.pos == entity.pos
            && update.data.len() <= BLOCK_ENTITY_BYTES
            && update.items.as_ref().is_none_or(|s| s.len() <= 256)
            && update.tanks.as_ref().is_none_or(|s| s.len() <= 16)
            && update.menu_data.as_ref().is_none_or(|s| s.len() <= 256))
        .then_some(update)
    }
    /// The multiblock groups running server mods registered, with the block
    /// ids of each group.
    pub fn multiblock_groups(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for k in self
            .entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .flat_map(|r| r.block_entities.iter())
        {
            let Some(spec) = &k.multiblock else { continue };
            // A block of several groups (a pressure disperser is part of boilers and of turbines) names them separated by commas.
            for group in spec.group.split(',').map(str::trim).filter(|g| !g.is_empty()) {
                match out.iter_mut().find(|(g, _)| g == group) {
                    Some((_, blocks)) => blocks.push(k.block.clone()),
                    None => out.push((group.to_owned(), vec![k.block.clone()])),
                }
            }
        }
        out
    }
    /// The structure candidates of `group` in `dimension` changed: the mod
    /// that registered the group gets them all, and answers with updates,
    /// kept for member blocks only.
    pub fn multiblock_changed(
        &mut self,
        dimension: &str,
        group: &str,
        structures: &[crate::multiblock::Structure],
    ) -> Vec<BlockEntityUpdate> {
        let Some(owner) = self.entries.iter().position(|e| {
            e.run
                .as_ref()
                .and_then(|r| r.hooks.server())
                .is_some_and(|r| {
                    r.block_entities
                        .iter()
                        .any(|k| k.multiblock.as_ref().is_some_and(|m| m.group.split(',').any(|g| g.trim() == group)))
                })
        }) else {
            return Vec::new();
        };
        let pos = |p: [i32; 3]| BlockPos { x: p[0], y: p[1], z: p[2] };
        let list: Vec<ModStructure> = structures
            .iter()
            .map(|s| ModStructure {
                members: s
                    .members
                    .iter()
                    .map(|m| MultiblockMember {
                        pos: pos(m.pos),
                        state: m.state.clone(),
                    })
                    .collect(),
                min: pos(s.min),
                max: pos(s.max),
            })
            .collect();
        let updates = self
            .call(owner, FUEL_EVENT, |run| match &run.hooks {
                Hooks::Server(m, _) => m
                    .horizon_mods_server_events()
                    .call_multiblock_changed(&mut run.store, group, dimension, &list),
                Hooks::Client(..) => Ok(Vec::new()),
            })
            .unwrap_or_default();
        let members: std::collections::HashSet<[i32; 3]> = structures
            .iter()
            .flat_map(|s| s.members.iter().map(|m| m.pos))
            .collect();
        updates
            .into_iter()
            .filter(|u| {
                u.dimension == dimension
                    && members.contains(&[u.pos.x, u.pos.y, u.pos.z])
                    && u.data.len() <= BLOCK_ENTITY_BYTES
                    && u.items.as_ref().is_none_or(|s| s.len() <= 256)
                    && u.tanks.as_ref().is_none_or(|s| s.len() <= 16)
                    && u.menu_data.as_ref().is_none_or(|s| s.len() <= 256)
            })
            .take(BLOCK_ENTITIES_PER_CALL)
            .collect()
    }
    /// The answer to mod `id`'s `spawn-mob` or `summon` `ticket`.
    pub fn entity_spawned(&mut self, id: &str, ticket: u64, entity: Option<u32>) {
        let Some(i) = self.entries.iter().position(|e| {
            e.manifest.as_ref().is_some_and(|m| m.id == id)
                && e.run.as_ref().and_then(|r| r.hooks.server()).is_some()
        }) else {
            return;
        };
        self.call(i, FUEL_EVENT, |run| match &run.hooks {
            Hooks::Server(m, _) => m
                .horizon_mods_server_events()
                .call_entity_spawned(&mut run.store, ticket, entity),
            Hooks::Client(..) => Ok(()),
        });
    }
    /// What `player`'s client half sent on `channel` (`<mod id>:<name>`): the
    /// server mod of that id hears it, if it asked for `net`.
    pub fn net_message(&mut self, player: &ServerPlayer, channel: &str, data: &[u8]) {
        let Some((id, _)) = channel.split_once(':') else {
            return;
        };
        let Some(i) = self.entries.iter().position(|e| {
            e.manifest.as_ref().is_some_and(|m| m.id == id)
                && e.run
                    .as_ref()
                    .and_then(|r| r.hooks.server())
                    .is_some_and(|r| r.net)
        }) else {
            return;
        };
        self.call(i, FUEL_EVENT, |run| match &run.hooks {
            Hooks::Server(m, _) => m.horizon_mods_server_events().call_net_message(
                &mut run.store,
                player,
                channel,
                data,
            ),
            Hooks::Client(..) => Ok(()),
        });
    }
    /// What the server's half sent on `channel` (`<mod id>:<name>`): the client
    /// mod of that id hears it, if it exports `net-events`.
    pub fn net_from_server(&mut self, channel: &str, data: &[u8]) {
        let Some((id, _)) = channel.split_once(':') else {
            return;
        };
        let Some(i) = self.entries.iter().position(|e| {
            e.manifest.as_ref().is_some_and(|m| m.id == id)
                && e.run.as_ref().is_some_and(|r| r.net.is_some())
        }) else {
            return;
        };
        self.call(i, FUEL_EVENT, |run| {
            let net = run.net.as_ref().expect("checked above");
            net.horizon_mods_net_events()
                .call_message(&mut run.store, channel, data)
        });
    }
    /// `player` joined the server (they are in the snapshot's players).
    pub fn player_joined(&mut self, player: &ServerPlayer) {
        self.each_server(
            FUEL_EVENT,
            |h| h.joins,
            |m, s| m.call_player_joined(s, player),
        );
    }
    /// `player` left the server (they are no longer in the snapshot's players).
    pub fn player_left(&mut self, player: &ServerPlayer) {
        self.each_server(
            FUEL_EVENT,
            |h| h.joins,
            |m, s| m.call_player_left(s, player),
        );
    }
    /// A chat message `player` sent, as plain text.
    pub fn player_chat(&mut self, player: &ServerPlayer, text: &str) {
        self.each_server(FUEL_EVENT, |h| h.chat, |m, s| m.call_chat(s, player, text));
    }
    /// `player` broke `state` at `pos`.
    pub fn block_broken(&mut self, player: &ServerPlayer, pos: BlockPos, state: &str) {
        self.each_server(
            FUEL_EVENT,
            |h| h.blocks,
            |m, s| m.call_block_broken(s, player, pos, state),
        );
    }
    /// `player` placed `state` at `pos`.
    pub fn block_placed(&mut self, player: &ServerPlayer, pos: BlockPos, state: &str) {
        self.each_server(
            FUEL_EVENT,
            |h| h.blocks,
            |m, s| m.call_block_placed(s, player, pos, state),
        );
    }
    /// The commands running server mods own and their arguments, for the
    /// server's command tree.
    pub fn server_commands(&self) -> impl Iterator<Item = &ServerCommand> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .flat_map(|r| r.commands.iter())
    }
    /// Runs `line` (without its slash), typed by `player`, if a server mod
    /// owns its command; its feedback is an [`Output::Reply`]. False when
    /// none does.
    pub fn server_command(&mut self, player: &ServerPlayer, line: &str) -> bool {
        let (name, args) = line.split_once(' ').unwrap_or((line, ""));
        let Some(i) = self.command_owner(true, name) else {
            return false;
        };
        let reply = |text, error| Output::Reply {
            player: player.id.clone(),
            text,
            error,
        };
        let arguments = self.entries[i]
            .run
            .as_ref()
            .and_then(|r| r.hooks.server())
            .and_then(|r| r.commands.iter().find(|c| c.name == name))
            .map(|c| c.arguments.as_slice())
            .unwrap_or_default();
        let values = match crate::commands::parse(arguments, args, player, &self.snapshot.players) {
            Ok(values) => values,
            Err(e) => {
                self.out.push(reply(e, true));
                return true;
            }
        };
        let result = self.call(i, FUEL_EVENT, |run| match &run.hooks {
            Hooks::Server(m, _) => m.horizon_mods_server_events().call_command(
                &mut run.store,
                player,
                name,
                args,
                &values,
            ),
            Hooks::Client(..) => Ok(Ok(None)),
        });
        match result {
            Some(Ok(Some(text))) => self.out.push(reply(text, false)),
            Some(Err(text)) => self.out.push(reply(text, true)),
            Some(Ok(None)) | None => {}
        }
        true
    }
    /// The running mods' HUD panels.
    pub fn panels(&self) -> impl Iterator<Item = &Panel> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().panel.as_ref())
    }
    /// The running mods' corner panels (capability `render`), in load order.
    pub fn corner_panels(&self) -> impl Iterator<Item = &CornerPanel> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().corner.as_ref())
    }
    /// Some running mod calls `game.raycast`: the snapshot needs a
    /// [`Snapshot::view_hit`].
    pub fn wants_raycast(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.raycasts))
    }
    /// The panel beside container screens (`browser.set-items`): the first
    /// running mod with items or recipes, its index, the panel's version and
    /// the items.
    pub fn browser_items(&self) -> Option<(usize, u64, &[BrowserStack])> {
        self.entries.iter().enumerate().find_map(|(i, e)| {
            let b = &e.run.as_ref()?.store.data().browser;
            (!b.items.is_empty() || b.recipes.is_some()).then_some((
                i,
                b.version,
                b.items.as_slice(),
            ))
        })
    }
    /// What mod `owner`'s panel shows instead of its items
    /// (`browser.set-recipes`).
    pub fn browser_recipes(&self, owner: usize) -> Option<&RecipePage> {
        let b = &self.entries.get(owner)?.run.as_ref()?.store.data().browser;
        b.recipes.as_ref()
    }
    /// The player did `event` on mod `owner`'s panel.
    pub fn browser_event(&mut self, owner: usize, event: BrowserEvent) {
        if let Some(run) = self.entries.get_mut(owner).and_then(|e| e.run.as_mut()) {
            run.store.data_mut().browser.push(event);
        }
    }
    /// The key `key` went down with the pointer on `stack` in a container
    /// screen: each mod with a key on it hears its own name for it.
    pub fn browser_key(&mut self, key: &str, stack: &BrowserStack) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let data = run.store.data_mut();
            let names: Vec<String> = data
                .input
                .keys
                .iter()
                .filter(|k| k.key == key)
                .map(|k| k.name.clone())
                .collect();
            for key in names {
                data.browser.push(BrowserEvent::Key(BrowserItemKey {
                    key,
                    stack: stack.clone(),
                }));
            }
        }
    }
    /// Some running mod reads capability `recipes`: the snapshot needs
    /// [`Snapshot::recipes`] and [`Snapshot::known_recipes`].
    pub fn wants_recipes(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_recipes))
    }
    /// Some running mod has a `frame` hook: the game refreshes the snapshot
    /// every frame for it.
    pub fn wants_frames(&self) -> bool {
        self.entries.iter().any(|e| {
            e.run
                .as_ref()
                .is_some_and(|r| r.hooks.client().is_some_and(|h| h.frame))
        })
    }
    /// Some running mod claimed items (capability `input`).
    pub fn claims_any(&self) -> bool {
        self.inputs().any(|i| !i.claimed.is_empty())
    }
    /// Some running mod claimed the item `id`.
    pub fn claims(&self, id: &str) -> bool {
        self.inputs().any(|i| i.claimed.iter().any(|c| c == id))
    }
    /// Some running mod registered a key.
    pub fn has_keys(&self) -> bool {
        self.inputs().any(|i| !i.keys.is_empty())
    }
    /// The mods' keys: (mod name, key name, label, key), for Controls.
    pub fn mod_keys(&self) -> impl Iterator<Item = (&str, &ModKey)> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| {
                let d = r.store.data();
                d.input.keys.iter().map(move |k| (d.name.as_str(), k))
            })
    }
    /// Mod `i`'s keys, as it registered them (with the player's choices).
    pub fn keys_of(&self, i: usize) -> &[ModKey] {
        self.entries
            .get(i)
            .and_then(|e| e.run.as_ref())
            .map_or(&[], |r| r.store.data().input.keys.as_slice())
    }
    /// The keys the player chose (`ui`'s key widget), by mod id: each
    /// running mod's `name → key`. They replace the defaults registered
    /// so far and later ones. Keys that aren't keys are skipped.
    pub fn set_bindings<'a>(&mut self, chosen: impl Fn(&str) -> Vec<(&'a str, &'a str)>) {
        for e in &mut self.entries {
            let (Some(m), Some(run)) = (&e.manifest, e.run.as_mut()) else {
                continue;
            };
            let input = &mut run.store.data_mut().input;
            input.bindings = chosen(&m.id)
                .into_iter()
                .filter(|(_, key)| key_ok(key))
                .map(|(n, k)| (n.to_owned(), k.to_owned()))
                .collect();
            for (name, key) in input.bindings.clone() {
                input.rebind(&name, key);
            }
        }
    }
    /// The player binds mod `i`'s key `name` to `key` (`key.keyboard.x`,
    /// `key.mouse.4`, `key.keyboard.unknown` for none). False when the mod
    /// has no such key or `key` isn't one.
    pub fn rebind(&mut self, i: usize, name: &str, key: &str) -> bool {
        let Some(run) = self.entries.get_mut(i).and_then(|e| e.run.as_mut()) else {
            return false;
        };
        let input = &mut run.store.data_mut().input;
        if !key_ok(key) || !input.keys.iter().any(|k| k.name == name) {
            return false;
        }
        input.bindings.retain(|(n, _)| n != name);
        input.bindings.push((name.to_owned(), key.to_owned()));
        input.rebind(name, key.to_owned());
        true
    }
    /// The use button went down with `item`: `use-start` to each mod that
    /// claimed it.
    pub fn use_start(&mut self, item: &HeldItem) {
        let event = InputEvent::UseStart(ItemUse {
            item: item.clone(),
            ticks: 0,
        });
        self.push_input(|i| i.claimed.contains(&item.id), &event);
    }
    /// The claimed use ended after `ticks`.
    pub fn use_stop(&mut self, item: &HeldItem, ticks: u32) {
        let event = InputEvent::UseStop(ItemUse {
            item: item.clone(),
            ticks,
        });
        self.push_input(|i| i.claimed.contains(&item.id), &event);
    }
    /// The key `key` (`key.keyboard.r`) went down or up: each mod with a
    /// key on it hears its own name for it.
    pub fn key(&mut self, key: &str, down: bool) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let input = &mut run.store.data_mut().input;
            let names: Vec<String> = input
                .keys
                .iter()
                .filter(|k| k.key == key)
                .map(|k| k.name.clone())
                .collect();
            for name in names {
                let held = input.down.iter().position(|n| *n == name);
                match (down, held) {
                    (true, None) => input.down.push(name.clone()),
                    (false, Some(i)) => {
                        input.down.swap_remove(i);
                    }
                    // Already let go when a screen opened.
                    (false, None) => continue,
                    (true, Some(_)) => {}
                }
                input.push(if down {
                    InputEvent::KeyDown(name)
                } else {
                    InputEvent::KeyUp(name)
                });
            }
        }
    }
    /// A screen opened, or the game stopped playing: every mod key held
    /// lets go (`key-up`), so a key needs a new press afterwards.
    pub fn release_keys(&mut self) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let input = &mut run.store.data_mut().input;
            for name in std::mem::take(&mut input.down) {
                input.push(InputEvent::KeyUp(name));
            }
        }
    }
    /// Some running mod holds the mouse wheel.
    pub fn wheel_claimed(&self) -> bool {
        self.inputs().any(|i| i.wheel)
    }
    /// Wheel steps while playing: `wheel` to the mods holding it; false
    /// when none does (the hotbar takes them).
    pub fn wheel(&mut self, steps: f32) -> bool {
        let event = InputEvent::Wheel(steps);
        let claimed = self.wheel_claimed();
        self.push_input(|i| i.wheel, &event);
        claimed
    }
    /// The strongest zoom a running mod asks for (capability `render`).
    pub fn zoom(&self) -> Option<ModZoom> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().zoom)
            .max_by(|a, b| a.factor.total_cmp(&b.factor))
    }
    /// The minimap the first running mod shows (capability `render`).
    pub fn minimap(&self) -> Option<ModMinimap> {
        self.entries
            .iter()
            .find_map(|e| e.run.as_ref()?.store.data().minimap)
    }
    /// The far terrain the first running mod draws (capability `render`).
    pub fn far_terrain(&self) -> Option<ModFarTerrain> {
        self.entries
            .iter()
            .find_map(|e| e.run.as_ref()?.store.data().far_terrain)
    }
    fn inputs(&self) -> impl Iterator<Item = &InputState> {
        self.entries
            .iter()
            .filter_map(|e| Some(&e.run.as_ref()?.store.data().input))
    }
    fn push_input(&mut self, to: impl Fn(&InputState) -> bool, event: &InputEvent) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let input = &mut run.store.data_mut().input;
            if to(input) {
                input.push(event.clone());
            }
        }
    }
    /// The running mods' HUD shapes (capability `render`), each mod's in
    /// its own order, mods in load order.
    pub fn hud_shapes(&self) -> impl Iterator<Item = &Shape> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| &r.store.data().shapes)
    }
    /// The screen mod `i` last pushed with `ui.open-screen`, while it is up.
    pub fn screen(&self, i: usize) -> Option<&UiScreen> {
        self.entries
            .get(i)?
            .run
            .as_ref()?
            .store
            .data()
            .ui
            .screen
            .as_ref()
    }
    /// The widgets `ui.set-settings` pushed: mod `i`'s page in
    /// Options > Mods. Empty while it has none.
    pub fn settings_widgets(&self, i: usize) -> &[UiWidget] {
        self.entries
            .get(i)
            .and_then(|e| e.run.as_ref())
            .map_or(&[], |r| &r.store.data().ui.settings)
    }
    /// A `list` widget's row was picked on a surface of mod `i`.
    pub fn ui_select(&mut self, i: usize, id: u32, row: u32) {
        self.ui_event(i, |ui, s| {
            ui.horizon_mods_ui_events().call_select(s, id, row)
        });
    }
    /// The player left mod `i`'s screen (Esc or Done).
    pub fn ui_closed(&mut self, i: usize) {
        self.ui_event(i, |ui, s| ui.horizon_mods_ui_events().call_closed(s));
    }
    /// The player opened mod `i`'s settings page.
    pub fn ui_settings_opened(&mut self, i: usize) {
        self.ui_event(i, |ui, s| {
            ui.horizon_mods_ui_events().call_settings_opened(s)
        });
    }
    /// A `ui-events` call on mod `i`; no export, no call (mods built
    /// before `ui` have none).
    fn ui_event(
        &mut self,
        i: usize,
        f: impl FnOnce(&UiHost, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        let has_ui = self
            .entries
            .get(i)
            .and_then(|e| e.run.as_ref())
            .is_some_and(|r| r.ui.is_some());
        if has_ui {
            self.call(i, FUEL_EVENT, |run| {
                f(run.ui.as_ref().expect("checked above"), &mut run.store)
            });
        }
    }
    /// What mods asked to show since the last call.
    pub fn drain(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.out)
    }

    /// Restarts the mods whose file changed since they loaded (mod
    /// development: rebuild, and the game picks it up). True when any did.
    pub fn reload_changed(&mut self, reserved: impl Fn(&str) -> bool) -> bool {
        let mut any = false;
        for i in 0..self.entries.len() {
            let entry = &self.entries[i];
            let now = modified(&entry.path);
            let was_on = !matches!(entry.status, Status::Off | Status::Elsewhere);
            if now == entry.modified || !was_on {
                continue;
            }
            any = true;
            self.generation += 1;
            self.entries[i].modified = now;
            self.entries[i].run = None;
            let path = self.entries[i].path.clone();
            let started = Package::read(&path).and_then(|p| {
                self.entries[i].manifest = Some(p.manifest.clone());
                self.start(&path, &p, &reserved)
            });
            let entry = &mut self.entries[i];
            let id = entry.manifest.as_ref().map_or("?", |m| m.id.as_str());
            match started {
                Ok(run) => {
                    log::info!("Horizon: mod {id} reloaded");
                    entry.run = run;
                    entry.status = Status::Running;
                }
                Err(e) => {
                    log::warn!("Horizon: mod {id} did not reload: {e}");
                    entry.status = Status::Failed(e);
                }
            }
        }
        any
    }
}

/// A command's arguments are well formed: names given and distinct,
/// optional ones last, free text only last.
fn check_arguments(arguments: &[Argument]) -> Result<(), String> {
    for (i, a) in arguments.iter().enumerate() {
        if a.name.is_empty()
            || !a
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(format!("argument name '{}' isn't [A-Za-z0-9_]+", a.name));
        }
        if arguments[..i].iter().any(|b| b.name == a.name) {
            return Err(format!("two arguments are named {}", a.name));
        }
        if i > 0 && arguments[i - 1].optional && !a.optional {
            return Err(format!("{} is required after an optional argument", a.name));
        }
        if a.kind == ArgumentKind::Text && i + 1 < arguments.len() {
            return Err(format!(
                "{} takes the rest of the line, so it must be last",
                a.name
            ));
        }
    }
    Ok(())
}

fn modified(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_dir() {
        // A folder mod changes when its component does.
        return std::fs::metadata(path.join(crate::manifest::WASM))
            .ok()?
            .modified()
            .ok();
    }
    meta.modified().ok()
}

/// A trap or error, as the player reads it.
fn reason(e: &wasmtime::Error) -> String {
    match e.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => "it took too long (an endless loop?)".into(),
        Some(Trap::UnreachableCodeReached) => "it crashed (a panic)".into(),
        Some(trap) => format!("it crashed: {trap}"),
        None => format!("{e:#}"),
    }
}

fn clip(text: String) -> String {
    clip_to(text, LINE_CHARS)
}
fn clip_to(text: String, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((at, _)) => text[..at].to_string(),
        None => text,
    }
}

/// Widgets on a `ui` screen or settings page, and rows on one `list`.
const WIDGETS: usize = 64;
const LIST_ROWS: usize = 512;

fn clip_widgets(widgets: Vec<UiWidget>) -> Vec<UiWidget> {
    widgets
        .into_iter()
        .take(WIDGETS)
        .map(|w| match w {
            UiWidget::Text(mut t) => {
                t.text = clip(t.text);
                UiWidget::Text(t)
            }
            UiWidget::Button(mut b) => {
                b.label = clip(b.label);
                UiWidget::Button(b)
            }
            UiWidget::Item(mut i) => {
                i.item = clip(i.item);
                i.name = clip(i.name);
                UiWidget::Item(i)
            }
            UiWidget::List(mut l) => {
                l.rows = l.rows.into_iter().take(LIST_ROWS).map(clip).collect();
                l.selected = l.selected.filter(|&s| (s as usize) < l.rows.len());
                UiWidget::List(l)
            }
        })
        .collect()
}

impl mod_log::Host for State {
    fn write(&mut self, level: mod_log::Level, message: String) {
        let level = match level {
            mod_log::Level::Debug => log::Level::Debug,
            mod_log::Level::Info => log::Level::Info,
            mod_log::Level::Warn => log::Level::Warn,
            mod_log::Level::Error => log::Level::Error,
        };
        log::log!(level, "[mod {}] {}", self.id, clip(message));
    }
}
impl game::Host for State {
    fn get_player(&mut self) -> Option<Player> {
        self.snapshot.player.clone()
    }
    fn get_clock(&mut self) -> Option<Clock> {
        self.snapshot.clock
    }
    fn raycast(&mut self, distance: f32) -> Option<Hit> {
        let (at, hit) = self.snapshot.view_hit.as_ref()?;
        // NaN compares false: no hit.
        (*at <= distance.min(RAYCAST_MAX) && !distance.is_nan()).then(|| hit.clone())
    }
}
impl browser::Host for State {
    fn set_items(&mut self, items: Vec<BrowserStack>) {
        self.browser.items = items
            .into_iter()
            .take(BROWSER_ITEMS)
            .map(|s| BrowserStack {
                item: clip(s.item),
                components: clip_to(s.components, 4096),
                ..s
            })
            .collect();
        self.browser.version = BROWSER_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn set_recipes(
        &mut self,
        title: String,
        tabs: Vec<BrowserStack>,
        tab: u32,
        recipes: Vec<BrowserRecipe>,
    ) {
        let stack = |s: BrowserStack| BrowserStack {
            item: clip(s.item),
            components: clip_to(s.components, 4096),
            ..s
        };
        let cycle = |list: Vec<BrowserStack>| -> Vec<BrowserStack> {
            list.into_iter().take(SLOT_STACKS).map(stack).collect()
        };
        self.browser.recipes = (!recipes.is_empty()).then(|| {
            let recipes = recipes
                .into_iter()
                .take(BROWSER_RECIPES)
                .map(|r| BrowserRecipe {
                    width: r.width.clamp(1, 3),
                    height: r.height.clamp(1, 3),
                    slots: r.slots.into_iter().take(RECIPE_SLOTS).map(cycle).collect(),
                    results: cycle(r.results),
                    note: clip(r.note),
                    ..r
                })
                .collect();
            RecipePage {
                title: clip(title),
                tabs: tabs.into_iter().take(RECIPE_TABS).map(stack).collect(),
                tab,
                recipes,
            }
        });
        self.browser.version = BROWSER_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn poll(&mut self) -> Vec<BrowserEvent> {
        self.browser.events.drain(..).collect()
    }
}
impl recipes::Host for State {
    fn items(&mut self) -> Vec<RecipeItem> {
        self.snapshot
            .recipes
            .as_ref()
            .map_or_else(Vec::new, |r| r.items.clone())
    }
    fn all(&mut self) -> Vec<Recipe> {
        self.snapshot
            .recipes
            .as_ref()
            .map_or_else(Vec::new, |r| r.recipes.clone())
    }
    fn known(&mut self) -> Vec<i32> {
        self.snapshot.known_recipes.to_vec()
    }
    fn place(&mut self, book_id: i32, all: bool) -> Result<(), String> {
        self.recipe_action()?;
        if !self.snapshot.placing {
            return Err("no screen that takes recipes is open".into());
        }
        if !self.snapshot.known_recipes.contains(&book_id) {
            return Err(format!(
                "recipe {book_id} is not in the player's recipe book"
            ));
        }
        self.out.push(Output::PlaceRecipe { book_id, all });
        Ok(())
    }
    fn give(&mut self, stack: RecipeStack) -> Result<(), String> {
        self.recipe_action()?;
        if !self.snapshot.creative {
            return Err("only in creative mode".into());
        }
        let item = self
            .snapshot
            .recipes
            .as_ref()
            .and_then(|r| r.items.get(stack.item as usize))
            .ok_or_else(|| format!("no item {}", stack.item))?
            .id
            .clone();
        self.out.push(Output::Give {
            item,
            count: stack.count.clamp(1, 99),
            components: clip_to(stack.components, 4096),
        });
        Ok(())
    }
}
impl State {
    /// `recipes.place` and `give` take at most [`RECIPE_ACTIONS`] a call.
    fn recipe_action(&self) -> Result<(), String> {
        let done = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::PlaceRecipe { .. } | Output::Give { .. }))
            .count();
        if done < RECIPE_ACTIONS {
            Ok(())
        } else {
            Err(format!("at most {RECIPE_ACTIONS} places and gives a call"))
        }
    }
}
impl State {
    /// Queues a chat line unless the call already sent [`CHAT_PER_CALL`].
    fn say(&mut self, line: impl FnOnce(String) -> Output) -> bool {
        let shown = self
            .out
            .iter()
            .filter(|o| {
                matches!(
                    o,
                    Output::Chat { .. } | Output::Tell { .. } | Output::TellOverlay { .. } | Output::Broadcast { .. }
                )
            })
            .count();
        if shown < CHAT_PER_CALL {
            let name = self.name.clone();
            self.out.push(line(name));
        }
        shown < CHAT_PER_CALL
    }
}
impl chat::Host for State {
    fn show(&mut self, text: String) {
        self.say(|name| Output::Chat {
            name,
            text: clip(text),
        });
    }
}
impl mod_world::Host for State {
    fn set_block_property(
        &mut self,
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
        property: String,
        value: String,
    ) -> Result<(), String> {
        let word = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        };
        if !word(&property) || !word(&value) {
            return Err("a property and a value are lowercase words".into());
        }
        if let Some(view) = self.world.lock().ok().as_ref().and_then(|v| v.as_ref())
            && !view.has_dimension(&dimension)
        {
            return Err(format!("no dimension {dimension}"));
        }
        let made = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::SetBlockProperty { .. }))
            .count();
        if made >= BLOCKS_PER_CALL {
            return Err(format!("at most {BLOCKS_PER_CALL} property changes a call"));
        }
        self.out.push(Output::SetBlockProperty {
            name: self.name.clone(),
            dimension,
            pos: [x, y, z],
            property,
            value,
        });
        Ok(())
    }
    fn set_item_faces(
        &mut self,
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
        faces: Vec<Vec<u32>>,
    ) -> Result<(), String> {
        if faces.len() != 6 || faces.iter().any(|f| f.len() > 32 || f.iter().any(|s| *s > 31)) {
            return Err("six faces, each with at most 32 slots numbered 0 to 31".into());
        }
        let made = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::SetItemFaces { .. } | Output::SetItemEject { .. }))
            .count();
        if made >= BLOCKS_PER_CALL {
            return Err(format!("at most {BLOCKS_PER_CALL} container changes a call"));
        }
        self.out.push(Output::SetItemFaces { name: self.name.clone(), dimension, pos: [x, y, z], faces });
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn spawn_particles(
        &mut self,
        dimension: String,
        particle: String,
        x: f64,
        y: f64,
        z: f64,
        spread_x: f32,
        spread_y: f32,
        spread_z: f32,
        speed: f32,
        count: u32,
        color: Option<u32>,
        scale: f32,
        max_distance: f32,
    ) -> Result<(), String> {
        let numbers = [x, y, z].iter().all(|v| v.is_finite() && v.abs() < TP_XZ)
            && [spread_x, spread_y, spread_z, speed, scale, max_distance].iter().all(|v| v.is_finite());
        if !numbers || !kind_ok(&particle) {
            return Err("a particle id and finite numbers".into());
        }
        if count == 0 || count > 512 || !(0.0..=128.0).contains(&max_distance) || !(0.0..=4.0).contains(&scale) {
            return Err("1 to 512 particles, a distance of 0 to 128 blocks and a scale of 0 to 4".into());
        }
        let made = self.out.iter().filter(|o| matches!(o, Output::SpawnParticles { .. })).count();
        if made >= 1024 {
            return Err("at most 1024 particle requests a call".into());
        }
        self.out.push(Output::SpawnParticles {
            name: self.name.clone(),
            dimension,
            particle,
            pos: [x, y, z],
            spread: [spread_x, spread_y, spread_z],
            speed,
            count,
            color,
            scale,
            max_distance,
        });
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn pack_block(&mut self, dimension: String, x: i32, y: i32, z: i32, into_x: i32, into_y: i32, into_z: i32) -> Result<(), String> {
        let made = self.out.iter().filter(|o| matches!(o, Output::PackBlock { .. } | Output::UnpackBlock { .. })).count();
        if made >= BLOCKS_PER_CALL {
            return Err(format!("at most {BLOCKS_PER_CALL} packs a call"));
        }
        self.out.push(Output::PackBlock { name: self.name.clone(), dimension, pos: [x, y, z], into: [into_x, into_y, into_z] });
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn unpack_block(&mut self, dimension: String, box_x: i32, box_y: i32, box_z: i32, x: i32, y: i32, z: i32) -> Result<(), String> {
        let made = self.out.iter().filter(|o| matches!(o, Output::PackBlock { .. } | Output::UnpackBlock { .. })).count();
        if made >= BLOCKS_PER_CALL {
            return Err(format!("at most {BLOCKS_PER_CALL} packs a call"));
        }
        self.out.push(Output::UnpackBlock { name: self.name.clone(), dimension, box_pos: [box_x, box_y, box_z], pos: [x, y, z] });
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn set_capabilities_enabled(
        &mut self,
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
        items: bool,
        fluids: bool,
        energy: bool,
    ) -> Result<(), String> {
        let made = self.out.iter().filter(|o| matches!(o, Output::SetCapabilitiesEnabled { .. })).count();
        if made >= BLOCKS_PER_CALL {
            return Err(format!("at most {BLOCKS_PER_CALL} capability changes a call"));
        }
        self.out.push(Output::SetCapabilitiesEnabled { name: self.name.clone(), dimension, pos: [x, y, z], items, fluids, energy });
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn mine_block(
        &mut self,
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
        tool: String,
        silk_touch: bool,
        to_x: i32,
        to_y: i32,
        to_z: i32,
        face: u32,
    ) -> Result<(), String> {
        if face > 5 || tool.len() > 100 {
            return Err("a face 0 to 5 and a tool id".into());
        }
        let made = self.out.iter().filter(|o| matches!(o, Output::MineBlock { .. })).count();
        if made >= 64 {
            return Err("at most 64 blocks mined a call".into());
        }
        self.out.push(Output::MineBlock { name: self.name.clone(), dimension, pos: [x, y, z], tool, silk_touch, to: [to_x, to_y, to_z], face });
        Ok(())
    }
    fn pull_items(&mut self, dimension: String, x: i32, y: i32, z: i32, face: u32, items: Vec<String>, max: u32) -> Result<(), String> {
        if face > 5 || items.len() > 64 {
            return Err("a face 0 to 5 and at most 64 item ids".into());
        }
        let made = self.out.iter().filter(|o| matches!(o, Output::PullItems { .. })).count();
        if made >= BLOCKS_PER_CALL {
            return Err(format!("at most {BLOCKS_PER_CALL} pulls a call"));
        }
        self.out.push(Output::PullItems { name: self.name.clone(), dimension, pos: [x, y, z], face, items, max: max.min(64) });
        Ok(())
    }
    fn set_item_eject(&mut self, dimension: String, x: i32, y: i32, z: i32, on: bool) -> Result<(), String> {
        let made = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::SetItemFaces { .. } | Output::SetItemEject { .. }))
            .count();
        if made >= BLOCKS_PER_CALL {
            return Err(format!("at most {BLOCKS_PER_CALL} container changes a call"));
        }
        self.out.push(Output::SetItemEject { name: self.name.clone(), dimension, pos: [x, y, z], on });
        Ok(())
    }
    fn set_tank_filter(
        &mut self,
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
        tank: u32,
        accepts: Vec<String>,
    ) -> Result<(), String> {
        if tank > 7 || accepts.len() > 128 || accepts.iter().any(|a| a.len() > 128) {
            return Err("tanks 0 to 7, at most 128 substances of 128 bytes".into());
        }
        let made = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::SetTankFilter { .. }))
            .count();
        if made >= BLOCKS_PER_CALL {
            return Err(format!("at most {BLOCKS_PER_CALL} tank filters a call"));
        }
        self.out.push(Output::SetTankFilter { name: self.name.clone(), dimension, pos: [x, y, z], tank, accepts });
        Ok(())
    }
    fn set_energy_capacity(&mut self, dimension: String, x: i32, y: i32, z: i32, capacity: u64) -> Result<(), String> {
        let made = self.out.iter().filter(|o| matches!(o, Output::SetEnergyCapacity { .. })).count();
        if made >= BLOCKS_PER_CALL {
            return Err(format!("at most {BLOCKS_PER_CALL} energy capacities a call"));
        }
        self.out.push(Output::SetEnergyCapacity { name: self.name.clone(), dimension, pos: [x, y, z], capacity });
        Ok(())
    }
    fn get_block(&mut self, dimension: String, x: i32, y: i32, z: i32) -> Option<String> {
        let view = self.world.lock().ok()?;
        view.as_ref()?.block(&dimension, x, y, z)
    }
    fn get_biome(&mut self, dimension: String, x: i32, y: i32, z: i32) -> Option<String> {
        let view = self.world.lock().ok()?;
        view.as_ref()?.biome(&dimension, x, y, z)
    }
    fn get_redstone(&mut self, dimension: String, x: i32, y: i32, z: i32) -> u8 {
        let Ok(view) = self.world.lock() else { return 0 };
        view.as_ref().map_or(0, |v| v.redstone(&dimension, x, y, z))
    }
    fn get_environment(&mut self, dimension: String, x: i32, y: i32, z: i32) -> Option<Environment> {
        let view = self.world.lock().ok()?;
        view.as_ref()?.environment(&dimension, x, y, z)
    }
    fn get_collision(&mut self, dimension: String, x: i32, y: i32, z: i32) -> Option<Vec<CollisionBox>> {
        let view = self.world.lock().ok()?;
        view.as_ref()?.collision(&dimension, x, y, z)
    }
    fn set_blocks(&mut self, dimension: String, blocks: Vec<BlockChange>) -> Result<(), String> {
        if blocks.len() > BLOCKS_PER_CALL {
            return Err(format!(
                "{} blocks: at most {BLOCKS_PER_CALL} per call",
                blocks.len()
            ));
        }
        if let Some(view) = self.world.lock().ok().as_ref().and_then(|v| v.as_ref()) {
            if !view.has_dimension(&dimension) {
                return Err(format!("no dimension {dimension}"));
            }
            for b in &blocks {
                view.check_state(&b.state)?;
            }
        }
        if !blocks.is_empty() {
            self.out.push(Output::SetBlocks {
                name: self.name.clone(),
                dimension,
                blocks,
            });
        }
        Ok(())
    }
}
impl chunks::Host for State {
    fn set_forced(
        &mut self,
        dimension: String,
        chunk_x: i32,
        chunk_z: i32,
        forced: bool,
    ) -> Result<(), String> {
        if chunk_x.abs() > 1_875_000 || chunk_z.abs() > 1_875_000 {
            return Err("that chunk is outside the world".into());
        }
        if let Some(view) = self.world.lock().ok().as_ref().and_then(|v| v.as_ref())
            && !view.has_dimension(&dimension)
        {
            return Err(format!("no dimension {dimension}"));
        }
        let made = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Chunk { .. }))
            .count();
        if made >= ENTITY_REQUESTS {
            return Err(format!("at most {ENTITY_REQUESTS} chunk requests a call"));
        }
        self.out.push(Output::Chunk {
            name: self.name.clone(),
            id: self.id.clone(),
            dimension,
            chunk: [chunk_x, chunk_z],
            forced,
        });
        Ok(())
    }
}
impl player_items::Host for State {
    fn get_inventory(&mut self, player: String) -> Option<Vec<Option<ModItemStack>>> {
        self.snapshot
            .inventories
            .iter()
            .find(|(id, _)| *id == player)
            .map(|(_, slots)| slots.clone())
    }
    fn set_slot(
        &mut self,
        player: String,
        slot: u32,
        stack: Option<ModItemStack>,
    ) -> Result<(), String> {
        if slot > 40 {
            return Err(format!("slot {slot}: 0 to 40"));
        }
        if let Some(s) = &stack
            && (s.item.is_empty() || s.count == 0 || s.components.len() > SUMMON_BYTES)
        {
            return Err("a stack has an item, a count over 0 and small components".into());
        }
        self.player_request(player, PlayerItemsCommand::SetSlot { slot, stack })
    }
    fn compare_and_set_slot(
        &mut self,
        player: String,
        slot: u32,
        expected: Option<ModItemStack>,
        replacement: Option<ModItemStack>,
    ) -> Result<(), String> {
        if slot > 40 {
            return Err(format!("slot {slot}: 0 to 40"));
        }
        for s in [&expected, &replacement].into_iter().flatten() {
            if s.item.is_empty() || s.count == 0 || s.components.len() > SUMMON_BYTES {
                return Err("a stack has an item, a count over 0 and small components".into());
            }
        }
        self.player_request(player, PlayerItemsCommand::CompareAndSet { slot, expected, replacement })
    }
    fn hurt(&mut self, player: String, damage_type: String, amount: f32) -> Result<(), String> {
        if !amount.is_finite() || amount <= 0.0 || !kind_ok(&damage_type) {
            return Err("an amount over 0 and a damage type id".into());
        }
        self.player_request(player, PlayerItemsCommand::Hurt { damage_type, amount })
    }
    fn add_effect(
        &mut self,
        player: String,
        effect: String,
        duration_ticks: i32,
        amplifier: i32,
    ) -> Result<(), String> {
        if !kind_ok(&effect) || duration_ticks == 0 || duration_ticks < -1 {
            return Err("an effect id, and a duration over 0 (or -1)".into());
        }
        let command = PlayerItemsCommand::Effect {
            effect,
            duration: duration_ticks,
            amplifier: amplifier.clamp(0, 255),
        };
        self.player_request(player, command)
    }
}
impl State {
    fn body_request(&mut self, player: String, change: BodyChange) -> Result<(), String> {
        if !self.snapshot.players.iter().any(|p| p.id == player) {
            return Err(format!("no player {player} is online"));
        }
        let made = self.out.iter().filter(|o| matches!(o, Output::PlayerBody { .. })).count();
        if made >= CONTROL_PER_CALL * 4 {
            return Err("too many body changes in one call".into());
        }
        self.out.push(Output::PlayerBody { name: self.name.clone(), player, change });
        Ok(())
    }
    fn player_request(&mut self, player: String, command: PlayerItemsCommand) -> Result<(), String> {
        if !self.snapshot.players.iter().any(|p| p.id == player) {
            return Err(format!("no player {player} is online"));
        }
        let made = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::PlayerItems { .. }))
            .count();
        if made >= ENTITY_REQUESTS {
            return Err(format!("at most {ENTITY_REQUESTS} player requests a call"));
        }
        self.out.push(Output::PlayerItems {
            name: self.name.clone(),
            player,
            command,
        });
        Ok(())
    }
}
/// Entity requests one call may make, and the longest `summon` compound.
const ENTITY_REQUESTS: usize = 64;
const SUMMON_BYTES: usize = 8192;
impl State {
    fn entity_request(
        &mut self,
        dimension: String,
        command: EntityCommand,
        numbers: &[f64],
    ) -> Result<(), String> {
        if numbers.iter().any(|v| !v.is_finite()) {
            return Err("positions and velocities must be numbers".into());
        }
        if let Some(view) = self.world.lock().ok().as_ref().and_then(|v| v.as_ref())
            && !view.has_dimension(&dimension)
        {
            return Err(format!("no dimension {dimension}"));
        }
        let made = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Entity { .. }))
            .count();
        if made >= ENTITY_REQUESTS {
            return Err(format!("at most {ENTITY_REQUESTS} entity requests a call"));
        }
        self.out.push(Output::Entity {
            name: self.name.clone(),
            id: self.id.clone(),
            dimension,
            command,
        });
        Ok(())
    }
    fn ticket(&mut self) -> u64 {
        let t = self.next_ticket;
        self.next_ticket += 1;
        t
    }
}
/// An entity kind: a namespaced id of plain characters.
fn kind_ok(kind: &str) -> bool {
    (1..=128).contains(&kind.len())
        && kind.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'/' | b'-' | b':')
        })
}
impl entities::Host for State {
    fn mobs(&mut self, dimension: String) -> Vec<entities::Mob> {
        if self.snapshot.mobs_dimension != dimension {
            return Vec::new();
        }
        self.snapshot
            .mobs
            .iter()
            .map(|m| entities::Mob {
                id: m.id,
                kind: m.kind.clone(),
                x: m.pos[0],
                y: m.pos[1],
                z: m.pos[2],
                yaw: m.yaw,
                health: m.health,
                max_health: m.max_health,
            })
            .collect()
    }
    fn spawn_mob(
        &mut self,
        dimension: String,
        kind: String,
        x: f64,
        y: f64,
        z: f64,
        options: entities::MobOptions,
    ) -> Result<u64, String> {
        if !kind_ok(&kind) {
            return Err(format!("{kind:?} is not an entity kind"));
        }
        let ticket = self.next_ticket;
        let command = EntityCommand::SpawnMob {
            ticket,
            kind,
            pos: [x, y, z],
            options,
        };
        self.entity_request(dimension, command, &[x, y, z])?;
        self.next_ticket += 1;
        Ok(ticket)
    }
    fn summon(
        &mut self,
        dimension: String,
        kind: String,
        x: f64,
        y: f64,
        z: f64,
        nbt: String,
    ) -> Result<u64, String> {
        if !kind_ok(&kind) {
            return Err(format!("{kind:?} is not an entity kind"));
        }
        if nbt.len() > SUMMON_BYTES {
            return Err(format!("nbt is {SUMMON_BYTES} bytes at most"));
        }
        let ticket = self.next_ticket;
        let command = EntityCommand::Summon {
            ticket,
            kind,
            pos: [x, y, z],
            nbt,
        };
        self.entity_request(dimension, command, &[x, y, z])?;
        self.next_ticket += 1;
        Ok(ticket)
    }
    fn move_entity(
        &mut self,
        dimension: String,
        id: u32,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
    ) -> Result<(), String> {
        if !yaw.is_finite() || !pitch.is_finite() {
            return Err("yaw and pitch must be numbers".into());
        }
        let command = EntityCommand::Move {
            id,
            pos: [x, y, z],
            yaw,
            pitch,
        };
        self.entity_request(dimension, command, &[x, y, z])
    }
    fn set_display(
        &mut self,
        dimension: String,
        id: u32,
        display: entities::Display,
    ) -> Result<(), String> {
        let d = &display;
        let floats = [
            d.translation.0, d.translation.1, d.translation.2,
            d.left_rotation.0, d.left_rotation.1, d.left_rotation.2, d.left_rotation.3,
            d.scale.0, d.scale.1, d.scale.2,
            d.right_rotation.0, d.right_rotation.1, d.right_rotation.2, d.right_rotation.3,
        ];
        if floats.iter().any(|v| !v.is_finite()) {
            return Err("a display's numbers must be finite".into());
        }
        self.entity_request(dimension, EntityCommand::SetDisplay { id, display }, &[])
    }
    fn remove_entity(&mut self, dimension: String, id: u32) -> Result<(), String> {
        self.entity_request(dimension, EntityCommand::RemoveEntity { id }, &[])
    }
    fn teleport_mob(&mut self, dimension: String, id: u32, x: f64, y: f64, z: f64) -> Result<(), String> {
        self.entity_request(dimension, EntityCommand::TeleportMob { id, pos: [x, y, z] }, &[x, y, z])
    }
    fn push_mob(&mut self, dimension: String, id: u32, vx: f64, vy: f64, vz: f64) -> Result<(), String> {
        let command = EntityCommand::PushMob { id, velocity: [vx, vy, vz] };
        self.entity_request(dimension, command, &[vx, vy, vz])
    }
    fn set_mob_health(&mut self, dimension: String, id: u32, health: f32) -> Result<(), String> {
        if !health.is_finite() || health <= 0.0 {
            return Err("health must be above 0 (kill-mob kills)".into());
        }
        self.entity_request(dimension, EntityCommand::SetMobHealth { id, health }, &[])
    }
    fn kill_mob(&mut self, dimension: String, id: u32) -> Result<(), String> {
        self.entity_request(dimension, EntityCommand::KillMob { id }, &[])
    }
    fn remove_mob(&mut self, dimension: String, id: u32) -> Result<(), String> {
        self.entity_request(dimension, EntityCommand::RemoveMob { id }, &[])
    }
}
/// Most bytes a mod message holds, and messages a call may send.
const NET_BYTES: usize = 30_000;
const NET_PER_CALL: usize = 32;
/// A channel of the mod `id`: `<id>:<name>`, the name `[a-z0-9_./-]{1,64}`.
fn own_channel(id: &str, channel: &str) -> bool {
    channel.split_once(':').is_some_and(|(prefix, name)| {
        prefix == id
            && (1..=64).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'/' | b'-'))
    })
}
impl State {
    fn check_net(&self, channel: &str, data: &[u8]) -> Result<(), String> {
        if !own_channel(&self.id, channel) {
            return Err(format!("the channel {channel:?} is not {}:<name>", self.id));
        }
        if data.len() > NET_BYTES {
            return Err(format!("{} bytes: at most {NET_BYTES} a message", data.len()));
        }
        let sent = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::NetSend { .. } | Output::NetToServer { .. }))
            .count();
        if sent >= NET_PER_CALL {
            return Err(format!("at most {NET_PER_CALL} messages a call"));
        }
        Ok(())
    }
}
impl net::Host for State {
    fn send(&mut self, player: String, channel: String, data: Vec<u8>) -> Result<(), String> {
        self.check_net(&channel, &data)?;
        if !self.snapshot.players.iter().any(|p| p.id == player) {
            return Err(format!("no player {player} is online"));
        }
        self.out.push(Output::NetSend {
            name: self.id.clone(),
            player,
            channel,
            data,
        });
        Ok(())
    }
}
impl net_client::Host for State {
    fn send_to_server(&mut self, channel: String, data: Vec<u8>) -> Result<(), String> {
        self.check_net(&channel, &data)?;
        self.out.push(Output::NetToServer {
            name: self.id.clone(),
            channel,
            data,
        });
        Ok(())
    }
}
/// Most recipes one `recipes-of-type` call hands over.
const RECIPES_PER_CALL: usize = 1 << 16;
impl recipe_types::Host for State {
    fn recipes_of_type(&mut self, kind: String) -> Vec<recipe_types::CustomRecipe> {
        let Ok(slot) = self.recipe_data.lock() else {
            return Vec::new();
        };
        let Some(data) = slot.as_ref() else {
            return Vec::new();
        };
        data.recipes.get(&kind).map_or_else(Vec::new, |list| {
            list.iter()
                .take(RECIPES_PER_CALL)
                .map(|(id, json)| recipe_types::CustomRecipe {
                    id: id.clone(),
                    json: json.clone(),
                })
                .collect()
        })
    }
    fn crafting_find(&mut self, grid: Vec<Option<ModItemStack>>) -> Option<recipe_types::CraftMatch> {
        if grid.len() != 9 {
            return None;
        }
        let slot = self.world.lock().ok()?;
        slot.as_ref()?.crafting_find(&grid)
    }
    fn crafting_matches(&mut self, id: String, grid: Vec<Option<ModItemStack>>) -> bool {
        if grid.len() != 9 {
            return false;
        }
        let Ok(slot) = self.world.lock() else { return false };
        slot.as_ref().is_some_and(|v| v.crafting_matches(&id, &grid))
    }
    fn item_max_stack_size(&mut self, item: String) -> Option<u32> {
        let slot = self.world.lock().ok()?;
        slot.as_ref()?.item_max_stack(&item)
    }
    fn item_tags(&mut self, item: String) -> Vec<String> {
        let Ok(slot) = self.recipe_data.lock() else { return Vec::new() };
        let Some(data) = slot.as_ref() else { return Vec::new() };
        let mut tags: Vec<String> = data.tags.iter().filter(|(_, items)| items.contains(&item)).map(|(t, _)| t.clone()).collect();
        tags.sort();
        tags
    }
    fn burn_time(&mut self, item: String) -> Option<u32> {
        let slot = self.recipe_data.lock().ok()?;
        slot.as_ref()?.burn_times.get(&item).copied().filter(|t| *t > 0)
    }
    fn cooking_recipes(&mut self, kind: String) -> Vec<recipe_types::CookingRecipe> {
        let Ok(slot) = self.recipe_data.lock() else {
            return Vec::new();
        };
        slot.as_ref().map_or_else(Vec::new, |d| {
            d.cooking
                .iter()
                .filter(|c| c.kind == kind)
                .take(RECIPES_PER_CALL)
                .map(|c| recipe_types::CookingRecipe {
                    inputs: c.inputs.clone(),
                    output: c.output.clone(),
                    count: c.count,
                    experience: c.experience,
                    time: c.time,
                })
                .collect()
        })
    }
    fn ingredient_items(&mut self, ingredient: String) -> Vec<String> {
        let Ok(slot) = self.recipe_data.lock() else {
            return Vec::new();
        };
        slot.as_ref()
            .map_or_else(Vec::new, |d| d.ingredient_items(&ingredient))
    }
}
/// `player-control.teleport`, `take-xp` and `play-sound` a mod may queue in one call.
const CONTROL_PER_CALL: usize = 8;
const SOUNDS_PER_CALL: usize = 16;
/// `/tp`'s bounds: horizontally within 30,000,000 blocks, vertically 20,000,000.
const TP_XZ: f64 = 30_000_000.0;
const TP_Y: f64 = 20_000_000.0;
impl player_control::Host for State {
    fn get_motion(&mut self, player: String) -> Option<Motion> {
        self.snapshot
            .motions
            .iter()
            .find(|(id, _)| *id == player)
            .map(|(_, m)| m.clone())
    }
    fn get_state(&mut self, player: String) -> Option<PlayerState> {
        self.snapshot.states.iter().find(|(id, _)| *id == player).map(|(_, s)| s.clone())
    }
    fn set_air(&mut self, player: String, air: i32) -> Result<(), String> {
        self.body_request(player, BodyChange::Air(air.clamp(0, 300)))
    }
    fn reduce_effect(&mut self, player: String, effect: String, ticks: i32) -> Result<(), String> {
        if ticks < 1 || !kind_ok(&effect) {
            return Err("an effect id and at least 1 tick".into());
        }
        self.body_request(player, BodyChange::ReduceEffect { effect, ticks })
    }
    fn reset_fall_distance(&mut self, player: String) -> Result<(), String> {
        self.body_request(player, BodyChange::ResetFall)
    }
    fn set_attribute_modifier(&mut self, player: String, attribute: String, id: String, amount: f64, operation: String) -> Result<(), String> {
        if !amount.is_finite() || !kind_ok(&attribute) || !kind_ok(&id) || !matches!(operation.as_str(), "add_value" | "add_multiplied_base" | "add_multiplied_total") {
            return Err("an attribute id, a modifier id, a finite amount and add_value, add_multiplied_base or add_multiplied_total".into());
        }
        self.body_request(player, BodyChange::Modifier { attribute, id, amount, operation, present: true })
    }
    fn remove_attribute_modifier(&mut self, player: String, attribute: String, id: String) -> Result<(), String> {
        if !kind_ok(&attribute) || !kind_ok(&id) {
            return Err("an attribute id and a modifier id".into());
        }
        self.body_request(player, BodyChange::Modifier { attribute, id, amount: 0.0, operation: "add_value".into(), present: false })
    }
    fn set_food(&mut self, player: String, food: u32, saturation: f32) -> Result<(), String> {
        if food > 20 || !saturation.is_finite() || saturation < 0.0 {
            return Err("a food level of 0 to 20 and a saturation of 0 or more".into());
        }
        if !self.snapshot.players.iter().any(|p| p.id == player) {
            return Err(format!("no player {player} is online"));
        }
        let queued = self.out.iter().filter(|o| matches!(o, Output::SetFood { .. })).count();
        if queued >= CONTROL_PER_CALL {
            return Err(format!("at most {CONTROL_PER_CALL} food changes per call"));
        }
        self.out.push(Output::SetFood { name: self.name.clone(), player, food, saturation });
        Ok(())
    }
    fn get_look(&mut self, player: String) -> Option<(f32, f32)> {
        self.snapshot
            .looks
            .iter()
            .find(|(id, _)| *id == player)
            .map(|(_, l)| *l)
    }
    fn teleport(
        &mut self,
        player: String,
        dimension: String,
        x: f64,
        y: f64,
        z: f64,
        yaw: Option<f32>,
        pitch: Option<f32>,
    ) -> Result<(), String> {
        if !self.snapshot.players.iter().any(|p| p.id == player) {
            return Err(format!("no player {player} is online"));
        }
        if !(x.is_finite() && y.is_finite() && z.is_finite())
            || x.abs() >= TP_XZ
            || z.abs() >= TP_XZ
            || y.abs() >= TP_Y
        {
            return Err("that position is outside the world".into());
        }
        if yaw.is_some_and(|v| !v.is_finite()) || pitch.is_some_and(|v| !v.is_finite()) {
            return Err("yaw and pitch must be numbers".into());
        }
        if let Some(view) = self.world.lock().ok().as_ref().and_then(|v| v.as_ref())
            && !view.has_dimension(&dimension)
        {
            return Err(format!("no dimension {dimension}"));
        }
        let queued = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Teleport { .. }))
            .count();
        if queued >= CONTROL_PER_CALL {
            return Err(format!("at most {CONTROL_PER_CALL} teleports per call"));
        }
        self.out.push(Output::Teleport {
            name: self.name.clone(),
            player,
            dimension,
            pos: [x, y, z],
            yaw,
            pitch,
        });
        Ok(())
    }
    fn set_motion(&mut self, player: String, x: f64, y: f64, z: f64) -> Result<(), String> {
        if ![x, y, z].iter().all(|v| v.is_finite() && v.abs() <= 10.0) {
            return Err("each component is a number from -10 to 10".into());
        }
        if !self.snapshot.players.iter().any(|p| p.id == player) {
            return Err(format!("no player {player} is online"));
        }
        let queued = self.out.iter().filter(|o| matches!(o, Output::SetMotion { .. })).count();
        if queued >= CONTROL_PER_CALL {
            return Err(format!("at most {CONTROL_PER_CALL} motions per call"));
        }
        self.out.push(Output::SetMotion { name: self.name.clone(), player, velocity: [x, y, z] });
        Ok(())
    }
    fn set_allow_flight(&mut self, player: String, allowed: bool) -> Result<(), String> {
        if !self.snapshot.players.iter().any(|p| p.id == player) {
            return Err(format!("no player {player} is online"));
        }
        let queued = self.out.iter().filter(|o| matches!(o, Output::SetAllowFlight { .. })).count();
        if queued >= CONTROL_PER_CALL {
            return Err(format!("at most {CONTROL_PER_CALL} flight changes per call"));
        }
        self.out.push(Output::SetAllowFlight { name: self.name.clone(), player, allowed });
        Ok(())
    }
    fn take_xp(&mut self, player: String, points: u32) -> Result<(), String> {
        if points == 0 {
            return Err("take at least 1 point".into());
        }
        if !self.snapshot.players.iter().any(|p| p.id == player) {
            return Err(format!("no player {player} is online"));
        }
        let queued = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::TakeXp { .. }))
            .count();
        if queued >= CONTROL_PER_CALL {
            return Err(format!("at most {CONTROL_PER_CALL} experience takes per call"));
        }
        self.out.push(Output::TakeXp {
            name: self.name.clone(),
            player,
            points,
        });
        Ok(())
    }
    fn play_sound(
        &mut self,
        dimension: String,
        sound: String,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
    ) {
        let queued = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::PlaySound { .. }))
            .count();
        if queued >= SOUNDS_PER_CALL
            || !(x.is_finite() && y.is_finite() && z.is_finite())
            || !volume.is_finite()
            || !pitch.is_finite()
        {
            return;
        }
        self.out.push(Output::PlaySound {
            name: self.name.clone(),
            dimension,
            sound: clip(sound),
            pos: [x, y, z],
            volume: volume.clamp(0.0, 10.0),
            pitch: pitch.clamp(0.5, 2.0),
        });
    }
}
impl players::Host for State {
    fn online(&mut self) -> Vec<ServerPlayer> {
        self.snapshot.players.clone()
    }
    fn tell(&mut self, player: String, text: String) -> bool {
        self.snapshot.players.iter().any(|p| p.id == player)
            && self.say(|name| Output::Tell {
                name,
                player,
                text: clip(text),
            })
    }
    fn tell_overlay(&mut self, player: String, text: String) -> bool {
        self.snapshot.players.iter().any(|p| p.id == player)
            && self.say(|name| Output::TellOverlay { name, player, text: clip(text) })
    }
    fn broadcast(&mut self, text: String) {
        self.say(|name| Output::Broadcast {
            name,
            text: clip(text),
        });
    }
}
/// A key a mod registered (capability `input`).
#[derive(Clone, Debug, PartialEq)]
pub struct ModKey {
    /// The mod's own name for it, echoed in `key-down`.
    pub name: String,
    /// What it does, for Controls.
    pub label: String,
    /// `key.keyboard.r`.
    pub key: String,
}

/// What capability `input` holds for one mod.
#[derive(Default)]
struct InputState {
    claimed: Vec<String>,
    keys: Vec<ModKey>,
    events: std::collections::VecDeque<InputEvent>,
    /// The mod's keys held now, by name.
    down: Vec<String>,
    /// It holds the mouse wheel.
    wheel: bool,
    /// The keys the player chose, `name → key`: they win over
    /// `register-key`'s.
    bindings: Vec<(String, String)>,
}
/// Events kept for `poll`, items claimed and keys, per mod.
const INPUT_EVENTS: usize = 256;
const CLAIMS: usize = 64;
const KEYS: usize = 16;

/// `key.keyboard.<name>` or `key.mouse.<name>`, as options.txt writes keys;
/// the game knows which names exist and ignores a key it has none for.
fn key_ok(key: &str) -> bool {
    let rest = key
        .strip_prefix("key.keyboard.")
        .or_else(|| key.strip_prefix("key.mouse."));
    rest.is_some_and(|r| {
        (1..=32).contains(&r.len())
            && r.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.')
    })
}

impl InputState {
    /// Key `name` is `key` now; held, it lets go first.
    fn rebind(&mut self, name: &str, key: String) {
        let Some(k) = self.keys.iter_mut().find(|k| k.name == name) else {
            return;
        };
        k.key = key;
        if let Some(i) = self.down.iter().position(|n| n == name) {
            let name = self.down.swap_remove(i);
            self.push(InputEvent::KeyUp(name));
        }
    }
    fn push(&mut self, event: InputEvent) {
        if self.events.len() == INPUT_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

impl input::Host for State {
    fn claim_items(&mut self, ids: Vec<String>) {
        self.input.claimed = ids.into_iter().take(CLAIMS).map(clip).collect();
    }
    fn register_key(&mut self, name: String, label: String, key: String) -> Result<(), String> {
        if !key_ok(&key) {
            return Err(format!(
                "{key:?} is not a key as options.txt writes them (key.keyboard.r)"
            ));
        }
        let (name, label) = (clip(name), clip(label));
        let key = match self.input.bindings.iter().find(|(n, _)| *n == name) {
            Some((_, chosen)) => chosen.clone(),
            None => key,
        };
        if let Some(k) = self.input.keys.iter_mut().find(|k| k.name == name) {
            (k.label, k.key) = (label, key);
            return Ok(());
        }
        if self.input.keys.len() == KEYS {
            return Err(format!("a mod may register {KEYS} keys"));
        }
        self.input.keys.push(ModKey { name, label, key });
        Ok(())
    }
    fn held(&mut self) -> Option<HeldItem> {
        self.snapshot.held.clone()
    }
    fn poll(&mut self) -> Vec<InputEvent> {
        self.input.events.drain(..).collect()
    }
    fn mapping(&mut self, name: String) -> Option<input::MappingState> {
        let (_, label, down) = self.snapshot.mappings.iter().find(|m| m.0 == name)?;
        Some(input::MappingState {
            label: label.clone(),
            down: *down,
        })
    }
    fn clicks_per_second(&mut self) -> (u32, u32) {
        self.snapshot.cps
    }
    fn claim_wheel(&mut self, on: bool) {
        self.input.wheel = on;
    }
}

/// HUD shapes one mod may keep (capability `render`).
const SHAPES: usize = 1024;
/// The longest jolt `render.shake` makes, in seconds.
const SHAKE_SECONDS: f32 = 10.0;
/// `render.set-zoom`'s limits: `Camera.tickFov` keeps the modifier at 0.1
/// or more, and turning never stops.
const ZOOM_MAX: f32 = 10.0;
const TURN_MIN: f32 = 0.05;
/// `set-minimap`'s side in GUI pixels and its blocks per map pixel.
const MINIMAP_SIZE: (f32, f32) = (16.0, 256.0);
const MINIMAP_SCALE: (f32, f32) = (0.5, 4.0);
/// `set-far-terrain`'s distance in chunks, as the LOD Distance setting's.
const FAR_TERRAIN_DISTANCE: (u32, u32) = (64, 1024);

/// A mod's corner panel: shapes in a `width` × `height` box the game
/// places in `corner`, stacked with the other overlays.
#[derive(Clone, Debug)]
pub struct CornerPanel {
    pub corner: RenderCorner,
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Shape>,
}

fn clip_shapes(shapes: Vec<Shape>) -> Vec<Shape> {
    shapes
        .into_iter()
        .take(SHAPES)
        .map(|shape| match shape {
            Shape::Sprite(mut s) => {
                s.sprite = clip(s.sprite);
                Shape::Sprite(s)
            }
            Shape::Item(mut i) => {
                i.item = clip(i.item);
                i.scale = if i.scale.is_finite() {
                    i.scale.clamp(0.0, 16.0)
                } else {
                    1.0
                };
                Shape::Item(i)
            }
            Shape::Text(mut t) => {
                t.text = clip(t.text);
                t.color &= 0xFF_FFFF;
                Shape::Text(t)
            }
            other => other,
        })
        .collect()
}

impl render::Host for State {
    fn set_hud(&mut self, shapes: Vec<Shape>) {
        self.shapes = clip_shapes(shapes);
    }
    fn set_corner(&mut self, corner: RenderCorner, width: f32, height: f32, shapes: Vec<Shape>) {
        let size = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1024.0)
            } else {
                0.0
            }
        };
        self.corner = Some(CornerPanel {
            corner,
            width: size(width),
            height: size(height),
            shapes: clip_shapes(shapes),
        });
    }
    fn clear_corner(&mut self) {
        self.corner = None;
    }
    fn text_width(&mut self, text: String) -> f32 {
        let widths = &self.snapshot.font_widths;
        text.chars()
            .map(|c| widths.get(c as usize).copied().unwrap_or(6.0))
            .sum::<f32>()
            .ceil()
    }
    fn set_zoom(&mut self, zoom: ModZoom) {
        let finite = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        let factor = finite(zoom.factor, 1.0, ZOOM_MAX);
        self.zoom = (factor > 1.0).then(|| ModZoom {
            factor,
            smooth: zoom.smooth,
            turn: if zoom.turn.is_finite() {
                zoom.turn.clamp(TURN_MIN, 1.0)
            } else {
                1.0
            },
        });
    }
    fn set_minimap(&mut self, minimap: Option<ModMinimap>) {
        let finite = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        self.minimap = minimap.map(|m| ModMinimap {
            size: finite(m.size, MINIMAP_SIZE.0, MINIMAP_SIZE.1),
            blocks_per_pixel: finite(m.blocks_per_pixel, MINIMAP_SCALE.0, MINIMAP_SCALE.1),
            ..m
        });
    }
    fn set_far_terrain(&mut self, far_terrain: Option<ModFarTerrain>) {
        self.far_terrain = far_terrain.map(|f| ModFarTerrain {
            distance: f
                .distance
                .clamp(FAR_TERRAIN_DISTANCE.0, FAR_TERRAIN_DISTANCE.1),
            ..f
        });
    }
    fn frame_stats(&mut self) -> FrameRate {
        let (fps, min, low) = self.snapshot.frame;
        FrameRate { fps, min, low }
    }
    fn gui_size(&mut self) -> (f32, f32) {
        self.snapshot.gui_size
    }
    fn shake(&mut self, strength: f32, seconds: f32) {
        let ok = |v: f32, max: f32| {
            if v.is_finite() {
                v.clamp(0.0, max)
            } else {
                0.0
            }
        };
        let (strength, seconds) = (ok(strength, 1.0), ok(seconds, SHAKE_SECONDS));
        if strength > 0.0 && seconds > 0.0 {
            self.out.push(Output::Shake { strength, seconds });
        }
    }
}

impl command::Host for State {
    fn run(&mut self, line: String) {
        let queued = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Command { .. }))
            .count();
        if queued < CHAT_PER_CALL {
            self.out.push(Output::Command { line: clip(line) });
        }
    }
    fn run_quiet(&mut self, lines: Vec<String>) {
        let queued: usize = self
            .out
            .iter()
            .map(|o| match o {
                Output::QuietCommands { lines, .. } => lines.len(),
                _ => 0,
            })
            .sum();
        let room = QUIET_PER_CALL.saturating_sub(queued);
        if room > 0 && !lines.is_empty() {
            self.out.push(Output::QuietCommands {
                mod_id: self.id.clone(),
                lines: lines.into_iter().take(room).map(clip).collect(),
            });
        }
    }
}
impl hud::Host for State {
    fn set_panel(&mut self, corner: Corner, lines: Vec<Line>) {
        let lines = lines
            .into_iter()
            .take(HUD_LINES)
            .map(|l| Line {
                text: clip(l.text),
                color: l.color & 0xFF_FFFF,
            })
            .collect();
        self.panel = Some(Panel { corner, lines });
    }
    fn clear(&mut self) {
        self.panel = None;
    }
}
impl ui::Host for State {
    fn open_screen(&mut self, mut screen: ui::Screen) {
        screen.title = clip(screen.title);
        screen.widgets = clip_widgets(screen.widgets);
        self.ui.screen = Some(screen);
        self.ui.open = true;
    }
    fn close_screen(&mut self) {
        self.ui.screen = None;
        self.ui.close = true;
    }
    fn set_settings(&mut self, widgets: Vec<ui::Widget>) {
        self.ui.settings = clip_widgets(widgets);
    }
}
impl storage::Host for State {
    fn get(&mut self, key: String) -> Option<Vec<u8>> {
        storage_key_ok(&key)
            .then(|| std::fs::read(self.data_dir.join(&key)).ok())
            .flatten()
    }
    fn set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        if !storage_key_ok(&key) {
            return Err(format!("{key:?} is not [a-z0-9_.-]{{1,64}}"));
        }
        let len = value.len() as u64;
        if len > self.storage_budget {
            return Err("too much storage traffic in one call".into());
        }
        // Everything already on disk but the file this call replaces.
        let existing: Vec<std::fs::DirEntry> = std::fs::read_dir(&self.data_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy() != key)
            .collect();
        if existing.len() >= STORAGE_MAX_KEYS {
            return Err(format!("would hold over {STORAGE_MAX_KEYS} keys"));
        }
        let used: u64 = existing
            .iter()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();
        if used + len > STORAGE_QUOTA {
            return Err(format!(
                "would use over {} MiB of storage",
                STORAGE_QUOTA >> 20
            ));
        }
        std::fs::create_dir_all(&self.data_dir).map_err(|e| e.to_string())?;
        std::fs::write(self.data_dir.join(&key), value).map_err(|e| e.to_string())?;
        self.storage_budget -= len;
        Ok(())
    }
    fn delete(&mut self, key: String) {
        if storage_key_ok(&key) {
            let _ = std::fs::remove_file(self.data_dir.join(&key));
        }
    }
}

/// A storage key: one path segment of `[a-z0-9_.-]{1,64}`, not `.` or `..`.
/// No `/`, so a key can never leave the mod's own directory.
fn storage_key_ok(key: &str) -> bool {
    let chars_ok = (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
    chars_ok && key != "." && key != ".."
}
