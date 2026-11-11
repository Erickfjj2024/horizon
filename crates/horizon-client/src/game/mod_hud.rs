//! The server mods' HUD lines on this client (`players.set-hud-line`): what the server sent, by id, drawn at the screen's top left
//! with the other HUD boxes (the MekaSuit's module HUD).
use super::Game;
use horizon_client::{extras_hud::Corners, hud::HudBatch};
use horizon_client_render::extras::Corner;
use std::{collections::BTreeMap, sync::Mutex};

/// One line: its icon, text and bar.
struct Line {
    icon: String,
    text: String,
    fraction: f32,
}

/// Most lines a client keeps.
const LINES: usize = 32;

static HUD: Mutex<BTreeMap<String, Line>> = Mutex::new(BTreeMap::new());

/// A `horizon:hud_line` message arrived: the line is set, or taken away when its text is empty.
pub(super) fn remember_hud_line(data: &[u8]) {
    let Some((id, icon, text, fraction)) = horizon_mods::screen::decode_hud_line(data) else {
        return;
    };
    let Ok(mut lines) = HUD.lock() else {
        return;
    };
    if text.is_empty() {
        lines.remove(&id);
    } else if lines.len() < LINES || lines.contains_key(&id) {
        lines.insert(id, Line { icon, text, fraction });
    }
}

/// The client left its server: the lines it was sent go.
pub(super) fn forget_hud_lines() {
    if let Ok(mut lines) = HUD.lock() {
        lines.clear();
    }
}

/// Height of a line: the icon's (16 at half size is 8) or the text's, and room for the bar.
const ROW: f32 = 12.0;

impl Game<'_> {
    /// Draws the lines in a box at the top left, under what that corner holds.
    pub(super) fn draw_mod_hud(&self, batch: &mut HudBatch, corners: &mut Corners) {
        let Ok(lines) = HUD.lock() else {
            return;
        };
        if lines.is_empty() {
            return;
        }
        let width = lines.values().map(|l| self.native_text_width(&l.text)).fold(0.0f32, f32::max) + 14.0;
        let (x, y) = corners.place(Corner::TopLeft, width, ROW * lines.len() as f32);
        for (i, line) in lines.values().enumerate() {
            let top = y + i as f32 * ROW;
            if !line.icon.is_empty() {
                let start = batch.vertices.len();
                self.native_item(batch, &line.icon, [x, top], 16.0);
                batch.scale_since(start, x, top, 0.5, 0.5);
            }
            self.native_text(batch, &line.text, [x + 11.0, top], 0xFFFF_FFFF, true);
            if line.fraction >= 0.0 {
                let w = width - 14.0;
                self.native_fill(batch, [x + 11.0, top + 9.0, w, 1.5], 0x8040_4040);
                self.native_fill(batch, [x + 11.0, top + 9.0, w * line.fraction.clamp(0.0, 1.0), 1.5], 0xFF3C_DCFF);
            }
        }
    }
}
