//! A content mod's `assets/` and `data/` (API 0.2, docs/modpacks.md): the
//! files it adds to the game's resource tree, and the blocks, items, mob
//! effects, entity types and menu types it declares in
//! `data/<id>/horizon/{block,item,mob_effect,entity_type,menu}/`.
//!
//! Content is data, not code: nothing here runs in the sandbox. What a mod
//! may add is fenced instead. Its own namespaces (its id and the ones its
//! manifest lists), plus additions to vanilla tags and to the convention
//! tags (`c`, which NeoForge and Fabric mods share). Paths are plain, files
//! and totals are capped, vanilla ids cannot be redefined, and no two mods
//! share a namespace ([`clash`]).
use crate::manifest::{Capability, MANIFEST, Manifest};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
};

/// Content files per mod, bytes per file and bytes per mod.
pub const MAX_FILES: usize = 65_536;
pub const MAX_FILE: u64 = 16 << 20;
pub const MAX_TOTAL: u64 = 512 << 20;

/// Checks a content path: `assets/` or `data/`, then `/`-separated parts of
/// `[a-z0-9_.-]`, none of them `.` or `..` (vanilla's resource-location
/// characters, so the path means the same on every OS).
pub fn check_path(name: &str) -> Result<(), String> {
    let bad = || format!("{name:?} is not a plain assets/ or data/ path");
    let mut parts = name.split('/');
    if !matches!(parts.next(), Some("assets" | "data")) {
        return Err(bad());
    }
    let mut count = 0;
    for part in parts {
        count += 1;
        let chars_ok = part
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
        if part.is_empty() || part == "." || part == ".." || !chars_ok {
            return Err(bad());
        }
    }
    // assets/<namespace>/<file> at least.
    if count < 2 {
        return Err(bad());
    }
    Ok(())
}

/// A vanilla identifier, `minecraft:path`.
/// `Identifier.isValidNamespace` and `isValidPath` on an id with both parts
/// given: `[a-z0-9_.-]`, and `/` in the path too.
fn namespaced_id(id: &str) -> bool {
    let ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b);
    id.split_once(':').is_some_and(|(namespace, path)| {
        !namespace.is_empty()
            && !path.is_empty()
            && namespace.bytes().all(ok)
            && path.bytes().all(|b| ok(b) || b == b'/')
    })
}

fn vanilla_id(id: &str) -> bool {
    id.strip_prefix("minecraft:").is_some_and(|path| {
        !path.is_empty()
            && path
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
    })
}

/// What a declared block changes from its template; unset keeps the
/// template's value.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockProperties {
    /// Destroy time in seconds of hand mining (-1: unbreakable).
    pub hardness: Option<f32>,
    pub explosion_resistance: Option<f32>,
    /// Light emitted, 0 to 15.
    pub light: Option<u8>,
    pub requires_tool: Option<bool>,
    pub friction: Option<f32>,
    /// A `MapColor` id, 0 to 61.
    pub map_color: Option<u8>,
    /// A vanilla block whose sounds this block uses.
    pub sounds: Option<String>,
}

/// Per-state outline and collision boxes, selected by one state property.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockShapes {
    pub property: String,
    pub values: BTreeMap<String, Vec<[f64; 6]>>,
}

/// A state property a block declares on its own, beside its template's.
/// A block's states become the template's states times these values, so a
/// pipe can have a connection per side without a template that has one.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StateProperty {
    /// `false` and `true`, in vanilla's spelling.
    Boolean,
    /// A list of values, in the order the state space is built in.
    Enum { values: Vec<String> },
}

impl StateProperty {
    /// Its values, in the order the state space is built in; a boolean's two
    /// come `false` first, as vanilla orders them.
    pub fn values(&self) -> Vec<&str> {
        match self {
            StateProperty::Boolean => vec!["false", "true"],
            StateProperty::Enum { values } => values.iter().map(String::as_str).collect(),
        }
    }
}

/// A vanilla state property's name, and a value's: lower case, digits and
/// underscores (`minecraft:oak_log` is `axis`, a repeater is `delay`).
fn state_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockFile {
    template: String,
    #[serde(default)]
    properties: BlockProperties,
    shapes: Option<BlockShapes>,
    #[serde(default)]
    state_properties: BTreeMap<String, StateProperty>,
    #[serde(default)]
    default_state: BTreeMap<String, String>,
    #[serde(default)]
    connects_to: Option<String>,
    #[serde(default)]
    thickness: Option<f32>,
    #[serde(default = "yes")]
    item: bool,
    #[serde(default)]
    components: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    redstone_emitter: bool,
}
fn yes() -> bool {
    true
}

/// A block the mod adds: a vanilla template's behaviour, states and shapes
/// under a new id, with `properties` applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// `<mod id>:<path>`.
    pub id: String,
    /// The vanilla block it copies, `minecraft:<path>`.
    pub template: String,
    pub properties: BlockProperties,
    pub shapes: Option<BlockShapes>,
    /// State properties of its own, which its template does not have.
    pub state_properties: BTreeMap<String, StateProperty>,
    /// State properties overlaid on the template default.
    pub default_state: BTreeMap<String, String>,
    /// Also add a block item with the same id.
    pub item: bool,
    /// Its item's component patch, as an [`Item`]'s.
    pub components: BTreeMap<String, serde_json::Value>,
    /// A connecting block (template `minecraft:chorus_plant`, a `PipeBlock`
    /// without the chorus plant's rules): the block tag, without `#`, whose
    /// blocks it joins besides itself (a cable's machines).
    pub connects_to: Option<String>,
    /// A connecting block's width in pixels (2 to 14): its shapes, the
    /// chorus plant's (10) when unset.
    pub thickness: Option<f32>,
    /// A redstone source (round 4, item 4; Mekanism's `AttributeRedstoneEmitter`): wires join it and it gives the signal its mod
    /// sets with `world.set-redstone-output` (none until then).
    pub redstone_emitter: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreativeTabFile {
    title: String,
    icon: String,
    items: Vec<String>,
}

/// A creative inventory tab the mod adds (`data/<id>/horizon/creative_tab/`):
/// its title, the item on its button, and its items in order. Items no mod
/// or vanilla registers are left out when the game loads.
#[derive(Clone, Debug, PartialEq)]
pub struct CreativeTab {
    /// `<mod id>:<path>`.
    pub id: String,
    pub title: String,
    pub icon: String,
    pub items: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemFile {
    template: String,
    max_stack_size: Option<u8>,
    rarity: Option<Rarity>,
    #[serde(default)]
    components: BTreeMap<String, serde_json::Value>,
    crafting_remainder: Option<String>,
    #[serde(default)]
    energy: Option<crate::item_energy::EnergySpec>,
}

/// An item the mod adds: a vanilla template item's components under a new
/// id, with these changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    /// The vanilla item it copies, `minecraft:<path>`.
    pub template: String,
    pub max_stack_size: Option<u8>,
    pub rarity: Option<Rarity>,
    /// A 26.3 data component patch over the template's: `!minecraft:<type>` removes one.
    pub components: BTreeMap<String, serde_json::Value>,
    /// What it leaves in the crafting grid (`Item.Properties.craftRemainder`):
    /// a vanilla item or one of the mod's own.
    pub crafting_remainder: Option<String>,
    /// The energy it holds (round 4, item 5; NeoForge's item `IEnergyStorage`): what any mod reads and moves with
    /// `recipe-types.item-energy`.
    pub energy: Option<crate::item_energy::EnergySpec>,
}

/// `MobEffectCategory`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EffectCategory {
    Beneficial,
    Harmful,
    Neutral,
}

/// A mob effect the mod adds: what `new MobEffect(category, color)` takes.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MobEffect {
    /// `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    pub category: EffectCategory,
    /// RGB, `0x000000` to `0xFFFFFF`.
    pub color: u32,
}

/// the vanilla entity types a mod's type may copy: thrown item
/// projectiles (`ThrowableItemProjectile`).
pub const ENTITY_TEMPLATES: [&str; 1] = ["minecraft:snowball"];

/// an entity type the mod adds: a copy of a vanilla template's, thrown
/// from `item` and drawn as it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntityType {
    /// `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    /// one of [`ENTITY_TEMPLATES`].
    pub template: String,
    /// one of the mod's own items, which throws it.
    pub item: String,
    /// what a hit deals to an entity, as `thrown` damage.
    #[serde(default)]
    pub damage: f32,
}

/// Most strings a menu carries and bytes each holds (`world.set-menu-text`, a screen's `text_data` and lists).
pub const MENU_TEXTS: usize = 64;
pub const MENU_TEXT_BYTES: usize = 256;

/// A living entity the mod adds (FORK-10, the Robit): its size, health and speed, its skins and the slots of its container. The game
/// keeps the entities of a kind with a state of the mod's own (`data`, items, energy, skin, name) and asks the mod to tick them.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntityKind {
    /// `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    pub width: f32,
    pub height: f32,
    /// Eye height; the entity's own height times 0.85 (a living entity's usual) when unset.
    #[serde(default)]
    pub eye_height: Option<f32>,
    #[serde(default = "default_kind_health")]
    pub max_health: f32,
    #[serde(default = "default_kind_speed")]
    pub speed: f32,
    /// Kept when its chunk unloads and never despawned (`persistent`).
    #[serde(default = "default_true")]
    pub persistent: bool,
    /// What it can look like: a texture and optionally a model of the mod's own. The first is the default.
    pub skins: Vec<EntitySkin>,
    /// Slots of its container (the Robit's 30); 0 for none.
    #[serde(default)]
    pub container_slots: u16,
    /// Fire and lava do not hurt it and it does not burn (`EntityType.fireImmune`).
    #[serde(default)]
    pub fire_immune: bool,
    /// The menu type (`data/<mod>/horizon/menu/<path>.json` of the same mod, `<mod>:<path>`) that `open-entity-menu` opens, drawn by
    /// its `screen` as a block's (FORK-10 5b): its first `container_slots` slots are the entity's container, the ones past them are
    /// virtual (a crafting grid, an anvil's), held only while the menu is open. None opens the container as a chest.
    #[serde(default)]
    pub menu: Option<String>,
    /// Its model in parts (FORK-10 5d): each bone is drawn by its own item model, turned about the model's centre and placed at its
    /// `pivot`, and a bone with `walk` swings while it walks. None draws the skin's one model whole.
    #[serde(default)]
    pub bones: Vec<EntityBone>,
}

/// One part of an entity kind's model (FORK-10 5d).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntityBone {
    pub id: String,
    /// The item model that draws it (`<mod>:<name>`, as `minecraft:item_model` names one), turned about its centre.
    pub model: String,
    /// Where its centre sits, in blocks from the entity's feet: x to its left, y up, z forward (before the entity's turn).
    pub pivot: [f32; 3],
    /// How it swings while the entity walks; none keeps it still.
    #[serde(default)]
    pub walk: Option<BoneWalk>,
}

/// A bone's swing while walking: vanilla's limb swing (`cos(walk position * 0.6662 + phase) * walk speed`), `degrees` at full speed
/// about `axis`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BoneWalk {
    pub axis: BoneAxis,
    pub degrees: f32,
    /// Degrees of the cycle it is ahead: 180 for the other leg.
    #[serde(default)]
    pub phase: f32,
}

/// The axis a bone turns about.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoneAxis {
    X,
    Y,
    Z,
}

fn default_kind_health() -> f32 {
    20.0
}
fn default_kind_speed() -> f32 {
    0.25
}
fn default_true() -> bool {
    true
}

/// One look of an entity kind.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EntitySkin {
    pub id: String,
    /// A sprite (`mymod:entity/robit`, from `assets/mymod/textures/entity/`).
    pub texture: String,
    /// A model of the mod's own (`assets/<ns>/horizon/entity_model/<name>.json`), the kind's default when unset.
    #[serde(default)]
    pub model: Option<String>,
    /// For a kind with bones: the item model of each bone this skin draws differently (bone id to model).
    #[serde(default)]
    pub bones: BTreeMap<String, String>,
}

impl EntityKind {
    fn check(&self, name: &str) -> Result<(), String> {
        let finite = |v: f32| v.is_finite();
        if !(finite(self.width) && finite(self.height) && (0.1..=16.0).contains(&self.width) && (0.1..=16.0).contains(&self.height)) {
            return Err(format!("{name}: width and height are 0.1 to 16"));
        }
        if self.eye_height.is_some_and(|e| !finite(e) || !(0.0..=16.0).contains(&e)) {
            return Err(format!("{name}: eye_height is 0 to 16"));
        }
        if !finite(self.max_health) || !(0.5..=1024.0).contains(&self.max_health) {
            return Err(format!("{name}: max_health is 0.5 to 1024"));
        }
        if !finite(self.speed) || !(0.0..=4.0).contains(&self.speed) {
            return Err(format!("{name}: speed is 0 to 4"));
        }
        if self.skins.is_empty() || self.skins.len() > 64 {
            return Err(format!("{name}: 1 to 64 skins"));
        }
        for (i, s) in self.skins.iter().enumerate() {
            let ok = |t: &str| t.split_once(':').is_some_and(|(n, p)| !n.is_empty() && !p.is_empty() && !t.contains(".."));
            if s.id.is_empty() || s.id.len() > 64 || self.skins[..i].iter().any(|o| o.id == s.id) {
                return Err(format!("{name}: skin ids are 1 to 64 bytes and not repeated"));
            }
            if !ok(&s.texture) || s.model.as_deref().is_some_and(|m| !ok(m)) {
                return Err(format!("{name}: a skin's texture and model are <mod id>:<path>"));
            }
        }
        if self.container_slots > 256 {
            return Err(format!("{name}: container_slots is 256 at most"));
        }
        let model_ok = |t: &str| t.split_once(':').is_some_and(|(n, p)| !n.is_empty() && !p.is_empty() && !t.contains(".."));
        if self.bones.len() > 16 {
            return Err(format!("{name}: 16 bones at most"));
        }
        for (i, b) in self.bones.iter().enumerate() {
            if b.id.is_empty() || b.id.len() > 64 || self.bones[..i].iter().any(|o| o.id == b.id) {
                return Err(format!("{name}: bone ids are 1 to 64 bytes and not repeated"));
            }
            if !model_ok(&b.model) {
                return Err(format!("{name}: bone {}: the model is <mod id>:<path>", b.id));
            }
            if !b.pivot.iter().all(|v| v.is_finite() && v.abs() <= 16.0) {
                return Err(format!("{name}: bone {}: the pivot is within 16 blocks", b.id));
            }
            if b.walk.as_ref().is_some_and(|w| !(w.degrees.is_finite() && (0.0..=180.0).contains(&w.degrees) && w.phase.is_finite())) {
                return Err(format!("{name}: bone {}: walk degrees are 0 to 180", b.id));
            }
        }
        for s in &self.skins {
            for (bone, model) in &s.bones {
                if !self.bones.iter().any(|b| &b.id == bone) || !model_ok(model) {
                    return Err(format!("{name}: skin {}: bone {bone} is not a bone of the kind, or its model is not <mod id>:<path>", s.id));
                }
            }
        }
        Ok(())
    }
}

