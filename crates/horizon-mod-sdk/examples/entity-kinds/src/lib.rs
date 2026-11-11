//! Robits (FORK-10): `robits:robit`, a living entity kind declared in `data/robits/horizon/entity_kind/robit.json`, drawn on the clients as an item display of the skin's model (`assets/robits/items/`). A message on
//! `robits:spawn` (a position as three little-endian whole numbers) spawns one; each tick the mod counts in its saved bytes and its
//! energy, takes the red skin from the second tick, walks a block east from the first, is named Bolt on the third, and removes the robit on the sixth.
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
    /// A right click takes the robit's click unless the player sneaks (FORK-10 d): a real mod would open its menu or pick it up.
    fn mod_entity_used(&mut self, _player: &Player, _entity: ModEntity, sneaking: bool) -> bool {
        !sneaking
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
                    teleport: None,
                    // From the first tick it walks a block east (FORK-10 c); from the sixth the mod removes it.
                    path_to: (count == 1).then(|| (e.x + 1.0, e.y, e.z, 1.0)),
                    stop: false,
                    remove: count >= 6,
                }
            })
            .collect()
    }
}

horizon_mod_sdk::export_server_mod!(Robits);
