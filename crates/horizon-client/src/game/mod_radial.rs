//! A server mod's radial menu on this client (`players.open-radial-menu`, Mekanism's mode selectors): the entries around the
//! screen's centre while it is open, the pointer let go, and the choice sent back to the mod as a message on `<mod id>:radial`.
use super::Game;
use horizon_client::hud::HudBatch;
use horizon_mods::screen::{decode_radial, radial_pick, radial_spot};
use std::sync::Mutex;

struct Radial {
    mod_id: String,
    entries: Vec<(String, String, String)>,
    current: u32,
}

static RADIAL: Mutex<Option<Radial>> = Mutex::new(None);

/// Ring radius and the middle that makes no choice, in GUI pixels.
const RADIUS: f32 = 48.0;
const DEAD_ZONE: f32 = 16.0;

fn open() -> bool {
    RADIAL.lock().is_ok_and(|r| r.is_some())
}

impl Game<'_> {
    /// A `horizon:radial` message arrived: the menu opens and the pointer is let go.
    pub(super) fn open_mod_radial(&mut self, data: &[u8]) {
        let Some((mod_id, entries, current)) = decode_radial(data) else {
            return;
        };
        if entries.len() < 2 {
            return;
        }
        if let Ok(mut r) = RADIAL.lock() {
            *r = Some(Radial { mod_id, entries, current });
        }
        if self.screen == super::Screen::Playing {
            self.capture(false);
        }
    }

    /// Whether a radial menu is open (a press goes to it).
    pub(super) fn mod_radial_open(&self) -> bool {
        open()
    }

    /// The centre of the screen, in GUI pixels.
    fn radial_centre(&self) -> Option<[f32; 2]> {
        let size = self.window.as_ref()?.inner_size();
        let b = HudBatch::new([size.width, size.height]);
        Some([b.width / 2.0, b.height / 2.0])
    }

    /// A press (or a tap at `at`, GUI pixels) while the menu is open: the entry under it is chosen and sent, the middle closes the menu
    /// with no choice. True when the menu took the press.
    pub(super) fn mod_radial_press(&mut self, at: [f32; 2]) -> bool {
        let Some(centre) = self.radial_centre() else {
            return false;
        };
        let Some(r) = RADIAL.lock().ok().and_then(|mut r| r.take()) else {
            return false;
        };
        if let Some(i) = radial_pick(r.entries.len(), centre, at, DEAD_ZONE) {
            let channel = format!("{}:radial", r.mod_id);
            let _ = self.native_net_send_to_server(&channel, r.entries[i].0.as_bytes());
        }
        if self.screen == super::Screen::Playing {
            self.capture(true);
        }
        true
    }

    /// Escape: the menu closes with no choice. True when one was open.
    pub(super) fn close_mod_radial(&mut self) -> bool {
        let was = RADIAL.lock().ok().and_then(|mut r| r.take()).is_some();
        if was && self.screen == super::Screen::Playing {
            self.capture(true);
        }
        was
    }

    /// Draws the open menu: each entry's icon and label around the centre, the current one and the one under the pointer marked.
    pub(super) fn draw_mod_radial(&self, batch: &mut HudBatch) {
        let Ok(r) = RADIAL.lock() else {
            return;
        };
        let Some(r) = r.as_ref() else {
            return;
        };
        let centre = [batch.width / 2.0, batch.height / 2.0];
        let n = r.entries.len();
        let pointed = radial_pick(n, centre, [self.cursor.0, self.cursor.1], DEAD_ZONE);
        self.native_fill(batch, [centre[0] - RADIUS - 24.0, centre[1] - RADIUS - 24.0, 2.0 * RADIUS + 48.0, 2.0 * RADIUS + 48.0], 0x6000_0000);
        for (i, (_, label, icon)) in r.entries.iter().enumerate() {
            let [x, y] = radial_spot(i, n, centre, RADIUS);
            let colour = if pointed == Some(i) {
                0xC0FF_FFFF
            } else if i as u32 == r.current {
                0x8040_80FF
            } else {
                0x6020_2020
            };
            self.native_fill(batch, [x - 10.0, y - 10.0, 20.0, 20.0], colour);
            if !icon.is_empty() {
                self.native_item(batch, icon, [x - 8.0, y - 8.0], 16.0);
            }
            let w = self.native_text_width(label);
            self.native_text(batch, label, [x - w / 2.0, y + 11.0], 0xFFFF_FFFF, true);
        }
    }
}
