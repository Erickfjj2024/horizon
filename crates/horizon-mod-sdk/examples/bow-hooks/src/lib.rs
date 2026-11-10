//! Bow Hooks: `item-released` and `item-shot`. `bowhooks:power_bow` (a mod item whose template is `minecraft:bow`, so the game treats it
//! as a bow) spends 100 energy every shot; with less the shot is cancelled, as `ItemElectricBow.releaseUsing` does. A stack whose
//! components name `fire` lights the arrows it shoots (the Flame arrow of the fire mode) for 100 ticks.
use horizon_mod_sdk::server::{entities, ItemStack, Player, Registration, ServerMod};

struct Bow {
    energy: u32,
}

const SHOT: u32 = 100;

impl ServerMod for Bow {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Bow { energy: 250 }, Registration::new().on_item_release()))
    }
    fn item_released(&mut self, _player: &Player, stack: ItemStack, _hand: &str, _ticks_held: u32) -> bool {
        if stack.item != "bowhooks:power_bow" {
            return true;
        }
        if self.energy < SHOT {
            return false;
        }
        self.energy -= SHOT;
        true
    }
    fn item_shot(&mut self, _player: &Player, stack: ItemStack, projectiles: Vec<u32>) {
        if stack.item == "bowhooks:power_bow" && stack.components.contains("fire") {
            for id in projectiles {
                let _ = entities::set_entity_fire("minecraft:overworld", id, 100);
            }
        }
    }
}

horizon_mod_sdk::export_server_mod!(Bow);
