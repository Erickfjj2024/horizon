# Menu screens drawn by code (capability `screen`, API 56)

A menu type's screen can be drawn by the mod's code instead of its JSON widgets, so a port can follow a Java GUI class
(`GuiXxx`, drawn with `GuiGraphics`) almost line by line. The JSON screens (`data/<mod>/horizon/menu/<name>.json`) keep
working; a menu type picks code with `"code": true` in its `screen`.

Example: `crates/horizon-mod-sdk/examples/screen-demo` (a frequency selector and an item grid), its test
`tests/screen.toml`, and the e2e scenario `python3 tools/agent_control_e2e.py --mode cli --scenario screen`.

## Where the code runs

- In the player's game (the window, also on Android), never on the server. The screens are a second component in the
  `.hmod`, `screens.wasm`, built by `cargo horizon-mod build` from the crate in `screens/` beside `horizon-mod.toml`.
  The manifest lists capability `screen` (experimental: `experimental = ["screen"]`).
- `screens.wasm` imports `log` and `screen-host` only: no files, no network, no world. It reads what the server
  synchronised to this client and sends through the menu's own paths.
- One screen of a mod is open at a time; the component is instantiated at the first one and kept.

## The menu JSON

```json
{
  "slots": 0,
  "data_slots": 3,
  "screen": { "code": true, "own_labels": true, "width": 176, "height": 200, "inventory_y": 118, "slots": [] }
}
```

`width`, `height`, `inventory_x`, `inventory_y`, `hide_inventory`, `own_labels` and the slots' places still come from
here. `open`'s `setup` carries the same fields; in this phase the JSON's win (`setup` is for later phases).

## WIT (`crates/horizon-mod-sdk/wit/horizon.wit`, world `horizon-screen-mod`)

The screen exports `screen-events`: `open(info) -> result<setup>`, `tick()` (20 a second), `render(frame) -> list<u8>`,
`input(event) -> bool` (true: taken; the game does nothing more with it), `close()`. Inputs are in GUI pixels from the
panel's top left: mouse pressed, released, dragged, scrolled, key pressed (`key.keyboard.escape`), char typed, and a
text field's `field-changed` / `field-submitted`.

