//! Block Use (FORK-29): a mod that decides what a right click on one of its blocks does. Holding a block, the click
//! passes (the block does not use itself; the held block is placed); sneaking with a hand held, or holding anything
//! else, leaves it to the game; an empty hand and no sneaking is the block's own use, taken by the mod.
use horizon_mod_sdk::server::{BlockPos, BlockUse, ItemStack, Player, Registration, ServerMod, menu_input, players, world};

struct Gate;

impl ServerMod for Gate {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Gate, Registration::new().on_blocks().on_net().on_block_break_start().redirect("blockuse:bound", 0, 1)))
    }
    /// FORK-35: a machine of the mod needs the block above it free: with one over it (y 81 over a block at 80), the placement is refused.
    fn can_place(&mut self, _player: &Player, pos: BlockPos, _state: &str) -> bool {
        world::get_block("minecraft:overworld", pos.x, pos.y + 1, pos.z).is_none_or(|b| b == "minecraft:air")
    }
    /// FORK-20: what a text box of the machine's screen confirms (widget 7800) comes back as the menu's first string.
    fn net_message(&mut self, player: &Player, channel: &str, data: &[u8]) {
        // API 35: a message on `blockuse:hardness` (a position) is answered with the block's hardness as the menu's first string.
        if channel == "blockuse:hardness" && let Some(input) = menu_input::decode(data) {
            let h = world::block_hardness(&player.dimension, input.pos.x, input.pos.y, input.pos.z);
            let text = h.map_or("unloaded".to_owned(), |h| format!("{h}"));
            let _ = world::set_menu_text(&player.dimension, input.pos.x, input.pos.y, input.pos.z, &[text]);
            return;
        }
        // API 42: the choice of the radial menu: its entry's id, said back to the player.
        if channel == "blockuse:radial" {
            let _ = players::tell(&player.id, &format!("mode {}", String::from_utf8_lossy(data)));
            return;
        }
        if channel != menu_input::channel("blockuse") {
            return;
        }
        if let Some(input) = menu_input::decode(data).filter(|i| i.id == 7800) {
            let _ = world::set_menu_text(&player.dimension, input.pos.x, input.pos.y, input.pos.z, &[input.text]);
        }
    }
    /// The mod's drill (`blockuse:drill`) breaks a 1 by 3 column instead of one block: the normal break is cancelled and the three go
    /// with `break-blocks`.
    fn block_break_start(&mut self, player: &Player, stack: ItemStack, pos: BlockPos, _state: &str) -> bool {
        if stack.item != "blockuse:drill" {
            return false;
        }
        let column = [(pos.x, pos.y, pos.z), (pos.x, pos.y - 1, pos.z), (pos.x, pos.y - 2, pos.z)];
        let _ = world::break_blocks(&player.dimension, &column, "minecraft:diamond_pickaxe", false, 0);
        true
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
        // API 38: the machine at y 77 gives a comparator the signal 9.
        if pos.y == 77 {
            let _ = world::set_comparator_output(&player.dimension, pos.x, pos.y, pos.z, 9);
        }
        // API 39: the machine at y 78 mines the 3 blocks east of it (one is air, one bedrock) with a silk touch pickaxe.
        if pos.y == 78 {
            let n = world::break_blocks(&player.dimension, &[(pos.x + 1, pos.y, pos.z), (pos.x + 2, pos.y, pos.z), (pos.x + 3, pos.y, pos.z)], "minecraft:diamond_pickaxe", true, 0).unwrap_or(99);
            let _ = world::set_menu_text(&player.dimension, pos.x, pos.y, pos.z, &[format!("breaks {n}")]);
        }
        // API 42: the machine at y 80 opens a radial menu of three modes on the player's screen, the second one current; the choice comes
        // back on `blockuse:radial` (see `net_message`).
        if pos.y == 80 {
            let entry = |id: &str, label: &str| players::RadialEntry { id: id.into(), label: label.into(), icon: String::new() };
            let _ = players::open_radial_menu(&player.id, &[entry("off", "Off"), entry("vein", "Vein"), entry("extended", "Extended")], 1);
        }
        // API 41: the machine at y 79 puts a line on the player's HUD: a redstone icon, a text and a bar half full.
        if pos.y == 79 {
            let _ = players::set_hud_line(&player.id, "blockuse:energy", "minecraft:redstone", "Energy: 50%", 0.5);
        }
        match stack {
            Some(s) if s.item == "minecraft:stone" => BlockUse::Pass,
            None if !sneaking => BlockUse::Consume,
            _ => BlockUse::Default,
        }
    }
}

horizon_mod_sdk::export_server_mod!(Gate);