/// a menu type the mod adds (vanilla's `MenuType`, NeoForge's
/// `IMenuTypeExtension.create`): its container's slot count, the
/// `ContainerData` count its menu syncs, and its take-only slots
/// (`FurnaceResultSlot.mayPlace`). The slots come first in the menu, then the
/// player's 36, and a mod screen (`native_screen`) places them.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MenuType {
    /// `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    /// 0 to [`MAX_MENU_SLOTS`]: a menu of buttons and text alone has none (the Portable Teleporter's, the Seismic Reader's).
    pub slots: u16,
    /// 0 to [`MAX_MENU_SLOTS`].
    #[serde(default)]
    pub data_slots: u16,
    /// slots a player takes from but never puts into, each below `slots`.
    #[serde(default)]
    pub results: Vec<u16>,
    /// The player's armor and off-hand slots shown beside the screen (Mekanism's `armorSideBar`): slots of the
    /// menu after its own, which hold the armor of whoever has it open.
    #[serde(default)]
    pub armor: Option<ArmorSlots>,
    /// Slots that take only some items (an upgrade slot): a player puts into `slot` only these item ids.
    #[serde(default)]
    pub accepts: Vec<SlotAccepts>,
    /// How the screen of a block of the same id is laid out and drawn, from
    /// the menu's data slots, without a line of code (API block 7).
    #[serde(default)]
    pub screen: Option<MenuScreen>,
}

/// A slot of a menu that takes only the items listed (`Slot.mayPlace`).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SlotAccepts {
    pub slot: u16,
    pub items: Vec<String>,
}

/// A container slot that sits in a window of the screen: shown (and clickable) only while that window is open, at a place
/// relative to the window's top left (so it follows the window when dragged).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WindowSlot {
    pub slot: u16,
    pub x: i32,
    pub y: i32,
}

/// Where the player's armor slots are on a menu's screen: four slots going down from `x`, `y` (the helmet first, then the
/// chestplate, the leggings and the boots, 18 apart), and the off-hand's `offhand` pixels under the last when given.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArmorSlots {
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub offhand: Option<i32>,
}

impl ArmorSlots {
    /// How many menu slots they are: the four, and the off-hand's.
    pub fn count(&self) -> usize {
        4 + usize::from(self.offhand.is_some())
    }
    /// Where slot `n` (0 the helmet .. 3 the boots, 4 the off-hand) is.
    pub fn at(&self, n: usize) -> Option<[i32; 2]> {
        match n {
            0..4 => Some([self.x, self.y + 18 * n as i32]),
            4 => self.offhand.map(|o| [self.x, self.y + 72 + o]),
            _ => None,
        }
    }
}

fn default_width() -> i32 {
    176
}
fn default_height() -> i32 {
    166
}
fn default_inventory_x() -> i32 {
    8
}
fn default_inventory_y() -> i32 {
    84
}

/// The screen of a mod menu type, in GUI pixels from its top left corner.
/// Vanilla's container screens are 176 wide and 166 high, with the player's
/// inventory at 8, 84.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MenuScreen {
    #[serde(default = "default_width")]
    pub width: i32,
    #[serde(default = "default_height")]
    pub height: i32,
    #[serde(default = "default_inventory_x")]
    pub inventory_x: i32,
    #[serde(default = "default_inventory_y")]
    pub inventory_y: i32,
    /// A sprite (`mymod:gui/machine`, from `assets/mymod/textures/gui/`) drawn
    /// over the whole panel instead of the plain grey one.
    #[serde(default)]
    pub texture: Option<String>,
    /// The screen draws its title and its "Inventory" label itself (as `text` widgets): the game leaves its own out.
    #[serde(default)]
    pub own_labels: bool,
    /// The player's inventory is not on the screen: its slots are neither drawn nor clicked, and the panel is `width` by `height`
    /// alone (a menu of buttons and text, the Portable Teleporter's).
    #[serde(default)]
    pub hide_inventory: bool,
    /// Where each container slot is (its item's top left); `null` hides the
    /// slot, and so do slots past the list.
    pub slots: Vec<Option<[i32; 2]>>,
    /// Drawn in this order, over the panel and under the items.
    #[serde(default)]
    pub widgets: Vec<Widget>,
    /// Windows a button can open over the panel (a machine's side configuration), each with widgets of its own at
    /// its own origin. One is open at a time; a press inside it goes to it alone.
    #[serde(default)]
    pub windows: Vec<Window>,
}

/// A window of a menu screen: a rectangle over the panel with its own widgets.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// A sprite drawn over the whole window, behind its widgets.
    #[serde(default)]
    pub texture: Option<String>,
    #[serde(default)]
    pub widgets: Vec<Widget>,
    /// Container slots that show in this window while it is open (their entry in the screen's `slots` is `null`).
    #[serde(default)]
    pub slots: Vec<WindowSlot>,
}

/// When a conditional group of widgets shows.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum When {
    /// A variable of the screen (set by a button's `local` action; unset is 0) is `eq`.
    Local { var: String, eq: i64 },
    /// What a value reads from the menu's data slots is `eq`.
    Data { value: Value, eq: i64 },
}

/// What a button does on this client, with no word to the server.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum LocalAction {
    /// Opens the window with this id (and closes the one open).
    Open(String),
    /// Closes the open window.
    Close,
    /// Sets a variable of the screen.
    Set { var: String, value: i64 },
}

/// A number a widget reads: a constant, a data slot of the menu, or two data
/// slots read as the low and the high 16 bits (the wire carries 16 bits a
/// data slot; an energy bar needs more).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum Value {
    Const(i64),
    Data(u16),
    Pair([u16; 2]),
    /// One nibble (4 bits) of a data slot: the slot and which nibble, 0 (lowest) to 3. A data slot carries four small
    /// numbers, as a machine's side configuration needs.
    Nibble([u16; 2]),
}

/// The way a bar or a progress arrow fills.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Up,
    Down,
    Left,
    Right,
}

/// What a gauge can hold: the sprite tiled in it (`mymod:gui/...`), its colour and its name.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GaugeType {
    pub texture: String,
    /// `#RRGGBB` or `#AARRGGBB`.
    pub tint: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub enum Widget {
    /// A rectangle `color` (`#RRGGBB` or `#AARRGGBB`) filled `value` of `max`.
    Bar {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        value: Value,
        max: Value,
        color: String,
        #[serde(default)]
        direction: Direction,
        /// Shown on hover; `{value}`, `{max}` and `{percent}` are filled in.
        #[serde(default)]
        tooltip: Option<String>,
    },
    /// Part of a sprite drawn at `x`, `y`, `w` by `h`: all of it, or its first
    /// `value` of `max` from the side `direction` says (a progress arrow).
    /// `src` is the part of the sprite, in its own pixels (`x, y, w, h`).
    Sprite {
        texture: String,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        #[serde(default)]
        src: Option<[i32; 4]>,
        #[serde(default)]
        value: Option<Value>,
        #[serde(default)]
        max: Option<Value>,
        #[serde(default)]
        direction: Direction,
        #[serde(default)]
        tooltip: Option<String>,
        /// Colours (`#RRGGBB`) the sprite is tinted with: `tints[tint_by - 1]`, untinted while `tint_by` is 0 or past the list
        /// (a progress arrow in the colour of the chemical it works on).
        #[serde(default)]
        tints: Vec<String>,
        #[serde(default)]
        tint_by: Option<Value>,
    },
    /// A line of text; `{0}`, `{1}`... are data slots. With `text_data: n` it shows string `n` of the block entity's menu text
    /// (`world.set-menu-text`) instead, nothing while there is none (FORK-20).
    Text {
        x: i32,
        y: i32,
        #[serde(default)]
        text: String,
        #[serde(default)]
        text_data: Option<u16>,
        #[serde(default)]
        color: Option<String>,
        /// Percent of the usual size, drawn from `x`, `y` (60 is the small print of an upgrade's description); 100 when unset.
        #[serde(default)]
        scale: Option<u16>,
    },
    /// A press inside it sends menu button `id` to the server, or does its `local` action here and sends nothing.
    /// A right press (a long press on a touch screen) sends `alt_id` when there is one.
    Button {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        #[serde(default)]
        id: i32,
        #[serde(default)]
        alt_id: Option<i32>,
        /// Sent instead of `id` when the press is made with Shift held (the on-screen Shift counts).
        #[serde(default)]
        shift_id: Option<i32>,
        #[serde(default)]
        local: Option<LocalAction>,
        #[serde(default)]
        tooltip: Option<String>,
        /// A colour (`#AARRGGBB`, usually translucent) drawn over the button while the cursor is on it.
        #[serde(default)]
        hover: Option<String>,
    },
    /// A rectangle filled with `colors[value]` (`#RRGGBB`), the last when `value` is past the list; its tooltip fills
    /// `{label}` with `labels[value]`.
    Swatch {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        value: Value,
        colors: Vec<String>,
        #[serde(default)]
        labels: Vec<String>,
        #[serde(default)]
        tooltip: Option<String>,
    },
    /// A tank: a rectangle filled from the bottom up to `value` of `max` with the chemical (or fluid) that `kind` says, a
    /// number from 1 into `types` (0 is an empty tank): the type's sprite tiled over it and tinted with its colour. Its
    /// tooltip fills `{label}` (the type's), `{value}` and `{max}`; `empty` is shown instead while `kind` is 0.
    Gauge {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        value: Value,
        max: Value,
        kind: Value,
        types: Vec<GaugeType>,
        /// Which way it fills: `up` (a tall gauge, the default) or `right` (a bar).
        #[serde(default)]
        direction: Direction,
        #[serde(default)]
        tooltip: Option<String>,
        #[serde(default)]
        empty: Option<String>,
    },
    /// Widgets shown while a condition holds.
    Show { when: When, widgets: Vec<Widget> },
    /// The icon of the block that stands beside the machine on one of its `side`s (0 front, 1 left, 2 right, 3 back, 4 top,
    /// 5 bottom, as the machine faces), drawn 16 by 16 at `x`, `y`: what Mekanism's side buttons show.
    /// `facing` is a number the mod gives (north 0, east 1, south 2, west 3) to take the machine's front from instead of the
    /// block's own `facing` property.
    Neighbor {
        x: i32,
        y: i32,
        side: u8,
        #[serde(default)]
        facing: Option<Value>,
    },
    /// The icon of item `item` (`minecraft:iron_ingot`), drawn 16 by 16 at `x`, `y`; nothing when the item is unknown.
    Item { x: i32, y: i32, item: String },
    /// A text box (`GuiTextField`, FORK-20): a press focuses it (and opens the keyboard on a touch screen), typing fills it
    /// up to `max_length` characters that `validator` allows, Enter confirms. The confirmed text goes to the server as a mod
    /// message on channel `<mod id>:menu_text` (`horizon_mod_sdk::server::menu_input` reads it), with this widget's `id`. It
    /// starts with string `text_data` of the menu text (`world.set-menu-text`) when it has one.
    /// A scrolling list (`GuiScrollList`, FORK-20): `count` rows (a data value), row `i` showing string `first + i` of the menu
    /// text (`world.set-menu-text`), `rows` visible at a time from the top of a `w` by `h` box, scrolled by the wheel or by
    /// dragging. A click on row `i` (not a drag) sends menu button `on_click + i`. With `source: items` the strings are item ids
    /// and each row shows the item's icon before its id.
    List {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        #[serde(default = "default_row_height")]
        row_height: i32,
        first: u16,
        count: Value,
        on_click: i32,
        #[serde(default)]
        source: ListSource,
        #[serde(default)]
        color: Option<String>,
    },
    /// A line graph (Mekanism's `GuiDoubleGraph`, the heating of the fission and fusion reactors; FORK-20 part 4): the client keeps the
    /// last `samples` readings of each series (one a tick while the screen is open) and draws them as lines from left (oldest) to right
    /// (newest); the vertical scale goes from the least to the greatest reading kept, at least `min_scale` tall. Its tooltip fills `{0}`,
    /// `{1}`... with data slots, as a text does.
    Graph {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        series: Vec<GraphSeries>,
        #[serde(default = "default_samples")]
        samples: u16,
        #[serde(default = "default_min_scale")]
        min_scale: i64,
        #[serde(default)]
        tooltip: Option<String>,
    },
    /// A grid of item cells (Mekanism's QIO Dashboard catalogue): `columns` by `rows` cells of `cell` pixels from `x`, `y`, showing the
    /// entries its mod gives (`world.set-menu-grid`, an item menu's `menu-grid`: item, components, count up to 2^64) from the scroll
    /// down, the count abbreviated over 999 (`1.2k`); the wheel and the bar at its right scroll it by rows, and a press on a cell
    /// goes to the mod (`menu-grid-click`). One a screen.
    ItemGrid {
        x: i32,
        y: i32,
        columns: u16,
        rows: u16,
        #[serde(default = "default_cell")]
        cell: u16,
    },
    #[serde(rename = "textfield")]
    TextField {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: i32,
        #[serde(default = "default_field_length")]
        max_length: u16,
        #[serde(default)]
        validator: Validator,
        #[serde(default)]
        text_data: Option<u16>,
    },
}

/// One line of a graph: what it reads and its colour.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphSeries {
    pub value: Value,
    /// `#RRGGBB` or `#AARRGGBB`.
    pub color: String,
}

fn default_cell() -> u16 {
    18
}
fn default_samples() -> u16 {
    40
}
fn default_min_scale() -> i64 {
    1
}

fn default_row_height() -> i32 {
    12
}

/// What a list's strings are.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListSource {
    /// Plain lines of text.
    #[default]
    Text,
    /// Item ids, shown as icons with their id.
    Items,
}

fn default_field_length() -> u16 {
    32
}

/// What a text box lets be typed (`TextValidator`).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Validator {
    /// Any printable character.
    #[default]
    Any,
    /// Digits only.
    Digit,
    /// Lower case letters, digits and `_ . / : - #` (a tag or a block, item or entity id).
    Id,
}

