//! Body Probe (`player-control.get-state`, `set-food`): using `bodyprobe:probe` tells the player how fast they move, whether the jump
//! key is held, their air, whether they burn, their effects, and tops their food up.
use horizon_mod_sdk::server::{BlockPos, ItemStack, Player, Registration, ServerMod, player_control, player_items, players};

struct Probe;

impl ServerMod for Probe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Probe, Registration::new().on_items()))
    }
    fn item_used(&mut self, player: &Player, stack: ItemStack, _pos: Option<BlockPos>) -> bool {
        if stack.item != "bodyprobe:probe" {
            return false;
        }
        let Some(s) = player_control::get_state(&player.id) else { return false };
        let effects: Vec<String> = s.effects.iter().map(|e| format!("{}:{}:{}", e.id, e.duration, e.amplifier)).collect();
        let _ = players::tell(
            &player.id,
            &format!(
                "vy {} jump {} air {}/{} fire {} food {} effects [{}]",
                s.velocity_y, s.jumping, s.air, s.max_air, s.fire_ticks, s.food, effects.join(",")
            ),
        );
        let _ = player_control::set_food(&player.id, 20, 5.0);
        // The small things a suit does: breathe for the player, ease a poison, stop a fall, run faster, and say so over the hotbar.
        let _ = player_control::set_air(&player.id, 300);
        let _ = player_control::reduce_effect(&player.id, "minecraft:poison", 40);
        let _ = player_control::reset_fall_distance(&player.id);
        let _ = player_control::set_attribute_modifier(&player.id, "minecraft:movement_speed", "bodyprobe:speed", 0.5, "add_multiplied_base");
        let _ = player_control::remove_attribute_modifier(&player.id, "minecraft:step_height", "bodyprobe:step");
        let _ = players::tell_overlay(&player.id, "Suit on");
        // The Elytra Unit: the chestplate worn gets the `minecraft:glider` component, so the player glides with it as with an elytra.
        if let Some(Some(mut chest)) = player_items::get_inventory(&player.id).and_then(|slots| slots.get(38).cloned())
            && !chest.components.contains("minecraft:glider")
        {
            chest.components = r#"{"minecraft:glider":{}}"#.into();
            let _ = player_items::set_slot(&player.id, 38, Some(&chest));
        }
        true
    }
}

horizon_mod_sdk::export_server_mod!(Probe);
