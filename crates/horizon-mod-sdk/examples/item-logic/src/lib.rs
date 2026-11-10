//! Item Logic: the smallest user of items with logic. `itemlogic:wand`,
//! used (right click), costs its user 2 magic damage and gives them speed II
//! for five seconds, taking the use; hitting an entity with it gives Strength
//! I for three seconds. A `itemlogic:jetpack` in the chest slot burns 1 of its
//! `itemlogic:energy` component each tick until it has none.
use horizon_mod_sdk::components::Components;
use horizon_mod_sdk::server::{
    BlockPos, ItemStack, Player, Registration, ServerMod, player_control, player_items, players,
};

struct Logic;

const WAND: &str = "itemlogic:wand";
const JETPACK: &str = "itemlogic:jetpack";
const CHEST: u32 = 38;

impl ServerMod for Logic {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Logic, Registration::new().on_items().on_tick()))
    }
    fn item_used(&mut self, player: &Player, stack: ItemStack, _pos: Option<BlockPos>) -> bool {
        if stack.item != WAND {
            return false;
        }
        let _ = player_items::hurt(&player.id, "minecraft:magic", 2.0);
        let _ = player_items::add_effect(&player.id, "minecraft:speed", 100, 1);
        // The wand also launches its user and lets them fly, and swaps itself for a stick only if still in slot 0.
        let _ = player_control::set_motion(&player.id, 0.0, 1.5, 0.0);
        let _ = player_control::set_allow_flight(&player.id, true);
        let _ = player_items::compare_and_set_slot(
            &player.id,
            0,
            Some(&stack),
            Some(&ItemStack { item: "minecraft:stick".into(), count: 1, components: String::new() }),
        );
        true
    }
    fn item_hit(&mut self, player: &Player, stack: ItemStack, _entity: u32, _kind: &str) {
        if stack.item == WAND {
            let _ = player_items::add_effect(&player.id, "minecraft:strength", 60, 0);
        }
    }
    fn tick(&mut self, _tick: u64) {
        for p in players::online() {
            let Some(slots) = player_items::get_inventory(&p.id) else { continue };
            let Some(Some(chest)) = slots.get(CHEST as usize) else { continue };
            if chest.item != JETPACK {
                continue;
            }
            let Some(mut c) = Components::parse(&chest.components) else { continue };
            let energy = c.get_long("itemlogic:energy").unwrap_or(0);
            if energy <= 0 {
                continue;
            }
            c.set_long("itemlogic:energy", energy - 1);
            let mut stack = chest.clone();
            stack.components = c.to_snbt();
            let _ = player_items::set_slot(&p.id, CHEST, Some(&stack));
        }
    }
}

horizon_mod_sdk::export_server_mod!(Logic);