impl Validator {
    /// Whether `c` may be typed.
    pub fn allows(self, c: char) -> bool {
        match self {
            Validator::Any => !c.is_control(),
            Validator::Digit => c.is_ascii_digit(),
            Validator::Id => c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '/' | ':' | '-' | '#'),
        }
    }
}

/// The text box of `id` in `widgets` and what they hold (groups and windows too).
pub fn find_field<'a>(screen: &'a MenuScreen, id: i32) -> Option<&'a Widget> {
    fn walk<'a>(widgets: &'a [Widget], id: i32) -> Option<&'a Widget> {
        widgets.iter().find_map(|w| match w {
            Widget::TextField { id: i, .. } if *i == id => Some(w),
            Widget::Show { widgets, .. } => walk(widgets, id),
            _ => None,
        })
    }
    walk(&screen.widgets, id).or_else(|| screen.windows.iter().find_map(|w| walk(&w.widgets, id)))
}

/// `#RRGGBB` (opaque) or `#AARRGGBB` as `0xAARRGGBB`.
pub fn parse_color(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        6 => u32::from_str_radix(hex, 16).ok().map(|v| 0xFF00_0000 | v),
        8 => u32::from_str_radix(hex, 16).ok(),
        _ => None,
    }
}

impl MenuScreen {
    /// Checks the screen against its menu's slot and data slot counts.
    fn check(&self, name: &str, slots: u16, data_slots: u16) -> Result<(), String> {
        let sprite = |t: &str| {
            let ok = t.split_once(':').is_some_and(|(ns, path)| {
                !ns.is_empty()
                    && !path.is_empty()
                    && !t.contains("..")
                    && t.bytes().all(|b| {
                        b.is_ascii_lowercase()
                            || b.is_ascii_digit()
                            || matches!(b, b'_' | b'-' | b'.' | b'/' | b':')
                    })
            });
            ok.then_some(()).ok_or_else(|| format!("{name}: sprite {t:?} is not <mod id>:<path>"))
        };
        if !(16..=1024).contains(&self.width) || !(16..=1024).contains(&self.height) {
            return Err(format!("{name}: width and height are 16 to 1024"));
        }
        if self.slots.len() > usize::from(slots) {
            return Err(format!(
                "{name}: screen.slots lists {} slots, the menu has {slots}",
                self.slots.len()
            ));
        }
        if let Some(t) = &self.texture {
            sprite(t)?;
        }
        let windows: Vec<&str> = self.windows.iter().map(|w| w.id.as_str()).collect();
        if windows.len() > 8 {
            return Err(format!("{name}: 8 windows at most"));
        }
        for (i, w) in self.windows.iter().enumerate() {
            if w.id.is_empty() || windows[..i].contains(&w.id.as_str()) {
                return Err(format!("{name}: window ids are not empty and not repeated"));
            }
            if !(16..=1024).contains(&w.width) || !(16..=1024).contains(&w.height) {
                return Err(format!("{name}: a window's width and height are 16 to 1024"));
            }
            if let Some(t) = &w.texture {
                sprite(t)?;
            }
            if w.slots.iter().any(|s| s.slot >= slots) {
                return Err(format!("{name}: a window's slots are slots of the menu"));
            }
        }
        for w in &self.widgets {
            check_widget(w, name, data_slots, &windows, &sprite, 0)?;
        }
        for window in &self.windows {
            for w in &window.widgets {
                check_widget(w, name, data_slots, &windows, &sprite, 0)?;
            }
        }
        Ok(())
    }
}

/// Checks one widget (and what it holds) against the menu's data slot count and the screen's windows.
fn check_widget(
    w: &Widget,
    name: &str,
    data_slots: u16,
    windows: &[&str],
    sprite: &dyn Fn(&str) -> Result<(), String>,
    depth: u8,
) -> Result<(), String> {
    let data = |v: &Value| -> Result<(), String> {
        let bad = |i: u16| i >= data_slots;
        match v {
            Value::Data(i) if bad(*i) => Err(format!(
                "{name}: data slot {i} is past the last one, {}",
                i64::from(data_slots) - 1
            )),
            Value::Pair([a, b]) if bad(*a) || bad(*b) => Err(format!(
                "{name}: data slots {a} and {b} must both exist ({data_slots})"
            )),
            Value::Nibble([slot, which]) if bad(*slot) || *which > 3 => Err(format!(
                "{name}: a nibble is a data slot that exists and 0 to 3"
            )),
            _ => Ok(()),
        }
    };
    let rect = |x: i32, y: i32, w: i32, h: i32| {
        if w < 1 || h < 1 || x < -1024 || y < -1024 || x > 4096 || y > 4096 || w > 1024 || h > 1024 {
            Err(format!("{name}: a widget at {x}, {y} of {w} by {h} is out of range"))
        } else {
            Ok(())
        }
    };
    let colour = |c: &str| {
        parse_color(c)
            .map(|_| ())
            .ok_or_else(|| format!("{name}: colour {c:?} is #RRGGBB or #AARRGGBB"))
    };
    match w {
        Widget::Bar { x, y, w, h, value, max, color, .. } => {
            rect(*x, *y, *w, *h)?;
            data(value)?;
            data(max)?;
            colour(color)?;
        }
        Widget::Sprite { texture, x, y, w, h, src, value, max, tints, tint_by, .. } => {
            sprite(texture)?;
            rect(*x, *y, *w, *h)?;
            if let Some(v) = tint_by {
                data(v)?;
            }
            for t in tints {
                colour(t)?;
            }
            if value.is_some() != max.is_some() {
                return Err(format!("{name}: a sprite takes value and max together"));
            }
            if let Some(v) = value {
                data(v)?;
            }
            if let Some(v) = max {
                data(v)?;
            }
            if let Some([_, _, sw, sh]) = src
                && (*sw < 1 || *sh < 1)
            {
                return Err(format!("{name}: a sprite's src is x, y, w, h with w and h over 0"));
            }
        }
        Widget::Text { x, y, text, color, scale, text_data } => {
            if text_data.is_some_and(|n| usize::from(n) >= MENU_TEXTS) {
                return Err(format!("{name}: text_data is a string of the menu, 0 to {}", MENU_TEXTS - 1));
            }
            if scale.is_some_and(|s| !(10..=400).contains(&s)) {
                return Err(format!("{name}: a text's scale is 10 to 400 percent"));
            }
            rect(*x, *y, 1, 1)?;
            if text.len() > 256 {
                return Err(format!("{name}: text is 256 bytes at most"));
            }
            if let Some(c) = color {
                colour(c)?;
            }
            // `{n}` must name a data slot.
            let mut rest = text.as_str();
            while let Some(open) = rest.find('{') {
                let after = &rest[open + 1..];
                let Some(close) = after.find('}') else { break };
                // `{n}` is a data slot and `{a+b}` two of them.
                let names = after[..close].split('+').map(str::parse::<u16>);
                for i in names.flatten() {
                    if i >= data_slots {
                        return Err(format!("{name}: text names data slot {i}, past the last one"));
                    }
                }
                rest = &after[close + 1..];
            }
        }
        Widget::Button { x, y, w, h, local, hover, .. } => {
            if let Some(h) = hover {
                colour(h)?;
            }
            rect(*x, *y, *w, *h)?;
            match local {
                Some(LocalAction::Open(id)) if !windows.contains(&id.as_str()) => {
                    return Err(format!("{name}: a button opens window {id:?}, which the screen does not have"));
                }
                Some(LocalAction::Set { var, .. }) if var.is_empty() || var.len() > 32 => {
                    return Err(format!("{name}: a variable's name is 1 to 32 bytes"));
                }
                _ => {}
            }
        }
        Widget::Swatch { x, y, w, h, value, colors, labels, .. } => {
            rect(*x, *y, *w, *h)?;
            data(value)?;
            if colors.is_empty() || colors.len() > 32 || labels.len() > 32 {
                return Err(format!("{name}: a swatch has 1 to 32 colours"));
            }
            for c in colors {
                colour(c)?;
            }
        }
        Widget::Gauge { x, y, w, h, value, max, kind, types, .. } => {
            rect(*x, *y, *w, *h)?;
            data(value)?;
            data(max)?;
            data(kind)?;
            if types.is_empty() || types.len() > 128 {
                return Err(format!("{name}: a gauge has 1 to 128 types"));
            }
            for t in types {
                sprite(&t.texture)?;
                colour(&t.tint)?;
                if t.label.len() > 64 {
                    return Err(format!("{name}: a gauge type's label is 64 bytes at most"));
                }
            }
        }
        Widget::Neighbor { x, y, side, facing } => {
            rect(*x, *y, 16, 16)?;
            if let Some(v) = facing {
                data(v)?;
            }
            if *side > 5 {
                return Err(format!("{name}: a neighbour's side is 0 to 5"));
            }
        }
        Widget::ItemGrid { x, y, columns, rows, cell } => {
            if !(1..=16).contains(columns) || !(1..=16).contains(rows) || !(8..=32).contains(cell) {
                return Err(format!("{name}: an item grid has 1 to 16 columns and rows of 8 to 32 pixels"));
            }
            rect(*x, *y, i32::from(*columns) * i32::from(*cell) + 6, i32::from(*rows) * i32::from(*cell))?;
        }
        Widget::Graph { x, y, w, h, series, samples, min_scale, tooltip } => {
            rect(*x, *y, *w, *h)?;
            if series.is_empty() || series.len() > 8 {
                return Err(format!("{name}: a graph has 1 to 8 series"));
            }
            for s in series {
                data(&s.value)?;
                colour(&s.color)?;
            }
            if !(2..=400).contains(samples) {
                return Err(format!("{name}: a graph keeps 2 to 400 samples"));
            }
            if *min_scale < 1 {
                return Err(format!("{name}: a graph's min_scale is at least 1"));
            }
            if tooltip.as_ref().is_some_and(|t| t.len() > 256) {
                return Err(format!("{name}: a tooltip is 256 bytes at most"));
            }
        }
        Widget::List { x, y, w, h, row_height, first, count, color, .. } => {
            rect(*x, *y, *w, *h)?;
            if !(8..=64).contains(row_height) || *row_height > *h {
                return Err(format!("{name}: a list's row_height is 8 to 64 and at most its height"));
            }
            if usize::from(*first) >= MENU_TEXTS {
                return Err(format!("{name}: a list's first string is 0 to {}", MENU_TEXTS - 1));
            }
            data(count)?;
            if let Some(c) = color {
                colour(c)?;
            }
        }
        Widget::TextField { x, y, w, h, max_length, text_data, .. } => {
            rect(*x, *y, *w, *h)?;
            if !(1..=256).contains(max_length) {
                return Err(format!("{name}: a text field's max_length is 1 to 256"));
            }
            if text_data.is_some_and(|n| usize::from(n) >= MENU_TEXTS) {
                return Err(format!("{name}: text_data is a string of the menu, 0 to {}", MENU_TEXTS - 1));
            }
        }
        Widget::Item { x, y, item } => {
            rect(*x, *y, 16, 16)?;
            if item.is_empty() || item.len() > 128 || !item.contains(':') {
                return Err(format!("{name}: an item widget names an item as namespace:path"));
            }
        }
        Widget::Show { when, widgets } => {
            if depth >= 3 || widgets.len() > 64 {
                return Err(format!("{name}: groups nest 3 deep and hold 64 widgets"));
            }
            match when {
                When::Data { value, .. } => data(value)?,
                When::Local { var, .. } if var.is_empty() || var.len() > 32 => {
                    return Err(format!("{name}: a variable's name is 1 to 32 bytes"));
                }
                When::Local { .. } => {}
            }
            for w in widgets {
                check_widget(w, name, data_slots, windows, sprite, depth + 1)?;
            }
        }
    }
    Ok(())
}

/// the most container or data slots a mod menu type has, as many as a mod
/// container (`native_block_entity_container`) may.
pub const MAX_MENU_SLOTS: u16 = 256;

/// How a mod block's block entity is drawn in the world, from what the server
/// sends of it (its tanks and energy), with no code on the client (API block
/// 11): `data/<id>/horizon/renderer/<name>.json` is for the block `<id>:<name>`.
/// Boxes are in texels, 16 to a block, from the block's lower north-west corner
/// as block models are (`from` below `to`).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockRenderer {
    /// The block this draws, `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    /// Drawn in this order.
    pub elements: Vec<RenderElement>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub enum RenderElement {
    /// A box with one sprite on every face.
    Cube {
        from: [f32; 3],
        to: [f32; 3],
        sprite: String,
        #[serde(default)]
        tint: Option<String>,
    },
    /// What a tank holds, as a box that rises with it: the box `from` to `to`
    /// is the full tank, and the part drawn is the tank's fraction of its
    /// height from the bottom. `sprites` and `tints` say how each substance
    /// looks (`minecraft:water` is `block/water_still` by default).
    Fluid {
        tank: u16,
        from: [f32; 3],
        to: [f32; 3],
        #[serde(default)]
        sprites: std::collections::BTreeMap<String, String>,
        #[serde(default)]
        default_sprite: Option<String>,
        #[serde(default)]
        tints: std::collections::BTreeMap<String, String>,
    },
    /// FORK-31: the fluid of a whole structure (a multiblock tank), drawn inside its casing as a box that rises with how full
    /// it is. The block's data (what the mod saved with its updates, the first 64 bytes) says where: 24 bytes at `box_at` are
    /// the least and greatest corners of the casing (six little-endian whole numbers, absolute block coordinates), the byte at
    /// `draw_at` is 1 on the one block of the structure that draws it (so the others do not draw it again) and the byte at
    /// `fill_at` is how full it is, 0 to 255. The interior is the casing's box less its walls; the substance is the block's
    /// tank `tank`; `sprites`, `default_sprite` and `tints` are as for `fluid`.
    FluidVolume {
        tank: u16,
        box_at: u16,
        draw_at: u16,
        fill_at: u16,
        #[serde(default)]
        sprites: std::collections::BTreeMap<String, String>,
        #[serde(default)]
        default_sprite: Option<String>,
        #[serde(default)]
        tints: std::collections::BTreeMap<String, String>,
    },
    /// The model of another block (never placed: a mod's own part, with `item = false`), drawn in this
    /// block's place when a byte of the block entity's data (what the mod saved with its updates)
    /// is one of `values`: what the original does with the data of a block entity, as Mekanism's energy
    /// cube shows a port only where its side is configured, without a state for each combination.
    Part {
        block: String,
        /// Draw only when this byte (an index in the entity's data) is one of these; always when none.
        #[serde(default)]
        when: Option<DataTest>,
    },
    /// A box that moves and turns as code draws a part of a machine: scaled about
    /// `center` (texels, the box's own centre by default), moved up and down by a sine
    /// (`bounce`), and turned by each of `rotations` in order, all from the game's
    /// time. `fullbright` draws it as bright as can be, and `only_with_energy`
    /// only while the block holds energy (its `energy` the server sent).
    Animated {
        from: [f32; 3],
        to: [f32; 3],
        sprite: String,
        #[serde(default)]
        scale: Option<f32>,
        #[serde(default)]
        center: Option<[f32; 3]>,
        #[serde(default)]
        bounce: Option<Bounce>,
        #[serde(default)]
        rotations: Vec<Rotation>,
        #[serde(default)]
        fullbright: bool,
        #[serde(default)]
        only_with_energy: bool,
        /// Fades with the energy: as bright as the block's energy is full, over a dark inside (what the
        /// original does with the alpha of the energy cube's core).
        #[serde(default)]
        energy_fade: bool,
    },
    /// A box that turns about `axis` (`x`, `y` or `z`) through its own centre
    /// (or `pivot`, in texels), `degrees_per_tick` each game tick.
    Spin {
        from: [f32; 3],
        to: [f32; 3],
        sprite: String,
        axis: String,
        degrees_per_tick: f32,
        #[serde(default)]
        pivot: Option<[f32; 3]>,
    },
}

