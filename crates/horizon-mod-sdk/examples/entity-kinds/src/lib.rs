//! Robits (FORK-10): `robits:robit`, a living entity kind declared in `data/robits/horizon/entity_kind/robit.json`. A message on
//! `robits:spawn` (a position as three little-endian whole numbers) spawns one; each tick the mod counts in its saved bytes and its
//! energy, takes the red skin from the second tick, is named Bolt on the third and teleports a block up, and removes the robit on the fifth.
use horizon_mod_sdk::server::{ModEntity, ModEntityUpdate, Player, Registration, ServerMod, entities};

struct Robits;

impl ServerMod for Robits {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Robits, Registration::new().on_net().on_mod_entities()))
    }
    fn net_message(&mut self, player: &Player, channel: &str, data: &[u8]) {
        if channel != "robits:spawn" || data.len() < 12 {
            return;
        }
        let int = |at: usize| i32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        let _ = entities::spawn_mod_entity(
            &player.dimension,
            "robits:robit",
            f64::from(int(0)) + 0.5,
            f64::from(int(4)),
            f64::from(int(8)) + 0.5,
            &[0],
        );
    }
    fn entities_tick(&mut self, _tick: u64, list: Vec<ModEntity>) -> Vec<ModEntityUpdate> {
        list.into_iter()
            .map(|e| {
                let count = e.data.first().copied().unwrap_or(0) + 1;
                ModEntityUpdate {
                    dimension: e.dimension,
                    id: e.id,
                    data: Some(vec![count]),
                    items: None,
                    energy: Some(e.energy + 10),
                    skin: (count >= 2).then(|| "red".to_owned()),
                    name: (count == 3).then(|| Some("Bolt".to_owned())),
                    teleport: (count == 3).then(|| (e.x, e.y + 1.0, e.z)),
                    remove: count >= 5,
                }
            })
            .collect()
    }
}

horizon_mod_sdk::export_server_mod!(Robits);
