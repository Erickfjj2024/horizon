//! Block Use (FORK-29): a mod that decides what a right click on one of its blocks does. Holding a block, the click
//! passes (the block does not use itself; the held block is placed); sneaking with a hand held, or holding anything
//! else, leaves it to the game; an empty hand and no sneaking is the block's own use, taken by the mod.
use horizon_mod_sdk::server::{BlockPos, BlockUse, ItemStack, Player, Registration, ServerMod, world};

struct Gate;

impl ServerMod for Gate {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Gate, Registration::new().on_blocks().redirect("blockuse:bound", 0, 1)))
    }
    /// FORK-35: a machine of the mod needs the block above it free: with one over it (y 81 over a block at 80), the placement is refused.
    fn can_place(&mut self, _player: &Player, pos: BlockPos, _state: &str) -> bool {
        world::get_block("minecraft:overworld", pos.x, pos.y + 1, pos.z).is_none_or(|b| b == "minecraft:air")
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
        // FORK-23 (cardboard box): the gate at y 72 packs the block beside it into itself; the one at y 73 puts it back where it stands.
        if pos.y == 72 {
            let _ = world::pack_block(&player.dimension, pos.x + 1, pos.y, pos.z, pos.x, pos.y, pos.z, None);
            // (In place: the box takes the packed block's own place with `Some("blockuse:box")` and `into` the same position.)
        } else if pos.y == 74 {
            let _ = world::pack_block(&player.dimension, pos.x + 1, pos.y, pos.z, pos.x + 1, pos.y, pos.z, Some("blockuse:box"));
        } else if pos.y == 73 {
            let _ = world::unpack_block(&player.dimension, pos.x, 72, pos.z, pos.x + 1, pos.y, pos.z);
        }
        // FORK-20: the machine at y 75 shows two strings in its menu (a filter's tag and the player's name), `text_data` 0 and 1.
        if pos.y == 75 {
            let _ = world::set_menu_text(&player.dimension, pos.x, pos.y, pos.z, &["forge:ores/iron".to_owned(), player.name.clone()]);
        }
        match stack {
            Some(s) if s.item == "minecraft:stone" => BlockUse::Pass,
            None if !sneaking => BlockUse::Consume,
            _ => BlockUse::Default,
        }
    }
}

horizon_mod_sdk::export_server_mod!(Gate);