/// A test on a byte of a block entity's data: `data[byte]` is one of `values` (a byte past the end is 0).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DataTest {
    pub byte: u32,
    pub values: Vec<u8>,
}

impl DataTest {
    pub fn matches(&self, data: &[u8]) -> bool {
        let b = data.get(self.byte as usize).copied().unwrap_or(0);
        self.values.contains(&b)
    }
}

impl BlockRenderer {
    /// Whether the clients need the block entity's data to draw it.
    pub fn uses_data(&self) -> bool {
        self.elements
            .iter()
            .any(|e| matches!(e, RenderElement::Part { when: Some(_), .. } | RenderElement::FluidVolume { .. }))
    }
}

/// A sine of the time that moves a part up and down: `amplitude` blocks (before the
/// element's scale), at `degrees_per_tick` of the sine's angle.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bounce {
    pub amplitude: f32,
    pub degrees_per_tick: f32,
}

/// A turn about `axis` (any direction, as `x, y, z`) of `offset` degrees plus
/// `degrees_per_tick` a tick.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rotation {
    pub axis: [f32; 3],
    pub degrees_per_tick: f32,
    #[serde(default)]
    pub offset: f32,
}

impl BlockRenderer {
    fn check(&self, name: &str) -> Result<(), String> {
        let sprite = |s: &str| {
            let ok = !s.is_empty()
                && s.len() <= 128
                && !s.contains("..")
                && s.bytes().all(|b| {
                    b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || matches!(b, b'_' | b'-' | b'.' | b'/' | b':')
                });
            ok.then_some(()).ok_or_else(|| format!("{name}: sprite {s:?} is not a sprite name"))
        };
        let colour = |c: &str| {
            parse_color(c)
                .map(drop)
                .ok_or_else(|| format!("{name}: colour {c:?} is #RRGGBB or #AARRGGBB"))
        };
        let boxed = |from: &[f32; 3], to: &[f32; 3]| {
            let fine = from.iter().chain(to).all(|v| v.is_finite() && (-32.0..=48.0).contains(v));
            if !fine || (0..3).any(|i| from[i] >= to[i]) {
                Err(format!("{name}: a box is from below to, within -32 to 48 texels"))
            } else {
                Ok(())
            }
        };
        if self.elements.is_empty() || self.elements.len() > 64 {
            return Err(format!("{name}: 1 to 64 elements"));
        }
        for e in &self.elements {
            match e {
                RenderElement::Cube { from, to, sprite: s, tint } => {
                    boxed(from, to)?;
                    sprite(s)?;
                    if let Some(c) = tint {
                        colour(c)?;
                    }
                }
                RenderElement::Fluid { from, to, sprites, default_sprite, tints, .. } => {
                    boxed(from, to)?;
                    for s in sprites.values().chain(default_sprite) {
                        sprite(s)?;
                    }
                    for c in tints.values() {
                        colour(c)?;
                    }
                }
                RenderElement::FluidVolume { box_at, draw_at, fill_at, sprites, default_sprite, tints, .. } => {
                    if u32::from(*box_at) + 24 > 64 || *draw_at >= 64 || *fill_at >= 64 {
                        return Err(format!("{name}: a fluid volume reads the first 64 bytes of the data"));
                    }
                    for s in sprites.values().chain(default_sprite) {
                        sprite(s)?;
                    }
                    for c in tints.values() {
                        colour(c)?;
                    }
                }
                RenderElement::Part { block, when } => {
                    if !block.contains(':') || block.len() > 128 {
                        return Err(format!("{name}: a part is a block id, mymod:part"));
                    }
                    if let Some(w) = when
                        && (w.byte >= 4096 || w.values.is_empty() || w.values.len() > 16)
                    {
                        return Err(format!("{name}: a part's test is a byte below 4096 and 1 to 16 values"));
                    }
                }
                RenderElement::Animated { from, to, sprite: s, scale, center, bounce, rotations, .. } => {
                    boxed(from, to)?;
                    sprite(s)?;
                    if scale.is_some_and(|v| !(0.01..=4.0).contains(&v)) {
                        return Err(format!("{name}: scale is 0.01 to 4"));
                    }
                    if center.is_some_and(|c| c.iter().any(|v| !v.is_finite())) {
                        return Err(format!("{name}: center is numbers"));
                    }
                    if let Some(b) = bounce
                        && (!b.amplitude.is_finite()
                            || b.amplitude.abs() > 4.0
                            || !b.degrees_per_tick.is_finite()
                            || b.degrees_per_tick.abs() > 360.0)
                    {
                        return Err(format!("{name}: a bounce is an amplitude within 4 and degrees_per_tick within 360"));
                    }
                    if rotations.len() > 4 {
                        return Err(format!("{name}: 4 rotations at most"));
                    }
                    for r in rotations {
                        let len = r.axis.iter().map(|v| v * v).sum::<f32>();
                        if !len.is_finite()
                            || len < 1e-6
                            || !r.degrees_per_tick.is_finite()
                            || r.degrees_per_tick.abs() > 360.0
                            || !r.offset.is_finite()
                        {
                            return Err(format!("{name}: a rotation has an axis and degrees_per_tick within 360"));
                        }
                    }
                }
                RenderElement::Spin { from, to, sprite: s, axis, degrees_per_tick, pivot } => {
                    boxed(from, to)?;
                    sprite(s)?;
                    if !matches!(axis.as_str(), "x" | "y" | "z") {
                        return Err(format!("{name}: axis is x, y or z"));
                    }
                    if !degrees_per_tick.is_finite() || degrees_per_tick.abs() > 360.0 {
                        return Err(format!("{name}: degrees_per_tick is within 360"));
                    }
                    if pivot.is_some_and(|p| p.iter().any(|v| !v.is_finite())) {
                        return Err(format!("{name}: pivot is numbers"));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
enum Source {
    Dir(PathBuf),
    Zip(PathBuf),
}

/// A mod's content: its files and declarations.
#[derive(Debug)]
pub struct Content {
    source: Source,
    /// Every content file (`assets/...`, `data/...`), sorted.
    pub files: Vec<String>,
    pub blocks: Vec<Block>,
    pub items: Vec<Item>,
    pub effects: Vec<MobEffect>,
    /// Its creative inventory tabs, by id.
    pub creative_tabs: Vec<CreativeTab>,
    pub entity_types: Vec<EntityType>,
    /// Its own living entity kinds (FORK-10), by id.
    pub entity_kinds: Vec<EntityKind>,
    /// The key mappings it adds (`data/<id>/horizon/key/`).
    pub keys: Vec<KeyMapping>,
    pub menu_types: Vec<MenuType>,
    /// Its block entity renderers, by block id.
    pub renderers: Vec<BlockRenderer>,
    /// The namespaces it owns: the mod id, then the manifest's `namespaces`.
    pub namespaces: Vec<String>,
}

/// Reads a mod's content files ([`Content::reader`]).
pub enum FileReader<'a> {
    Dir(&'a Path),
    Zip(Box<zip::ZipArchive<std::fs::File>>),
}
impl FileReader<'_> {
    /// One content file's bytes.
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, String> {
        check_path(name)?;
        let reader: Box<dyn Read + '_> = match self {
            Self::Dir(root) => {
                Box::new(std::fs::File::open(root.join(name)).map_err(|e| format!("{name}: {e}"))?)
            }
            Self::Zip(zip) => Box::new(zip.by_name(name).map_err(|e| format!("{name}: {e}"))?),
        };
        let mut out = Vec::new();
        reader
            .take(MAX_FILE + 1)
            .read_to_end(&mut out)
            .map_err(|e| format!("{name}: {e}"))?;
        if out.len() as u64 > MAX_FILE {
            return Err(format!("{name} is over {} MiB", MAX_FILE >> 20));
        }
        Ok(out)
    }
}

/// Why two content mods cannot be on together: a namespace both own (one's
/// id or listed namespace is the other's), which would mix their files.
/// `a` and `b` are (mod id, owned namespaces).
pub fn clash(a: (&str, &[String]), b: (&str, &[String])) -> Option<String> {
    let shared = a.1.iter().find(|ns| b.1.contains(ns))?;
    Some(format!(
        "mods {} and {} both use the namespace {shared}; turn one of them off",
        a.0, b.0
    ))
}

impl Content {
    /// The content of the mod at `path` (a `.hmod` or a mod folder).
    pub fn read(path: &Path, manifest: &Manifest) -> Result<Content, String> {
        let (source, sized) = if path.is_dir() {
            let mut out = Vec::new();
            for top in ["assets", "data"] {
                walk(path, &path.join(top), &mut out)?;
            }
            (Source::Dir(path.to_owned()), out)
        } else {
            let mut zip = open_zip(path)?;
            let mut out = Vec::new();
            for i in 0..zip.len() {
                let entry = zip.by_index_raw(i).map_err(|e| e.to_string())?;
                let name = entry.name().map_err(|e| e.to_string())?.into_owned();
                if (name.starts_with("assets/") || name.starts_with("data/")) && !entry.is_dir() {
                    out.push((name.to_owned(), entry.size()));
                }
            }
            (Source::Zip(path.to_owned()), out)
        };
        let mut files = Vec::with_capacity(sized.len());
        let mut total = 0u64;
        for (name, size) in sized {
            check_path(&name)?;
            if size > MAX_FILE {
                return Err(format!("{name} is over {} MiB", MAX_FILE >> 20));
            }
            total += size;
            files.push(name);
        }
        if files.len() > MAX_FILES {
            return Err(format!("more than {MAX_FILES} content files"));
        }
        if total > MAX_TOTAL {
            return Err(format!("content is over {} MiB", MAX_TOTAL >> 20));
        }
        files.sort();
        files.dedup();
        let mut content = Content {
            source,
            files,
            blocks: Vec::new(),
            items: Vec::new(),
            effects: Vec::new(),
            creative_tabs: Vec::new(),
            entity_types: Vec::new(),
            entity_kinds: Vec::new(),
            keys: Vec::new(),
            menu_types: Vec::new(),
            renderers: Vec::new(),
            namespaces: manifest.owned_namespaces().map(str::to_owned).collect(),
        };
        content.check(manifest)?;
        Ok(content)
    }

    /// Where the files are read from: the mod's folder or its `.hmod`.
    pub fn source(&self) -> &Path {
        match &self.source {
            Source::Dir(path) | Source::Zip(path) => path,
        }
    }

    /// No files: a mod with code only.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// One content file's bytes.
    pub fn file(&self, name: &str) -> Result<Vec<u8>, String> {
        self.reader()?.read(name)
    }

    /// A reader of its content files. A packaged mod's archive is opened
    /// once for all of them (opening it reads its whole index).
    pub fn reader(&self) -> Result<FileReader<'_>, String> {
        Ok(match &self.source {
            Source::Dir(root) => FileReader::Dir(root),
            Source::Zip(path) => FileReader::Zip(Box::new(open_zip(path)?)),
        })
    }

    /// Where content may go, and the declarations.
    fn check(&mut self, manifest: &Manifest) -> Result<(), String> {
        let id = manifest.id.as_str();
        if !self.files.is_empty() && !manifest.capabilities.contains(&Capability::Registries) {
            return Err(format!(
                "has assets/ or data/ but not capability registries in {MANIFEST}"
            ));
        }
        let mut blocks = Vec::new();
        let mut items = Vec::new();
        let mut effects = Vec::new();
        let mut tabs = Vec::new();
        let mut types = Vec::new();
        let mut menus = Vec::new();
        let mut renderers = Vec::new();
        let mut kinds = Vec::new();
        let mut keys = Vec::new();
        for name in &self.files {
            let mut parts = name.splitn(3, '/');
            let (top, namespace, rest) = (
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
            );
            let own = namespace == id;
            let owned = own || manifest.namespaces.iter().any(|n| n == namespace);
            // Vanilla's own files stay vanilla's; a mod may only add to its
            // tags, and to the convention tags mods share.
            let shared = matches!(namespace, "minecraft" | "c");
            let allowed = owned || (top == "data" && shared && rest.starts_with("tags/"));
            if !allowed {
                return Err(format!(
                    "{name}: a mod adds files in its own namespaces ({top}/{id}/ and those listed in namespaces) or to vanilla and convention (c) tags"
                ));
            }
            if !own && top == "data" && rest.starts_with("horizon/") {
                return Err(format!(
                    "{name}: blocks, items, mob effects, entity types and menu types are declared in data/{id}/horizon/"
                ));
            }
            let Some(decl) = (own && top == "data")
                .then(|| rest.strip_prefix("horizon/"))
                .flatten()
            else {
                continue;
            };
            let parse = |kind: &str| -> Option<String> {
                let path = decl
                    .strip_prefix(kind)?
                    .strip_prefix('/')?
                    .strip_suffix(".json")?;
                Some(format!("{id}:{path}"))
            };
            if let Some(block) = parse("block") {
                blocks.push((block, name.clone()));
            } else if let Some(item) = parse("item") {
                items.push((item, name.clone()));
            } else if let Some(effect) = parse("mob_effect") {
                effects.push((effect, name.clone()));
            } else if let Some(tab) = parse("creative_tab") {
                tabs.push((tab, name.clone()));
            } else if let Some(kind) = parse("entity_type") {
                types.push((kind, name.clone()));
            } else if let Some(kind) = parse("entity_kind") {
                kinds.push((kind, name.clone()));
            } else if let Some(menu) = parse("menu") {
                menus.push((menu, name.clone()));
            } else if let Some(block) = parse("renderer") {
                renderers.push((block, name.clone()));
            } else if let Some(key) = parse("key") {
                keys.push((key, name.clone()));
            } else {
                return Err(format!(
                    "{name}: data/{id}/horizon/ holds block/, item/, mob_effect/, creative_tab/, entity_type/, entity_kind/, menu/, renderer/ and key/*.json only"
                ));
            }
        }
        let mut ids = BTreeSet::new();
        for (block, name) in blocks {
            let file: BlockFile =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            check_block(&file).map_err(|e| format!("{name}: {e}"))?;
            if file.item {
                ids.insert(block.clone());
            }
            self.blocks.push(Block {
                id: block,
                template: file.template,
                properties: file.properties,
                shapes: file.shapes,
                state_properties: file.state_properties,
                default_state: file.default_state,
                item: file.item,
                components: file.components,
                connects_to: file
                    .connects_to
                    .map(|t| t.trim_start_matches('#').to_owned()),
                thickness: file.thickness,
                redstone_emitter: file.redstone_emitter,
            });
        }
        let mut remainders = Vec::new();
        for (item, name) in items {
            let file: ItemFile =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if !vanilla_id(&file.template) {
                return Err(format!(
                    "{name}: template {:?} is not a vanilla item (minecraft:...)",
                    file.template
                ));
            }
            if file.max_stack_size.is_some_and(|n| !(1..=99).contains(&n)) {
                return Err(format!("{name}: max_stack_size is 1 to 99"));
            }
            check_components(&file.components, &["max_stack_size", "rarity"])
                .map_err(|e| format!("{name}: {e}"))?;
            if !ids.insert(item.clone()) {
                return Err(format!("{name}: {item} is also a block's item"));
            }
            if let Some(rest) = &file.crafting_remainder {
                remainders.push((name.clone(), rest.clone()));
            }
            if file.energy.is_some_and(|e| e.capacity == 0) {
                return Err(format!("{name}: energy capacity is above 0"));
            }
            self.items.push(Item {
                id: item,
                template: file.template,
                max_stack_size: file.max_stack_size,
                rarity: file.rarity,
                components: file.components,
                crafting_remainder: file.crafting_remainder,
                energy: file.energy,
            });
        }
        for (effect, name) in effects {
            let file: MobEffect =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if file.color > 0xFF_FFFF {
                return Err(format!("{name}: color is RGB, 0 to 16777215 (0xFFFFFF)"));
            }
            self.effects.push(MobEffect { id: effect, ..file });
        }
        // After every item, so a remainder may be declared in a later file.
        for (name, rest) in remainders {
            if rest == "minecraft:air" {
                return Err(format!(
                    "{name}: crafting_remainder {rest:?} is no remainder (leave it out)"
                ));
            }
            if !vanilla_id(&rest) && !ids.contains(&rest) {
                return Err(format!(
                    "{name}: crafting_remainder {rest:?} is not a vanilla item (minecraft:...) or one of this mod's"
                ));
            }
        }
        for (tab, name) in tabs {
            let file: CreativeTabFile =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            let id_like = |s: &str| {
                s.split_once(':')
                    .is_some_and(|(n, p)| !n.is_empty() && !p.is_empty())
            };
            if !id_like(&file.icon) || !file.items.iter().all(|i| id_like(i)) {
                return Err(format!(
                    "{name}: icon and items are item ids (namespace:path)"
                ));
            }
            self.creative_tabs.push(CreativeTab {
                id: tab,
                title: file.title,
                icon: file.icon,
                items: file.items,
            });
        }
        for (kind, name) in types {
            let file: EntityType =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if !ENTITY_TEMPLATES.contains(&file.template.as_str()) {
                return Err(format!(
                    "{name}: template {:?} is not one of {ENTITY_TEMPLATES:?}",
                    file.template
                ));
            }
            // its own item, so no vanilla item loses its use to it.
            if !ids.contains(&file.item) {
                return Err(format!(
                    "{name}: item {:?} is not one of this mod's items",
                    file.item
                ));
            }
            if self.entity_types.iter().any(|t| t.item == file.item) {
                return Err(format!(
                    "{name}: item {:?} already throws another entity type",
                    file.item
                ));
            }
            if !(file.damage.is_finite() && file.damage >= 0.0) {
                return Err(format!("{name}: damage is 0 or more"));
            }
            self.entity_types.push(EntityType { id: kind, ..file });
        }
        for (block, name) in renderers {
            let file: BlockRenderer =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            file.check(&name)?;
            self.renderers.push(BlockRenderer { id: block, ..file });
        }
        for (menu, name) in menus {
            let file: MenuType =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if file.slots > MAX_MENU_SLOTS || file.data_slots > MAX_MENU_SLOTS {
                return Err(format!(
                    "{name}: slots is 0 to {MAX_MENU_SLOTS}, data_slots 0 to {MAX_MENU_SLOTS}"
                ));
            }
            if file.accepts.iter().any(|a| a.slot >= file.slots || a.items.is_empty() || a.items.len() > 64) {
                return Err(format!("{name}: accepts lists slots of the menu, each with 1 to 64 items"));
            }
            let mut seen = BTreeSet::new();
            for &r in &file.results {
                if r >= file.slots {
                    return Err(format!(
                        "{name}: result {r} is past the last slot, {}",
                        file.slots - 1
                    ));
                }
                if !seen.insert(r) {
                    return Err(format!("{name}: result {r} is listed twice"));
                }
            }
            if let Some(a) = &file.armor {
                let far = |v: i32| !(-1024..=1024).contains(&v);
                if far(a.x) || far(a.y) || a.offhand.is_some_and(far) {
                    return Err(format!("{name}: the armor slots are within 1024 pixels"));
                }
                if usize::from(file.slots) + a.count() > usize::from(MAX_MENU_SLOTS) {
                    return Err(format!("{name}: the armor slots do not fit the {MAX_MENU_SLOTS} slots"));
                }
            }
            if let Some(screen) = &file.screen {
                screen.check(&name, file.slots, file.data_slots)?;
            }
            self.menu_types.push(MenuType { id: menu, ..file });
        }
        for (kind, name) in kinds {
            let file: EntityKind =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            file.check(&name)?;
            if let Some(menu) = &file.menu {
                let Some(m) = self.menu_types.iter().find(|m| &m.id == menu) else {
                    return Err(format!("{name}: menu {menu} is not a menu type of this mod"));
                };
                if m.slots < file.container_slots {
                    return Err(format!(
                        "{name}: menu {menu} has {} slots, fewer than the {} of the container",
                        m.slots, file.container_slots
                    ));
                }
            }
            self.entity_kinds.push(EntityKind { id: kind, ..file });
        }
        if keys.len() > MAX_KEYS {
            return Err(format!("data/{id}/horizon/key/: {MAX_KEYS} keys at most"));
        }
        for (key, name) in keys {
            let file: KeyMapping = serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            file.check(&name)?;
            self.keys.push(KeyMapping { id: key, ..file });
        }
        Ok(())
    }
}

/// The most keys a mod declares.
pub const MAX_KEYS: usize = 32;

/// A key mapping a mod adds (`data/<mod>/horizon/key/<name>.json`, `<mod>:<name>`): it shows in Controls under its category and the
/// player binds it as vanilla's; a press while playing reaches the server mod's `key-pressed` (Mekanism's "Open Module Tweaker").
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KeyMapping {
    #[serde(skip)]
    pub id: String,
    /// What Controls shows: a translation key of the mod's lang files, or the text itself.
    pub label: String,
    /// The heading it is under in Controls, a translation key or text; its own after vanilla's.
    pub category: String,
    /// The default key as options.txt writes it (`key.keyboard.o`, `key.mouse.4`); empty for none (`key.keyboard.unknown`).
    #[serde(default)]
    pub key: String,
}

impl KeyMapping {
    fn check(&self, name: &str) -> Result<(), String> {
        let text = |t: &str| !t.is_empty() && t.len() <= 128;
        if !text(&self.label) || !text(&self.category) {
            return Err(format!("{name}: label and category are 1 to 128 bytes"));
        }
        if !self.key.is_empty() && !(self.key.starts_with("key.keyboard.") || self.key.starts_with("key.mouse.")) {
            return Err(format!("{name}: key is empty, key.keyboard.<name> or key.mouse.<name>"));
        }
        Ok(())
    }
    /// The default key, `key.keyboard.unknown` for none.
    pub fn default_key(&self) -> &str {
        if self.key.is_empty() { "key.keyboard.unknown" } else { &self.key }
    }
}

/// Namespaced component types, removals as `{}`, and none of `fields`,
/// which the file sets with its own keys.
fn check_components(
    components: &BTreeMap<String, serde_json::Value>,
    fields: &[&str],
) -> Result<(), String> {
    for (key, value) in components {
        let (removed, kind) = match key.strip_prefix('!') {
            Some(kind) => (true, kind),
            None => (false, key.as_str()),
        };
        if !namespaced_id(kind) {
            return Err(format!("component {key:?} must be namespaced and valid"));
        }
        if let Some(own) = kind
            .strip_prefix("minecraft:")
            .filter(|k| fields.contains(k))
        {
            return Err(format!("component {key:?}: set {own} instead"));
        }
        if removed && !value.as_object().is_some_and(|o| o.is_empty()) {
            return Err(format!("component {key:?}: a removal's value is {{}}"));
        }
    }
    Ok(())
}

fn check_block(file: &BlockFile) -> Result<(), String> {
    if !vanilla_id(&file.template) {
        return Err(format!(
            "template {:?} is not a vanilla block (minecraft:...)",
            file.template
        ));
    }
    if let Some(shapes) = &file.shapes {
        if shapes.property.is_empty() || shapes.values.is_empty() {
            return Err("shapes needs a property and values".into());
        }
        for boxes in shapes.values.values() {
            if boxes.len() > 16
                || boxes
                    .iter()
                    .any(|b| b.iter().any(|n| !n.is_finite()) || (0..3).any(|i| b[i] > b[i + 3]))
            {
                return Err("shape needs at most 16 finite boxes with ordered bounds".into());
            }
        }
    }
    for (name, property) in &file.state_properties {
        if !state_name(name) {
            return Err(format!(
                "state property {name:?} is lower case letters, digits and underscores"
            ));
        }
        let values = property.values();
        if values.is_empty() {
            return Err(format!("state property {name:?} needs a value"));
        }
        let mut seen = BTreeSet::new();
        for value in values {
            if !state_name(value) {
                return Err(format!(
                    "state property {name:?}: value {value:?} is lower case letters, digits and underscores"
                ));
            }
            if !seen.insert(value) {
                return Err(format!("state property {name:?}: value {value:?} repeats"));
            }
        }
    }
    for key in file.default_state.keys() {
        if !state_name(key) {
            return Err(format!(
                "default_state key {key:?} is lower case letters, digits and underscores"
            ));
        }
    }
    let p = &file.properties;
    let finite = |v: Option<f32>| v.is_none_or(f32::is_finite);
    if !finite(p.hardness) || !finite(p.explosion_resistance) || !finite(p.friction) {
        return Err("hardness, explosion_resistance and friction are numbers".into());
    }
    if p.hardness.is_some_and(|h| h < 0.0 && h != -1.0) {
        return Err("hardness is 0 or more, or -1 (unbreakable)".into());
    }
    if p.explosion_resistance.is_some_and(|r| r < 0.0) {
        return Err("explosion_resistance is 0 or more".into());
    }
    if p.friction.is_some_and(|f| !(0.0..=2.0).contains(&f)) {
        return Err("friction is 0 to 2".into());
    }
    if p.light.is_some_and(|l| l > 15) {
        return Err("light is 0 to 15".into());
    }
    if p.map_color.is_some_and(|c| c > 61) {
        return Err("map_color is a MapColor id, 0 to 61".into());
    }
    if p.sounds.as_deref().is_some_and(|s| !vanilla_id(s)) {
        return Err("sounds names a vanilla block (minecraft:...)".into());
    }
    if !file.item && !file.components.is_empty() {
        return Err("item is false, so the block has no item to take components".into());
    }
    check_components(&file.components, &[])?;
    // A block file has no `max_stack_size` / `rarity` keys, so those two take the same checks here.
    if let Some(n) = file.components.get("minecraft:max_stack_size")
        && !n.as_u64().is_some_and(|n| (1..=99).contains(&n))
    {
        return Err("component \"minecraft:max_stack_size\" is an integer 1 to 99".into());
    }
    if let Some(r) = file.components.get("minecraft:rarity")
        && serde_json::from_value::<Rarity>(r.clone()).is_err()
    {
        return Err("component \"minecraft:rarity\" is common, uncommon, rare or epic".into());
    }
    if let Some(tag) = &file.connects_to {
        if file.template != "minecraft:chorus_plant" {
            return Err("connects_to needs template minecraft:chorus_plant (a PipeBlock)".into());
        }
        let id = tag.strip_prefix('#').unwrap_or_default();
        let valid = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-./".contains(c))
        };
        if !id
            .split_once(':')
            .is_some_and(|(ns, path)| valid(ns) && valid(path))
        {
            return Err("connects_to is a block tag, #namespace:path".into());
        }
    }
    match file.thickness {
        Some(_) if file.connects_to.is_none() => {
            return Err("thickness is a connecting block's (connects_to)".into());
        }
        Some(t) if !(2.0..=14.0).contains(&t) => return Err("thickness is 2 to 14 pixels".into()),
        Some(_) if file.shapes.is_some() => {
            return Err("thickness sets the shapes, so a block has one or the other".into());
        }
        _ => {}
    }
    Ok(())
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    zip::ZipArchive::new(file).map_err(|e| e.to_string())
}

/// Files under `dir`, relative to `root` with `/` separators. Symbolic links
/// are not followed: a mod folder holds its own files.
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) -> Result<(), String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in read.flatten() {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let parts: Option<Vec<&str>> = rel.iter().map(|p| p.to_str()).collect();
            let name = parts.ok_or_else(|| format!("{} is not UTF-8", rel.display()))?;
            out.push((name.join("/"), meta.len()));
        }
        if out.len() > MAX_FILES {
            return Err(format!("more than {MAX_FILES} content files"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_part_is_drawn_by_a_byte_of_the_data() {
        let w = DataTest { byte: 2, values: vec![2, 3] };
        assert!(w.matches(&[0, 0, 2]));
        assert!(w.matches(&[0, 0, 3, 9]));
        assert!(!w.matches(&[0, 0, 1]));
        assert!(!w.matches(&[0]));
        assert!(DataTest { byte: 5, values: vec![0] }.matches(&[1]));
    }

    use super::*;
    use crate::manifest::Package;

    const MANIFEST_TEXT: &str = r#"
id = "ruby"
name = "Ruby"
version = "0.1.0"
api = "0.2"
side = "both"
capabilities = ["registries"]
"#;
    const BLOCK: &[u8] = br#"{"template": "minecraft:iron_block",
        "properties": {"hardness": 4.0, "light": 3, "sounds": "minecraft:amethyst_block"}}"#;
    const ITEM: &[u8] = br#"{"template": "minecraft:diamond", "rarity": "uncommon"}"#;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-content-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn hmod(name: &str, manifest: &str, files: &[(&str, &[u8])]) -> Result<Content, String> {
        let dir = scratch(name);
        let path = dir.join("m.hmod");
        std::fs::write(&path, Package::write_files(manifest, None, files)?).unwrap();
        let package = Package::read(&path);
        let content = package.and_then(|p| Content::read(&path, &p.manifest));
        std::fs::remove_dir_all(&dir).unwrap();
        content
    }

    #[test]
    fn content_only_mod_round_trips() {
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/horizon/block/ruby_block.json", BLOCK),
            ("data/ruby/horizon/item/ruby.json", ITEM),
            ("data/ruby/recipe/ruby_block.json", b"{}"),
            ("data/minecraft/tags/block/mineable/pickaxe.json", b"{}"),
            ("data/c/tags/item/gems/ruby.json", b"{}"),
            ("assets/ruby/textures/block/ruby_block.png", b"png"),
        ];
        let content = hmod("ok", MANIFEST_TEXT, files).unwrap();
        assert_eq!(content.files.len(), 6);
        assert_eq!(
            content.blocks,
            [Block {
                id: "ruby:ruby_block".into(),
                template: "minecraft:iron_block".into(),
                shapes: None,
                state_properties: BTreeMap::new(),
                default_state: BTreeMap::new(),
                properties: BlockProperties {
                    hardness: Some(4.0),
                    light: Some(3),
                    sounds: Some("minecraft:amethyst_block".into()),
                    ..BlockProperties::default()
                },
                item: true,
                components: BTreeMap::new(),
                connects_to: None,
                thickness: None,
                redstone_emitter: false,
            }]
        );
        // A block that gives redstone (round 4, item 4) says so.
        let emitter = br#"{"template": "minecraft:iron_block", "redstone_emitter": true}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/block/laser_amplifier.json", emitter)];
        assert!(hmod("emitter", MANIFEST_TEXT, files).unwrap().blocks[0].redstone_emitter);
        assert_eq!(content.items[0].id, "ruby:ruby");
        assert_eq!(content.items[0].rarity, Some(Rarity::Uncommon));
        assert!(content.items[0].components.is_empty());
        assert_eq!(content.items[0].crafting_remainder, None);
        assert_eq!(content.items[0].energy, None);
        // An item that holds energy says how much and how fast (round 4, item 5); the rates default to no limit.
        let battery = br#"{"template": "minecraft:stick", "energy": {"capacity": 1000, "max_insert": 100}}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/item/battery.json", battery)];
        let spec = hmod("battery", MANIFEST_TEXT, files).unwrap().items[0].energy;
        assert_eq!(spec, Some(crate::item_energy::EnergySpec { capacity: 1000, max_insert: 100, max_extract: u64::MAX }));
        let empty = br#"{"template": "minecraft:stick", "energy": {"capacity": 0}}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/item/battery.json", empty)];
        assert!(hmod("battery-empty", MANIFEST_TEXT, files).unwrap_err().contains("energy capacity is above 0"));
    }

