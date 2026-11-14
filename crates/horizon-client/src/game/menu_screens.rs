//! Screens of content mods' menu types drawn from their data
//! (`data/<ns>/horizon/menu/<name>.json`'s `screen`, API block 7): a block
//! whose id is the menu type's gets this screen with no code of its own on
//! the client. The pure parts (what a value reads, where a bar fills, which
//! button a press hits, a tooltip's text) are free functions with tests.
use super::{Game, native_hooks::NativeScreen};
use horizon_client::hud::{HudBatch, rgba};
use horizon_mods::{
    content::{ArmorSlots, Direction, ListSource, MenuScreen, Validator, Widget, find_field, parse_color},
    screen::{list_rows, scroll_lists, scroll_var, DRAG_HEIGHT, Locals, Press, press_with, fill_text, neighbor_step, window_origin, filled, fraction, hovered, open_window, press, read, showing},
};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

/// The mods' menu screens by menu type id, set once with the menu types.
pub(super) static MENU_SCREENS: OnceLock<HashMap<String, MenuScreen>> = OnceLock::new();

/// What the open screen keeps on this client alone (the window open, a tab chosen): one screen is open at a time.
static LOCALS: Mutex<Option<Locals>> = Mutex::new(None);

/// A window being dragged: where the cursor was (from the screen's top left) and the offset the window had.
static DRAG: Mutex<Option<([f32; 2], [f32; 2])>> = Mutex::new(None);

/// A press on a list row waits for its release: a click when the pointer stayed, a scroll when it was dragged.
struct ListPress {
    button: i32,
    first: u16,
    start_y: f32,
    start_scroll: i64,
    row_height: f32,
    max_scroll: i64,
    dragged: bool,
}
static LIST_PRESS: Mutex<Option<ListPress>> = Mutex::new(None);

/// The left button was released: a dragged window is let go, and a press on a list row that was not a drag is the menu
/// button it names (returned).
pub(super) fn end_drag() -> Option<i32> {
    if let Ok(mut d) = DRAG.lock() {
        *d = None;
    }
    LIST_PRESS.lock().ok().and_then(|mut p| p.take()).filter(|p| !p.dragged).map(|p| p.button)
}

/// The strings of the mods' menus this client was sent (`world.set-menu-text`), by block position, and the block position of the
/// screen being drawn.
static MENU_TEXT: Mutex<Option<HashMap<[i32; 3], Vec<String>>>> = Mutex::new(None);
static SCREEN_AT: Mutex<Option<[i32; 3]>> = Mutex::new(None);
/// The item grids' entries the server sent, by the position of their menu (API 55).
static MENU_GRID: Mutex<Option<HashMap<[i32; 3], Vec<horizon_mods::screen::GridEntry>>>> = Mutex::new(None);
/// A press on a grid cell waiting to go to the server: its entry, button and Shift.
static GRID_PRESS: Mutex<Option<(u32, u8, bool)>> = Mutex::new(None);

/// A `horizon:menu_grid` message arrived: the item grid of the menu at its position.
pub(super) fn remember_menu_grid(data: &[u8]) {
    let Some((pos, entries)) = horizon_mods::screen::decode_menu_grid(data) else {
        return;
    };
    if let Ok(mut m) = MENU_GRID.lock() {
        let map = m.get_or_insert_with(HashMap::new);
        if map.len() >= MENU_TEXT_BLOCKS && !map.contains_key(&pos) {
            map.clear();
        }
        map.insert(pos, entries);
    }
}

/// The open screen's item grid entries.
pub(super) fn menu_grid() -> Vec<horizon_mods::screen::GridEntry> {
    let Some(at) = SCREEN_AT.lock().ok().and_then(|s| *s) else { return Vec::new() };
    MENU_GRID.lock().ok().and_then(|m| m.as_ref()?.get(&at).cloned()).unwrap_or_default()
}

/// The press on a grid cell made since the last call, for the server.
pub(super) fn take_grid_press() -> Option<(u32, u8, bool)> {
    GRID_PRESS.lock().ok()?.take()
}

/// The readings each graph of the open screen keeps (FORK-20 part 4), by the graph's place, with the tick of the last reading.
static GRAPHS: Mutex<Option<HashMap<(i32, i32), (u64, horizon_mods::screen::GraphHistory)>>> = Mutex::new(None);

/// Most blocks whose strings a client keeps (the oldest are dropped).
const MENU_TEXT_BLOCKS: usize = 64;

