//! Crafting Probe (`crafting-find`, `crafting-matches`, `item-max-stack-size`): using `craftprobe:probe` tells the player what two
//! planks over each other make, whether recipe `minecraft:stick` accepts the grid, what stays in the cells, and a stick's stack size.
use horizon_mod_sdk::server::{BlockPos, ItemStack, Player, Registration, ServerMod, players, recipe_types};

struct Probe;

fn stack(item: &str, count: u32) -> Option<ItemStack> {
    Some(ItemStack { item: item.into(), count, components: String::new() })
}

impl ServerMod for Probe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Probe, Registration::new().on_items()))
    }
    fn item_used(&mut self, player: &Player, s: ItemStack, _pos: Option<BlockPos>) -> bool {
        if s.item != "craftprobe:probe" {
            return false;
        }
        let mut grid: Vec<Option<ItemStack>> = vec![None; 9];
        grid[1] = stack("minecraft:oak_planks", 1);
        grid[4] = stack("minecraft:oak_planks", 1);
        let found = recipe_types::crafting_find(&grid);
        let text = match &found {
            Some(m) => format!(
                "{} makes {}*{} keeps {}",
                m.id,
                m.output.item,
                m.output.count,
                m.remaining.iter().flatten().count()
            ),
            None => "nothing".to_owned(),
        };
        let matches = recipe_types::crafting_matches("minecraft:stick", &grid);
        let other = recipe_types::crafting_matches("minecraft:torch", &grid);
        let size = recipe_types::item_max_stack_size("minecraft:stick").map_or("none".to_owned(), |n| n.to_string());
        let tags = recipe_types::item_tags("minecraft:oak_planks").join(",");
        // API 40: the items of a tag, in its order; a `#` in front is the same tag, an unknown tag has none.
        let members = recipe_types::item_tag_members("#minecraft:planks").join(",");
        let none = recipe_types::item_tag_members("c:nothing").len();
        let _ = players::tell(&player.id, &format!("{text}; stick {matches}; torch {other}; size {size}; tags {tags}; planks {members}; nothing {none}"));
        true
    }
}

horizon_mod_sdk::export_server_mod!(Probe);
