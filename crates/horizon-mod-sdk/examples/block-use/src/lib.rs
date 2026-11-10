//! Block Use (FORK-29): a mod that decides what a right click on one of its blocks does. Holding a block, the click
//! passes (the block does not use itself; the held block is placed); sneaking with a hand held, or holding anything
//! else, leaves it to the game; an empty hand and no sneaking is the block's own use, taken by the mod.
use horizon_mod_sdk::server::{BlockPos, BlockUse, ItemStack, Player, Registration, ServerMod};

struct Gate;

impl ServerMod for Gate {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Gate, Registration::new().on_blocks()))
    }
    fn block_used(&mut self, _player: &Player, _pos: BlockPos, sneaking: bool, stack: Option<ItemStack>) -> BlockUse {
        match stack {
            Some(s) if s.item == "minecraft:stone" => BlockUse::Pass,
            None if !sneaking => BlockUse::Consume,
            _ => BlockUse::Default,
        }
    }
}

horizon_mod_sdk::export_server_mod!(Gate);
