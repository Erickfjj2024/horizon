//! Screens of content mods' menu types drawn from their data
//! (`data/<ns>/horizon/menu/<name>.json`'s `screen`, API block 7): a block
//! whose id is the menu type's gets this screen with no code of its own on
//! the client. The pure parts (what a value reads, where a bar fills, which
//! button a press hits, a tooltip's text) are free functions with tests.
use super::{Game, native_hooks::NativeScreen};
use horizon_client::hud::{HudBatch, rgba};
use horizon_mods::{
    content::{ArmorSlots, Direction, MenuScreen, Widget, parse_color},
    screen::{DRAG_HEIGHT, Locals, Press, press_with, fill_text, neighbor_step, window_origin, filled, fraction, hovered, open_window, press, read, showing},
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

/// The left button was released: a dragged window is let go.
pub(super) fn end_drag() {
    if let Ok(mut d) = DRAG.lock() {
        *d = None;
    }
}

/// The id a press that stays on this client gives `native_screen_click`'s `button`: not a menu button.
pub(super) const LOCAL_PRESS: i32 = i32::MIN;

/// A new screen opened: its windows are closed and its variables unset.
pub(super) fn reset_locals() {
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
                Widget::Text { x, y, text, color, scale } => {
                    let rgb = color.as_deref().and_then(parse_color).unwrap_or(0xFF40_4040);
                    let text = fill_text(text, data);
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
                Widget::Button { .. } | Widget::Show { .. } => {}
            }
        }
    }

    /// Fills the bottom `frac` of rectangle `rect` with `texture` tiled from its bottom left corner and tinted (a tank's
    /// contents, as `GuiUtils.drawTiledSprite` with `TilingDirection.UP_RIGHT` draws them).
    fn draw_tiled_bottom_up(&self, batch: &mut HudBatch, texture: &str, rect: [f32; 4], frac: f32, tint: u32) {
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
        let mut l = LOCALS.lock().ok()?;
        let mut now = l.take().unwrap_or_default();
        // A press on the title edge of an open window, over none of its buttons, takes the window to drag.
        let grab = open_window(screen, &now).map(|w| {
            let o = window_origin(w, &now);
            let inside = at[0] >= o[0] && at[0] < o[0] + w.width as f32 && at[1] >= o[1] && at[1] < o[1] + DRAG_HEIGHT;
            (inside, now.offset)
        });
        let result = press_with(screen, data, &mut now, at, alt, self.has_shift_down());
        *l = Some(now);
        let result = result?;
        if let (Press::Local, (true, offset), false) = (&result, grab.unwrap_or((false, [0.0; 2])), alt)
            && let Ok(mut d) = DRAG.lock()
        {
            *d = Some((at, offset));
        }
        match result {
            Press::Send(id) => Some(id),
            Press::Local => Some(LOCAL_PRESS),
        }
    }

    /// The tooltip under `at` on block `block`'s declared screen.
    pub(super) fn declared_screen_tooltip(
        &self,
        block: &str,
        data: &[i32],
        at: [f32; 2],
    ) -> Vec<String> {
        screen_of(block)
            .and_then(|s| hovered(s, data, &locals(), at))
            .unwrap_or_default()
    }
}