/// The text box of the open mod screen that has the focus (FORK-20): its widget `id`, what it takes and the edit box.
pub(super) struct ModField {
    pub(super) id: i32,
    pub(super) validator: Validator,
    pub(super) edit: horizon_client::ui::EditBox,
}
pub(super) static FIELD: Mutex<Option<ModField>> = Mutex::new(None);
/// A text box just took the focus: a touch screen opens its keyboard once (`take_field_keyboard`).
pub(super) static FIELD_KEYBOARD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a text box took the focus since the last call.
pub(super) fn take_field_keyboard() -> bool {
    FIELD_KEYBOARD.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// A new screen opened or the old one closed: no text box has the focus.
pub(super) fn drop_field() {
    if let Ok(mut f) = FIELD.lock() {
        *f = None;
    }
}

/// The list that starts at string `first` on `screen` (its widgets, groups and windows).
fn find_list(screen: &MenuScreen, first: u16) -> Option<&Widget> {
    fn walk(widgets: &[Widget], first: u16) -> Option<&Widget> {
        widgets.iter().find_map(|w| match w {
            Widget::List { first: f, .. } if *f == first => Some(w),
            Widget::Show { widgets, .. } => walk(widgets, first),
            _ => None,
        })
    }
    walk(&screen.widgets, first).or_else(|| screen.windows.iter().find_map(|w| walk(&w.widgets, first)))
}

/// A `horizon:menu_text` message arrived: the strings of the block at its position.
pub(super) fn remember_menu_text(data: &[u8]) {
    let Some((pos, texts)) = horizon_mods::screen::decode_menu_text(data) else {
        return;
    };
    if let Ok(mut m) = MENU_TEXT.lock() {
        let map = m.get_or_insert_with(HashMap::new);
        if map.len() >= MENU_TEXT_BLOCKS && !map.contains_key(&pos) {
            map.clear();
        }
        map.insert(pos, texts);
    }
}

/// The screen drawn now is the one of the block at `pos`.
pub(super) fn set_screen_at(pos: [i32; 3]) {
    if let Ok(mut s) = SCREEN_AT.lock() {
        *s = Some(pos);
    }
}

/// Every string of the open screen's block (a code screen's `menu-strings`).
pub(super) fn menu_strings() -> Vec<String> {
    let Some(at) = SCREEN_AT.lock().ok().and_then(|s| *s) else { return Vec::new() };
    MENU_TEXT.lock().ok().and_then(|m| m.as_ref()?.get(&at).cloned()).unwrap_or_default()
}

/// String `n` of the open screen's block.
fn menu_string(n: usize) -> Option<String> {
    let at = (*SCREEN_AT.lock().ok()?)?;
    MENU_TEXT.lock().ok()?.as_ref()?.get(&at)?.get(n).cloned()
}

/// The id a press that stays on this client gives `native_screen_click`'s `button`: not a menu button.
pub(super) const LOCAL_PRESS: i32 = i32::MIN;

/// A new screen opened: its windows are closed and its variables unset.
pub(super) fn reset_locals() {
    super::code_screens::reset();
    drop_field();
    if let Ok(mut g) = GRAPHS.lock() {
        *g = None;
    }
    if let Ok(mut l) = LOCALS.lock() {
        *l = None;
    }
    end_drag();
}

fn locals() -> Locals {
    LOCALS.lock().ok().and_then(|l| l.clone()).unwrap_or_default()
}

/// Where container slot `slot` of the screen of menu type `id` is while a window of it that holds the slot is open, from the
/// panel's top left (the window's own place, dragged or not, and the slot's place in it); None when no window has it open.
pub(super) fn window_slot_position(id: &str, slot: usize) -> Option<(f32, f32)> {
    let def = screen_of(id)?;
    let l = locals();
    let open = l.window.as_ref()?;
    let window = def.windows.iter().find(|w| &w.id == open)?;
    let at = window.slots.iter().find(|s| usize::from(s.slot) == slot)?;
    let o = horizon_mods::screen::window_origin(window, &l);
    Some((o[0] + at.x as f32, o[1] + at.y as f32))
}

/// Whether a window of the screen of menu type `id` holds container slot `slot` (shown only while it is open).
pub(super) fn window_holds_slot(id: &str, slot: usize) -> bool {
    screen_of(id).is_some_and(|d| d.windows.iter().any(|w| w.slots.iter().any(|s| usize::from(s.slot) == slot)))
}

/// The armor slots of the mods' menu types by id (`armor` in the menu file), set once with the screens.
pub(super) static MENU_ARMOR: OnceLock<HashMap<String, ArmorSlots>> = OnceLock::new();

/// Game slot ids and places of the armor and off-hand slots menu type `id` adds.
fn armor_of(id: &str) -> Vec<(usize, [f32; 2])> {
    let Some(a) = MENU_ARMOR.get().and_then(|m| m.get(id)) else {
        return Vec::new();
    };
    (0..a.count())
        .filter_map(|n| a.at(n).map(|p| (if n < 4 { 500 + n } else { 504 }, p.map(|v| v as f32))))
        .collect()
}

/// The screen of menu type `id`, if it has one.
pub(super) fn screen_of(id: &str) -> Option<&'static MenuScreen> {
    MENU_SCREENS.get()?.get(id)
}