    #[test]
    fn items_name_their_crafting_remainder() {
        let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/horizon/item/a_stew.json",
                br#"{"template": "minecraft:mushroom_stew", "crafting_remainder": "minecraft:bowl"}"#,
            ),
            (
                "data/ruby/horizon/item/b_flask.json",
                br#"{"template": "minecraft:potion", "crafting_remainder": "ruby:c_flask"}"#,
            ),
            ("data/ruby/horizon/item/c_flask.json", ITEM),
        ];
        let content = hmod("remainder", MANIFEST_TEXT, files).unwrap();
        let rests: Vec<_> = content
            .items
            .iter()
            .map(|i| i.crafting_remainder.as_deref())
            .collect();
        assert_eq!(rests, [Some("minecraft:bowl"), Some("ruby:c_flask"), None]);
    }

    #[test]
    fn items_patch_their_template_components() {
        let sword = br#"{"template": "minecraft:iron_sword", "components": {
            "minecraft:max_damage": 900,
            "minecraft:attribute_modifiers": [{"type": "minecraft:attack_damage",
                "id": "minecraft:base_attack_damage", "amount": 7.0,
                "operation": "add_value", "slot": "mainhand"}],
            "!minecraft:repairable": {}}}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/item/ruby_sword.json", sword)];
        let content = hmod("components", MANIFEST_TEXT, files).unwrap();
        let components = &content.items[0].components;
        assert_eq!(
            components.keys().collect::<Vec<_>>(),
            [
                "!minecraft:repairable",
                "minecraft:attribute_modifiers",
                "minecraft:max_damage"
            ]
        );
        assert_eq!(components["minecraft:max_damage"], 900);
    }

    #[test]
    fn armour_and_mod_components_are_data_too() {
        // 26.3 armour is data: a worn slot, protection as attribute modifiers, a
        // durability, and a component of the mod's own beside them (API block 13).
        let helmet = br#"{"template": "minecraft:iron_helmet", "components": {
            "minecraft:max_damage": 400,
            "minecraft:equippable": {"slot": "head", "equip_sound": "minecraft:item.armor.equip_iron"},
            "minecraft:attribute_modifiers": [
                {"type": "minecraft:armor", "id": "ruby:helmet_armor", "amount": 3.0,
                 "operation": "add_value", "slot": "head"},
                {"type": "minecraft:armor_toughness", "id": "ruby:helmet_toughness", "amount": 1.0,
                 "operation": "add_value", "slot": "head"}],
            "ruby:energy": 0}}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/item/ruby_helmet.json", helmet)];
        let content = hmod("armour", MANIFEST_TEXT, files).unwrap();
        let components = &content.items[0].components;
        assert!(components.contains_key("minecraft:equippable"));
        assert_eq!(components["minecraft:max_damage"], 400);
    }

    #[test]
    fn mob_effects_take_a_category_and_a_colour() {
        let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/horizon/mob_effect/glow.json",
                br#"{"category": "beneficial", "color": 16711680}"#,
            ),
            (
                "data/ruby/horizon/mob_effect/curse/dark.json",
                br#"{"category": "harmful", "color": 0}"#,
            ),
        ];
        let content = hmod("effects", MANIFEST_TEXT, files).unwrap();
        let effects: Vec<_> = content
            .effects
            .iter()
            .map(|e| (e.id.as_str(), e.category, e.color))
            .collect();
        assert_eq!(
            effects,
            [
                ("ruby:curse/dark", EffectCategory::Harmful, 0),
                ("ruby:glow", EffectCategory::Beneficial, 0xFF0000),
            ]
        );
        for (case, file, why) in [
            (
                "effect-colour",
                r#"{"category": "neutral", "color": 16777216}"#,
                "color is RGB",
            ),
            (
                "effect-category",
                r#"{"category": "good", "color": 1}"#,
                "unknown variant",
            ),
            (
                "effect-key",
                r#"{"category": "neutral", "color": 1, "id": "x"}"#,
                "unknown field",
            ),
            (
                "effect-missing",
                r#"{"category": "neutral"}"#,
                "missing field",
            ),
        ] {
            let files: &[(&str, &[u8])] =
                &[("data/ruby/horizon/mob_effect/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn menu_types_take_slots_data_slots_and_results() {
        let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/horizon/menu/press.json",
                br#"{"slots": 9, "data_slots": 2, "results": [7, 8]}"#,
            ),
            ("data/ruby/horizon/menu/bag.json", br#"{"slots": 4}"#),
        ];
        let content = hmod("menus", MANIFEST_TEXT, files).unwrap();
        let menus: Vec<_> = content
            .menu_types
            .iter()
            .map(|m| (m.id.as_str(), m.slots, m.data_slots, m.results.clone()))
            .collect();
        assert_eq!(
            menus,
            [("ruby:bag", 4, 0, vec![]), ("ruby:press", 9, 2, vec![7, 8])]
        );
        for (case, file, why) in [
            ("menu-large", r#"{"slots": 257}"#, "slots is 0 to 256"),
            (
                "menu-data",
                r#"{"slots": 1, "data_slots": 257}"#,
                "data_slots 0 to 256",
            ),
            (
                "menu-result",
                r#"{"slots": 3, "results": [3]}"#,
                "result 3 is past the last slot, 2",
            ),
            (
                "menu-twice",
                r#"{"slots": 3, "results": [1, 1]}"#,
                "result 1 is listed twice",
            ),
            ("menu-key", r#"{"slots": 3, "rows": 1}"#, "unknown field"),
            ("menu-missing", r#"{"data_slots": 1}"#, "missing field"),
        ] {
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/menu/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn an_entity_kind_is_declared_in_content_and_bad_ones_are_refused() {
        let good = br##"{"width": 0.6, "height": 0.65, "max_health": 1, "speed": 0.3, "container_slots": 30,
            "skins": [{"id": "default", "texture": "ruby:entity/robit"}, {"id": "gold", "texture": "ruby:entity/gold", "model": "ruby:robit_gold"}]}"##;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/entity_kind/robit.json", good)];
        let content = hmod("kind-ok", MANIFEST_TEXT, files).unwrap();
        let k = &content.entity_kinds[0];
        assert_eq!((k.id.as_str(), k.container_slots, k.skins.len(), k.persistent), ("ruby:robit", 30, 2, true));
        for (case, file, why) in [
            ("kind-size", r#"{"width":0,"height":1,"skins":[{"id":"a","texture":"ruby:a"}]}"#, "width and height are 0.1 to 16"),
            ("kind-health", r#"{"width":1,"height":1,"max_health":0,"skins":[{"id":"a","texture":"ruby:a"}]}"#, "max_health is 0.5 to 1024"),
            ("kind-skins", r#"{"width":1,"height":1,"skins":[]}"#, "1 to 64 skins"),
            ("kind-twice", r#"{"width":1,"height":1,"skins":[{"id":"a","texture":"ruby:a"},{"id":"a","texture":"ruby:b"}]}"#, "not repeated"),
            ("kind-texture", r#"{"width":1,"height":1,"skins":[{"id":"a","texture":"nope"}]}"#, "<mod id>:<path>"),
            ("kind-slots", r#"{"width":1,"height":1,"container_slots":300,"skins":[{"id":"a","texture":"ruby:a"}]}"#, "256 at most"),
            ("kind-key", r#"{"width":1,"height":1,"legs":4,"skins":[{"id":"a","texture":"ruby:a"}]}"#, "unknown field"),
        ] {
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/entity_kind/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn an_entity_kind_has_bones_that_swing_and_skins_that_change_them() {
        let good = br#"{"width":0.6,"height":0.65,"skins":[{"id":"a","texture":"ruby:a"},{"id":"gold","texture":"ruby:g","bones":{"leg_left":"ruby:gold_leg"}}],
            "bones":[{"id":"body","model":"ruby:body","pivot":[0,0.4,0]},
                     {"id":"leg_left","model":"ruby:leg","pivot":[0.15,0.15,0],"walk":{"axis":"x","degrees":40}},
                     {"id":"leg_right","model":"ruby:leg","pivot":[-0.15,0.15,0],"walk":{"axis":"x","degrees":40,"phase":180}}]}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/entity_kind/robit.json", good)];
        let k = &hmod("bones-ok", MANIFEST_TEXT, files).unwrap().entity_kinds[0];
        assert_eq!(k.bones.len(), 3);
        assert_eq!(k.bones[2].walk.as_ref().map(|w| (w.axis, w.degrees, w.phase)), Some((BoneAxis::X, 40.0, 180.0)));
        assert_eq!(k.skins[1].bones.get("leg_left").map(String::as_str), Some("ruby:gold_leg"));
        let skin = r#""skins":[{"id":"a","texture":"ruby:a"}]"#;
        for (case, bones, why) in [
            ("bones-twice", r#"[{"id":"b","model":"ruby:b","pivot":[0,0,0]},{"id":"b","model":"ruby:c","pivot":[0,0,0]}]"#, "not repeated"),
            ("bones-model", r#"[{"id":"b","model":"b","pivot":[0,0,0]}]"#, "the model is <mod id>:<path>"),
            ("bones-far", r#"[{"id":"b","model":"ruby:b","pivot":[0,20,0]}]"#, "within 16 blocks"),
            ("bones-swing", r#"[{"id":"b","model":"ruby:b","pivot":[0,0,0],"walk":{"axis":"x","degrees":200}}]"#, "0 to 180"),
            ("bones-axis", r#"[{"id":"b","model":"ruby:b","pivot":[0,0,0],"walk":{"axis":"w","degrees":20}}]"#, "unknown variant"),
        ] {
            let file = format!(r#"{{"width":1,"height":1,{skin},"bones":{bones}}}"#);
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/entity_kind/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
        let file = br#"{"width":1,"height":1,"skins":[{"id":"a","texture":"ruby:a","bones":{"tail":"ruby:t"}}],"bones":[{"id":"b","model":"ruby:b","pivot":[0,0,0]}]}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/entity_kind/a.json", file)];
        assert!(hmod("bones-skin", MANIFEST_TEXT, files).unwrap_err().contains("bone tail is not a bone of the kind"));
    }

    #[test]
    fn an_entity_kind_names_a_menu_of_its_mod_with_room_for_its_container() {
        let menu = br#"{"slots": 40, "data_slots": 2, "results": [39]}"#;
        let kind = br#"{"width":1,"height":1,"container_slots":30,"menu":"ruby:robit","skins":[{"id":"a","texture":"ruby:a"}]}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/menu/robit.json", menu), ("data/ruby/horizon/entity_kind/robit.json", kind)];
        let content = hmod("kind-menu", MANIFEST_TEXT, files).unwrap();
        assert_eq!(content.entity_kinds[0].menu.as_deref(), Some("ruby:robit"));
        let small = br#"{"slots": 9}"#;
        for (case, menu_file, why) in [
            ("kind-menu-missing", None, "menu ruby:robit is not a menu type of this mod"),
            ("kind-menu-small", Some(&small[..]), "has 9 slots, fewer than the 30 of the container"),
        ] {
            let mut files: Vec<(&str, &[u8])> = vec![("data/ruby/horizon/entity_kind/robit.json", kind)];
            if let Some(m) = menu_file {
                files.push(("data/ruby/horizon/menu/robit.json", m));
            }
            let e = hmod(case, MANIFEST_TEXT, &files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn a_mod_declares_keys_with_a_label_a_category_and_a_default() {
        let tweaker = br#"{"label": "key.mekanism.module_tweaker", "category": "key.categories.mekanism", "key": "key.keyboard.backslash"}"#;
        let none = br#"{"label": "Boost", "category": "Mekanism"}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/key/module_tweaker.json", tweaker), ("data/ruby/horizon/key/boost.json", none)];
        let keys = hmod("keys", MANIFEST_TEXT, files).unwrap().keys;
        let mut found: Vec<(String, String)> = keys.iter().map(|k| (k.id.clone(), k.default_key().to_owned())).collect();
        found.sort();
        assert_eq!(found, [("ruby:boost".into(), "key.keyboard.unknown".into()), ("ruby:module_tweaker".into(), "key.keyboard.backslash".into())]);
        for (case, file, why) in [
            ("key-label", r#"{"label": "", "category": "x"}"#, "label and category are 1 to 128 bytes"),
            ("key-name", r#"{"label": "a", "category": "x", "key": "o"}"#, "key is empty, key.keyboard.<name> or key.mouse.<name>"),
            ("key-field", r#"{"label": "a", "category": "x", "modifier": "shift"}"#, "unknown field"),
        ] {
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/key/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn a_menu_of_buttons_alone_has_no_slots_and_hides_the_inventory() {
        let file = br#"{"slots": 0, "data_slots": 2, "screen": {"width": 176, "height": 120, "hide_inventory": true, "slots": [],
            "widgets": [{"type": "button", "x": 8, "y": 20, "w": 50, "h": 14, "id": 1}]}}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/menu/teleporter.json", file)];
        let content = hmod("menu-none", MANIFEST_TEXT, files).unwrap();
        let m = &content.menu_types[0];
        assert_eq!((m.slots, m.data_slots), (0, 2));
        assert!(m.screen.as_ref().is_some_and(|s| s.hide_inventory && s.height == 120));
        // A menu of slots keeps the inventory unless it says so.
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/menu/bag.json", br#"{"slots": 1, "screen": {"slots": [[8, 8]]}}"#)];
        assert!(!hmod("menu-inventory", MANIFEST_TEXT, files).unwrap().menu_types[0].screen.as_ref().unwrap().hide_inventory);
    }

    #[test]
    fn a_screen_takes_lists_and_text_fields_and_refuses_bad_ones() {
        let good = br##"{"slots": 1, "data_slots": 2, "screen": {"slots": [null], "widgets": [
            {"type": "list", "x": 8, "y": 20, "w": 100, "h": 48, "row_height": 12, "first": 8, "count": {"data": 1}, "on_click": 7700, "source": "items"},
            {"type": "textfield", "x": 8, "y": 6, "w": 80, "h": 12, "id": 7800, "validator": "id", "max_length": 64, "text_data": 0},
            {"type": "text", "x": 8, "y": 70, "text_data": 3}]}}"##;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/menu/filters.json", good)];
        let content = hmod("list-ok", MANIFEST_TEXT, files).unwrap();
        assert_eq!(content.menu_types[0].screen.as_ref().unwrap().widgets.len(), 3);
        for (case, file, why) in [
            (
                "list-row",
                r#"{"slots":1,"data_slots":1,"screen":{"slots":[null],"widgets":[{"type":"list","x":0,"y":0,"w":50,"h":30,"row_height":4,"first":0,"count":{"data":0},"on_click":1}]}}"#,
                "row_height is 8 to 64",
            ),
            (
                "list-first",
                r#"{"slots":1,"data_slots":1,"screen":{"slots":[null],"widgets":[{"type":"list","x":0,"y":0,"w":50,"h":30,"first":64,"count":{"data":0},"on_click":1}]}}"#,
                "first string is 0 to 63",
            ),
            (
                "list-count",
                r#"{"slots":1,"data_slots":1,"screen":{"slots":[null],"widgets":[{"type":"list","x":0,"y":0,"w":50,"h":30,"first":0,"count":{"data":5},"on_click":1}]}}"#,
                "data slot 5 is past the last one",
            ),
            (
                "graph-series",
                r#"{"slots":1,"screen":{"slots":[null],"widgets":[{"type":"graph","x":0,"y":0,"w":50,"h":30,"series":[]}]}}"#,
                "1 to 8 series",
            ),
            (
                "graph-samples",
                r##"{"slots":1,"data_slots":1,"screen":{"slots":[null],"widgets":[{"type":"graph","x":0,"y":0,"w":50,"h":30,"samples":1,"series":[{"value":{"data":0},"color":"#FF0000"}]}]}}"##,
                "2 to 400 samples",
            ),
            (
                "graph-scale",
                r##"{"slots":1,"data_slots":1,"screen":{"slots":[null],"widgets":[{"type":"graph","x":0,"y":0,"w":50,"h":30,"min_scale":0,"series":[{"value":{"data":0},"color":"#FF0000"}]}]}}"##,
                "min_scale is at least 1",
            ),
            (
                "graph-slot",
                r##"{"slots":1,"data_slots":1,"screen":{"slots":[null],"widgets":[{"type":"graph","x":0,"y":0,"w":50,"h":30,"series":[{"value":{"data":4},"color":"#FF0000"}]}]}}"##,
                "data slot 4 is past the last one",
            ),
            (
                "graph-color",
                r##"{"slots":1,"data_slots":1,"screen":{"slots":[null],"widgets":[{"type":"graph","x":0,"y":0,"w":50,"h":30,"series":[{"value":{"data":0},"color":"red"}]}]}}"##,
                "is #RRGGBB or #AARRGGBB",
            ),
            (
                "field-length",
                r#"{"slots":1,"screen":{"slots":[null],"widgets":[{"type":"textfield","x":0,"y":0,"w":50,"h":12,"id":1,"max_length":0}]}}"#,
                "max_length is 1 to 256",
            ),
            (
                "field-validator",
                r#"{"slots":1,"screen":{"slots":[null],"widgets":[{"type":"textfield","x":0,"y":0,"w":50,"h":12,"id":1,"validator":"emoji"}]}}"#,
                "unknown variant",
            ),
        ] {
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/menu/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn a_menu_screen_places_slots_and_widgets() {
        let screen = br##"{
            "slots": 2, "data_slots": 3,
            "screen": {
                "height": 180, "texture": "ruby:gui/press",
                "slots": [[56, 17], null],
                "widgets": [
                    {"type": "bar", "x": 8, "y": 16, "w": 6, "h": 54, "value": {"pair": [0, 1]},
                     "max": {"const": 100000}, "color": "#AA2020", "direction": "up",
                     "tooltip": "Energy: {value} / {max} ({percent}%)"},
                    {"type": "sprite", "texture": "ruby:gui/arrow", "x": 79, "y": 34, "w": 24, "h": 17,
                     "value": {"data": 2}, "max": {"const": 200}, "direction": "right"},
                    {"type": "text", "x": 8, "y": 6, "text": "Progress {2}"},
                    {"type": "button", "x": 150, "y": 6, "w": 18, "h": 18, "id": 1, "tooltip": "Mode"}
                ]
            }
        }"##;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/menu/press.json", screen)];
        let content = hmod("screen", MANIFEST_TEXT, files).unwrap();
        let s = content.menu_types[0].screen.as_ref().unwrap();
        assert_eq!((s.width, s.height, s.inventory_y), (176, 180, 84));
        assert_eq!(s.slots, [Some([56, 17]), None]);
        assert_eq!(s.widgets.len(), 4);
        assert_eq!(parse_color("#AA2020"), Some(0xFFAA_2020));
        assert_eq!(parse_color("#80AA2020"), Some(0x80AA_2020));
        assert_eq!(parse_color("AA2020"), None);
        for (case, file, why) in [
            ("scr-slots", r#"{"slots":1,"screen":{"slots":[null,null]}}"#, "lists 2 slots"),
            ("scr-size", r#"{"slots":1,"screen":{"width":4,"slots":[]}}"#, "16 to 1024"),
            ("scr-sprite", r#"{"slots":1,"screen":{"texture":"nope","slots":[]}}"#, "is not <mod id>:<path>"),
            (
                "scr-data",
                r##"{"slots":1,"data_slots":1,"screen":{"slots":[],"widgets":[{"type":"bar","x":0,"y":0,"w":4,"h":4,"value":{"data":1},"max":{"const":1},"color":"#FFFFFF"}]}}"##,
                "data slot 1 is past the last one",
            ),
            (
                "scr-color",
                r##"{"slots":1,"screen":{"slots":[],"widgets":[{"type":"bar","x":0,"y":0,"w":4,"h":4,"value":{"const":1},"max":{"const":1},"color":"red"}]}}"##,
                "is #RRGGBB or #AARRGGBB",
            ),
            (
                "scr-text",
                r#"{"slots":1,"data_slots":1,"screen":{"slots":[],"widgets":[{"type":"text","x":0,"y":0,"text":"{3}"}]}}"#,
                "text names data slot 3",
            ),
            (
                "scr-pair",
                r#"{"slots":1,"screen":{"slots":[],"widgets":[{"type":"sprite","texture":"ruby:a","x":0,"y":0,"w":4,"h":4,"value":{"const":1}}]}}"#,
                "value and max together",
            ),
            (
                "scr-widget",
                r#"{"slots":1,"screen":{"slots":[],"widgets":[{"type":"star"}]}}"#,
                "unknown variant",
            ),
        ] {
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/menu/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn a_block_renderer_lists_cubes_fluids_and_spins() {
        let file = br##"{"elements": [
            {"type": "cube", "from": [0, 0, 0], "to": [16, 2, 16], "sprite": "ruby:block/base", "tint": "#FFAA00"},
            {"type": "fluid", "tank": 0, "from": [2, 2, 2], "to": [14, 14, 14],
             "sprites": {"ruby:steam": "ruby:block/steam"}, "default_sprite": "ruby:block/gas",
             "tints": {"ruby:steam": "#CCCCCC"}},
            {"type": "spin", "from": [7, 2, 2], "to": [9, 14, 14], "sprite": "block/iron_block",
             "axis": "x", "degrees_per_tick": 6.0}
        ]}"##;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/renderer/press.json", file)];
        let content = hmod("renderer", MANIFEST_TEXT, files).unwrap();
        let r = &content.renderers[0];
        assert_eq!((r.id.as_str(), r.elements.len()), ("ruby:press", 3));
        for (case, file, why) in [
            ("r-empty", r#"{"elements":[]}"#, "1 to 64 elements"),
            ("r-box", r#"{"elements":[{"type":"cube","from":[4,0,0],"to":[4,1,1],"sprite":"a:b"}]}"#, "from below to"),
            ("r-sprite", r#"{"elements":[{"type":"cube","from":[0,0,0],"to":[1,1,1],"sprite":"A B"}]}"#, "is not a sprite name"),
            ("r-axis", r#"{"elements":[{"type":"spin","from":[0,0,0],"to":[1,1,1],"sprite":"a:b","axis":"w","degrees_per_tick":1}]}"#, "axis is x, y or z"),
            ("r-speed", r#"{"elements":[{"type":"spin","from":[0,0,0],"to":[1,1,1],"sprite":"a:b","axis":"x","degrees_per_tick":900}]}"#, "within 360"),
            ("r-tint", r#"{"elements":[{"type":"cube","from":[0,0,0],"to":[1,1,1],"sprite":"a:b","tint":"red"}]}"#, "is #RRGGBB"),
            ("r-kind", r#"{"elements":[{"type":"ring"}]}"#, "unknown variant"),
        ] {
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/renderer/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn a_block_renderer_can_draw_the_fluid_of_a_whole_structure() {
        let file = br##"{"elements": [
            {"type": "fluid_volume", "tank": 0, "box_at": 20, "draw_at": 44, "fill_at": 45,
             "sprites": {"ruby:steam": "ruby:block/steam"}, "default_sprite": "ruby:block/gas",
             "tints": {"ruby:steam": "#CCCCCC"}}
        ]}"##;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/renderer/tank.json", file)];
        let content = hmod("volume", MANIFEST_TEXT, files).unwrap();
        // The clients are sent the data of a block that draws by it.
        assert!(content.renderers[0].uses_data());
        for (case, file, why) in [
            ("v-box", r#"{"elements":[{"type":"fluid_volume","tank":0,"box_at":41,"draw_at":0,"fill_at":1}]}"#, "first 64 bytes"),
            ("v-fill", r#"{"elements":[{"type":"fluid_volume","tank":0,"box_at":0,"draw_at":0,"fill_at":64}]}"#, "first 64 bytes"),
        ] {
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/renderer/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn entity_types_copy_a_throwable_and_name_its_item() {
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/horizon/item/gem.json", ITEM),
            (
                "data/ruby/horizon/entity_type/bolt.json",
                br#"{"template": "minecraft:snowball", "item": "ruby:gem", "damage": 2.5}"#,
            ),
            ("data/ruby/horizon/item/dust.json", ITEM),
            (
                "data/ruby/horizon/entity_type/puff.json",
                br#"{"template": "minecraft:snowball", "item": "ruby:dust"}"#,
            ),
        ];
        let content = hmod("types", MANIFEST_TEXT, files).unwrap();
        let types: Vec<_> = content
            .entity_types
            .iter()
            .map(|t| (t.id.as_str(), t.item.as_str(), t.damage))
            .collect();
        assert_eq!(
            types,
            [
                ("ruby:bolt", "ruby:gem", 2.5),
                ("ruby:puff", "ruby:dust", 0.0)
            ]
        );
        let snowball = |rest: &str| format!(r#"{{"template": "minecraft:snowball"{rest}}}"#);
        for (case, file, why) in [
            (
                "type-template",
                r#"{"template": "minecraft:egg", "item": "ruby:gem"}"#.to_owned(),
                "is not one of",
            ),
            (
                "type-vanilla-item",
                snowball(r#", "item": "minecraft:stick""#),
                "is not one of this mod's items",
            ),
            (
                "type-unknown-item",
                snowball(r#", "item": "ruby:none""#),
                "is not one of this mod's items",
            ),
            (
                "type-damage",
                snowball(r#", "item": "ruby:gem", "damage": -1"#),
                "damage is 0 or more",
            ),
            (
                "type-key",
                snowball(r#", "item": "ruby:gem", "size": 1"#),
                "unknown field",
            ),
        ] {
            let files: &[(&str, &[u8])] = &[
                ("data/ruby/horizon/item/gem.json", ITEM),
                ("data/ruby/horizon/entity_type/a.json", file.as_bytes()),
            ];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
        let shared = snowball(r#", "item": "ruby:gem""#);
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/horizon/item/gem.json", ITEM),
            ("data/ruby/horizon/entity_type/a.json", shared.as_bytes()),
            ("data/ruby/horizon/entity_type/b.json", shared.as_bytes()),
        ];
        let e = hmod("type-shared", MANIFEST_TEXT, files).unwrap_err();
        assert!(e.contains("already throws another entity type"), "{e}");
    }

    #[test]
    fn block_items_patch_their_components() {
        let block = br#"{"template": "minecraft:iron_block", "components": {
            "minecraft:max_stack_size": 16, "!minecraft:lore": {}}}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/block/ruby_block.json", block)];
        let content = hmod("block-components", MANIFEST_TEXT, files).unwrap();
        let components = &content.blocks[0].components;
        assert_eq!(
            components.keys().collect::<Vec<_>>(),
            ["!minecraft:lore", "minecraft:max_stack_size"]
        );
        assert_eq!(components["minecraft:max_stack_size"], 16);
    }

    #[test]
    fn folder_mod_reads_the_same() {
        let dir = scratch("folder");
        std::fs::write(dir.join(MANIFEST), MANIFEST_TEXT).unwrap();
        let block = dir.join("data/ruby/horizon/block");
        std::fs::create_dir_all(&block).unwrap();
        std::fs::write(block.join("ruby_block.json"), BLOCK).unwrap();
        let package = Package::read(&dir).unwrap();
        assert!(package.wasm.is_none());
        let content = Content::read(&dir, &package.manifest).unwrap();
        assert_eq!(content.files, ["data/ruby/horizon/block/ruby_block.json"]);
        assert_eq!(content.file(&content.files[0]).unwrap(), BLOCK);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A connecting block names a block tag and copies a `PipeBlock`.
    #[test]
    fn connecting_blocks_copy_a_pipe_and_name_a_tag() {
        let block = |json: &str| {
            let files: &[(&str, &[u8])] =
                &[("data/ruby/horizon/block/cable.json", json.as_bytes())];
            hmod("cable", MANIFEST_TEXT, files)
        };
        let content = block(
            r##"{"template": "minecraft:chorus_plant", "connects_to": "#ruby:cable_joins"}"##,
        )
        .unwrap();
        assert_eq!(
            content.blocks[0].connects_to.as_deref(),
            Some("ruby:cable_joins")
        );
        let thin =
            r##"{"template": "minecraft:chorus_plant", "connects_to": "#ruby:j", "thickness": 4}"##;
        assert_eq!(block(thin).unwrap().blocks[0].thickness, Some(4.0));
        let e = block(r#"{"template": "minecraft:chorus_plant", "thickness": 4}"#).unwrap_err();
        assert!(e.contains("connects_to"), "{e}");
        let e = block(r##"{"template": "minecraft:chorus_plant", "connects_to": "#ruby:j", "thickness": 16}"##)
            .unwrap_err();
        assert!(e.contains("2 to 14"), "{e}");
        let e = block(r##"{"template": "minecraft:stone", "connects_to": "#ruby:cable_joins"}"##)
            .unwrap_err();
        assert!(e.contains("chorus_plant"), "{e}");
        for tag in ["ruby:cable_joins", "#cable_joins", "#Ruby:Cable"] {
            let json =
                format!(r#"{{"template": "minecraft:chorus_plant", "connects_to": "{tag}"}}"#);
            let e = block(&json).unwrap_err();
            assert!(e.contains("block tag"), "{tag}: {e}");
        }
    }

    /// A creative tab: a title, an icon and its items, all item ids.
    #[test]
    fn creative_tabs_name_their_items() {
        let tab = |json: &str| {
            let files: &[(&str, &[u8])] =
                &[("data/ruby/horizon/creative_tab/gems.json", json.as_bytes())];
            hmod("tab", MANIFEST_TEXT, files)
        };
        let content =
            tab(r#"{"title": "Ruby Gems", "icon": "ruby:ruby", "items": ["ruby:ruby", "minecraft:diamond"]}"#)
                .unwrap();
        assert_eq!(
            content.creative_tabs,
            [CreativeTab {
                id: "ruby:gems".into(),
                title: "Ruby Gems".into(),
                icon: "ruby:ruby".into(),
                items: vec!["ruby:ruby".into(), "minecraft:diamond".into()],
            }]
        );
        let e = tab(r#"{"title": "Gems", "icon": "ruby", "items": []}"#).unwrap_err();
        assert!(e.contains("item ids"), "{e}");
    }

    #[test]
    fn refuses_content_outside_its_fence() {
        let refused = |name: &str, files: &[(&str, &[u8])]| {
            let e = hmod(name, MANIFEST_TEXT, files).unwrap_err();
            assert!(!e.is_empty());
            e
        };
        let e = refused("other-ns", &[("data/other/horizon/block/a.json", BLOCK)]);
        assert!(e.contains("own namespaces"), "{e}");
        refused(
            "vanilla-asset",
            &[("assets/minecraft/textures/block/stone.png", b"x")],
        );
        refused(
            "vanilla-recipe",
            &[("data/minecraft/recipe/stone.json", b"{}")],
        );
        refused("convention-recipe", &[("data/c/recipe/stone.json", b"{}")]);
        refused("convention-asset", &[("assets/c/lang/en_us.json", b"{}")]);
        let e = refused(
            "modded-template",
            &[(
                "data/ruby/horizon/block/a.json",
                br#"{"template": "ruby:other"}"#,
            )],
        );
        assert!(e.contains("not a vanilla block"), "{e}");
        refused(
            "light",
            &[(
                "data/ruby/horizon/block/a.json",
                br#"{"template": "minecraft:stone", "properties": {"light": 16}}"#,
            )],
        );
        refused(
            "unknown-key",
            &[(
                "data/ruby/horizon/block/a.json",
                br#"{"template": "minecraft:stone", "glow": true}"#,
            )],
        );
        refused("shape-reversed", &[("data/ruby/horizon/block/a.json", br#"{"template":"minecraft:stone","shapes":{"property":"axis","values":{"x":[[1,0,0,0,1,1]]}}}"#)]);
        refused("shape-malformed", &[("data/ruby/horizon/block/a.json", br#"{"template":"minecraft:stone","shapes":{"property":"axis","values":{"x":[[0,0,0,1,1]]}}}"#)]);
        refused(
            "shape-no-values",
            &[(
                "data/ruby/horizon/block/a.json",
                br#"{"template":"minecraft:stone","shapes":{"property":"axis","values":{}}}"#,
            )],
        );
        let e = refused(
            "clash",
            &[
                ("data/ruby/horizon/block/a.json", BLOCK),
                ("data/ruby/horizon/item/a.json", ITEM),
            ],
        );
        assert!(e.contains("also a block's item"), "{e}");
        refused("other-kind", &[("data/ruby/horizon/entity/a.json", b"{}")]);
        refused(
            "stack",
            &[(
                "data/ruby/horizon/item/a.json",
                br#"{"template": "minecraft:stick", "max_stack_size": 100}"#,
            )],
        );
        for (case, components, why) in [
            (
                "bare-component",
                r#"{"max_damage": 3}"#,
                "must be namespaced",
            ),
            (
                "uppercase-path",
                r#"{"minecraft:Foo": 3}"#,
                "must be namespaced and valid",
            ),
            (
                "uppercase-namespace",
                r#"{"Example:charge": 3}"#,
                "must be namespaced and valid",
            ),
            (
                "empty-namespace",
                r#"{":charge": 3}"#,
                "must be namespaced and valid",
            ),
            (
                "empty-path",
                r#"{"example:": 3}"#,
                "must be namespaced and valid",
            ),
            (
                "invalid-removal",
                r#"{"!example:Foo": {}}"#,
                "must be namespaced and valid",
            ),
            (
                "own-key",
                r#"{"minecraft:rarity": "epic"}"#,
                "set rarity instead",
            ),
            (
                "own-removal",
                r#"{"!minecraft:max_stack_size": {}}"#,
                "set max_stack_size",
            ),
            (
                "removal-value",
                r#"{"!minecraft:food": true}"#,
                "a removal's value is {}",
            ),
        ] {
            let item = format!(r#"{{"template": "minecraft:stick", "components": {components}}}"#);
            let e = refused(case, &[("data/ruby/horizon/item/a.json", item.as_bytes())]);
            assert!(e.contains(why), "{case}: {e}");
        }
        let glass = br#"{"template": "minecraft:glass", "item": false}"#;
        for (case, rest) in [
            ("bare-remainder", "bowl"),
            ("missing-remainder", "ruby:bowl"),
            ("other-mod-remainder", "jade:bowl"),
            ("itemless-remainder", "ruby:glass"),
            ("air-remainder", "minecraft:air"),
        ] {
            let item =
                format!(r#"{{"template": "minecraft:stick", "crafting_remainder": "{rest}"}}"#);
            let files: &[(&str, &[u8])] = &[
                ("data/ruby/horizon/item/a.json", item.as_bytes()),
                ("data/ruby/horizon/block/glass.json", glass),
            ];
            let e = refused(case, files);
            assert!(e.contains("crafting_remainder"), "{case}: {e}");
        }
        for (case, block, why) in [
            (
                "block-removal-value",
                r#"{"template": "minecraft:stone", "components": {"!minecraft:lore": []}}"#,
                "a removal's value is {}",
            ),
            (
                "block-stack-zero",
                r#"{"template": "minecraft:stone", "components": {"minecraft:max_stack_size": 0}}"#,
                "integer 1 to 99",
            ),
            (
                "block-stack-100",
                r#"{"template": "minecraft:stone", "components": {"minecraft:max_stack_size": 100}}"#,
                "integer 1 to 99",
            ),
            (
                "block-stack-string",
                r#"{"template": "minecraft:stone", "components": {"minecraft:max_stack_size": "16"}}"#,
                "integer 1 to 99",
            ),
            (
                "block-rarity",
                r#"{"template": "minecraft:stone", "components": {"minecraft:rarity": "bogus"}}"#,
                "common, uncommon",
            ),
            (
                "block-without-item",
                r#"{"template": "minecraft:glass", "item": false,
                    "components": {"minecraft:max_stack_size": 16}}"#,
                "item is false",
            ),
        ] {
            let e = refused(
                case,
                &[("data/ruby/horizon/block/a.json", block.as_bytes())],
            );
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn listed_namespaces_hold_content() {
        let manifest = format!("{MANIFEST_TEXT}namespaces = [\"kaisyn\"]\n");
        let files: &[(&str, &[u8])] = &[
            ("data/kaisyn/worldgen/template_pool/a.json", b"{}"),
            ("data/kaisyn/structure/a.nbt", b"nbt"),
            ("assets/kaisyn/lang/en_us.json", b"{}"),
        ];
        let content = hmod("listed", &manifest, files).unwrap();
        assert_eq!(content.files.len(), 3);
        assert_eq!(content.namespaces, ["ruby", "kaisyn"]);
        // Declarations stay in the id's namespace.
        let decl: &[(&str, &[u8])] = &[("data/kaisyn/horizon/block/a.json", BLOCK)];
        let e = hmod("listed-decl", &manifest, decl).unwrap_err();
        assert!(e.contains("declared in data/ruby/horizon/"), "{e}");
        // A namespace it does not list is still outside.
        let other: &[(&str, &[u8])] = &[("data/other/structure/a.nbt", b"nbt")];
        assert!(hmod("listed-other", &manifest, other).is_err());
    }

    #[test]
    fn mods_sharing_a_namespace_clash() {
        let t_and_t = ["t_and_t".to_owned(), "kaisyn".to_owned()];
        let kaisyn = ["kaisyn".to_owned()];
        let ruby = ["ruby".to_owned(), "gems".to_owned()];
        assert_eq!(
            clash(("t_and_t", &t_and_t), ("kaisyn", &kaisyn)).as_deref(),
            Some("mods t_and_t and kaisyn both use the namespace kaisyn; turn one of them off")
        );
        assert_eq!(clash(("t_and_t", &t_and_t), ("ruby", &ruby)), None);
    }

    #[test]
    fn content_needs_the_registries_capability() {
        let code_only = MANIFEST_TEXT
            .replace("api = \"0.2\"", "api = \"0.1\"")
            .replace("capabilities = [\"registries\"]", "capabilities = []");
        // No code and no registries: nothing to load.
        let e = hmod("empty", &code_only, &[]).unwrap_err();
        assert!(e.contains("mod.wasm: missing"), "{e}");
        // Registries on API 0.1.
        let old = MANIFEST_TEXT.replace("api = \"0.2\"", "api = \"0.1\"");
        assert!(Manifest::parse(&old).unwrap_err().contains("needs api"));
        // Content without the capability, in a mod with code.
        let dir = scratch("no-cap");
        let path = dir.join("m.hmod");
        let files: &[(&str, &[u8])] = &[("assets/ruby/lang/en_us.json", b"{}")];
        std::fs::write(
            &path,
            Package::write_files(&code_only, Some(b"\0asm"), files).unwrap(),
        )
        .unwrap();
        let package = Package::read(&path).unwrap();
        let e = Content::read(&path, &package.manifest).err().unwrap();
        assert!(e.contains("not capability registries"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paths_are_plain() {
        assert!(check_path("assets/ruby/textures/block/ruby_block.png").is_ok());
        assert!(check_path("data/ruby/loot_table/blocks/a.json").is_ok());
        for bad in [
            "assets/ruby",
            "assets/../x/y",
            "assets/ruby//a",
            "assets/Ruby/a",
            "assets/ruby/a b",
            "/assets/ruby/a",
            "other/ruby/a",
            "assets\\ruby\\a",
        ] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
    }
}
