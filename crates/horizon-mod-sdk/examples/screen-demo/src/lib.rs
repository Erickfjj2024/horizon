//! Screen Demo (API 56, capability `screen`): the server half of a menu whose screen is drawn by code
//! (`screens/`, built into `screens.wasm`). Its key `screendemo:open` (default backslash) opens `screendemo:panel`, a
//! player menu, as Mekanism's Portable QIO Dashboard opens. Each tick the menu gets its data (`[presses, frequencies]`),
//! its strings (0 the header, then one per frequency) and an item grid; menu button 10 to 12 picks the security mode,
//! 100 + n selects frequency n, 2 deletes the selected one, and the text box (`screendemo:menu_text`, field 1) adds a
//! frequency and selects it.
use horizon_mod_sdk::server::{
    GridEntry, ItemMenu, ItemMenuUpdate, Player, Registration, ServerMod, menu_input, players,
};

const MENU: &str = "screendemo:panel";

struct Demo {
    frequencies: Vec<String>,
    selected: Option<usize>,
    /// The security mode a button picked (0 public, 1 trusted, 2 private) and how many presses there were.
    mode: i32,
    presses: i32,
    /// The strings, data and grid last sent: an update goes only when they change.
    sent: Option<(Vec<String>, Vec<i32>)>,
}

impl Demo {
    fn strings(&self) -> Vec<String> {
        let header = match self.selected.and_then(|i| self.frequencies.get(i)) {
            Some(f) => format!("Frequency: {f}"),
            None => "No frequency".to_owned(),
        };
        std::iter::once(header)
            .chain(self.frequencies.iter().cloned())
            .collect()
    }
    fn data(&self) -> Vec<i32> {
        vec![
            self.presses,
            self.mode,
            self.selected.map_or(-1, |s| s as i32),
        ]
    }
}

fn grid() -> Vec<GridEntry> {
    [
        ("minecraft:cobblestone", 1_500_000u64),
        ("minecraft:stone", 12_345),
        ("minecraft:dirt", 64),
        ("minecraft:diamond", 3),
        ("minecraft:oak_log", 1),
    ]
    .into_iter()
    .map(|(item, count)| GridEntry {
        item: item.into(),
        components: String::new(),
        count,
    })
    .collect()
}

impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        let demo = Demo {
            frequencies: vec!["Base".into(), "Mine".into()],
            selected: None,
            mode: 0,
            presses: 0,
            sent: None,
        };
        Ok((demo, Registration::new().on_net()))
    }
    fn key_pressed(&mut self, player: &Player, key: &str) -> bool {
        if key != "screendemo:open" {
            return false;
        }
        self.sent = None;
        players::open_player_menu(&player.id, MENU)
    }
    fn net_message(&mut self, _player: &Player, channel: &str, data: &[u8]) {
        // The text box (field 1) of the player menu: (i32::MIN, 1, slot).
        if channel == "screendemo:menu_text"
            && let Some(input) = menu_input::decode(data)
            && input.pos.x == i32::MIN
            && input.id == 1
            && !input.text.is_empty()
        {
            let at = match self.frequencies.iter().position(|f| *f == input.text) {
                Some(i) => i,
                None => {
                    self.frequencies.push(input.text);
                    self.frequencies.len() - 1
                }
            };
            self.selected = Some(at);
        }
    }
    fn item_menu_tick(&mut self, _player: &Player, menu: ItemMenu) -> Option<ItemMenuUpdate> {
        if menu.menu != MENU {
            return None;
        }
        let now = (self.strings(), self.data());
        if self.sent.as_ref() == Some(&now) {
            return None;
        }
        self.sent = Some(now.clone());
        Some(ItemMenuUpdate {
            items: None,
            menu_data: Some(now.1),
            menu_text: Some(now.0),
            close: false,
            menu_grid: Some(grid()),
        })
    }
    fn item_menu_button(
        &mut self,
        _player: &Player,
        menu: ItemMenu,
        id: i32,
    ) -> Option<ItemMenuUpdate> {
        if menu.menu != MENU {
            return None;
        }
        self.presses += 1;
        match id {
            10..=12 => self.mode = id - 10,
            2 => {
                if let Some(i) = self.selected.take()
                    && i < self.frequencies.len()
                {
                    self.frequencies.remove(i);
                }
            }
            n if n >= 100 && ((n - 100) as usize) < self.frequencies.len() => {
                self.selected = Some((n - 100) as usize)
            }
            _ => {}
        }
        None
    }
}

horizon_mod_sdk::export_server_mod!(Demo);
