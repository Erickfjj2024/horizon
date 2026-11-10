//! Fuel Demo (FORK-32): `recipe_types::burn_time` answers from the game's own fuel registry. Using `fueldemo:probe` tells the player
//! how many ticks coal and a stick burn, and whether dirt does.
use horizon_mod_sdk::server::{BlockPos, ItemStack, Player, Registration, ServerMod, players, recipe_types};

struct Demo;

impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Demo, Registration::new().on_items()))
    }
    fn item_used(&mut self, player: &Player, stack: ItemStack, _pos: Option<BlockPos>) -> bool {
        if stack.item != "fueldemo:probe" {
            return false;
        }
        let ticks = |item: &str| recipe_types::burn_time(item).map_or("none".to_owned(), |t| t.to_string());
        let _ = players::tell(&player.id, &format!("coal {} stick {} dirt {}", ticks("minecraft:coal"), ticks("minecraft:stick"), ticks("minecraft:dirt")));
        true
    }
}

horizon_mod_sdk::export_server_mod!(Demo);