It imports `screen-host`: `menu-data`, `menu-strings`, `menu-grid`, `slot(i)` (the menu's slots, then the player's 36),
`carried`, `text-width`, `texture-size`, `translate`; `send-button(id)` (a menu button, as the JSON `button`),
`send-text(id, text)` (`<mod>:menu_text`, as a JSON `textfield`'s Enter), `send-grid-click(index, button, shift)`,
`play-click`, `close`.

In Rust: `horizon_mod_sdk::screen` (`ScreenMod`, `export_screen_mod!`, `Canvas`, `host`, `widgets`).

## The command buffer

`render` returns one buffer per frame, written with `Canvas` (methods named after `GuiGraphics`'): `u16` version 1,
`u8` flags (1: animate), then commands, each a `u8` op and its arguments, little-endian; coordinates `f32` GUI pixels
from the panel's top left, colours `u32` 0xAARRGGBB, strings a `u16` length and UTF-8. The format's one source is
`crates/horizon-mod-sdk/src/screen_format.rs` (the game includes it as it is).

| Op | Arguments | As |
|---|---|---|
| `FILL` | rect, argb | `fill` |
| `GRADIENT` | rect, top, bottom | `fillGradient` |
| `BLIT` | texture `<ns>:gui/<path>`, rect, uv (u, v, w, h), texture size, argb | `blit` |
| `SPRITE` | sprite `<ns>:<path>` (under `textures/gui/sprites/`), rect, argb | `blitSprite` (nine-sliced by its `.mcmeta`) |
| `NINE` | texture, rect, borders, uv, texture size, argb | Mekanism's `blitNineSlicedSized` (edges and middle repeated) |
| `TEXT` | text, x, y, argb, flags (shadow 1, centre 2, right 4), scale | `drawString`, Mekanism's `drawScaledText` |
| `ITEM` | item, components, count, x, y, flags (decorations 1), scale | `renderItem` + `renderItemDecorations` |
| `FLUID` | sprite, rect, fill 0..1, argb | a gauge's contents, tiled from the bottom |
| `SCISSOR` / `UNSCISSOR` | rect | `enableScissor` / `disableScissor` (nested, under the pose) |
| `PUSH` / `POP` / `TRANSLATE` / `SCALE` | dx, dy / sx, sy | `pose()` |
| `SLOTS` | | the real slots' frames and items go here; what follows (windows) comes over them |
| `SLOT` | index, x, y, active | the menu's slot `index` this frame; inactive hides it and takes its clicks (a window over it) |
| `AREA` | id, rect, kind (button, tab, row, toggle, area), active, label | hover, `horizon-ctl ui` (`screen-<id>`) and `click` |
| `FIELD` | id, rect, max length, filter (any, digits, id), text, flags (no border 1), colour | a text field the game owns |
| `TOOLTIP` | lines and colours | drawn at the cursor over everything |

Without `SLOT` commands the game draws the slots' frames itself (at `SLOTS`, or after the panel); a screen that places
its slots draws their frames too, as Mekanism's `GuiSlot`s do. The slots' clicks, shift-clicks and drags stay the
game's; a press the screen takes (`input` true) never reaches a slot.

Limits: 8192 commands and 256 KiB a frame, strings of 1024 bytes, 16 levels of pose and scissor, 64 tooltip lines;
fuel per call as the client hooks'. A bad buffer or a crash stops the screen: the panel shows the error, the log has it,
and the menu still closes with Esc.

### When the game calls `render`

After `open`, after each `tick`, after an input, when the cursor moves to another area or over a tooltip, and, when
the last frame set the animate flag, at most 30 times a second. Every other frame draws the last buffer again: a still
screen costs no WebAssembly.

### Measured (desktop x86, release build; `cargo run --release -p horizon-mods --features runtime --example screen_bench -- screendemo.hmod`)

- screen-demo's `render` and its buffer decoded: 4.7 µs (43 commands); in the game, with the menu's data read for it,
  0.05 ms (`horizon-ctl state`'s `screen_code.average_render_ms`).
- Decoding a frame the size of the QIO Dashboard's (600 commands, 29 KB): 28 µs.
- One call into a component that does nothing: 0.15 µs; a call per command would pay at least 88 µs for 600
  commands before passing any argument, three times the buffer's decoding.

On a phone: `horizon-ctl state` gives `screen_code.last_render_ms`, `average_render_ms`, `renders` and `fps` (frames in
the last second with the screen open).

## Text fields

The game owns them: `FIELD` places one, the game keeps its focus, cursor, selection, clipboard, the touch keyboard and
the input method, and the screen hears `field-changed` as the player types and `field-submitted` on Enter (then it
sends what it wants with `send-text`). A text the screen gives a field later (a cleared box) replaces what it shows.
`horizon-ctl type --menu-field ID TEXT --enter` types into it as a player would.

Why not text fields drawn by the screen: each would need the cursor, selection, clipboard and the Android input
method's composition of its own, and the control socket couldn't type into them.

## Windows, tabs, lists (`horizon_mod_sdk::screen::widgets`)

Mekanism's elements with its integer arithmetic, from the panel's top left:

- `Scrollable`, `ScrollList` (`GuiScrollList`: rows, the 6-pixel bar, selection, wheel, thumb drag), `ScrollBar`
  (`GuiScrollBar`, 14 wide, greyed with nothing to scroll).
- `Window` (`GuiWindow`: shadow, base, close button at 6, 6, the 18-pixel title band drags it within the GUI, a press
  outside goes to the inventory or is taken; Esc is the screen's to close it), `SideTab` (a tab at the panel's side,
  inactive while its window is open).
- `background(c, texture, side, rect, size)`: `GuiUtils.renderBackgroundTexture`.

Their textures default to Mekanism's names (`mekanism:gui/scroll_list`, `mekanism:gui/base`...): put Mekanism's PNGs
under `assets/mekanism/textures/gui/`.

## Testing

`[[screen]]` steps in a mod's `tests/*.toml` run its `screens.wasm` without the game (`cargo horizon-mod test`): `open`,
the menu's `data`, `strings`, `grid`, `slots`, `carried`, `ticks`, `mouse`, `click`, `right-click`, `shift`, `drag`,
`scroll`, `key`, `chars`, `field`; `expect.commands` (lines of the frame, each the start of one, in order),
`expect.absent`, `expect.sent` (`button 12`, `text 1 "Home"`, `grid 3 0`, `click`, `close`), `expect.taken`,
`expect.setup`, `expect.error`. The tests' font is 6 pixels a character and 4 a space.

In the game, `horizon-ctl ui` lists the screen's areas (`screen-<id>`) and fields (`menu-field-<id>`) with their
bounds, `screen_commands` (the last frame, one line a command) and `menu_texts`; `click screen-<id>` clicks one.
