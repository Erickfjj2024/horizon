//! Damage Mods: the two damage hooks. A player wearing `damagemods:suit` on the chest takes half of the fire damage and none of
//! the magic damage (before the armour, as NeoForge's `LivingIncomingDamageEvent`); a `damagemods:blade` in the hand doubles the
//! attack damage of the blow.
use horizon_mod_sdk::server::{DamageSource, ItemStack, Player, Registration, ServerMod, player_items};

struct Damage;

const CHEST: usize = 38;

impl ServerMod for Damage {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Damage, Registration::new().on_damage()))
    }
    fn player_damaged(&mut self, player: &Player, source: DamageSource, amount: f32) -> f32 {
        let wears = player_items::get_inventory(&player.id)
            .and_then(|slots| slots.get(CHEST).cloned().flatten())
            .is_some_and(|s| s.item == "damagemods:suit");
        if !wears {
            return amount;
        }
        if source.is_magic {
            0.0
        } else if source.is_fire {
            amount / 2.0
        } else {
            amount
        }
    }
    fn item_hit_damage(&mut self, _player: &Player, stack: ItemStack, _kind: &str, damage: f32) -> f32 {
        if stack.item == "damagemods:blade" { damage * 2.0 } else { damage }
    }
}

horizon_mod_sdk::export_server_mod!(Damage);
