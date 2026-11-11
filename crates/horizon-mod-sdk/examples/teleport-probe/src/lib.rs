//! Teleport Probe (round 4, item 3, `entities.entities-in-box` and `teleport-entity`): a message on `tpprobe:go` lists to its sender
//! what has its feet in the box from (0, 63, 0) to (4, 66, 4), nearest the middle first, and sends each entity that is not a player
//! away as Mekanism's teleporter does: a pig to the Nether (at an eighth of its place), anything else 100 blocks east, facing east.
//! A player found is left for `player-control.teleport`.
use horizon_mod_sdk::server::{Player, Registration, ServerMod, entities, players};

struct Probe;

impl ServerMod for Probe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Probe, Registration::new().on_net()))
    }
    fn net_message(&mut self, player: &Player, channel: &str, _data: &[u8]) {
        if channel != "tpprobe:go" {
            return;
        }
        let found = entities::entities_in_box(&player.dimension, (0.0, 63.0, 0.0), (4.0, 66.0, 4.0));
        let names: Vec<String> = found.iter().map(|e| format!("{}#{}", e.kind, e.id)).collect();
        let _ = players::tell(&player.id, &names.join(" "));
        for e in found.iter().filter(|e| e.player.is_none()) {
            let _ = if e.kind == "minecraft:pig" {
                entities::teleport_entity(&player.dimension, e.id, "minecraft:the_nether", e.x / 8.0, e.y, e.z / 8.0, None)
            } else {
                entities::teleport_entity(&player.dimension, e.id, &player.dimension, e.x + 100.0, e.y, e.z, Some(-90.0))
            };
        }
        // A bad position is refused at once.
        if entities::teleport_entity(&player.dimension, 1, &player.dimension, f64::NAN, 0.0, 0.0, None).is_err() {
            let _ = players::tell(&player.id, "refused");
        }
    }
}

horizon_mod_sdk::export_server_mod!(Probe);
