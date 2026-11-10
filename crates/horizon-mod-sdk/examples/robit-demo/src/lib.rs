//! Robit Demo: the smallest user of entities. `/robit` summons an
//! `item_display` showing a stone (the model of a companion) and, once the
//! game answers with its id, moves it beside its owner every tick and hurts
//! the mobs within 4 blocks of it: 2 health a tick, and kills what has
//! 2 or less.
use horizon_mod_sdk::server::{
    Args, Player, Registration, ServerMod, entities, players,
};

struct Robit {
    owner: Option<String>,
    ticket: Option<u64>,
    id: Option<u32>,
}

const DIMENSION: &str = "minecraft:overworld";

impl ServerMod for Robit {
    fn init() -> Result<(Self, Registration), String> {
        Ok((
            Robit { owner: None, ticket: None, id: None },
            Registration::new().command_with("robit", []).on_tick(),
        ))
    }
    fn command(&mut self, player: &Player, _name: &str, _args: &Args) -> Result<Option<String>, String> {
        let nbt = r#"{item:{id:"minecraft:stone",count:1},teleport_duration:3}"#;
        let ticket = entities::summon(
            DIMENSION,
            "minecraft:item_display",
            player.x + 1.0,
            player.y + 1.0,
            player.z,
            nbt,
        )?;
        self.owner = Some(player.id.clone());
        self.ticket = Some(ticket);
        Ok(Some("Robit coming".into()))
    }
    fn entity_spawned(&mut self, ticket: u64, id: Option<u32>) {
        if self.ticket == Some(ticket) {
            self.id = id;
        }
    }
    fn tick(&mut self, _tick: u64) {
        let (Some(id), Some(owner)) = (self.id, self.owner.clone()) else { return };
        let Some(me) = players::online().into_iter().find(|p| p.id == owner) else { return };
        let (x, y, z) = (me.x + 1.0, me.y + 1.0, me.z);
        let _ = entities::move_entity(DIMENSION, id, x, y, z, 0.0, 0.0);
        for m in entities::mobs(DIMENSION) {
            let d = (m.x - x).powi(2) + (m.y - y).powi(2) + (m.z - z).powi(2);
            if d > 16.0 {
                continue;
            }
            if m.health > 2.0 {
                let _ = entities::set_mob_health(DIMENSION, m.id, m.health - 2.0);
            } else {
                let _ = entities::kill_mob(DIMENSION, m.id);
            }
        }
    }
}

horizon_mod_sdk::export_server_mod!(Robit);
