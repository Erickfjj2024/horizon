//! Energy Items Demo (round 4, item 5, `recipe-types.item-energy`): `ebattery:battery` (data/ebattery/horizon/item/battery.json) holds
//! up to 1000 FE, takes 100 a call and gives 50. A message on `ebattery:charge` charges the item in the sender's first slot with 300
//! (a simulation first), `ebattery:drain` drains 300 from it; each puts the stack back and tells what moved and what it holds. A stick
//! holds none.
use horizon_mod_sdk::server::{Player, Registration, ServerMod, player_items, players, recipe_types};

struct Demo;

impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Demo, Registration::new().on_net()))
    }
    fn net_message(&mut self, player: &Player, channel: &str, _data: &[u8]) {
        let Some(stack) = player_items::get_inventory(&player.id).and_then(|inv| inv.first().cloned().flatten()) else { return };
        let Some(before) = recipe_types::item_energy(&stack) else {
            let _ = players::tell(&player.id, &format!("{} holds no energy", stack.item));
            return;
        };
        let (after, moved) = match channel {
            "ebattery:charge" => {
                let (_, would) = recipe_types::item_energy_insert(&stack, 300, true);
                let (after, moved) = recipe_types::item_energy_insert(&stack, 300, false);
                let _ = players::tell(&player.id, &format!("would take {would}"));
                (after, moved)
            }
            "ebattery:drain" => recipe_types::item_energy_extract(&stack, 300, false),
            _ => return,
        };
        let _ = player_items::set_slot(&player.id, 0, Some(&after));
        let now = recipe_types::item_energy(&after).map_or(0, |e| e.stored);
        let _ = players::tell(&player.id, &format!("moved {moved}: {}/{} (was {})", now, before.capacity, before.stored));
    }
}

horizon_mod_sdk::export_server_mod!(Demo);
