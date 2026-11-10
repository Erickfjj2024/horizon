//! Block Use (FORK-29): a mod that decides what a right click on one of its blocks does. Holding a block, the click
//! passes (the block does not use itself; the held block is placed); sneaking with a hand held, or holding anything
//! else, leaves it to the game; an empty hand and no sneaking is the block's own use, taken by the mod.
use horizon_mod_sdk::server::{BlockPos, BlockUse, ItemStack, Player, Registration, ServerMod, world};

struct Gate;

impl ServerMod for Gate {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Gate, Registration::new().on_blocks()))
    }
    fn block_used(&mut self, player: &Player, pos: BlockPos, sneaking: bool, stack: Option<ItemStack>) -> BlockUse {
        // FORK-34: the gate at y 70 is a structure that is not formed: its items, tanks and energy are closed to the outside.
        if pos.y == 70 {
            let _ = world::set_capabilities_enabled(&player.dimension, pos.x, pos.y, pos.z, false, false, false);
        }
        // FORK-30: the gate at y 71 is a structure that has just formed: it sparkles for the players within 40 blocks.
        if pos.y == 71 {
            let _ = world::spawn_particles(&player.dimension, "minecraft:dust", f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5, 0.3, 0.3, 0.3, 0.0, 8, Some(0x5555FF), 1.0, 40.0);
        }
        match stack {
            Some(s) if s.item == "minecraft:stone" => BlockUse::Pass,
            None if !sneaking => BlockUse::Consume,
            _ => BlockUse::Default,
        }
    }
}

horizon_mod_sdk::export_server_mod!(Gate);