impl Game<'_> {
    /// The layout of block `block`'s screen: its menu type's, if it has one.
    pub(super) fn declared_screen(&self, block: &str) -> Option<NativeScreen> {
        let def = screen_of(block)?;
        Some(NativeScreen {
            width: def.width as f32,
            height: def.height as f32,
            slots: def
                .slots
                .iter()
                .map(|s| s.map(|[x, y]| [x as f32, y as f32]))
                .collect(),
            inventory_y: def.inventory_y as f32,
            inventory_x: def.inventory_x as f32,
            hide_inventory: def.hide_inventory,
            own_labels: def.own_labels,
            armor: armor_of(block),
        })
    }

    /// Draws a list of widgets (showing now) with their origin at `origin`.
    fn draw_widgets(&self, widgets: &[Widget], data: &[i32], locals: &Locals, batch: &mut HudBatch, origin: [f32; 2], cursor: [f32; 2]) {
        let [ox, oy] = origin;
        for w in showing(widgets, data, locals) {
            match w {
                Widget::Bar { x, y, w, h, value, max, color, direction, .. } => {
                    let rect = [ox + *x as f32, oy + *y as f32, *w as f32, *h as f32];
                    // The well behind the fill.
                    self.native_fill(batch, rect, 0xFF37_3737);
                    let frac = fraction(read(value, data), read(max, data));
                    if frac > 0.0 {
                        let argb = parse_color(color).unwrap_or(0xFFFF_FFFF);
                        self.native_fill(batch, filled(rect, frac, *direction), argb);
                    }
                }
                Widget::Sprite { texture, x, y, w, h, src, value, max, direction, tints, tint_by, .. } => {
                    let Some([sw, sh]) = self.native_sprite_size(texture) else {
                        continue;
                    };
                    let src = src.map_or([0.0, 0.0, sw, sh], |s| s.map(|v| v as f32));
                    let dst = [ox + *x as f32, oy + *y as f32, *w as f32, *h as f32];
                    let (src, dst) = match (value, max) {
                        (Some(v), Some(m)) => {
                            let frac = fraction(read(v, data), read(m, data));
                            if frac <= 0.0 {
                                continue;
                            }
                            // The same part of the sprite as of the box.
                            (filled(src, frac, *direction), filled(dst, frac, *direction))
                        }
                        _ => (src, dst),
                    };
                    let tint = tint_by
                        .as_ref()
                        .and_then(|v| usize::try_from(read(v, data)).ok())
                        .and_then(|i| i.checked_sub(1))
                        .and_then(|i| tints.get(i))
                        .and_then(|c| parse_color(c))
                        .map_or(u32::MAX, |argb| {
                            let [a, r, g, b] = argb.to_be_bytes();
                            rgba(r, g, b, a)
                        });
                    self.native_sprite_tinted(batch, texture, src, dst, tint);
                }
                Widget::Text { x, y, text, color, scale, text_data } => {
                    let rgb = color.as_deref().and_then(parse_color).unwrap_or(0xFF40_4040);
                    let text = match text_data {
                        Some(n) => menu_string(usize::from(*n)).unwrap_or_default(),
                        None => fill_text(text, data),
                    };
                    let at = [ox + *x as f32, oy + *y as f32];
                    let start = batch.vertices.len();
                    self.native_text(batch, &text, at, rgb, false);
                    if let Some(s) = scale {
                        let f = f32::from(*s) / 100.0;
                        batch.scale_since(start, at[0], at[1], f, f);
                    }
                }
                Widget::Swatch { x, y, w, h, value, colors, .. } => {
                    let i = read(value, data).max(0) as usize;
                    let argb = colors.get(i).or(colors.last()).and_then(|c| parse_color(c)).unwrap_or(0xFF80_8080);
                    self.native_fill(batch, [ox + *x as f32, oy + *y as f32, *w as f32, *h as f32], argb);
                }
                // Buttons draw nothing of their own (a sprite or a swatch under them does); a group was opened by `showing`.
                Widget::Gauge { x, y, w, h, value, max, kind, types, direction, .. } => {
                    let k = read(kind, data).max(0) as usize;
                    let frac = fraction(read(value, data), read(max, data));
                    if let Some(t) = k.checked_sub(1).and_then(|i| types.get(i)).filter(|_| frac > 0.0) {
                        // The batch packs a colour as `rgba` does (red lowest); `parse_color` gives 0xAARRGGBB.
                        let [a, r, g, b] = parse_color(&t.tint).unwrap_or(u32::MAX).to_be_bytes();
                        let tint = rgba(r, g, b, a);
                        let rect = [ox + *x as f32, oy + *y as f32, *w as f32, *h as f32];
                        if matches!(direction, Direction::Right) {
                            self.draw_tiled_left_right(batch, &t.texture, rect, frac, tint);
                        } else {
                            self.draw_tiled_bottom_up(batch, &t.texture, rect, frac, tint);
                        }
                    }
                }
                Widget::Neighbor { x, y, side, facing } => {
                    let named = facing.as_ref().map(|v| match read(v, data) {
                        1 => "east",
                        2 => "south",
                        3 => "west",
                        _ => "north",
                    });
                    if let Ok(icon) = self.neighbor_icon(*side, named) {
                        self.native_item(batch, &icon, [ox + *x as f32, oy + *y as f32], 16.0);
                    }
                }
                Widget::Item { x, y, item } => {
                    self.native_item(batch, item, [ox + *x as f32, oy + *y as f32], 16.0);
                }
                Widget::Button { x, y, w, h, hover: Some(c), .. } => {
                    let on = cursor[0] >= ox + *x as f32 && cursor[1] >= oy + *y as f32 && cursor[0] < ox + (*x + *w) as f32 && cursor[1] < oy + (*y + *h) as f32;
                    if on && let Some(argb) = parse_color(c) {
                        self.native_fill(batch, [ox + *x as f32, oy + *y as f32, *w as f32, *h as f32], argb);
                    }
                }
                Widget::Graph { x, y, w, h, series, samples, min_scale, .. } => {
                    let rect = [ox + *x as f32, oy + *y as f32, *w as f32, *h as f32];
                    let values: Vec<i64> = series.iter().map(|s| read(&s.value, data)).collect();
                    let Ok(mut graphs) = GRAPHS.lock() else {
                        continue;
                    };
                    let map = graphs.get_or_insert_with(HashMap::new);
                    let entry = map
                        .entry((*x, *y))
                        .or_insert_with(|| (u64::MAX, horizon_mods::screen::GraphHistory::new(series.len(), usize::from(*samples))));
                    // One reading a tick while the screen is open (drawing runs every frame).
                    let tick = self.ticks;
                    if entry.0 != tick && !data.is_empty() {
                        entry.0 = tick;
                        entry.1.push(&values);
                    }
                    let lines = entry.1.points(rect, *min_scale);
                    drop(graphs);
                    for (points, s) in lines.iter().zip(series) {
                        let argb = parse_color(&s.color).unwrap_or(0xFFFF_FFFF);
                        // Each step drawn as 1-pixel columns from one point to the next, so a steep rise stays joined.
                        for pair in points.windows(2) {
                            let ([x0, y0], [x1, y1]) = (pair[0], pair[1]);
                            let cols = (x1 - x0).max(1.0).ceil() as i32;
                            for c in 0..cols {
                                let t0 = c as f32 / cols as f32;
                                let t1 = (c + 1) as f32 / cols as f32;
                                let ya = y0 + (y1 - y0) * t0;
                                let yb = y0 + (y1 - y0) * t1;
                                let top = ya.min(yb).clamp(rect[1], rect[1] + rect[3] - 1.0);
                                let bottom = ya.max(yb).clamp(rect[1], rect[1] + rect[3] - 1.0);
                                self.native_fill(batch, [x0 + c as f32, top, 1.0, (bottom - top).max(1.0)], argb);
                            }
                        }
                    }
                }
                Widget::List { x, y, w, h, row_height, first, count, source, color, .. } => {
                    let total = read(count, data);
                    let rows = list_rows(*h, *row_height);
                    let top = locals.scroll(*first, total, rows);
                    let rgb = color.as_deref().and_then(parse_color).unwrap_or(0xFF40_4040);
                    for r in 0..rows {
                        let index = top + r;
                        if index >= total {
                            break;
                        }
                        let row = [ox + *x as f32, oy + (*y + r as i32 * *row_height) as f32, *w as f32, *row_height as f32];
                        if cursor[0] >= row[0] && cursor[0] < row[0] + row[2] && cursor[1] >= row[1] && cursor[1] < row[1] + row[3] {
                            self.native_fill(batch, row, 0x4080_8080);
                        }
                        let Some(text) = menu_string(usize::from(*first) + index as usize) else {
                            continue;
                        };
                        let pad = if matches!(source, ListSource::Items) {
                            self.native_item(batch, &text, [row[0] + 1.0, row[1] + (row[3] - 16.0) / 2.0], 16.0);
                            19.0
                        } else {
                            2.0
                        };
                        self.native_text(batch, &text, [row[0] + pad, row[1] + (row[3] - 8.0) / 2.0], rgb, false);
                    }
                    // A thin bar at the right edge when there is more than shows.
                    if total > rows {
                        let bar_h = (*h as f32 * rows as f32 / total as f32).max(6.0);
                        let travel = *h as f32 - bar_h;
                        let at = if total > rows { travel * top as f32 / (total - rows) as f32 } else { 0.0 };
                        self.native_fill(batch, [ox + (*x + *w) as f32 - 2.0, oy + *y as f32 + at, 2.0, bar_h], 0xFF80_8080);
                    }
                }
                Widget::ItemGrid { x, y, columns, rows, cell } => {
                    let entries = menu_grid();
                    let total = entries.len() as i64;
                    let top = horizon_mods::screen::grid_top(locals, *columns, *rows, total);
                    let c = f32::from(*cell);
                    let (gx, gy) = (ox + *x as f32, oy + *y as f32);
                    for r in 0..i64::from(*rows) {
                        for col in 0..i64::from(*columns) {
                            let cell_rect = [gx + col as f32 * c, gy + r as f32 * c, c, c];
                            // A slot's frame: dark top left, light bottom right, grey inside.
                            self.native_fill(batch, cell_rect, 0xFF37_3737);
                            self.native_fill(batch, [cell_rect[0] + 1.0, cell_rect[1] + 1.0, c - 1.0, c - 1.0], 0xFFFF_FFFF);
                            self.native_fill(batch, [cell_rect[0] + 1.0, cell_rect[1] + 1.0, c - 2.0, c - 2.0], 0xFF8B_8B8B);
                            let index = (top + r) * i64::from(*columns) + col;
                            let Some(e) = entries.get(index as usize) else { continue };
                            let inset = ((c - 16.0) / 2.0).max(0.0);
                            self.native_item(batch, &e.item, [cell_rect[0] + inset, cell_rect[1] + inset], c.min(16.0));
                            let count = horizon_mods::screen::abbreviate(e.count);
                            let width = count.len() as f32 * 6.0;
                            self.native_text(batch, &count, [cell_rect[0] + c - width, cell_rect[1] + c - 8.0], 0xFFFF_FFFF, true);
                            if cursor[0] >= cell_rect[0] && cursor[0] < cell_rect[0] + c && cursor[1] >= cell_rect[1] && cursor[1] < cell_rect[1] + c {
                                self.native_fill(batch, [cell_rect[0] + 1.0, cell_rect[1] + 1.0, c - 2.0, c - 2.0], 0x80FF_FFFF);
                            }
                        }
                    }
                    // The bar at its right: the place of the rows shown among all of them.
                    let lines = (total + i64::from(*columns) - 1) / i64::from(*columns);
                    let h = f32::from(*rows) * c;
                    self.native_fill(batch, [gx + f32::from(*columns) * c + 1.0, gy, 4.0, h], 0xFF37_3737);
                    if lines > i64::from(*rows) {
                        let bar = (h * f32::from(*rows) / lines as f32).max(6.0);
                        let at = (h - bar) * top as f32 / (lines - i64::from(*rows)) as f32;
                        self.native_fill(batch, [gx + f32::from(*columns) * c + 1.0, gy + at, 4.0, bar], 0xFFC6_C6C6);
                    }
                }
                Widget::TextField { x, y, w, h, id, max_length, text_data, .. } => {
                    let rect = [ox + *x as f32, oy + *y as f32, *w as f32, *h as f32];
                    let mut focused = false;
                    if let Ok(mut f) = FIELD.lock()
                        && let Some(field) = f.as_mut().filter(|f| f.id == *id)
                    {
                        let edit = &mut field.edit;
                        [edit.x, edit.y, edit.width, edit.height] = rect;
                        edit.focused = true;
                        edit.draw(batch, &self.gui.atlas, None);
                        focused = true;
                    }
                    if !focused {
                        let mut edit = horizon_client::ui::EditBox::new(rect[0], rect[1], rect[2], rect[3]);
                        edit.max_length = usize::from(*max_length);
                        edit.set_value(&text_data.and_then(|n| menu_string(usize::from(n))).unwrap_or_default());
                        edit.draw(batch, &self.gui.atlas, None);
                    }
                }
                Widget::Button { .. } | Widget::Show { .. } => {}
            }
        }
    }

    /// Fills the bottom `frac` of rectangle `rect` with `texture` tiled from its bottom left corner and tinted (a tank's
    /// contents, as `GuiUtils.drawTiledSprite` with `TilingDirection.UP_RIGHT` draws them).
    pub(super) fn draw_tiled_bottom_up(&self, batch: &mut HudBatch, texture: &str, rect: [f32; 4], frac: f32, tint: u32) {
        let Some(sprite) = self.gui.atlas.sprites.get(texture).copied() else {
            return;
        };
        let [x, y, w, h] = rect;
        let (tw, th) = (sprite.w as f32, sprite.h as f32);
        if tw < 1.0 || th < 1.0 {
            return;
        }
        let filled = (h * frac).round().clamp(1.0, h);
        let (du, dv) = ((sprite.u1 - sprite.u0) / tw, (sprite.v1 - sprite.v0) / th);
        let bottom = y + h;
        let mut done = 0.0;
        while done < filled {
            let rows = (filled - done).min(th);
            let mut across = 0.0;
            while across < w {
                let cols = (w - across).min(tw);
                let mut r = sprite;
                // The tile's bottom `rows` and left `cols`, so a partial row at the top shows the bottom of the sprite.
                r.u1 = r.u0 + cols * du;
                r.v0 = sprite.v1 - rows * dv;
                batch.rect_uv(x + across, bottom - done - rows, cols, rows, r, tint);
                across += cols;
            }
            done += rows;
        }
    }

    /// Fills the left `frac` of rectangle `rect` with `texture` tiled from its left edge and tinted (a horizontal bar).
    fn draw_tiled_left_right(&self, batch: &mut HudBatch, texture: &str, rect: [f32; 4], frac: f32, tint: u32) {
        let Some(sprite) = self.gui.atlas.sprites.get(texture).copied() else {
            return;
        };
        let [x, y, w, h] = rect;
        let (tw, th) = (sprite.w as f32, sprite.h as f32);
        if tw < 1.0 || th < 1.0 {
            return;
        }
        let filled = (w * frac).round().clamp(1.0, w);
        let (du, dv) = ((sprite.u1 - sprite.u0) / tw, (sprite.v1 - sprite.v0) / th);
        let mut across = 0.0;
        while across < filled {
            let cols = (filled - across).min(tw);
            let mut down = 0.0;
            while down < h {
                let rows = (h - down).min(th);
                let mut r = sprite;
                r.u1 = r.u0 + cols * du;
                r.v1 = r.v0 + rows * dv;
                batch.rect_uv(x + across, y + down, cols, rows, r, tint);
                down += rows;
            }
            across += cols;
        }
    }

    /// Draws block `block`'s declared screen at `origin`; false when it has none.
    pub(super) fn draw_declared_screen(
        &self,
        block: &str,
        data: &[i32],
        batch: &mut HudBatch,
        origin: [f32; 2],
    ) -> bool {
        let Some(def) = screen_of(block) else {
            return false;
        };
        // Drawn by the mod's code (capability `screen`, API 56); if that cannot run, the JSON screen below, with why.
        if def.code && self.code_screen_draw(block, batch, origin) {
            return true;
        }
        let [ox, oy] = origin;
        let locals = locals();
        if let Some(texture) = &def.texture
            && let Some([sw, sh]) = self.native_sprite_size(texture)
        {
            self.native_sprite(
                batch,
                texture,
                [0.0, 0.0, sw, sh],
                [ox, oy, def.width as f32, def.height as f32],
            );
        }
        let cursor = [self.cursor.0, self.cursor.1];
        self.draw_widgets(&def.widgets, data, &locals, batch, origin, cursor);
        // The window open, over the panel.
        if let Some(window) = open_window(def, &locals) {
            let origin = window_origin(window, &locals);
            let at = [ox + origin[0], oy + origin[1]];
            if let Some(texture) = &window.texture
                && let Some([sw, sh]) = self.native_sprite_size(texture)
            {
                self.native_sprite(
                    batch,
                    texture,
                    [0.0, 0.0, sw, sh],
                    [at[0], at[1], window.width as f32, window.height as f32],
                );
            }
            self.draw_widgets(&window.widgets, data, &locals, batch, at, cursor);
        }
        // A code screen that could not run: this JSON screen stands in for it, and says why.
        if def.code
            && let Some(why) = super::code_screens::failure(block)
        {
            let text = horizon_client::text::Text::literal(format!("Screen by code unavailable: {why}"));
            batch.text_styled(&self.gui.atlas, &text, ox + 2.0, oy + def.height as f32 + 2.0, rgba(255, 85, 85, 255), true);
        }
        true
    }

    /// The rectangle `[x, y, w, h]` (from the screen's top left) of the window open on the screen shown now, if any.
    pub(super) fn open_window_rect(&self) -> Option<[f32; 4]> {
        let block = self.native_screen_block()?;
        let screen = screen_of(&block)?;
        let w = open_window(screen, &locals())?;
        let [x, y] = window_origin(w, &locals());
        Some([x, y, w.width as f32, w.height as f32])
    }

    /// The item of the block beside the machine whose screen is open, on its relative `side`; None for air or a
    /// block without an item.
    fn neighbor_icon(&self, side: u8, named: Option<&str>) -> Result<String, char> {
        let [x, y, z] = self.native_screen_pos().ok_or('P')?;
        let facing = match named {
            Some(f) => f.to_owned(),
            None => self.native_block_property([x, y, z], "facing").unwrap_or_default(),
        };
        let [dx, dy, dz] = neighbor_step(&facing, side);
        let id = self.native_block([x + dx, y + dy, z + dz]);
        if id == "minecraft:air" || id.is_empty() { Err('B') } else { Ok(id) }
    }

    /// The cursor moved with a window held: it follows (kept on the screen's near area).
    pub(super) fn drag_declared_window(&self, at: [f32; 2]) {
        self.code_screen_motion();
        if let Ok(mut p) = LIST_PRESS.lock()
            && let Some(p) = p.as_mut()
        {
            let moved = p.start_y - at[1];
            if moved.abs() > 4.0 {
                p.dragged = true;
            }
            if p.dragged
                && let Ok(mut l) = LOCALS.lock()
                && let Some(now) = l.as_mut()
            {
                let next = (p.start_scroll + (moved / p.row_height).round() as i64).clamp(0, p.max_scroll);
                now.vars.insert(scroll_var(p.first), next);
            }
        }
        let Ok(drag) = DRAG.lock() else { return };
        let Some((from, offset)) = *drag else { return };
        if let Ok(mut l) = LOCALS.lock()
            && let Some(now) = l.as_mut()
        {
            now.offset = [
                (offset[0] + at[0] - from[0]).clamp(-200.0, 400.0),
                (offset[1] + at[1] - from[1]).clamp(-100.0, 300.0),
            ];
        }
    }

    /// Whether the open window covers container slot `slot` (16 by 16 at its position): a window is over everything, so
    /// the slots under it are neither drawn nor pressed (as Mekanism's windows take the clicks that land on them).
    pub(super) fn window_covers_slot(&self, slot: usize) -> bool {
        // A code screen hides a slot it says is inactive (a window over it).
        if slot >= 300
            && let Some((_, _, active)) = self.code_screen_slot(slot - 300)
        {
            return !active;
        }
        let Some([wx, wy, ww, wh]) = self.open_window_rect() else {
            return false;
        };
        // The window's own slots (an upgrade window's) sit inside it and are the ones it is there for.
        if self.native_window_slot(slot) {
            return false;
        }
        let (sx, sy) = self.slot_position_for_window(slot);
        sx + 16.0 > wx && sx < wx + ww && sy + 16.0 > wy && sy < wy + wh
    }

    /// What a press at `at` does on block `block`'s declared screen: a menu button for the server, or something done
    /// here (a window opened); `alt` is a right press.
    pub(super) fn declared_screen_press(&self, block: &str, data: &[i32], at: [f32; 2], alt: bool) -> Option<i32> {
        let screen = screen_of(block)?;
        if screen.code && self.code_screen_running() {
            return (self.code_screen_press(at, alt) == Some(true)).then_some(super::code_screens::CODE_PRESS);
        }
        let mut l = LOCALS.lock().ok()?;
        let mut now = l.take().unwrap_or_default();
        now.grid_total = menu_grid().len() as i64;
        // A press on the title edge of an open window, over none of its buttons, takes the window to drag.
        let grab = open_window(screen, &now).map(|w| {
            let o = window_origin(w, &now);
            let inside = at[0] >= o[0] && at[0] < o[0] + w.width as f32 && at[1] >= o[1] && at[1] < o[1] + DRAG_HEIGHT;
            (inside, now.offset)
        });
        let result = press_with(screen, data, &mut now, at, alt, self.has_shift_down());
        *l = Some(now);
        // A press anywhere but on the text box that has the focus lets it go.
        let on_field = matches!(result, Some(Press::Field(_)));
        if !on_field {
            drop_field();
        }
        let result = result?;
        if let Press::Row { button, first } = result
            && let Some(Widget::List { y, h, row_height, count, .. }) = find_list(screen, first)
        {
            let rows = list_rows(*h, *row_height);
            let total = read(count, data);
            let locals = locals();
            let _ = y;
            if let Ok(mut p) = LIST_PRESS.lock() {
                *p = Some(ListPress {
                    button,
                    first,
                    start_y: at[1],
                    start_scroll: locals.scroll(first, total, rows),
                    row_height: *row_height as f32,
                    max_scroll: (total - rows).max(0),
                    dragged: false,
                });
            }
            return Some(LOCAL_PRESS);
        }
        if let Press::Field(id) = result {
            if let Some(Widget::TextField { max_length, validator, text_data, .. }) = find_field(screen, id) {
                let mut edit = horizon_client::ui::EditBox::new(0.0, 0.0, 100.0, 20.0);
                edit.max_length = usize::from(*max_length);
                edit.set_value(&text_data.and_then(|n| menu_string(usize::from(n))).unwrap_or_default());
                edit.move_cursor_to_end(false);
                if let Ok(mut f) = FIELD.lock() {
                    *f = Some(ModField { id, validator: *validator, edit });
                    FIELD_KEYBOARD.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            return Some(LOCAL_PRESS);
        }
        if let (Press::Local, (true, offset), false) = (&result, grab.unwrap_or((false, [0.0; 2])), alt)
            && let Ok(mut d) = DRAG.lock()
        {
            *d = Some((at, offset));
        }
        match result {
            Press::Send(id) => Some(id),
            // A grid cell: its entry goes to the server (`menu-grid-click`) once the press is handled.
            Press::Grid(index) => {
                if let Ok(mut g) = GRID_PRESS.lock() {
                    *g = Some((index, u8::from(alt), self.has_shift_down()));
                }
                Some(LOCAL_PRESS)
            }
            Press::Local | Press::Field(_) | Press::Row { .. } => Some(LOCAL_PRESS),
        }
    }

    /// The wheel over a list of the open mod screen scrolls it (FORK-20); true when a list took it.
    pub(super) fn mod_list_scroll(&mut self, delta: f32) -> bool {
        let Some((block, data, at)) = self.native_screen_at() else {
            return false;
        };
        let Some(screen) = screen_of(&block) else {
            return false;
        };
        if screen.code && self.code_screen_running() {
            return self.code_screen_scroll(delta) == Some(true);
        }
        let Ok(mut l) = LOCALS.lock() else {
            return false;
        };
        let mut now = l.take().unwrap_or_default();
        now.grid_total = menu_grid().len() as i64;
        let took = scroll_lists(screen, &data, &mut now, at, delta.signum() as i64 * (delta.abs().ceil() as i64).max(1));
        *l = Some(now);
        took
    }

    /// The left button went up: a click on a list row that was not a drag is its menu button, sent to the server.
    pub(super) fn end_declared_drag(&mut self) {
        if let Some(button) = end_drag() {
            self.native_menu_button_send(button);
        }
        self.code_screen_release();
        self.code_screen_actions();
    }

    /// Whether a text box of the open mod screen has the focus (FORK-20).
    pub(super) fn mod_field_typing(&self) -> bool {
        self.screen == super::Screen::Inventory && FIELD.lock().is_ok_and(|f| f.is_some())
    }

    /// The focused text box's text, for the touch keyboard's first value.
    pub(super) fn mod_field_value(&self) -> String {
        FIELD.lock().ok().and_then(|mut f| f.as_mut().map(|f| {
            f.edit.move_cursor_to_end(false);
            f.edit.value.clone()
        })).unwrap_or_default()
    }

    /// Typed text (what the validator allows) or a backspace in the focused text box.
    pub(super) fn mod_field_text(&mut self, text: Option<&str>, backspace: bool) {
        let Ok(mut f) = FIELD.lock() else {
            return;
        };
        let Some(field) = f.as_mut() else {
            return;
        };
        if backspace {
            field.edit.delete_chars(-1);
        } else if let Some(t) = text {
            let allowed: String = t.chars().filter(|c| field.validator.allows(*c)).collect();
            if !allowed.is_empty() {
                field.edit.insert_text(&allowed);
            }
        }
        let (id, value) = (field.id, field.edit.value.clone());
        drop(f);
        self.code_screen_field(id, &value, false);
    }

    /// A key in the focused text box (cursor, delete, select...): Enter confirms, and false lets the key go on.
    pub(super) fn mod_field_key(&mut self, key: winit::keyboard::KeyCode) -> bool {
        if key == winit::keyboard::KeyCode::Enter || key == winit::keyboard::KeyCode::NumpadEnter {
            self.mod_field_confirm();
            return true;
        }
        let (ctrl, shift) = (self.has_control_down(), self.has_shift_down());
        let before = FIELD.lock().ok().and_then(|f| f.as_ref().map(|f| (f.id, f.edit.value.clone())));
        let took = FIELD.lock().ok().and_then(|mut f| f.as_mut().map(|f| f.edit.key(key, ctrl, shift))).unwrap_or(false);
        let after = FIELD.lock().ok().and_then(|f| f.as_ref().map(|f| (f.id, f.edit.value.clone())));
        if let Some((id, value)) = after
            && before.as_ref().map(|b| &b.1) != Some(&value)
        {
            self.code_screen_field(id, &value, false);
        }
        took
    }

    /// The buttons and text boxes of the open mod screen, in GUI pixels: what the control socket's `ui` lists.
    pub(super) fn declared_controls(&self) -> Vec<horizon_mods::screen::Control> {
        let Some((block, data, [ox, oy])) = self.native_screen_origin() else {
            return Vec::new();
        };
        let Some(screen) = screen_of(&block) else {
            return Vec::new();
        };
        let mut controls = horizon_mods::screen::controls(screen, &data, &locals());
        for c in &mut controls {
            c.rect[0] += ox;
            c.rect[1] += oy;
        }
        controls
    }

    /// The `text` widgets of the open mod screen as they are drawn now (the open window's, then the screen's), each with its
    /// `text_data` (None for a fixed or data-filled text) and its place in GUI pixels: what the control socket's `ui` reports
    /// as `menu_texts`, so a test sees whether the strings the server sent reached this client.
    pub(super) fn declared_texts(&self) -> Vec<(Option<u16>, String, [f32; 2])> {
        let Some((block, data, [ox, oy])) = self.native_screen_origin() else {
            return Vec::new();
        };
        let Some(screen) = screen_of(&block) else {
            return Vec::new();
        };
        let l = locals();
        let mut groups = Vec::new();
        if let Some(w) = open_window(screen, &l) {
            groups.push((&w.widgets, window_origin(w, &l)));
        }
        groups.push((&screen.widgets, [0.0, 0.0]));
        let mut out = Vec::new();
        for (widgets, [wx, wy]) in groups {
            for w in showing(widgets, &data, &l) {
                if let Widget::Text { x, y, text, text_data, .. } = w {
                    let shown = match text_data {
                        Some(n) => menu_string(usize::from(*n)).unwrap_or_default(),
                        None => fill_text(text, &data),
                    };
                    out.push((*text_data, shown, [ox + wx + *x as f32, oy + wy + *y as f32]));
                }
            }
        }
        out
    }

    /// The control socket's `type --menu-field ID [--replace] [--enter] TEXT`: text box `id` of the open mod screen takes
    /// the focus as a press on it gives it, then the text as typed (what its validator allows; `\b` is a backspace),
    /// and with `enter` the Enter a player presses, which sends `<mod id>:menu_text`. The box's text afterwards, or why
    /// nothing was typed.
    pub(super) fn mod_field_type(&mut self, id: i32, text: &str, replace: bool, enter: bool) -> Result<String, String> {
        let block = self.native_open_block().ok_or("--menu-field needs a mod menu open")?;
        let screen = screen_of(&block).ok_or_else(|| format!("the open menu {block} has no declared screen"))?;
        // A code screen's field: focused as a press would, then typed as below.
        if screen.code {
            let fields: Vec<i32> =
                self.code_screen_controls().unwrap_or_default().into_iter().filter(|c| c.kind == "field").map(|c| c.id).collect();
            self.code_screen_focus_field(id).ok_or_else(|| format!("no text field {id} showing on the {block} screen; its fields: {fields:?}"))?;
            if replace && let Ok(mut f) = FIELD.lock() && let Some(f) = f.as_mut() {
                f.edit.set_value("");
            }
            if text == "\\b" {
                self.mod_field_text(None, true);
            } else if !text.is_empty() {
                self.mod_field_text(Some(text), false);
            }
            let value = self.mod_field_value();
            if enter {
                self.mod_field_confirm();
            }
            return Ok(value);
        }
        let fields: Vec<i32> = self.declared_controls().into_iter().filter(|c| c.field).map(|c| c.id).collect();
        if !fields.contains(&id) {
            return Err(format!("no text field {id} showing on the {block} menu; its fields: {fields:?}"));
        }
        let focused = FIELD.lock().ok().is_some_and(|f| f.as_ref().is_some_and(|f| f.id == id));
        if !focused {
            let Some(Widget::TextField { max_length, validator, text_data, .. }) = find_field(screen, id) else {
                return Err(format!("no text field {id} on the {block} menu"));
            };
            let mut edit = horizon_client::ui::EditBox::new(0.0, 0.0, 100.0, 20.0);
            edit.max_length = usize::from(*max_length);
            edit.set_value(&text_data.and_then(|n| menu_string(usize::from(n))).unwrap_or_default());
            edit.move_cursor_to_end(false);
            if let Ok(mut f) = FIELD.lock() {
                *f = Some(ModField { id, validator: *validator, edit });
            }
        }
        if replace && let Ok(mut f) = FIELD.lock() && let Some(f) = f.as_mut() {
            f.edit.set_value("");
        }
        if text == "\\b" {
            self.mod_field_text(None, true);
        } else if !text.is_empty() {
            self.mod_field_text(Some(text), false);
        }
        let value = self.mod_field_value();
        if enter {
            self.mod_field_confirm();
        }
        Ok(value)
    }

    /// Enter: the text goes to the server, as a mod message on `<mod id>:menu_text`, and the box lets go of the focus.
    pub(super) fn mod_field_confirm(&mut self) {
        let Some((id, text)) = FIELD.lock().ok().and_then(|mut f| f.take()).map(|f| (f.id, f.edit.value)) else {
            return;
        };
        // A code screen's field: the screen hears Enter and sends what it wants (`send-text`).
        if self.code_screen_field(id, &text, true) {
            self.code_screen_actions();
            return;
        }
        let (Some(at), Some(block)) = (*SCREEN_AT.lock().ok().as_deref().unwrap_or(&None), self.native_open_block()) else {
            return;
        };
        let Some((namespace, _)) = block.split_once(':') else {
            return;
        };
        let data = horizon_mods::screen::encode_menu_input(at, id, &text);
        let _ = self.native_net_send_to_server(&format!("{namespace}:menu_text"), &data);
    }

    /// The tooltip under `at` on block `block`'s declared screen.
    pub(super) fn declared_screen_tooltip(
        &self,
        block: &str,
        data: &[i32],
        at: [f32; 2],
    ) -> Vec<String> {
        let Some(screen) = screen_of(block) else { return Vec::new() };
        if screen.code && self.code_screen_running() {
            return Vec::new();
        }
        // Over a grid cell: the item and its whole count.
        let mut l = locals();
        let entries = menu_grid();
        l.grid_total = entries.len() as i64;
        for w in showing(&screen.widgets, data, &l) {
            if let Widget::ItemGrid { x, y, columns, rows, cell } = w {
                let c = f32::from(*cell);
                let (col, row) = ((at[0] - *x as f32) / c, (at[1] - *y as f32) / c);
                if col >= 0.0 && row >= 0.0 && col < f32::from(*columns) && row < f32::from(*rows) {
                    let index = (horizon_mods::screen::grid_top(&l, *columns, *rows, l.grid_total) + row as i64) * i64::from(*columns) + col as i64;
                    if let Some(e) = entries.get(index as usize) {
                        return vec![self.pack.names.display_item(&e.item), format!("{}", e.count)];
                    }
                }
            }
        }
        hovered(screen, data, &l, at).unwrap_or_default()
    }
}
