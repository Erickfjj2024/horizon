//! Item Menu Demo (round 4, item 2, `players.open-item-menu`): a message on `imenu:open` (one byte, the slot) opens the menu
//! `imenu:dictionary` over that slot of the player's. Each tick (`item-menu-tick`) it names the item it was opened over and the input's,
//! shows the logs in the input as data slot 0, and puts 4 oak planks in the result for a log; taking them (`item-menu-taken`) uses up a
//! log. Button 1 empties the input, button 2 closes the menu, and the text box (`imenu:menu_text`) names the dictionary.
use horizon_mod_sdk::server::{ItemMenu, ItemMenuUpdate, ItemStack, Player, Registration, ServerMod, menu_input, player_items, players};

struct Demo {
    name: String,
}

fn update() -> ItemMenuUpdate {
    ItemMenuUpdate { items: None, menu_data: None, menu_text: None, close: false, menu_grid: None }
}

impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Demo { name: "-".into() }, Registration::new().on_net()))
    }
    fn net_message(&mut self, player: &Player, channel: &str, data: &[u8]) {
        match channel {
            // A menu of text and a button alone (no slots, no inventory): what the Seismic Reader shows.
            "imenu:open_reader" => {
                let _ = players::open_item_menu(&player.id, u32::from(data.first().copied().unwrap_or(0)), "imenu:reader");
            }
            "imenu:open" => {
                let _ = players::open_item_menu(&player.id, u32::from(data.first().copied().unwrap_or(0)), "imenu:dictionary");
            }
            // The text box: (i32::MIN, 1, slot) marks an item menu.
            "imenu:menu_text" => {
                if let Some(input) = menu_input::decode(data)
                    && input.pos.x == i32::MIN
                    && input.pos.y == 1
                    && input.id == 5
                {
                    self.name = input.text;
                }
            }
            _ => {}
        }
    }
    /// Its key `imenu:tweaker` (Controls, default backslash) opens the tweaker over no item (`open-player-menu`), as Mekanism's
    /// "Open Module Tweaker" does; another key of its own is not used.
    fn key_pressed(&mut self, player: &Player, key: &str) -> bool {
        key == "imenu:tweaker" && players::open_player_menu(&player.id, "imenu:tweaker")
    }
    fn item_menu_tick(&mut self, player: &Player, menu: ItemMenu) -> Option<ItemMenuUpdate> {
        // The tweaker reads the player's equipment: the main hand's slot and stack, and the head (slot 39) and off hand (40).
        if menu.menu == "imenu:tweaker" {
            let inv = player_items::get_inventory(&player.id).unwrap_or_default();
            let name = |slot: usize| inv.get(slot).cloned().flatten().map_or_else(|| "-".to_owned(), |s| s.item);
            let held = player_items::selected_slot(&player.id).unwrap_or(0);
            return Some(ItemMenuUpdate {
                menu_text: Some(vec![format!("main {held} {}", name(held as usize)), format!("head {} off {}", name(39), name(40))]),
                ..update()
            });
        }
        if menu.menu == "imenu:reader" {
            // Its data slot counts the ticks it has been open (the mod's own count, kept in the menu's data).
            let reads = menu.menu_data.first().copied().unwrap_or(0) + 1;
            return Some(ItemMenuUpdate { menu_data: Some(vec![reads]), menu_text: Some(vec![format!("{} slot(s)", menu.items.len())]), ..update() });
        }
        let input = menu.items.first().cloned().flatten();
        let made = input.as_ref().filter(|s| s.item == "minecraft:oak_log").map(|_| ItemStack {
            item: "minecraft:oak_planks".into(),
            count: 4,
            components: String::new(),
        });
        let mut u = update();
        if menu.items.get(1).cloned().flatten() != made {
            let mut items = menu.items.clone();
            items[1] = made;
            u.items = Some(items);
        }
        u.menu_data = Some(vec![input.as_ref().map_or(0, |s| s.count as i32)]);
        let held = menu.stack.map_or_else(|| "-".to_owned(), |s| s.item);
        u.menu_text = Some(vec![held, input.map_or_else(|| "-".to_owned(), |s| s.item), self.name.clone()]);
        Some(u)
    }
    fn item_menu_button(&mut self, _player: &Player, menu: ItemMenu, id: i32) -> Option<ItemMenuUpdate> {
        if menu.menu == "imenu:reader" {
            return (id == 1).then(|| ItemMenuUpdate { menu_data: Some(vec![0]), ..update() });
        }
        let mut u = update();
        match id {
            1 => {
                let mut items = menu.items.clone();
                items[0] = None;
                u.items = Some(items);
            }
            2 => u.close = true,
            _ => return None,
        }
        Some(u)
    }
    fn item_menu_taken(&mut self, _player: &Player, menu: ItemMenu, slot: u32, _stack: ItemStack) -> Option<ItemMenuUpdate> {
        if slot != 1 {
            return None;
        }
        let mut items = menu.items.clone();
        let log = items.first_mut()?.as_mut()?;
        log.count -= 1;
        if log.count == 0 {
            items[0] = None;
        }
        Some(ItemMenuUpdate { items: Some(items), ..update() })
    }
}

horizon_mod_sdk::export_server_mod!(Demo);
