//! Body Probe (`player-control.get-state`, `set-food`): using `bodyprobe:probe` tells the player how fast they move, whether the jump
//! key is held, their air, whether they burn, their effects, and tops their food up.
use horizon_mod_sdk::server::{BlockPos, ItemStack, Player, Registration, ServerMod, player_control, players};

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
        true
    }
}

horizon_mod_sdk::export_server_mod!(Probe);
