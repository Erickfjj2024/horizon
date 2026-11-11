<!-- Generated from crates/horizon-mod-sdk/wit/horizon.wit and the manifest's capabilities by crates/horizon-mods/tests/api_reference.rs; a test fails when this file is out of date. Update it with: HORIZON_WRITE_DOCS=1 cargo test -p horizon-mods --features pack --test api_reference -->
# Mod API reference

The contract between Horizon and its mods: the WIT package `horizon:mods@0.1.0`, every interface, type, function and hook in it, and the capabilities a manifest may list. The guide is [mods.md](../mods.md); stability and versions: [stability.md](stability.md). Manifest `api` lines this game serves: `0.1`, `0.2`, `0.3`.

## Capabilities

| In `capabilities` | Grants | The player reads "May …" |
| --- | --- | --- |
| `game-read` | [`game`](#game) | see your position, health and the time |
| `chat` | [`chat`](#chat) | show you chat messages |
| `hud` | [`hud`](#hud) | draw text on your screen |
| `storage` | [`storage`](#storage) | save its own settings to disk |
| `command` | [`command`](#command) | run commands as you, with your permissions |
| `registries` | data files (`assets/`, `data/`), no interface | add blocks, items, mob effects and entity types to your worlds |
| `ui` | [`ui`](#ui) | show its own screens and a settings page |
| `render` | [`render`](#render) | draw on your screen, shake and zoom the camera |
| `input` | [`input`](#input) | see the keys you hold and your clicks, take over the use button on its own items and the mouse wheel, and add keys |
| `players` | [`players`](#players) | see who is on the server and message them |
| `world` | [`world`](#world) | read and change the world's blocks |
| `player-control` | [`player-control`](#player-control) | see whether players sneak, jump and how much experience they have, move them, take experience from them and play sounds |
| `recipe-types` | [`recipe-types`](#recipe-types) | read the recipes of the types it brings and the item tags they use |
| `net` | [`net`](#net) | send bytes to its half running in players' games, and hear theirs |
| `entities` | [`entities`](#entities) | see the mobs of the world, spawn, move, hurt and remove them |
| `player-items` | [`player-items`](#player-items) | see and change what players carry, hurt them and give them effects |
| `chunks` | [`chunks`](#chunks) | keep chunks loaded and add to chunks as they are generated |
| `net-client` | [`net-client`](#net-client) | send bytes to its half running on the server, and hear it |
| `recipes` | [`recipes`](#recipes) | read the game's recipes and items and which recipes you know, place recipes in your crafting grid and, in creative mode, give you items |

## World `horizon-mod`

A client mod: runs in the player's game.

- Imports (what the game offers): `log`, `game`, `chat`, `hud`, `storage`, `command`
- Exports (what the mod provides): `events`

## World `horizon-server-mod`

A server mod (manifest `side = "server"`, `api = "0.3"`, experimental): runs where the world is simulated, on the dedicated server, a LAN host and single player's own server.

- Imports (what the game offers): `log`, `storage`, `players`, `player-control`, `item-types`, `recipe-types`, `net`, `entities`, `player-items`, `chunks`, `world`
- Exports (what the mod provides): `server-events`

## World `horizon-ui-mod`

A mod with widgets (capability `ui`): the SDK builds every mod against this world. Mods built for `horizon-mod` alone (no `ui-events` export) still load and simply hear nothing of their widgets.

- Imports (what the game offers): `ui`, `render`, `input`, `recipes`, `browser`, `net-client`, `log`, `game`, `chat`, `hud`, `storage`, `command`
- Exports (what the mod provides): `ui-events`, `net-events`, `events`

## World `net-host`

The game's side of capability `net-client` (an optional export).

- Imports (what the game offers): `net-client`
- Exports (what the mod provides): `net-events`

## World `ui-host`

The game's side of `horizon-ui-mod` beyond `horizon-mod`.

- Imports (what the game offers): `ui`
- Exports (what the mod provides): `ui-events`

## World `render-host`

The game's side of capability `render` (imports only).

- Imports (what the game offers): `render`, `input`
- Exports (what the mod provides): 

## World `recipes-host`

The game's side of capability `recipes` (imports only).

- Imports (what the game offers): `recipes`, `browser`
- Exports (what the mod provides): 

## `log`

Access: always granted.

The mod's log, in the game's log under the mod's id. Always granted.

### enum `level`

| Case | |
| --- | --- |
| `debug` |  |
| `info` |  |
| `warn` |  |
| `error` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `write` | `func(level: level, message: string)` | One line in the game's log as `[mod <id>] message`, at that level. |

## `game`

Access: capability `game-read`.

Capability `game-read`: the local player, the world clock and the block the player looks at.

### record `player`

| Field | Type | |
| --- | --- | --- |
| `x` | `f64` | Feet position, in blocks. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `yaw` | `f32` | Degrees, as F3 shows them. |
| `pitch` | `f32` | Degrees: -90 looks straight up, 90 straight down. |
| `health` | `f32` | Hit points as vanilla counts them: 20 is full health without effects. |
| `food` | `u32` | 0 to 20, as the hunger bar. |
| `dimension` | `string` | `minecraft:overworld`, `minecraft:the_nether`, ... |

### record `clock`

| Field | Type | |
| --- | --- | --- |
| `game-time` | `u64` | Ticks since the world was created. |
| `day-time` | `u64` | 24000 ticks per day; 0 is 6:00 on the first day. |

### record `hit`

A block along the player's view.

| Field | Type | |
| --- | --- | --- |
| `block-x` | `s32` | The block hit. |
| `block-y` | `s32` |  |
| `block-z` | `s32` |  |
| `x` | `f64` | The point on its face where the view meets it. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `block` | `string` | `minecraft:stone`, ... |

| Function | Signature | What it does |
| --- | --- | --- |
| `get-player` | `func() -> option<player>` | None on the title screen. |
| `get-clock` | `func() -> option<clock>` | None on the title screen. |
| `raycast` | `func(distance: f32) -> option<hit>` | The first block along the player's view (fluids pass), up to `distance` blocks (at most 1024; past the loaded chunks, across the distant terrain when LOD Distance is on), as of this tick. None when nothing is hit, and on the title screen. |

## `chat`

Access: capability `chat`.

Capability `chat`: messages only the local player sees.

| Function | Signature | What it does |
| --- | --- | --- |
| `show` | `func(text: string)` | Shown in chat after the mod's name, like a system message. |

## `hud`

Access: capability `hud`.

Capability `hud`: one panel of text lines per mod, drawn by the game every frame until the mod changes or clears it (no call per frame).

### enum `corner`

| Case | |
| --- | --- |
| `top-left` |  |
| `top-right` |  |
| `bottom-left` |  |
| `bottom-right` |  |

### record `line`

| Field | Type | |
| --- | --- | --- |
| `text` | `string` | Plain text, up to 256 characters. |
| `color` | `u32` | 0xRRGGBB. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-panel` | `func(corner: corner, lines: list<line>)` | Replaces the mod's panel: up to 16 lines in that corner of the screen. |
| `clear` | `func()` | Removes the mod's panel. |

## `storage`

Access: capability `storage`.

Capability `storage`: a per-mod folder of small key/value files, quota'd (docs/mods.md). Keys are `[a-z0-9_.-]{1,64}`, one path segment.

| Function | Signature | What it does |
| --- | --- | --- |
| `get` | `func(key: string) -> option<list<u8>>` | None if `key` has never been `set`, or the key is not well-formed. |
| `set` | `func(key: string, value: list<u8>) -> result<_, string>` | Errs on a malformed key or over quota (bytes for the whole mod). |
| `delete` | `func(key: string)` | No error if `key` was already absent. |

## `command`

Access: capability `command`.

Capability `command`: vanilla commands run as the player, with the player's permissions, after the hook call returns (8 per call). A mod's own commands are not run this way.

| Function | Signature | What it does |
| --- | --- | --- |
| `run` | `func(line: string)` | `line` without its slash; its feedback goes to the chat as if typed. |
| `run-quiet` | `func(lines: list<string>)` | `lines` (each without its slash, at most 256 a call) as `run` does, but with nothing in the chat: no feedback and no failure. In single player and on Horizon's servers, each runs exactly as the player typing it would (the same rights), in order, 64 a tick, and the server logs one line a batch; one that carries a chat message (`say`, `msg`) is refused. On any other server they go as chat commands, and there their feedback shows: a mod running many should say so. |

## `ui`

Access: capability `ui`.

Capability `ui`: widgets the game draws with its own menus' widgets, for a screen the mod owns and a settings page in Options > Mods. Layout and drawing are the game's: a mod says what its widgets hold, never where they sit or how they look, and the game clips everything it is handed.

### record `text-widget`

A line of text.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget; `ui-events` calls echo it. |
| `text` | `string` | Up to 256 characters. |
| `color` | `u32` | 0xRRGGBB; 0 is the menus' white. |

### record `item-widget`

An item's icon and a name beside it, as recipe viewers show them.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget. |
| `item` | `string` | `minecraft:diamond`, `<mod>:<path>`; an unknown id draws a "?". |
| `name` | `string` | The name shown beside the icon (empty: the item's own). |

### record `list-widget`

Rows the player picks one of, as an options page lists choices: a pick is `ui-events::select`, `selected` is the row to mark now.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget; `ui-events::select` echoes it. |
| `rows` | `list<string>` | The rows' texts, at most 512, each up to 256 characters. |
| `selected` | `option<u32>` | The row to mark as the current choice, if any. |

### record `button-widget`

A thing on a mod's screen or settings page. A button as the options pages have them (200 × 20): a click is `ui-events::select(id, 0)`. Mods cycle its label for a choice ("Corner: Top Left").

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget. |
| `label` | `string` | Up to 256 characters. |

### variant `widget`

| Case | Payload | |
| --- | --- | --- |
| `text` | `text-widget` | Text the player reads. |
| `button` | `button-widget` | A button. |
| `item` | `item-widget` | An item's icon and name. |
| `list` | `list-widget` | Rows the player picks from. |

### record `screen`

A screen the mod owns: a title over widgets stacked down the middle, the Done button the game adds, and Esc to leave it.

| Field | Type | |
| --- | --- | --- |
| `title` | `string` | Its title, up to 256 characters. |
| `widgets` | `list<widget>` | Its widgets, in order, at most 64. |

| Function | Signature | What it does |
| --- | --- | --- |
| `open-screen` | `func(screen: screen)` | Show `screen` over the game; a second call while it is open replaces it. Only the last call of one hook counts. |
| `close-screen` | `func()` | Close the mod's screen when it is open (the mod hears no `closed` for this: it was its own doing). |
| `set-settings` | `func(widgets: list<widget>)` | The mod's page in Options > Mods, reached from a settings button on its row: the widgets it shows, an empty list for no page. The player opening the page is `ui-events::settings-opened`: push fresh widgets then; later calls update the page while it shows. |

## `ui-events`

Access: exported by the mod: the game calls it.

What the game calls on a mod that shows widgets (capability `ui`): what the player did with them.

| Function | Signature | What it does |
| --- | --- | --- |
| `select` | `func(id: u32, row: u32)` | A row of one of the mod's `list` widgets was picked: the widget's `id` and the row, on the screen or the settings page that showed it. |
| `closed` | `func()` | The player left the mod's screen (Esc or Done); not for a screen the mod closed itself or one that went away with the mod. |
| `settings-opened` | `func()` | The player opened the mod's settings page in Options > Mods: call `ui::set-settings` now to fill it with fresh widgets. |

## `input`

Access: capability `input`.

Capability `input` (experimental: the manifest needs `experimental = ["input"]`): the use button on items the mod claims, and the mod's own keys. Events wait for `poll`, at most 256.

### record `held-item`

The main hand's stack.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:spyglass`, `<mod>:<path>`. |
| `count` | `u32` |  |
| `slot` | `u32` | The hotbar slot, 0 to 8. |

### record `item-use`

| Field | Type | |
| --- | --- | --- |
| `item` | `held-item` |  |
| `ticks` | `u32` | Game ticks the button was held (0 on `use-start`). |

### variant `event`

| Case | Payload | |
| --- | --- | --- |
| `use-start` | `item-use` | The use button went down with a claimed item in the main hand. The game does nothing else with the click: no placing, no eating, no swing. |
| `use-stop` | `item-use` | It came up again, or the item left the hand, or a screen opened. |
| `key-down` | `string` | A key `register-key` named went down or up while playing (no screen open). The game's own mapping on that key still runs. A screen opening lets every held key up. |
| `key-up` | `string` |  |
| `wheel` | `f32` | Mouse wheel steps while the mod holds the wheel (`claim-wheel`): positive away from the player, as the hotbar reads them. |

### record `mapping-state`

A mapping as the player has bound it.

| Field | Type | |
| --- | --- | --- |
| `label` | `string` | The bound key's name as Controls shows it: `W`, `Space`, `LMB`. |
| `down` | `bool` | Held down now (as of this frame for a mod with a `frame` hook, else this tick). |

| Function | Signature | What it does |
| --- | --- | --- |
| `claim-items` | `func(ids: list<string>)` | The items whose use the mod handles instead of the game, by id (at most 64). Replaces the previous list. |
| `register-key` | `func(name: string, label: string, key: string) -> result<_, string>` | A key of the mod's: `name` comes back in `key-down` and `key-up`, `label` says what it does, `key` is the key as options.txt writes it (`key.keyboard.r`), the default: the player can bind another one on the mod's page in Options > Mods, and that one wins. At most 16; an unknown key is an error. |
| `held` | `func() -> option<held-item>` | The main hand's stack, as of this tick; none when it is empty. |
| `poll` | `func() -> list<event>` | The events since the last call, oldest first. |
| `claim-wheel` | `func(on: bool)` | Takes the mouse wheel while playing (true) or gives it back: while taken, its steps are the mod's `wheel` events and the hotbar doesn't move. The game gives it back when the mod stops. |
| `mapping` | `func(name: string) -> option<mapping-state>` | A movement or click mapping as the player has bound it: `key.forward`, `key.left`, `key.back`, `key.right`, `key.jump`, `key.sneak`, `key.sprint`, `key.attack` or `key.use`. None for another name. |
| `clicks-per-second` | `func() -> tuple<u32, u32>` | Clicks of the attack and use buttons in the last second. |

## `render`

Access: capability `render`.

Capability `render` (experimental: the manifest needs `experimental = ["render"]`): shapes the game draws for the mod with its own renderer, kept until the mod replaces them (no call per frame). A mod never supplies shader code.

### enum `corner`

A corner of the screen, as `hud`'s panels use them.

| Case | |
| --- | --- |
| `top-left` |  |
| `top-right` |  |
| `bottom-left` |  |
| `bottom-right` |  |

### record `rect`

A rectangle in GUI pixels: the top left corner and the size.

| Field | Type | |
| --- | --- | --- |
| `x` | `f32` |  |
| `y` | `f32` |  |
| `w` | `f32` |  |
| `h` | `f32` |  |

### record `point`

A point in GUI pixels from the screen's top left.

| Field | Type | |
| --- | --- | --- |
| `x` | `f32` |  |
| `y` | `f32` |  |

### record `fill-shape`

| Field | Type | |
| --- | --- | --- |
| `rect` | `rect` |  |
| `color` | `u32` | 0xAARRGGBB. |

### record `quad-shape`

Any four-cornered shape (a wheel's segment, an arrow): the corners in order round it.

| Field | Type | |
| --- | --- | --- |
| `corners` | `tuple<point, point, point, point>` |  |
| `color` | `u32` | 0xAARRGGBB. |

### record `sprite-shape`

| Field | Type | |
| --- | --- | --- |
| `sprite` | `string` | One of the game's GUI sprites, as `gui/sprites/` names them (`hud/heart/full`, `widget/button`), the resource packs' versions included, drawn over `rect` as vanilla draws it: nine-sliced when its `gui.scaling` says so (`tooltip/frame`, `widget/button`: the borders keep their size), stretched otherwise. An unknown one draws nothing. |
| `rect` | `rect` |  |
| `color` | `u32` | A tint, 0xAARRGGBB: 0xFFFFFFFF draws it as it is. |

### record `item-shape`

| Field | Type | |
| --- | --- | --- |
| `item` | `string` | `minecraft:diamond`, `<mod>:<path>`; an unknown one draws nothing. |
| `at` | `point` | The icon's top left. |
| `scale` | `f32` | 1.0 is the hotbar's 16 × 16. |

### record `text-shape`

| Field | Type | |
| --- | --- | --- |
| `text` | `string` | Up to 256 characters, one line. |
| `at` | `point` |  |
| `color` | `u32` | 0xRRGGBB, as `hud`'s lines. |
| `centered` | `bool` | Centred on `at.x` instead of starting there. |
| `shadow` | `bool` |  |

### variant `shape`

| Case | Payload | |
| --- | --- | --- |
| `fill` | `fill-shape` |  |
| `quad` | `quad-shape` |  |
| `sprite` | `sprite-shape` |  |
| `item` | `item-shape` |  |
| `text` | `text-shape` |  |

### record `frame-rate`

| Field | Type | |
| --- | --- | --- |
| `fps` | `u32` | Frames drawn in the last second. |
| `min` | `u32` | The slowest frame's rate, and the 1% low (the 99th percentile frame time's rate), over the last 5 seconds. |
| `low` | `u32` |  |

### record `zoom`

| Field | Type | |
| --- | --- | --- |
| `factor` | `f32` | The field of view is divided by it: 1 is none, at most 10. |
| `smooth` | `bool` | Ease in and out through the game's field-of-view easing, as the spyglass does, instead of at once. |
| `turn` | `f32` | Mouse turning is multiplied by it while zoomed: 1 keeps the player's sensitivity, at least 0.05. |

### record `minimap`

| Field | Type | |
| --- | --- | --- |
| `corner` | `corner` |  |
| `size` | `f32` | Its side in GUI pixels, 16 to 256. |
| `blocks-per-pixel` | `f32` | World blocks per map pixel, 0.5 to 4; the map is 128 × 128 pixels. |
| `rotate` | `bool` | The map turns with the player, who faces up, and an N on its rim marks north. Otherwise north is up and the player's arrow turns. |
| `round` | `bool` | Round, the corners left clear; otherwise square. |

### record `far-terrain`

| Field | Type | |
| --- | --- | --- |
| `distance` | `u32` | How far the distant terrain reaches, in chunks: 64 to 1024. On integrated graphics other than Apple's the game caps it lower. |
| `reduced` | `bool` | Coarser far tiles, as the Far Detail setting's Reduced: 2 to 4 ms less GPU time per frame on integrated graphics. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-hud` | `func(shapes: list<shape>)` | Replaces the mod's HUD shapes: up to 1024, drawn every frame in order over the HUD (after the hotbar, under chat and screens), hidden with the HUD (F1). An empty list removes them. |
| `gui-size` | `func() -> tuple<f32, f32>` | The screen's size in GUI pixels (width, height), as of this tick. |
| `shake` | `func(strength: f32, seconds: f32)` | Shakes the camera: a jolt of `strength` (0 to 1, 1 the hardest) that fades out over `seconds` (at most 10). Jolts add up to 1. The view turns, the player doesn't, and the player's Distortion Effects option scales it (0 turns it off). |
| `set-corner` | `func(corner: corner, width: f32, height: f32, shapes: list<shape>)` | Replaces the mod's corner panel: shapes in GUI pixels from the top left of a `width` × `height` box that the game places in `corner`, stacked with the game's overlays and the other mods' panels (under the status effects at the top right), as `hud`'s panels are. |
| `clear-corner` | `func()` | Removes the mod's corner panel. |
| `text-width` | `func(text: string) -> f32` | The width of one line of text in GUI pixels, in the HUD's font: exact for Latin letters (U+0000 to U+024F), 6 per other character. |
| `frame-stats` | `func() -> frame-rate` | The game's frame counter, recomputed twice a second. |
| `set-zoom` | `func(zoom: zoom)` | Zooms the camera until the next call (`factor` 1 ends it), only while playing: a screen opening or the spyglass's own view (which wins) leave the camera vanilla's. With several mods, the strongest zoom counts. |
| `set-minimap` | `func(minimap: option<minimap>)` | Shows a map of the world around the player in `corner`, stacked with the game's overlays and the mods' panels, until the next call (none hides it). It looks like a map item: vanilla's map colours with their height shading and water depth, and the player's arrow. Underground, under a roof and in the Nether it maps the floor around the player's height. The game keeps the map's tiles and refreshes them within its own frame budget; hidden, they are dropped. One map shows at a time: the first loaded mod's. |
| `set-far-terrain` | `func(far-terrain: option<far-terrain>)` | Draws the world beyond the render distance as distant terrain, out to `distance` chunks, until the next call (none ends it). The game builds it on its own workers, from the chunks already loaded or saved and, where nobody has been, from the world's generator; the simulation distance doesn't change. Only worlds whose generator the game has (single player and a LAN world you host) show it. The player's own LOD Distance setting, when on, wins. One far terrain shows at a time: the first loaded mod's. |

## `events`

Access: exported by the mod: the game calls it.

What the game calls. Only the hooks `init` asks for are ever called.

### record `registration`

| Field | Type | |
| --- | --- | --- |
| `commands` | `list<string>` | Chat commands, without the slash. A name vanilla already uses is refused (vanilla's command runs). |
| `tick` | `bool` | Call `tick` every game tick (20 per second while playing). |
| `frame` | `bool` | Call `frame` every rendered frame. Budget: 0.1 ms. |
| `chat` | `bool` | Call `chat` for each chat line the player receives. |

| Function | Signature | What it does |
| --- | --- | --- |
| `init` | `func() -> result<registration, string>` | Once, after loading. An error keeps the mod off and is shown. |
| `tick` | `func(tick: u64)` | Each game tick while in a world, with the game's tick counter. |
| `frame` | `func(seconds: f32)` | Each rendered frame, with the seconds since the last one. |
| `command` | `func(name: string, args: string) -> result<option<string>, string>` | `/name args`: ok(some(text)) is shown as feedback, err(text) in red. |
| `chat` | `func(text: string)` | Each chat line the player receives, as plain text. |

## `recipes`

Access: capability `recipes`.

Capability `recipes` (experimental): the recipes and items the game knows, for recipe viewers. In single player and on Horizon's servers they are the world's; on any other server they are vanilla 26.3's (a client is sent no others), while `known` follows that server's book.

### record `stack`

An item of `items`, by its index there, with a count and the components it has beyond the item's own, in vanilla's item syntax (`[potion_contents={potion:"minecraft:swiftness"}]`), or "".

| Field | Type | |
| --- | --- | --- |
| `item` | `u32` |  |
| `count` | `u32` |  |
| `components` | `string` |  |

### enum `kind`

The station a recipe is made at.

| Case | |
| --- | --- |
| `crafting-shaped` |  |
| `crafting-shapeless` |  |
| `smelting` |  |
| `blasting` |  |
| `smoking` |  |
| `campfire` |  |
| `stonecutting` |  |
| `smithing` |  |
| `brewing` |  |

### record `recipe`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:oak_planks`. |
| `kind` | `kind` |  |
| `category` | `string` | The recipe book's tab (`crafting_building_blocks`, `furnace_food`), or "". |
| `group` | `string` | Recipes the book shows on one button share a group, or "". |
| `width` | `u32` | A shaped recipe's grid: `slots` holds its cells row by row. 0 for the other kinds. |
| `height` | `u32` |  |
| `slots` | `list<list<stack>>` | Each slot's stacks, any one of which fits; an empty list is an empty cell. Cooking and stonecutting: the input. Smithing: the template, the base, the addition. Brewing: the bottle, the reagent. |
| `results` | `list<stack>` | What it makes; several when that varies (a trim on any armour). |
| `cooking-time` | `u32` | Cooking: game ticks and experience; 0 for the other kinds. |
| `experience` | `f32` |  |
| `book-id` | `s32` | The recipe book's id for it (one per way the book shows it), as `known` lists them; -1 for the kinds the book doesn't show (campfire, stonecutting, smithing, brewing). |

### record `item`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:diamond`, `<mod>:<path>`. |
| `name` | `string` | In the player's language. |
| `tags` | `list<string>` | Its item tags, without the `#`. |

| Function | Signature | What it does |
| --- | --- | --- |
| `items` | `func() -> list<item>` | Every item, in the registry's order: `stack.item` indexes this list. |
| `all` | `func() -> list<recipe>` | Every recipe: the book's in its order, then campfire, stonecutting, smithing and brewing. They don't change while a world is open. |
| `known` | `func() -> list<s32>` | The `book-id`s of the recipes the player's recipe book holds now. |
| `place` | `func(book-id: s32, all: bool) -> result<_, string>` | Places a recipe in the open screen's grid as a click in the recipe book does (`ServerboundPlaceRecipePacket`): the inventory's grid or a crafting table. `all` places as many as the items allow, as with Shift. Refused while no such screen is open, for a recipe the player's book doesn't hold and for one that can't be placed; one missing items shows as a ghost in the grid, as in the book. |
| `give` | `func(stack: stack) -> result<_, string>` | Creative mode only: gives the player the stack, as taking it from the creative inventory does (`ServerboundSetCreativeModeSlot`): the selected slot first, then the hotbar, then the inventory. The count is cut to one stack of the item (64, 16 or 1). |

## `browser`

Access: not granted by any capability.

Capability `ui` (experimental): a recipe viewer's panel beside the game's inventory and container screens (not the creative inventory), on the right of the screen. The game draws it with its own widgets and slots, pages it and keeps its search box; the mod says what it holds. The first loaded mod with a panel shows.

### record `stack`

An item to show: its id, a count (1 draws none) and its components beyond the item's own, in vanilla's item syntax, or "".

| Field | Type | |
| --- | --- | --- |
| `item` | `string` |  |
| `count` | `u32` |  |
| `components` | `string` |  |

### record `click`

A click on an item of the panel, by its index in `set-items`'s list.

| Field | Type | |
| --- | --- | --- |
| `index` | `u32` |  |
| `right` | `bool` |  |

### record `item-key`

A key the mod registered (`input.register-key`) went down while the pointer was on an item: a slot of the screen's, or the panel's.

| Field | Type | |
| --- | --- | --- |
| `key` | `string` |  |
| `stack` | `stack` |  |

### record `slot-click`

A click on a slot of a recipe the panel shows (`set-recipes`).

| Field | Type | |
| --- | --- | --- |
| `recipe` | `u32` | The recipe's index in `set-recipes`'s list and the slot's in it; the results come after the slots. |
| `slot` | `u32` |  |
| `stack` | `stack` | The stack the slot showed at that moment. |
| `right` | `bool` |  |

### variant `event`

| Case | Payload | |
| --- | --- | --- |
| `clicked` | `click` |  |
| `search` | `string` | The search box's text, each time the player changes it. |
| `key` | `item-key` |  |
| `slot-clicked` | `slot-click` |  |
| `action` | `u32` | The action button of the recipe at this index in `set-recipes`'s list. |
| `page` | `u32` | The player turned the recipes' page (`set-recipes`): the index of the first recipe now shown. |
| `back` |  | The back button of `set-recipes`. |
| `tab` | `u32` | The player picked the tab at this index of `set-recipes`' tabs. |

### enum `layout`

How the game lays out a recipe of `set-recipes`.

| Case | |
| --- | --- |
| `grid` | A crafting grid of `width` x `height` slots, an arrow and the result. |
| `cooking` | One input over a flame, an arrow and the result (furnaces, smokers, campfires). |
| `single` | One input, an arrow and the result (the stonecutter). |
| `smithing` | Template, base and addition, an arrow and the result. |
| `brewing` | A potion and an ingredient, an arrow and the result. |

### record `recipe-view`

A recipe to show. Each slot, and the result, cycles through its stacks, one a second; an empty slot shows nothing.

| Field | Type | |
| --- | --- | --- |
| `layout` | `layout` |  |
| `width` | `u32` | The grid's size (`grid` only), at most 3 x 3. |
| `height` | `u32` |  |
| `slots` | `list<list<stack>>` |  |
| `results` | `list<stack>` |  |
| `note` | `string` | A short line under the recipe (a cooking time), or "". |
| `action` | `option<bool>` | The action button beside it: none, enabled, or shown greyed. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-items` | `func(items: list<stack>)` | The panel's items, at most 8192 (an empty list hides the panel). |
| `set-recipes` | `func(title: string, tabs: list<stack>, tab: u32, recipes: list<recipe-view>)` | Shows these recipes in the panel instead of its items, as many as fit a page, under `title` and a back button, with a row of `tabs` (each an icon; `tab` is the selected one) between them; an empty list of recipes shows the items again. At most 256 recipes and 16 tabs. |
| `poll` | `func() -> list<event>` | What the player did on the panel since the last poll, at most 256 kept. |

## `players`

Access: capability `players`.

Capability `players`, server mods only (manifest `api = "0.3"`, experimental): the players on the server, and messages to them.

### enum `game-mode`

| Case | |
| --- | --- |
| `survival` |  |
| `creative` |  |
| `adventure` |  |
| `spectator` |  |

### record `player`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | The player's UUID, `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`. |
| `name` | `string` |  |
| `x` | `f64` | Feet position, in blocks. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `dimension` | `string` | `minecraft:overworld`, `minecraft:the_nether`, ... |
| `game-mode` | `game-mode` |  |
| `health` | `f32` | Hit points as vanilla counts them: 20 is full health without effects. |

### record `radial-entry`

One choice of a radial menu.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | What the mod hears back when it is chosen (at most 64 bytes). |
| `label` | `string` | Its name under the icon (at most 64 bytes). |
| `icon` | `string` | An item drawn for it (`mymod:mode_vein`); empty for none. |

| Function | Signature | What it does |
| --- | --- | --- |
| `online` | `func() -> list<player>` | The players online, in the order they joined. |
| `tell` | `func(player: string, text: string) -> bool` | A system message to one player, after the mod's name; false when no player with that id is online. Up to 8 messages per call. |
| `tell-overlay` | `func(player: string, text: string) -> bool` | A message over the hotbar (the action bar, as `Player.displayClientMessage(text, true)`) to one player, for a line that is gone in seconds; false when no player with that id is online. No mod name in front of it. Counts toward the same 8 per call as `tell`. API 31. |
| `set-hud-line` | `func(player: string, id: string, icon: string, text: string, fraction: f32) -> bool` | Sets line `id` (`mymod:energy`, at most 64 bytes) of the mod's HUD on one player's screen (the MekaSuit's module HUD, `IModuleHUD`): the item `icon` (`mymod:jetpack_unit`; empty for none) drawn small, `text` beside it (at most 256 bytes), and, with `fraction` 0 to 1, a bar under the text filled that much (a negative fraction draws no bar). The lines show at the screen's top left, under what is there already, in the order of their ids, until changed; an empty text takes the line away, and the player's client forgets them all when it leaves the server. False for a player who is not online; at most 32 lines a call. API 41. |
| `open-radial-menu` | `func(player: string, entries: list<radial-entry>, current: u32) -> bool` | Opens a radial menu on one player's screen (Mekanism's mode selector of the Meka-Tool and the configurator, `IRadialSelectorEnum`): 2 to 16 entries around the screen's centre, `current` marked. The pointer is let go so the player can point; a click (a tap on a touch screen) on an entry chooses it and closes the menu, one in the middle or Escape closes it without a choice. The choice comes back as a mod message on channel `<mod id>:radial` whose bytes are the entry's id (UTF-8): register `on_net` to hear it. False for a player who is not online or entries out of bounds. API 42. |
| `open-entity-menu` | `func(player: string, dimension: string, id: u32) -> bool` | Opens the container of one of the mod's own entities (FORK-10 d2, the Robit's inventory) for one player, as a chest of as many rows of 9 as its `container_slots` fill (1 to 6; the slots past them stay out of the menu): the player moves items in and out as in a chest, the entity's slots follow each tick (what the mod's `entities-tick` puts there shows in the menu, and what the player puts there reaches the mod), and the menu closes when the player is more than 4 blocks from its box or the entity goes. Titled with the entity's name, else the kind's (`entity.<ns>.<path>`). False for a player who is not online, an entity of another mod, one with fewer than 9 slots, or one not in the level in place. API 43. |
| `broadcast` | `func(text: string)` | A system message to every player and the server's log, after the mod's name. Counts toward the same 8 per call. |

## `world`

Access: capability `world`.

Capability `world`, server mods only (manifest `api = "0.3"`, experimental): the blocks of the server's loaded chunks, and changes to them through the server's own block updates.

### record `block-change`

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `z` | `s32` |  |
| `state` | `string` | `minecraft:oak_log[axis=y]`; properties left out take the block's defaults, `minecraft:stone` alone is the default state. |

### record `environment`

The sky and weather where a position is: what a solar panel or a wind turbine reads.

| Field | Type | |
| --- | --- | --- |
| `day-time` | `u64` | The ticks of the day, 0 to 23999 (0 is 6:00). |
| `rain` | `f32` | How hard it rains and how hard it thunders in the dimension, 0 to 1 (the dimension has no weather but the overworld). |
| `thunder` | `f32` |  |
| `temperature` | `f32` | The biome's temperature and downfall, and whether it rains or snows in it. |
| `downfall` | `f32` |  |
| `precipitation` | `bool` |  |
| `has-skylight` | `bool` | Whether the dimension has a sky (the overworld). |

### record `collision-box`

One collision box of a block, in blocks, relative to the block's own corner (a full cube is `0,0,0` to `1,1,1`).

| Field | Type | |
| --- | --- | --- |
| `min-x` | `f64` |  |
| `min-y` | `f64` |  |
| `min-z` | `f64` |  |
| `max-x` | `f64` |  |
| `max-y` | `f64` |  |
| `max-z` | `f64` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `get-block` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<string>` | The block state at a position in a dimension (`minecraft:overworld`, ...), as `minecraft:oak_log[axis=y]`, as of this call: none where the server has no chunk loaded there. |
| `get-biome` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<string>` | The biome at a position (`minecraft:plains`, ...), as the server stores it (one per 4 x 4 x 4 cell): none where no chunk is loaded. |
| `get-environment` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<environment>` | The sky and weather at a position, as of this call: none where no chunk is loaded. |
| `get-redstone` | `func(dimension: string, x: s32, y: s32, z: s32) -> u8` | The redstone signal reaching a block of the mod that carries a block entity, 0 to 15 (the strongest of the six neighbours, what a dispenser reads), as of the start of this tick: 0 for any other position. |
| `set-energy-capacity` | `func(dimension: string, x: s32, y: s32, z: s32, capacity: u64) -> result<_, string>` | Sets how much energy a block entity of the mod holds at most, in place of its kind's `energy.capacity` (a machine whose upgrades raise it): 0 gives the kind's back. What it holds above the new capacity is lost. |
| `get-collision` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<list<collision-box>>` | The collision boxes of the block at a position, as an entity bumps into them (an empty list for air, grass or a carpet's top: none): none where the server has no chunk loaded there. |
| `set-blocks` | `func(dimension: string, blocks: list<block-change>) -> result<_, string>` | Sets blocks as `/setblock` does: the old block entity removed, neighbours and shapes updated, light, and every player told. Every state is checked now (an error queues nothing); the changes are made in order after this call returns, before that dimension's next tick; one in a chunk not loaded then is skipped. At most 4096 per call. |
| `set-block-property` | `func(dimension: string, x: s32, y: s32, z: s32, property: string, value: string) -> result<_, string>` | Sets one property of the block at a position (`lit`, `facing`) to a value, as a furnace's tick sets its `lit`: neighbours and players are told, and the block entity **stays** (a block set with `set-blocks` loses it). Made after this call returns, before the dimension's next tick; nothing happens for a block with no such property or value, or one already set. At most 4096 per call. |
| `set-item-faces` | `func(dimension: string, x: s32, y: s32, z: s32, faces: list<list<u32>>) -> result<_, string>` | Sets which slots of the block entity's container each face reaches: the six lists are down, up, north, south, west, east, each of slot numbers (0 to 31). A hopper or dropper put through a face uses its list, as it would the kind's own `faces`, which this replaces for that one entity (kept with it). Made after this call returns; nothing happens for a block with no mod container. API 17. |
| `set-capabilities-enabled` | `func(dimension: string, x: s32, y: s32, z: s32, items: bool, fluids: bool, energy: bool) -> result<_, string>` | Closes or opens, for everything outside the block entity (hoppers and droppers, pipes, tubes and cables, other machines), the three kinds of capability it carries: its `items` container, its `fluids` (tanks) and its `energy`. A closed capability gives and takes nothing through any face, whatever the faces say; the contents, the faces and the mod's own reads and writes are untouched, so a structure that becomes valid again finds everything where it was. All three are open until closed. Kept with the entity. Made after this call returns; nothing happens for a block that is not a mod's block entity. API 24. |
| `break-blocks` | `func(dimension: string, positions: list<tuple<s32, s32, s32>>, tool: string, silk-touch: bool, fortune: u32) -> result<u32, string>` | Breaks up to 4096 blocks as a player holding `tool` would (the Meka-Tool's vein mining and blasting; FORK-34's `mine-block` for many blocks): each loaded block that is not air, a fluid or unbreakable (negative hardness; bedrock, barriers, portals, command blocks) is replaced by air and its drops, worked out with the tool (`silk-touch`, `fortune` 0 to 3 as its enchantments), fall at its place. Returns how many blocks it will break, counted at the call; they are broken after the call returns. The tool is not worn, no experience is given, and protection (spawn, adventure mode) is not asked: the mod checks what it needs. API 39. |
| `set-comparator-output` | `func(dimension: string, x: s32, y: s32, z: s32, level: u8) -> result<_, string>` | Sets the signal a comparator reads from the mod's block entity at the position, 0 to 15 (`getAnalogOutputSignal`: Mekanism's tanks, bins and multiblock valves give the fill level), instead of the one its container gives; the comparators around are told. Kept with the entity. Made after this call returns; nothing happens for a block that is not a mod's block entity. A level past 15 is 15. API 38. |
| `block-hardness` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<f32>` | The block's destroy time (`BlockBehaviour.defaultDestroyTime`, what a pickaxe's speed is divided into; 0 for air and plants, 1.5 for stone, 50 for obsidian), negative for what cannot be broken (bedrock is -1); none where no chunk of the dimension is loaded. A mod that bills energy by hardness (the Laser Tractor Beam, 100 000 J a point) reads it. API 35. |
| `set-menu-text` | `func(dimension: string, x: s32, y: s32, z: s32, texts: list<string>) -> result<_, string>` | Sets the strings the menu of the block entity at the position carries (FORK-20): up to 64, each up to 256 bytes of text, kept with the entity. A screen's `text` widget with `text_data: n` shows string `n`; the clients that have the menu open get them when they change and when they open it. Made after this call returns; nothing happens for a block that is not a mod's block entity. An error for more than 64 strings or one over 256 bytes. API 34. |
| `spawn-particles` | `func(dimension: string, particle: string, x: f64, y: f64, z: f64, spread-x: f32, spread-y: f32, spread-z: f32, speed: f32, count: u32, color: option<u32>, scale: f32, max-distance: f32) -> result<_, string>` | Shows a particle to the players of `dimension` within `max-distance` blocks (at most 128) of `x y z`, as `/particle` does: `ServerLevel.sendParticles`. `particle` is a vanilla particle type (`minecraft:happy_villager`, `minecraft:dust`) or the mod's own (`mymod:spark`, from its `assets/<id>/particles/`). `spread-*` is the box the `count` particles (1 to 512) scatter over, `speed` how fast they move; `color` (0xRRGGBB) and `scale` are the options of `minecraft:dust` and the spell particles. Made after this call returns, at most 1024 requests a call. An error (nothing shown) for a particle the game does not know or numbers that are not finite. API 26. |
| `pull-items` | `func(dimension: string, x: s32, y: s32, z: s32, face: u32, items: list<string>, max: u32) -> result<_, string>` | Pulls items out of the container beside the block entity, on its face `face` (0 to 5: down, up, north, south, west, east), into the block entity's own container through that same face: up to `max` items of the ids in `items` (any when empty), the ones the other container lets out through its side toward this block and this one lets in. Once, after this call returns; a Mekanism sorter calls it every few ticks. API 22. |
| `mine-block` | `func(dimension: string, x: s32, y: s32, z: s32, tool: string, silk-touch: bool, to-x: s32, to-y: s32, to-z: s32, face: u32) -> result<_, string>` | Breaks the block at a position as a player with `tool` (an item id, say `minecraft:netherite_pickaxe`; with `silk-touch` it carries Silk Touch) would, rolls its drops, and puts them in the container at (`to-x`, `to-y`, `to-z`) through that block's face `face` (0 to 5: down, up, north, south, west, east); what does not fit drops where the block was. Nothing happens for air, liquids and unbreakable blocks. Once, after this call returns; at most 64 a call. API 23. |
| `pack-block` | `func(dimension: string, x: s32, y: s32, z: s32, into-x: s32, into-y: s32, into-z: s32, replacement: option<string>) -> result<_, string>` | Packs the block at `x y z`, whatever it is (its state and its block entity as the game saves it: slots, energy, a custom name, a mod's bytes), into the block entity of the mod at `into-x into-y into-z` (Mekanism's cardboard box), and takes it away without a drop or a spill, as `removeBlockEntity` then `removeBlock` do. The box keeps it with its own data, so an item of the box that keeps its data (`keep-data`) carries it. Nothing happens (an error) for air, a block the game cannot take away, or when `into` is not a block entity of a mod, or already holds one. `replacement` is the state (`mymod:box`) put where the block was, as Mekanism's box takes the place of what it packs; `into` may then be the same position (the replacement's own block entity keeps it), or none leaves air there. Once, after this call returns. API 32. |
| `unpack-block` | `func(dimension: string, box-x: s32, box-y: s32, box-z: s32, x: s32, y: s32, z: s32) -> result<_, string>` | Puts back what the box at `box-x box-y box-z` holds, at `x y z`: the block's state and its block entity as they were packed, replacing whatever is there; the box is empty again. Once, after this call returns. An error for a box that holds nothing or a position not loaded. API 32. |
| `set-item-eject` | `func(dimension: string, x: s32, y: s32, z: s32, on: bool) -> result<_, string>` | Turns on or off the block entity pushing items out: every 10 ticks, from the slots of the kind's `outputs` that a face reaches, into the container beside that face (a chest, a hopper, another machine), as Mekanism's machines do with auto-eject on. Kept with the entity. Made after this call returns. API 17. |
| `set-tank-filter` | `func(dimension: string, x: s32, y: s32, z: s32, tank: u32, accepts: list<string>) -> result<_, string>` | Limits what the block entity's tank number `tank` takes in from neighbouring tanks and transmitters: only these substances (`mymod:oxygen`); an empty list takes any again (the kind's default). What the mod puts in the tank itself is not checked. Kept with the entity. Made after this call returns. API 18. |

## `player-control`

Access: capability `player-control`.

Capability `player-control`, server mods only (manifest `api = "0.3"`, experimental): what a player is doing, and moving, charging and sounding for them. It is what an elevator, a waystone or a shop needs.

### record `motion`

What a player is doing as of the last server tick.

| Field | Type | |
| --- | --- | --- |
| `sneaking` | `bool` | The sneak key is held. |
| `on-ground` | `bool` | The player stands on the ground. |
| `jumps` | `u32` | How many times the player has jumped from the ground since the world opened: a mod compares it with the last tick's to see a jump. |
| `xp-level` | `s32` | The experience level (`Player.experienceLevel`). |
| `xp-progress` | `f32` | The progress toward the next level, 0 up to (not including) 1 (`Player.experienceProgress`). |

### record `effect-state`

One effect on a player.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:speed`, a content mod's own `mymod:radiation`. |
| `duration` | `s32` | Ticks left; -1 is infinite. |
| `amplifier` | `s32` | 0 is level I. |

### record `player-state`

What a player's body is doing as of the last server tick: what equipment that reacts to the player (a jetpack, a suit that breathes for them) needs and `motion` does not hold.

| Field | Type | |
| --- | --- | --- |
| `velocity-x` | `f64` | The velocity (`deltaMovement`), blocks a tick. |
| `velocity-y` | `f64` |  |
| `velocity-z` | `f64` |  |
| `jumping` | `bool` | The jump key is held. |
| `sprinting` | `bool` |  |
| `flying` | `bool` | Creative flight is on. |
| `may-fly` | `bool` | The player may fly (`Abilities.mayfly`). |
| `fall-flying` | `bool` | Gliding with an elytra. |
| `swimming` | `bool` |  |
| `in-water` | `bool` | Touching water (`isInWater`). |
| `eyes-in-water` | `bool` | The eyes are in water (`isUnderWater`). |
| `in-lava` | `bool` |  |
| `air` | `s32` | Air left, in ticks (300 is full), and the most they hold. |
| `max-air` | `s32` |  |
| `fire-ticks` | `s32` | Ticks of burning left (0 when not on fire). |
| `blocking` | `bool` | A shield (an item with `blocks_attacks`) is raised and ready to block, past its delay. API 45. |
| `passenger` | `bool` | Riding a vehicle. |
| `sleeping` | `bool` |  |
| `food` | `u32` | Food level 0 to 20 and saturation. |
| `saturation` | `f32` |  |
| `effects` | `list<effect-state>` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `get-motion` | `func(player: string) -> option<motion>` | What the player with this id is doing; none for an id that is not online. |
| `get-state` | `func(player: string) -> option<player-state>` | The body of the player with this id; none for an id that is not online. API 29. |
| `set-food` | `func(player: string, food: u32, saturation: f32) -> result<_, string>` | Sets the player's food level (0 to 20) and saturation (at most the food level, as vanilla's), made after this call returns. An error for a player not online. API 29. |
| `set-air` | `func(player: string, air: s32) -> result<_, string>` | Sets the air the player has left, in ticks (0 to 300, `Entity.setAirSupply`), made after this call returns. An error for a player not online. API 31. |
| `reduce-effect` | `func(player: string, effect: string, ticks: s32) -> result<_, string>` | Shortens the effect `effect` (`minecraft:poison`) on the player by `ticks`, removing it when that leaves nothing (an infinite effect is removed whatever `ticks` is); nothing happens when they do not have it. An error for a player not online or `ticks` under 1. API 31. |
| `reset-fall-distance` | `func(player: string) -> result<_, string>` | Sets the player's fall distance back to 0 (`Entity.resetFallDistance`: a jetpack's flight does not fall). An error for a player not online. API 31. |
| `set-attribute-modifier` | `func(player: string, attribute: string, id: string, amount: f64, operation: string) -> result<_, string>` | Adds the transient attribute modifier `id` (`mymod:speed`) to the player's `attribute` (`minecraft:movement_speed`, `step_height`, `water_movement_efficiency`, `attack_damage`...), replacing one with that id. `operation` is `add_value`, `add_multiplied_base` or `add_multiplied_total`. Transient: it is not saved with the player (Mekanism's equipment modifiers are not either); the mod puts it back when the player joins. An error for a player not online, an attribute that is not an attribute or an amount that is not a number. API 31. |
| `remove-attribute-modifier` | `func(player: string, attribute: string, id: string) -> result<_, string>` | Takes the transient modifier `id` off the player's `attribute`. API 31. |
| `get-look` | `func(player: string) -> option<tuple<f32, f32>>` | Where the player with this id looks, `(yaw, pitch)` in degrees as F3 shows them (yaw 0 faces south, pitch -90 looks straight up); none for an id that is not online. With the eye (the feet and 1.62 blocks, 1.27 sneaking) it gives the face of a block a use met. API 16. |
| `teleport` | `func(player: string, dimension: string, x: f64, y: f64, z: f64, yaw: option<f32>, pitch: option<f32>) -> result<_, string>` | Moves the player, as `/tp` does: the vehicle is left, velocity and fall distance reset. `yaw` and `pitch` stay as they are when none. The move is made after this call returns, before the next tick. An error (nothing queued) for a player not online, a dimension the server does not have, a position that is not finite or lies outside the world border of 30,000,000 blocks. |
| `take-xp` | `func(player: string, points: u32) -> result<_, string>` | Takes `points` experience points from the player, as vanilla's `Player.giveExperiencePoints(-points)` does, going down a level when the level's points run out; never below level 0. Made after this call returns. An error (nothing queued) for a player not online or 0 points. |
| `set-motion` | `func(player: string, x: f64, y: f64, z: f64) -> result<_, string>` | Sets the player's velocity in blocks a tick (each component within -10 and 10), as vanilla's `setDeltaMovement` followed by the motion packet does: the player's client moves with it from the next tick. Made after this call returns. An error for a player not online or a value that is not a number. API 25. |
| `set-allow-flight` | `func(player: string, allowed: bool) -> result<_, string>` | Lets the player fly (`Abilities.mayfly`) whatever their game mode is, or takes it away (a creative or spectator player keeps theirs); the player's client gets the abilities. Kept until the player leaves. An error for a player not online. API 25. |
| `play-sound` | `func(dimension: string, sound: string, x: f64, y: f64, z: f64, volume: f32, pitch: f32)` | Plays a sound event for the players near a position, as `/playsound` does at the `block` category. `sound` is vanilla's (`minecraft:...`) or the mod's own, from its `assets/<id>/sounds.json`. |

## `net`

Access: capability `net`.

Capability `net`, server mods only (manifest `api = "0.3"`, experimental): bytes between a mod's server half and its client half (`net-client`), on channels `<mod id>:<name>` (the mod's own id; the name is `[a-z0-9_./-]{1,64}`). At most 30000 bytes a message and 32 a player a tick; a player on a vanilla client gets nothing.

| Function | Signature | What it does |
| --- | --- | --- |
| `send` | `func(player: string, channel: string, data: list<u8>) -> result<_, string>` | Sends `data` on `channel` to the player with this id, at the end of the tick. Ok means accepted. An error for a channel that isn't the mod's own, too many bytes or too many messages this tick. |

## `net-client`

Access: capability `net-client`.

Capability `net-client`, client mods (API 0.1): the client half's side of `net`.

| Function | Signature | What it does |
| --- | --- | --- |
| `send-to-server` | `func(channel: string, data: list<u8>) -> result<_, string>` | Sends `data` on `channel` to the server (at most 32 a tick). An error when the server carries no mod messages (a vanilla server), before the player has joined, or for a channel that isn't the mod's own. |

## `net-events`

Access: exported by the mod: the game calls it.

What the game calls in a client mod that took `net-client`; a mod built before it has no such export and hears nothing.

| Function | Signature | What it does |
| --- | --- | --- |
| `message` | `func(channel: string, data: list<u8>)` | Bytes the server's half of the mod sent on `channel`. |

## `item-types`

Access: not granted by any capability.

Types shared by the server interfaces; no capability asks for it.

### record `item-stack`

An item stack. `components` is the SNBT of its data components (`{"minecraft:damage":3}`), empty for none.

| Field | Type | |
| --- | --- | --- |
| `item` | `string` |  |
| `count` | `u32` |  |
| `components` | `string` |  |


## `recipe-types`

Access: capability `recipe-types`.

Capability `recipe-types`, server mods only (manifest `api = "0.3"`, experimental): the recipes of types the mod brings. A recipe file `data/<namespace>/recipe/<path>.json` whose `type` is outside `minecraft` is kept as it is written; the mod reads it as it likes, and asks the game which items an ingredient of it names (the game knows the item tags).

### type `item-stack`

`type item-stack = item-stack`

### record `craft-match`

A crafting recipe that a 3 by 3 grid makes.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:iron_pickaxe`, `mymod:crushing_kit`: the recipe's id. |
| `output` | `item-stack` | What it makes. |
| `remaining` | `list<option<item-stack>>` | What stays in each of the nine cells after a craft (`Recipe.getRemainingItems`: a bucket, a bottle, the patterned banner a copy keeps); none for a cell that keeps nothing. |

### record `custom-recipe`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `<namespace>:<path>`, its place under `data/<namespace>/recipe/`. |
| `json` | `string` | The file's JSON. |

### record `cooking-recipe`

A cooking recipe of the game's own kinds (`smelting`, `blasting`, `smoking`, `campfire_cooking`), vanilla's and the mods', as a furnace reads it.

| Field | Type | |
| --- | --- | --- |
| `inputs` | `list<string>` | The items that cook, as `minecraft:iron_ore`. |
| `output` | `string` | What it makes. |
| `count` | `u32` |  |
| `experience` | `f32` |  |
| `time` | `u32` | Ticks a furnace takes (`cookingtime`). |

| Function | Signature | What it does |
| --- | --- | --- |
| `recipes-of-type` | `func(kind: string) -> list<custom-recipe>` | The recipes of type `kind` (`mymod:crushing`), in file order, from the content mods and the world's data packs. Empty for a type nobody uses. |
| `cooking-recipes` | `func(kind: string) -> list<cooking-recipe>` | The cooking recipes of a kind (`smelting`, `blasting`...), in the game's order. |
| `ingredient-items` | `func(ingredient: string) -> list<string>` | The items an ingredient names, in order, once each: `ingredient` is JSON, an item id, a `#tag`, a list of them, or an object with an `item`, a `tag` or `items`. A bare id is `minecraft:`; items the game doesn't know are left out. |
| `burn-time` | `func(item: string) -> option<u32>` | How many ticks `item` burns in a furnace, from the game's fuel registry (`FuelValues`: the `furnace_fuels` data map, data packs and the content mods' own, as a furnace reads them); none for an item that does not burn or is not known. (A smoker or blast furnace burns it the same ticks; those cook twice as fast.) API 27. |
| `crafting-find` | `func(grid: list<option<item-stack>>) -> option<craft-match>` | The first crafting recipe the 3 by 3 `grid` makes, in the game's order, vanilla, data packs and content mods alike (`RecipeManager.getRecipeFor(CRAFTING, ...)`): nine cells, row by row, none for an empty one. None when it makes nothing, or when the grid is not nine cells or has an item the game does not know. Asked while the world is lent (in a tick, a command or a hook of the server). API 28. |
| `crafting-matches` | `func(id: string, grid: list<option<item-stack>>) -> bool` | Whether the crafting recipe `id` accepts the 3 by 3 `grid` (`Recipe.matches`). False for an id the game does not know. API 28. |
| `item-max-stack-size` | `func(item: string) -> option<u32>` | How many of `item` fit a stack (`Item.getMaxStackSize`, the `max_stack_size` component); none for an item the game does not know. API 28. |
| `item-tags` | `func(item: string) -> list<string>` | The item tags `item` is in (`c:ores/iron`, `minecraft:logs`, without the `#`), sorted; empty for an item in none or one the game does not know. The reverse of `ingredient-items` for a tag. API 30. |
| `item-tag-members` | `func(tag: string) -> list<string>` | The items of the item tag `tag` (`c:ingots/iron`; a leading `#` is taken off) in the game's order (the tag files', nested tags in place, first occurrence kept: what `TagKey` lists and Mekanism's Oredictionificator offers as a filter's results); empty for a tag the game does not have. The reverse of `item-tags`. API 40. |

## `entities`

Access: capability `entities`.

Capability `entities`, server mods only (manifest `api = "0.3"`, experimental): the mobs and the other entities of the level the server is simulating. Reading is as of the last server tick. What a mod asks for is made after the call returns, before the next tick; a spawn answers with the new entity's id in `entity-spawned` the tick after (its `ticket` is what the call returned). An entity is named by its `dimension`: another level than the one in place is refused.

### type `item-stack`

`type item-stack = item-stack`

### record `dropped-item`

An item lying in the world (FORK-10 f).

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The item entity's id, for `take-dropped-item`. |
| `stack` | `item-stack` |  |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |

### record `mob`

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` |  |
| `kind` | `string` | `minecraft:zombie`. |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `yaw` | `f32` | Degrees; 0 faces south. |
| `health` | `f32` |  |
| `max-health` | `f32` |  |

### record `mob-options`

What a spawned mob starts with.

| Field | Type | |
| --- | --- | --- |
| `name` | `option<string>` | A name over it, shown to everyone. |
| `health` | `option<f32>` |  |
| `no-ai` | `bool` |  |
| `invisible` | `bool` |  |
| `silent` | `bool` |  |
| `invulnerable` | `bool` |  |
| `no-gravity` | `bool` |  |

### record `display`

A display entity's transformation (`block_display`, `item_display`, `text_display`): a translation, a left rotation (a quaternion `x, y, z, w`), a scale and a right rotation, eased over `interpolation` ticks from what it showed; a move eases over `teleport` ticks (0 to 59).

| Field | Type | |
| --- | --- | --- |
| `translation` | `tuple<f32, f32, f32>` |  |
| `left-rotation` | `tuple<f32, f32, f32, f32>` |  |
| `scale` | `tuple<f32, f32, f32>` |  |
| `right-rotation` | `tuple<f32, f32, f32, f32>` |  |
| `interpolation` | `s32` |  |
| `teleport` | `s32` |  |

### record `damage-result`

What a blow of `damage-entities-in-box` did to one entity.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mob's entity id; 0 for a player. |
| `player` | `option<string>` | The player's UUID, for a player. |
| `applied` | `f32` | The damage it takes (the blow's, as asked; 0 when blocked or immune). |
| `blocked` | `bool` | Whether a shield (raised, facing `source`) or an immunity (a creative or spectator player) kept it off. |

| Function | Signature | What it does |
| --- | --- | --- |
| `mobs` | `func(dimension: string) -> list<mob>` | The living mobs of `dimension` as of the last tick (none for a level that is not the one in place). |
| `spawn-mob` | `func(dimension: string, kind: string, x: f64, y: f64, z: f64, options: mob-options) -> result<u64, string>` | Spawns a mob of `kind` at the position, finalized as a natural spawn is and kept when its chunk unloads. Returns the ticket `entity-spawned` answers with. An error for a position that isn't finite or a kind that isn't a plain id, and past 64 requests in a call. |
| `summon` | `func(dimension: string, kind: string, x: f64, y: f64, z: f64, nbt: string) -> result<u64, string>` | Spawns any entity of `kind` (`minecraft:item_display`) from `nbt` (SNBT of its compound, as `/summon` takes it, at most 8192 bytes; an empty text is `{}`). Returns a ticket as `spawn-mob` does. |
| `move-entity` | `func(dimension: string, id: u32, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> result<_, string>` | Moves an entity Horizon keeps without simulating (a display, a marker, an interaction); a display eases there over its `teleport` ticks. |
| `set-display` | `func(dimension: string, id: u32, display: display) -> result<_, string>` | Sets a display's transformation. |
| `remove-entity` | `func(dimension: string, id: u32) -> result<_, string>` | Removes an entity Horizon keeps without simulating, as `/kill` does. |
| `teleport-mob` | `func(dimension: string, id: u32, x: f64, y: f64, z: f64) -> result<_, string>` | Moves a mob to the position at once, as `/tp` does. |
| `push-mob` | `func(dimension: string, id: u32, vx: f64, vy: f64, vz: f64) -> result<_, string>` | Adds a velocity to a mob's. |
| `set-mob-health` | `func(dimension: string, id: u32, health: f32) -> result<_, string>` | Sets a living mob's health (above 0, cut to its maximum). |
| `kill-mob` | `func(dimension: string, id: u32) -> result<_, string>` | Kills a mob as `/kill` does: it falls and drops its loot. |
| `remove-mob` | `func(dimension: string, id: u32) -> result<_, string>` | Takes a mob out of the world with no death and no loot. |
| `damage-entities-in-box` | `func(dimension: string, min: tuple<f64, f64, f64>, max: tuple<f64, f64, f64>, damage-type: string, amount: f32, source: option<tuple<f64, f64, f64>>) -> list<damage-result>` | Hurts every living mob and player whose feet are in the box (`min` to `max`, at most 64 blocks across) of `dimension` (the level in place) for `amount` of damage and sets it on fire for `amount` ticks unless it does not burn (Mekanism's lasers, `TileEntityBasicLaser`: `mekanism:laser` damage that also ignites). `source` is where the blow comes from (the laser): a player with a shield raised toward it (within 90 degrees of where they face) blocks it and the shield wears, as against an attack. A creative or spectator player is immune (blocked, nothing applied). `damage-type` names the damage: a player takes that type, a mob takes it as generic damage. Returns each one hit with the damage it takes, counted at the call; the blows land after it returns. Mod entities are not in. API 44 (players, `source` and shields: API 45). |
| `items-in-box` | `func(dimension: string, min: tuple<f64, f64, f64>, max: tuple<f64, f64, f64>) -> list<dropped-item>` | The items lying in the box of `dimension` (the level in place; none for another) as of the start of the call: up to 256, the nearest to the box's centre first, whatever their pickup delay. The box is `min` to `max`, each at most 64 blocks across. API 37. |
| `take-dropped-item` | `func(dimension: string, id: u32, count: u32) -> result<_, string>` | Takes `count` items from the item entity `id` (all of them when it has fewer; the entity goes when it has none left), made after this call returns: the stack is the mod's to put somewhere (an entity's slots). An error for a count of 0. API 37. |
| `spawn-mod-entity` | `func(dimension: string, kind: string, x: f64, y: f64, z: f64, data: list<u8>) -> result<u64, string>` | Spawns a living entity of the mod's own kind `kind` (`mymod:robit`, declared in `data/<id>/horizon/entity_kind/`) at the position with the saved bytes `data` (at most 64 KiB), kept and ticked by `entities-tick`. Returns the ticket `entity-spawned` answers with. An error for a kind that is not a plain id or too many bytes. API 36. |
| `set-entity-fire` | `func(dimension: string, id: u32, ticks: u32) -> result<_, string>` | Sets an entity on fire for `ticks` (`Entity.igniteForTicks`: it keeps a longer fire it already has): a mob, or a projectile in flight, which is how a mod's bow lights the arrow it just shot (Flame). API 33. |

## `chunks`

Access: capability `chunks`.

Capability `chunks`, server mods only (manifest `api = "0.3"`, experimental): keeping chunks loaded (a chunk loader), and, with `chunk-loaded`, the hook a mod's world generation runs from.

| Function | Signature | What it does |
| --- | --- | --- |
| `set-forced` | `func(dimension: string, chunk-x: s32, chunk-z: s32, forced: bool) -> result<_, string>` | Keeps the chunk (`chunk-x`, `chunk-z` in chunks) of `dimension` loaded and ticking even with no player near, or lets go of one the mod kept. The mod's own tickets only: it never lets go of a chunk a player forced with `/forceload`. At most 256 chunks a mod. Only the level the server is simulating takes it; the tickets are not saved, so the mod asks again when its blocks load. |

## `player-items`

Access: capability `player-items`.

Capability `player-items`, server mods only (manifest `api = "0.3"`, experimental): what players carry, and hurting them and giving them effects, for items with logic (a jetpack, a tablet that charges, armour with effects). Reading is as of the last server tick; what a mod asks for is made after the call returns, before the next tick.

### type `item-stack`

`type item-stack = item-stack`

| Function | Signature | What it does |
| --- | --- | --- |
| `get-inventory` | `func(player: string) -> option<list<option<item-stack>>>` | The stacks a player carries as of the last tick: slots 0 to 35 are the inventory (0 to 8 the hotbar), 36 the feet, 37 the legs, 38 the chest, 39 the head and 40 the off hand; none for an empty slot. None for a player who isn't online. |
| `set-slot` | `func(player: string, slot: u32, stack: option<item-stack>) -> result<_, string>` | Puts a stack in a slot (none empties it); the player's client gets it. An error for a slot past 40, a player not online or a stack of an item the game doesn't know. |
| `hurt` | `func(player: string, damage-type: string, amount: f32) -> result<_, string>` | Hurts the player by `amount` of a damage type (`minecraft:magic`, `minecraft:fall`...), as vanilla's damage does (armour, absorption and invulnerable time count; creative players take none). |
| `add-effect` | `func(player: string, effect: string, duration-ticks: s32, amplifier: s32) -> result<_, string>` | Gives the player an effect (`minecraft:speed`, or a content mod's own `mymod:radiation`) for `duration-ticks` (-1 is infinite), at `amplifier` (0 is level I). |
| `compare-and-set-slot` | `func(player: string, slot: u32, expected: option<item-stack>, replacement: option<item-stack>) -> result<_, string>` | Puts `replacement` in a slot only if the slot holds `expected` (item, count and components equal; none for an empty slot) at the moment the request is made, after the call returns; otherwise nothing changes. Lets two mods or a mod and the player swap a stack without losing it. Same errors as `set-slot`. API 25. |

## `server-events`

Access: exported by the mod: the game calls it.

What the server calls in a server mod (manifest `api = "0.3"`, experimental). Only the hooks `init` asks for are ever called. The player in each event is as the server saw them when it happened.

### type `player`

`type player = player`

### type `item-stack`

`type item-stack = item-stack`

### record `damage-source`

What hurts a player (`player-damaged`): the damage type and what the game knows of it.

| Field | Type | |
| --- | --- | --- |
| `damage-type` | `string` | `minecraft:fall`, `minecraft:magic`, `minecraft:player_attack`... |
| `causing` | `option<u32>` | The entity that caused it (a mob's id; the shooter of an arrow), none when there is none or it is not known. |
| `is-magic` | `bool` | `minecraft:magic` or `minecraft:indirect_magic`. |
| `is-fire` | `bool` | In the damage type tag `minecraft:is_fire`. |
| `is-fall` | `bool` | In `minecraft:is_fall`. |
| `is-explosion` | `bool` | In `minecraft:is_explosion`. |
| `tags` | `list<string>` | Every damage type tag the type is in (`minecraft:is_fire`, a mod's own...). |

### record `redirect-spec`

A block of the mod that stands for another one (Mekanism's bounding blocks stand for the machine whose model fills them). While byte `flag-at` of its block entity's data is 1, the three whole numbers (little-endian, four bytes each, absolute block coordinates) from byte `pos-at` name that block: a right click on it is a right click on that block (its menu opens, its `block-used` is asked), and breaking it breaks that block, whose drops are the only drops.

| Field | Type | |
| --- | --- | --- |
| `block` | `string` |  |
| `flag-at` | `u32` |  |
| `pos-at` | `u32` |  |

### enum `block-use`

How a mod answers a click on one of its blocks (`block-used`).

| Case | |
| --- | --- |
| `default` |  |
| `pass` |  |
| `consume` |  |

### record `block-pos`

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `z` | `s32` |  |

### variant `argument-kind`

What an argument of a command takes. The server sends each command and its arguments to players' games, whose chat box completes and colours them as vanilla's; the server parses them before the mod's `command` hook gets their values.

| Case | Payload | |
| --- | --- | --- |
| `word` |  | One word: letters, digits and `_ - . +`. |
| `text` |  | The rest of the line, spaces included: only last. |
| `integer` | `tuple<option<s32>, option<s32>>` | A whole number, within the bounds given. |
| `number` |  | A number with or without decimals. |
| `boolean` |  | `true` or `false`. |
| `player` |  | One online player: a name, or `@s` or `@p` (whoever typed it, as vanilla picks for a player). |
| `block-pos` |  | A block position: three whole numbers, each one also `~` or `~n`, relative to whoever typed it. |

### record `argument`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | Shown in the chat box's hint (`<height>`) and the key of its value. |
| `kind` | `argument-kind` |  |
| `optional` | `bool` | Optional arguments come after every required one. |

### record `server-command`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | Without the slash. A name vanilla already uses is refused (vanilla's command runs). |
| `arguments` | `list<argument>` | In order; none: the command takes nothing after its name. Free text is one optional `text` argument. |

### variant `argument-value`

An argument's value, parsed as its kind says.

| Case | Payload | |
| --- | --- | --- |
| `word` | `string` |  |
| `text` | `string` |  |
| `integer` | `s32` |  |
| `number` | `f64` |  |
| `boolean` | `bool` |  |
| `player` | `player` |  |
| `block-pos` | `block-pos` |  |

### record `multiblock-spec`

A block of a multiblock group. The loaded blocks of one group that touch through faces make a structure candidate; the game tells the mod (`multiblock-changed`) whenever the candidates of a group in a dimension change: a block placed, broken or turned into another state. The mod decides whether a candidate is a real structure (a cuboid of the right size, with the right blocks, an empty inside it checks with `world.get-block`) and keeps the structure's shared state in the data, slots, energy and tanks of the member blocks it chooses.

| Field | Type | |
| --- | --- | --- |
| `group` | `string` | The group's name (`mymod:turbine`); the mod's own. A block of several groups names them separated by commas. |

### record `multiblock-member`

| Field | Type | |
| --- | --- | --- |
| `pos` | `block-pos` |  |
| `state` | `string` |  |

### record `structure`

Touching blocks of a group: the members in position order and the least and greatest corner of the box around them.

| Field | Type | |
| --- | --- | --- |
| `members` | `list<multiblock-member>` |  |
| `min` | `block-pos` |  |
| `max` | `block-pos` |  |

### variant `transmitter-medium`

What a transmitter block carries.

| Case | Payload | |
| --- | --- | --- |
| `energy` |  | Energy: the blocks beside the network with an `energy-spec`. |
| `tank` | `string` | Substance of a tank `kind` (`fluid`, `gas`...): the blocks beside the network with a tank of that kind. |
| `items` |  | Items: the item slots of the blocks beside the network. Every ten ticks the game moves up to `throughput` items from the slots the mod blocks beside it let out (their `outputs`, through the face toward the network) into the containers beside it that take them (chests, hoppers, a block's `inputs`), shared round robin. |

### record `transmitter-spec`

A cable or a pipe. Transmitters of one medium that touch through faces both connect through make one network. Every tick, after the mods' `block-entities-tick`, the game moves from the blocks beside the network that give toward it (their face toward it is `output` or `both`) to those that take (`input` or `both`), shared evenly among them, at most `throughput` in all, and each block within its own rates (`max-extract`, `max-receive`, a tank's `max-rate`). A network's throughput is its least transmitter's.

| Field | Type | |
| --- | --- | --- |
| `medium` | `transmitter-medium` |  |
| `throughput` | `u64` |  |

### record `tank`

What a tank holds: an empty substance is an empty tank.

| Field | Type | |
| --- | --- | --- |
| `substance` | `string` |  |
| `amount` | `u64` |  |

### enum `flow-side`

Which way energy, fluid or a chemical may pass through a face of a block.

| Case | |
| --- | --- |
| `none` |  |
| `input` | Energy may flow in through this face. |
| `output` | Energy may flow out through this face. |
| `both` |  |

### record `tank-spec`

One tank of a block. A `kind` is a family of substances that mix with nothing outside it (`fluid`, `gas`, `infusion`...: the mod's own words); a tank holds one substance (`minecraft:water`, `mymod:hydrogen`) at a time, counted in whole units (millibuckets for a fluid). Every tick the game moves substance from each block's output faces into the tank of the block touching it that takes it in through the face that touches back, has the same kind, and is empty or holds the same substance; a tank gives and takes at most `max-rate` a tick.

| Field | Type | |
| --- | --- | --- |
| `kind` | `string` |  |
| `capacity` | `u64` |  |
| `max-rate` | `u64` |  |
| `faces` | `list<flow-side>` | The sides of the six faces: down, up, north, south, west, east. |

### record `energy-spec`

A block's energy store. The game moves energy every tick, after the mods' `block-entities-tick`, from each block's output faces to the block touching it, when that block takes energy in through the face that touches back. The unit is the mod's own (Mekanism's joules); the game only counts whole numbers. Each block gives at most `max-extract` and takes at most `max-receive` a tick, in all faces together.

| Field | Type | |
| --- | --- | --- |
| `capacity` | `u64` |  |
| `max-receive` | `u64` |  |
| `max-extract` | `u64` |  |
| `faces` | `list<flow-side>` | The faces' sides, in this order of faces: down, up, north, south, west, east. A block's mod may change them with `energy-faces` (a machine turned to face another way). |

### record `container-spec`

The slots of a block entity. Slot numbers start at 0; at most 256 slots.

| Field | Type | |
| --- | --- | --- |
| `slots` | `u32` |  |
| `faces` | `list<list<u32>>` | The slots each face reaches, in this order of faces: down, up, north, south, west, east (what a hopper next to that face sees). |
| `inputs` | `list<u32>` | The slots hoppers may put into. |
| `outputs` | `list<u32>` | The slots hoppers may take from. |
| `menu` | `bool` | Using the block opens a menu with these slots, drawn as the content mod's menu type of the same id as the block describes (`data/<ns>/horizon/menu/<block>.json`, with as many slots). |

### record `block-entity-kind`

| Field | Type | |
| --- | --- | --- |
| `block` | `string` | The block's id, `mymod:machine`. |
| `ticks` | `bool` | Include this block's entities in `block-entities-tick`. |
| `container` | `option<container-spec>` | Item slots the block entity holds, as a vanilla container does (players, hoppers and droppers use them; a broken block drops them). None: no slots. |
| `energy` | `option<energy-spec>` | Stored energy, moved between neighbouring blocks by the game (see `energy-spec`). None: the block holds no energy. |
| `tanks` | `list<tank-spec>` | Tanks for fluids and chemicals, moved between neighbouring blocks by the game (see `tank-spec`). |
| `keep-data` | `bool` | The block keeps its entity's data in the item it drops: breaking it gives an item that carries the saved bytes, slots, energy and tanks, and placing that item gives them back (a machine that keeps its contents when picked up). The slots are then not spilled. |
| `transmitter` | `option<transmitter-spec>` | The block is a cable or a pipe (see `transmitter-spec`). None: not. |
| `multiblock` | `option<multiblock-spec>` | The block is a part of a multiblock structure (see `multiblock-spec`). |

### record `registration`

| Field | Type | |
| --- | --- | --- |
| `commands` | `list<server-command>` | Commands players may type. |
| `tick` | `bool` | Call `tick` every server tick (20 per second). |
| `joins` | `bool` | Call `player-joined` and `player-left`. |
| `chat` | `bool` | Call `chat` for each chat message a player sends. |
| `blocks` | `bool` | Call `block-broken`, `block-placed` and `block-used`. |
| `block-entities` | `list<block-entity-kind>` | Blocks of the mod that carry a block entity: its saved bytes (`block-entity.data`), kept with the chunk and ticked by `block-entities-tick`. The block's content `template` must itself have a block entity (a furnace, a chest...), so that breaking the block removes it. |
| `net` | `bool` | Call `net-message` for what players' client halves send. |
| `items` | `bool` | Call `item-used` and `item-hit`. |
| `chunks` | `bool` | Call `chunk-loaded`. |
| `damage` | `bool` | Call `player-damaged` and `item-hit-damage`. |
| `item-release` | `bool` | Call `item-released` and `item-shot` (a bow-like item of the mod, whose template is `minecraft:bow`). |
| `block-break` | `bool` | Call `block-break-start` when a player breaks a block holding an item of the mod. |
| `mod-entities` | `bool` | Call `entities-tick` with the living entities of the mod's own kinds (`data/<id>/horizon/entity_kind/`). |
| `redirects` | `list<redirect-spec>` | Blocks of the mod that stand for another block (FORK-35): a click on them, or breaking them, reaches the block they name. |

### record `block-entity`

A block entity of the mod, as the server holds it.

| Field | Type | |
| --- | --- | --- |
| `dimension` | `string` |  |
| `pos` | `block-pos` |  |
| `state` | `string` | The block state it sits in, `mymod:machine[facing=north]`. |
| `data` | `list<u8>` | What the mod saved last (empty for a new one): at most 65536 bytes. |
| `items` | `list<option<item-stack>>` | Its slots, as many as its container has (none for a block with no container). |
| `energy` | `u64` | The energy it stores (0 for a block with no energy), and the sides of its faces now. |
| `energy-faces` | `list<flow-side>` |  |
| `tanks` | `list<tank>` | Its tanks, in the order of its kind's `tanks`, and their faces' sides now. |
| `tank-faces` | `list<list<flow-side>>` |  |
| `menu-data` | `list<s32>` | The data slots its menu shows (progress, a mode), 16 bits each on the wire. |
| `transmitter-faces` | `list<bool>` | The faces a transmitter connects through now (down, up, north, south, west, east); empty for a block that is not one. |

### record `mod-entity`

A living entity of one of the mod's kinds (FORK-10, the Robit): the game keeps it, its container and its saved bytes, and the mod decides what it does each tick.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` |  |
| `dimension` | `string` |  |
| `kind` | `string` | The kind's id, `mymod:robit`. |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `yaw` | `f32` |  |
| `health` | `f32` |  |
| `data` | `list<u8>` | The mod's own saved bytes (at most 64 KiB). |
| `items` | `list<option<item-stack>>` | The container's slots (the kind's `container_slots`). |
| `energy` | `u64` |  |
| `skin` | `string` | The id of the kind's skin it shows. |
| `name` | `option<string>` | The name it was given, shown over it. |

### record `mod-entity-update`

What the mod makes of an entity in a tick; a none leaves that as it is.

| Field | Type | |
| --- | --- | --- |
| `dimension` | `string` |  |
| `id` | `u32` |  |
| `data` | `option<list<u8>>` |  |
| `items` | `option<list<option<item-stack>>>` | The slots after the call; extra slots past the container's, and stacks of unknown items, are dropped. |
| `energy` | `option<u64>` |  |
| `skin` | `option<string>` | A skin id of the kind; an unknown one is ignored. |
| `name` | `option<option<string>>` | `some(none)` takes the name away. |
| `teleport` | `option<tuple<f64, f64, f64>>` | Moves it at once (as `/tp` does). |
| `path-to` | `option<tuple<f64, f64, f64, f32>>` | Sets where it walks to (FORK-10 c): the point `x, y, z` and a speed multiplier of the kind's `speed` (1 is its walk). It walks there each tick over the ground, climbing a block and dropping up to 3, and stops when it arrives or a wall is in its way; `some` of a new goal replaces the old, and a teleport clears it. The path is a straight line, not a search. |
| `stop` | `bool` | Stops walking. |
| `health` | `option<f32>` | Its health after the call (above 0, cut to the kind's maximum); none leaves it. |
| `remove` | `bool` | Takes it out of the world with no death and no drops. |

### record `block-entity-update`

New bytes and slots for the block entity at `pos` in `dimension`.

| Field | Type | |
| --- | --- | --- |
| `dimension` | `string` |  |
| `pos` | `block-pos` |  |
| `data` | `list<u8>` |  |
| `items` | `option<list<option<item-stack>>>` | The slots after the call; none leaves them as they are. Extra slots past the container's, and stacks of unknown items, are dropped. |
| `energy` | `option<u64>` | The energy after the call (kept within the capacity); none leaves it as it is. The game's transfers after the tick change it again. |
| `energy-faces` | `option<list<flow-side>>` | New sides for the faces; none leaves them as they are. |
| `tanks` | `option<list<tank>>` | The tanks after the call (kept within their capacities); none leaves them as they are. |
| `tank-faces` | `option<list<list<flow-side>>>` | New sides for each tank's faces; none leaves them as they are. |
| `menu-data` | `option<list<s32>>` | The menu's data slots after the call; none leaves them as they are. |
| `transmitter-faces` | `option<list<bool>>` | New connecting faces for a transmitter; none leaves them as they are. |

| Function | Signature | What it does |
| --- | --- | --- |
| `init` | `func() -> result<registration, string>` | Once, after loading. An error keeps the mod off and is logged. |
| `tick` | `func(tick: u64)` | Each server tick, with the server's tick counter. |
| `player-joined` | `func(player: player)` | A player joined the server; they are in `players.online` already. |
| `player-left` | `func(player: player)` | A player left the server; they are no longer in `players.online`. |
| `chat` | `func(player: player, text: string)` | A chat message a player sent, as plain text. |
| `command` | `func(player: player, name: string, args: string, values: list<tuple<string, argument-value>>) -> result<option<string>, string>` | `/name args` typed by `player`: ok(some(text)) is their feedback, err(text) their error in red. `values` holds each argument given, by name, in order (an optional one left out isn't there); a line that doesn't parse never reaches the mod: the player gets vanilla's error. |
| `block-broken` | `func(player: player, pos: block-pos, state: string)` | A player broke the block at `pos` in their dimension: `state` is what was there, as `minecraft:oak_log[axis=y]`. |
| `block-placed` | `func(player: player, pos: block-pos, state: string)` | A player placed `state` at `pos` in their dimension. |
| `block-entities-tick` | `func(tick: u64, entities: list<block-entity>) -> list<block-entity-update>` | Each server tick, for the loaded block entities of blocks `registration.block-entities` marked `ticks` (those of the level the server is simulating; at most 4096 a call, the rest wait their turn). Returns the entities whose bytes changed; the chunk is then saved with them. |
| `entities-tick` | `func(tick: u64, entities: list<mod-entity>) -> list<mod-entity-update>` | Each tick, the living entities of the mod's own kinds (`registration.mod-entities`, FORK-10) in the level in place, up to 256: what the mod answers is applied at once. Not asked of an entity whose chunk is not loaded. API 36. |
| `mod-entity-used` | `func(player: player, entity: mod-entity, sneaking: bool) -> bool` | A player right-clicked one of the mod's own entities (`registration.mod-entities`, FORK-10 d): its box is the kind's `width` by `height`, reached like an interaction entity, before the item in hand is used on it. Answer true to take the click (the arm swings, nothing else happens); false leaves it to the game, which does nothing with it. The first mod that owns the kind answers. |
| `block-break-start` | `func(player: player, stack: item-stack, pos: block-pos, state: string) -> bool` | A player is breaking the block `state` at `pos` holding `stack`, an item of the mod (`registration.block-break`; NeoForge's `IItemExtension.onBlockStartBreak`, where the Meka-Tool mines a vein or blasts an area): asked once the game allowed the break (the item can break blocks; a dig in survival when it ends, a creative click at once), on the PC and on Android (a long press) alike. Answer true to cancel the normal break: the block stays, nothing drops, and the mod breaks what it wants (`world.break-blocks`); false lets the game break it. The first mod that answers true decides. (An export: it has no level; a mod that implements it needs a Horizon that has it, from API 43.) |
| `mod-entity-damaged` | `func(entity: mod-entity, source: damage-source, amount: f32) -> f32` | One of the mod's own entities is about to take `amount` damage from `source` (FORK-10 e; today a player's blow on its box, `minecraft:player_attack`, the damage the player's weapon makes before armour, which a mod entity has none of). Returns the damage to apply: the same to leave it, less to reduce it, 0 to cancel the blow. The first mod that owns the kind answers; a mod that does not implement it takes the damage as it is. At 0 health the entity dies (`mod-entity-died`). |
| `mod-entity-died` | `func(entity: mod-entity)` | An entity of the mod's own kind died of damage (its health reached 0): last look at it, as it was. The game removes it right after; its drops, if it has any, are the mod's to spawn. |
| `menu-button` | `func(player: player, dimension: string, pos: block-pos, id: s32, entity: block-entity) -> option<block-entity-update>` | A player pressed button `id` of the menu of the block entity at `pos` (a `button` widget of the menu type's screen), with the entity as it is. Returns what to change in it, if anything. |
| `net-message` | `func(player: player, channel: string, data: list<u8>)` | What `player`'s client half sent on `channel` (the mod's own), when `registration.net` is set. The bytes are the player's own: check them. |
| `multiblock-changed` | `func(group: string, dimension: string, structures: list<structure>) -> list<block-entity-update>` | The structure candidates of `group` in `dimension` changed (they are all of them, in the order of their least corner, empty when none is left). Called at the end of the tick, with the world lent, before the tick's transfers. Returns what to change in member blocks: updates for other blocks are dropped. |
| `entity-spawned` | `func(ticket: u64, id: option<u32>)` | The answer to a `spawn-mob` or `summon`: the new entity's id, or none when it could not be made (an unknown kind, a chunk not loaded, a monster on peaceful, a mod keeping it out). |
| `chunk-loaded` | `func(dimension: string, chunk-x: s32, chunk-z: s32, new: bool)` | A chunk came into the world (`registration.chunks`): `new` when it was just generated, false when it was read from the save. Its blocks are there (read them with `world.get-block`); what the mod changes with `world.set-blocks` is made at the end of the tick. This is where a mod adds its ores. The chunks around the spawn come before the first tick. |
| `item-used` | `func(player: player, stack: item-stack, pos: option<block-pos>) -> bool` | A player used `stack` (right click), in the air (no `pos`) or on the block at `pos`, when `registration.items` is set. Answer true to take the use: the hand swings and neither the item's own use nor the block's runs after it; false leaves it to the game. It is asked at the moment of the click, so what it asks for through imports is made at the end of the tick; `world.get-block` reads the blocks as they are now. |
| `block-used` | `func(player: player, pos: block-pos, sneaking: bool, stack: option<item-stack>) -> block-use` | What a right click on a block of this mod does (`registration.blocks`), asked at the moment of the click for the mod's own blocks, with the block's own use still to come: `default` leaves it to the game (the block opens its menu or acts as it would), `pass` skips the block's own use and goes on to the held item (so a block in hand is placed against it), `consume` takes the click (the hand swings and nothing else runs). `sneaking` and `stack` (the held item, none for an empty hand) are as at the click. API 24. |
| `player-damaged` | `func(player: player, source: damage-source, amount: f32) -> f32` | A player is about to be hurt (`registration.damage`): where NeoForge's `LivingIncomingDamageEvent` runs, in `hurtServer` once its guards (creative, dead, immune) have passed and before the armour, the enchantments, the resistance and the absorption take their share of `amount`. Returns the damage to go on with: the same to leave it, less to reduce it, 0 (or less) to cancel the hit. The first mod to change it decides. What the mod asks for through imports is made at the end of the tick; `player-items.get-inventory` is as of the last tick. (An export: it has no level; a mod that implements it needs a Horizon that has it.) |
| `item-hit-damage` | `func(player: player, stack: item-stack, kind: string, damage: f32) -> f32` | A player's melee blow is being worked out (`registration.damage`): the attack damage attribute, with the held weapon's modifiers, is `damage` before the attack strength, the critical hit and the enchantments scale it, against an entity of `kind`, holding `stack`. Returns the damage to use. The first mod to change it decides. |
| `item-released` | `func(player: player, stack: item-stack, hand: string, ticks-held: u32) -> bool` | A player let go of a bow-like item (`registration.item-release`; `BowItem.releaseUsing`): `stack` is the held weapon, `hand` is `main` or `off`, `ticks-held` how long the string was drawn. Answer false to cancel the shot (nothing is consumed, no arrow, no sound); true lets the game shoot. The first mod to answer false decides. (An export: it has no level; a mod that implements it needs a Horizon that has it.) |
| `item-shot` | `func(player: player, stack: item-stack, projectiles: list<u32>)` | The arrows of a shot exist (`ItemElectricBow.shoot`'s place in `ProjectileWeaponItem.shoot`): their entity ids, to light with `entities.set-entity-fire` or to charge for. Asked after `item-released` answered true. Nothing to answer. |
| `can-place` | `func(player: player, pos: block-pos, state: string) -> bool` | A player placed the mod's block `state` at `pos` (`registration.blocks`): asked right after the game set it, for the blocks of any mod that asked. Answer false to refuse: the block is taken away again and nothing is used up (a big machine whose model needs the blocks around it free). Where `BlockItem.canPlace` is in the original. |
| `item-hit` | `func(player: player, stack: item-stack, entity: u32, kind: string)` | A player hit the entity `entity` of `kind` holding `stack` (`registration.items`): where `Item.hurtEnemy` starts. It only hears. |
