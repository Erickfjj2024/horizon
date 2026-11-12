//! Anvil Probe (round 4, item 6): a message on `anvilprobe:go` reads the sender's first two slots and tells the first's wear and
//! enchantments, whether the second repairs it, and what an anvil makes of the two (renamed to the message's text when it has one),
//! as the Robit's repair tab does; then it mends the first slot by 10 (`item-set-damage`).
use horizon_mod_sdk::server::{Player, Registration, ServerMod, player_items, players, recipe_types};

struct Probe;

impl ServerMod for Probe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Probe, Registration::new().on_net()))
    }
    fn net_message(&mut self, player: &Player, channel: &str, data: &[u8]) {
        if channel != "anvilprobe:go" {
            return;
        }
        let inv = player_items::get_inventory(&player.id).unwrap_or_default();
        let (Some(left), right) = (inv.first().cloned().flatten(), inv.get(1).cloned().flatten()) else { return };
        let wear = recipe_types::item_durability(&left).map_or_else(|| "no wear".to_owned(), |d| format!("{}/{}", d.damage, d.max_damage));
        let enchants: Vec<String> = recipe_types::item_enchantments(&left).iter().map(|(id, l)| format!("{id} {l}")).collect();
        let repairs = right.as_ref().is_some_and(|r| recipe_types::item_repairs_with(&left, r));
        let _ = players::tell(&player.id, &format!("wear {wear}; enchantments [{}]; repairs {repairs}", enchants.join(", ")));
        let name = (!data.is_empty()).then(|| String::from_utf8_lossy(data).into_owned());
        let answer = match recipe_types::anvil_result(&left, right.as_ref(), name.as_deref()) {
            Some(a) => format!("anvil {}*{}{} for {} levels, {} used", a.output.item, a.output.count, a.output.components, a.cost, a.material_cost),
            None => "anvil nothing".to_owned(),
        };
        let _ = players::tell(&player.id, &answer);
        if let Some(d) = recipe_types::item_durability(&left)
            && let Some(mended) = recipe_types::item_set_damage(&left, d.damage.saturating_sub(10))
        {
            let _ = player_items::set_slot(&player.id, 0, Some(&mended));
        }
    }
}

horizon_mod_sdk::export_server_mod!(Probe);
