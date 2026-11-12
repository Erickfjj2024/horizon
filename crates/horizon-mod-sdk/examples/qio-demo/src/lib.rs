//! QIO Grid Demo (API 55, the item grid of Mekanism's QIO Dashboard): a catalogue of item counts past a stack's size, shown in the
//! `item_grid` of `qio:dashboard` (a player menu, as the Portable QIO Dashboard's), largest first, filtered by the search box. A press
//! takes from it (left a stack, right half of one, Shift-left a stack into the inventory) or, with a stack on the cursor, stores it.
//! A message on `qio:open` opens it; on `qio:block` the grid of the block menu at 1 77 1 is set (a block's dashboard).
use horizon_mod_sdk::server::{
    GridAnswer, GridTarget, ItemMenu, ItemMenuUpdate, ItemStack, Player, Registration, ServerMod, menu_input, players, world,
};
use std::collections::BTreeMap;

const STACK: u64 = 64;

struct Qio {
    catalogue: BTreeMap<String, u64>,
    search: String,
    /// The items in the order the grid shows them now: a press's index is a place in it.
    shown: Vec<String>,
}

impl Qio {
    /// The catalogue's items matching the search, largest count first (then by id).
    fn listed(&self) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> =
            self.catalogue.iter().filter(|(id, n)| **n > 0 && id.contains(&self.search)).map(|(id, n)| (id.clone(), *n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }
    fn entries(&mut self) -> Vec<horizon_mod_sdk::server::GridEntry> {
        let listed = self.listed();
        self.shown = listed.iter().map(|(id, _)| id.clone()).collect();
        listed.into_iter().map(|(item, count)| horizon_mod_sdk::server::GridEntry { item, components: String::new(), count }).collect()
    }
}

impl ServerMod for Qio {
    fn init() -> Result<(Self, Registration), String> {
        let catalogue = [("minecraft:stone", 12_345u64), ("minecraft:dirt", 64), ("minecraft:diamond", 3), ("minecraft:cobblestone", 1_500_000)]
            .into_iter()
            .map(|(id, n)| (id.to_owned(), n))
            .collect();
        Ok((Qio { catalogue, search: String::new(), shown: Vec::new() }, Registration::new().on_net()))
    }
    fn net_message(&mut self, player: &Player, channel: &str, data: &[u8]) {
        match channel {
            "qio:open" => {
                let _ = players::open_player_menu(&player.id, "qio:dashboard");
            }
            "qio:block" => {
                let entries = self.entries();
                let _ = world::set_menu_grid(&player.dimension, 1, 77, 1, &entries);
            }
            // The search box: its text filters the list.
            "qio:menu_text" => {
                if let Some(input) = menu_input::decode(data) {
                    self.search = input.text;
                }
            }
            _ => {}
        }
    }
    fn item_menu_tick(&mut self, _player: &Player, _menu: ItemMenu) -> Option<ItemMenuUpdate> {
        Some(ItemMenuUpdate { items: None, menu_data: None, menu_text: None, close: false, menu_grid: Some(self.entries()) })
    }
    fn menu_grid_click(&mut self, _player: &Player, _target: GridTarget, index: u32, button: u8, shift: bool, carried: Option<ItemStack>) -> Option<GridAnswer> {
        // A stack on the cursor goes into the catalogue.
        if let Some(c) = carried {
            *self.catalogue.entry(c.item).or_default() += u64::from(c.count);
            return Some(GridAnswer { carried: None, to_inventory: Vec::new() });
        }
        let item = self.shown.get(index as usize)?.clone();
        let have = self.catalogue.get(&item).copied().unwrap_or(0);
        let want = if button == 1 { STACK / 2 } else { STACK };
        let n = want.min(have);
        if n == 0 {
            return None;
        }
        self.catalogue.insert(item.clone(), have - n);
        let stack = ItemStack { item, count: n as u32, components: String::new() };
        Some(if shift { GridAnswer { carried: None, to_inventory: vec![stack] } } else { GridAnswer { carried: Some(stack), to_inventory: Vec::new() } })
    }
}

horizon_mod_sdk::export_server_mod!(Qio);
