//! Robits (FORK-10): `robits:robit`, a living entity kind declared in `data/robits/horizon/entity_kind/robit.json`, drawn on the clients as an item display of the skin's model (`assets/robits/items/`). A message on
//! `robits:spawn` (a position as three little-endian whole numbers) spawns one; each tick the mod counts in its saved bytes and its
//! energy, takes the red skin from the second tick, walks a block east from the first, is named Bolt on the third, and removes the robit on the sixth.
use horizon_mod_sdk::server::{DamageSource, ModEntity, ModEntityUpdate, Player, Registration, ServerMod, entities, players, world};

struct Robits;

impl ServerMod for Robits {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Robits, Registration::new().on_net().on_mod_entities()))
    }
    fn net_message(&mut self, player: &Player, channel: &str, data: &[u8]) {
        // `robits:spawn_old` spawns one already counted past its errands (its bytes start at 10): it only falls and takes harm.
        let old = channel == "robits:spawn_old";
        if !(channel == "robits:spawn" || old) || data.len() < 12 {
            return;
        }
        let int = |at: usize| i32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        let _ = entities::spawn_mod_entity(
            &player.dimension,
            "robits:robit",
            f64::from(int(0)) + 0.5,
            f64::from(int(4)),
            f64::from(int(8)) + 0.5,
            &[if old { 10 } else { 0 }],
        );
    }
    /// A red robit takes half of every blow (FORK-10 e); the others take it whole.
    fn mod_entity_damaged(&mut self, entity: ModEntity, _source: DamageSource, amount: f32) -> f32 {
        if entity.skin == "red" { amount / 2.0 } else { amount }
    }
    /// A robit that dies leaves its last words in the menu text of the block under it.
    fn mod_entity_died(&mut self, entity: ModEntity) {
        let _ = world::set_menu_text(&entity.dimension, entity.x.floor() as i32, entity.y.floor() as i32 - 1, entity.z.floor() as i32, &[format!("died:{}", entity.id)]);
    }
    /// A right click takes the robit's click unless the player sneaks (FORK-10 d): a real mod would open its menu or pick it up.
    fn mod_entity_used(&mut self, player: &Player, entity: ModEntity, sneaking: bool) -> bool {
        // FORK-10 d2: the click opens the robit's container as a chest (its 9 slots, one row).
        if !sneaking {
            let _ = players::open_entity_menu(&player.id, &entity.dimension, entity.id);
        }
        !sneaking
    }
    fn entities_tick(&mut self, _tick: u64, list: Vec<ModEntity>) -> Vec<ModEntityUpdate> {
        list.into_iter()
            .map(|e| {
                let count = e.data.first().copied().unwrap_or(0) + 1;
                // FORK-10 f: it picks up the nearest item lying within a block and a half, into its first free slot.
                let mut items = None;
                if let Some(slot) = e.items.iter().position(Option::is_none)
                    && let Some(found) = entities::items_in_box(
                        &e.dimension,
                        (e.x - 1.5, e.y - 1.0, e.z - 1.5),
                        (e.x + 1.5, e.y + 1.0, e.z + 1.5),
                    )
                    .into_iter()
                    .next()
                    && entities::take_dropped_item(&e.dimension, found.id, found.stack.count).is_ok()
                {
                    let mut now = e.items.clone();
                    now[slot] = Some(found.stack);
                    items = Some(now);
                }
                ModEntityUpdate {
                    dimension: e.dimension,
                    id: e.id,
                    data: Some(vec![count]),
                    items,
                    energy: Some(e.energy + 10),
                    skin: (count >= 2).then(|| "red".to_owned()),
                    name: (count == 3).then(|| Some("Bolt".to_owned())),
                    teleport: None,
                    // From the first tick it walks a block east (FORK-10 c); from the sixth the mod removes it.
                    path_to: (count == 1).then(|| (e.x + 1.0, e.y, e.z, 1.0)),
                    stop: false,
                    health: None,
                    remove: count == 6,
                }
            })
            .collect()
    }
}

horizon_mod_sdk::export_server_mod!(Robits);
