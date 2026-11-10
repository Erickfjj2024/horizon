//! Recipe Crusher: the smallest user of recipe types. Recipes are files
//! `data/<ns>/recipe/*.json` of type `recipecrusher:crushing`:
//!
//! ```json
//! { "type": "recipecrusher:crushing",
//!   "input": { "ingredient": "#c:ores/iron", "count": 1 },
//!   "output": { "id": "minecraft:raw_iron", "count": 2 } }
//! ```
//!
//! `recipecrusher:crusher` has slot 0 (input) and slot 1 (output). Every
//! tick it crushes once when slot 0 holds an item some recipe takes (and
//! enough of it) and slot 1 has room for the output.
use std::collections::HashMap;

use horizon_mod_sdk::server::serde_json::Value;
use horizon_mod_sdk::server::{
    BlockEntity, BlockEntityKind, BlockEntityUpdate, ContainerSpec, ItemStack, Registration,
    ServerMod, recipe_types,
};

const KIND: &str = "recipecrusher:crushing";

struct Recipe {
    count: u32,
    output: ItemStack,
}

struct Crusher {
    /// The recipe each input item matches, resolved once from the tags.
    by_input: Option<HashMap<String, Recipe>>,
}

impl Crusher {
    fn load() -> HashMap<String, Recipe> {
        let mut by_input = HashMap::new();
        for r in recipe_types::recipes_of_type(KIND) {
            let Ok(json) = horizon_mod_sdk::server::serde_json::from_str::<Value>(&r.json) else {
                continue;
            };
            let Some(id) = json["output"]["id"].as_str() else { continue };
            let out = ItemStack::new(id, json["output"]["count"].as_u64().unwrap_or(1) as u32);
            let count = json["input"]["count"].as_u64().unwrap_or(1) as u32;
            let ingredient = json["input"]["ingredient"].to_string();
            for item in recipe_types::ingredient_items(&ingredient) {
                by_input.entry(item).or_insert(Recipe {
                    count,
                    output: out.clone(),
                });
            }
        }
        by_input
    }
}

impl ServerMod for Crusher {
    fn init() -> Result<(Self, Registration), String> {
        let kind = BlockEntityKind {
            block: "recipecrusher:crusher".into(),
            ticks: true,
            container: Some(ContainerSpec::open(2)),
            energy: None,
            tanks: Vec::new(),
            keep_data: true,
            transmitter: None,
            multiblock: None,
        };
        Ok((Crusher { by_input: None }, Registration::new().block_entity_kind(kind)))
    }
    fn block_entities_tick(&mut self, _tick: u64, entities: Vec<BlockEntity>) -> Vec<BlockEntityUpdate> {
        // The tables are lent after the mod starts: read them on the first tick.
        let recipes = self.by_input.get_or_insert_with(Self::load);
        let mut out = Vec::new();
        for e in entities {
            let mut items = e.items.clone();
            items.resize(2, None);
            let Some(input) = items[0].clone() else { continue };
            let Some(r) = recipes.get(&input.item) else { continue };
            if input.count < r.count {
                continue;
            }
            let fits = match &items[1] {
                None => true,
                Some(o) => o.item == r.output.item && o.count + r.output.count <= 64,
            };
            if !fits {
                continue;
            }
            match &mut items[1] {
                Some(o) => o.count += r.output.count,
                slot => *slot = Some(r.output.clone()),
            }
            items[0] = (input.count > r.count).then(|| ItemStack {
                count: input.count - r.count,
                ..input
            });
            out.push(BlockEntityUpdate {
                dimension: e.dimension,
                pos: e.pos,
                data: e.data,
                items: Some(items),
                energy: None,
                energy_faces: None,
                tanks: None,
                tank_faces: None,
                menu_data: None,
                transmitter_faces: None,
            });
        }
        out
    }
}

horizon_mod_sdk::export_server_mod!(Crusher);
