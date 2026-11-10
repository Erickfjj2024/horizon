//! Recipes of types a mod brings (API block 6): `data/<ns>/recipe/*.json`
//! whose `type` is outside `minecraft`, kept as their files say, and the
//! item tags that resolve an ingredient. The game builds a [`RecipeData`]
//! from its tables and lends it; the mod reads the recipes and asks which
//! items an ingredient names, and does its own matching.
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// What capability `recipe-types` answers from.
#[derive(Clone, Debug, Default)]
pub struct RecipeData {
    /// Recipe type (`mymod:crushing`) to its recipes, each one's id
    /// (`mymod:crushing/iron`) and file, in file order.
    pub recipes: BTreeMap<String, Vec<(String, String)>>,
    /// Item tag (`c:ores/iron`, no `#`) to its items in the game's order.
    pub tags: HashMap<String, Vec<String>>,
    /// Every item the game knows. Empty: nothing is filtered out.
    pub items: HashSet<String>,
    /// The cooking recipes of the game's own kinds, in the game's order.
    pub cooking: Vec<Cooking>,
    /// Item to the ticks it burns in a furnace (only the items that burn).
    pub burn_times: HashMap<String, u32>,
}

/// One cooking recipe (`smelting`, `blasting`...).
#[derive(Clone, Debug, PartialEq)]
pub struct Cooking {
    pub kind: String,
    pub inputs: Vec<String>,
    pub output: String,
    pub count: u32,
    pub experience: f32,
    pub time: u32,
}

/// A bare id as `minecraft:`, as vanilla reads one.
fn qualified(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

impl RecipeData {
    /// The items an ingredient names, in order, once each: its `ingredient`
    /// JSON is an item id, a `#tag`, a list of them, or an object with an
    /// `item`, a `tag` or `items`. Items the game doesn't know are left out,
    /// and so is anything else it doesn't understand.
    pub fn ingredient_items(&self, ingredient: &str) -> Vec<String> {
        let Ok(json) = serde_json::from_str::<Value>(ingredient) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        self.collect(&json, &mut out);
        let mut seen = HashSet::new();
        out.retain(|i| (self.items.is_empty() || self.items.contains(i)) && seen.insert(i.clone()));
        out
    }

    fn collect(&self, v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => match s.strip_prefix('#') {
                Some(tag) => {
                    if let Some(items) = self.tags.get(&qualified(tag)) {
                        out.extend(items.iter().cloned());
                    }
                }
                None => out.push(qualified(s)),
            },
            Value::Array(a) => a.iter().for_each(|v| self.collect(v, out)),
            Value::Object(o) => {
                // NeoForge's `neoforge:intersection` (what every child matches) and `neoforge:difference` (the base
                // without what `subtracted` matches).
                match o.get("neoforge:ingredient_type").or_else(|| o.get("type")).and_then(Value::as_str) {
                    Some("neoforge:intersection") => {
                        let mut sets = o
                            .get("children")
                            .and_then(Value::as_array)
                            .map(|c| {
                                c.iter()
                                    .map(|child| {
                                        let mut found = Vec::new();
                                        self.collect(child, &mut found);
                                        found
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        if let Some(first) = (!sets.is_empty()).then(|| sets.remove(0)) {
                            out.extend(first.into_iter().filter(|i| sets.iter().all(|s| s.contains(i))));
                        }
                        return;
                    }
                    Some("neoforge:difference") => {
                        let (mut base, mut subtracted) = (Vec::new(), Vec::new());
                        if let Some(b) = o.get("base") {
                            self.collect(b, &mut base);
                        }
                        if let Some(s) = o.get("subtracted") {
                            self.collect(s, &mut subtracted);
                        }
                        out.extend(base.into_iter().filter(|i| !subtracted.contains(i)));
                        return;
                    }
                    _ => {}
                }
                if let Some(item) = o.get("item").and_then(Value::as_str) {
                    out.push(qualified(item));
                }
                if let Some(tag) = o.get("tag").and_then(Value::as_str) {
                    self.collect(&Value::String(format!("#{tag}")), out);
                }
                if let Some(items) = o.get("items") {
                    self.collect(items, out);
                }
                // NeoForge's `neoforge:or` ingredient: any of its `values`.
                if let Some(values) = o.get("values") {
                    self.collect(values, out);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> RecipeData {
        RecipeData {
            recipes: BTreeMap::new(),
            tags: HashMap::from([
                (
                    "c:dusts/redstone".into(),
                    vec!["minecraft:redstone".into(), "mymod:redstone_dust".into()],
                ),
                ("c:empty".into(), vec![]),
            ]),
            cooking: Vec::new(),
            burn_times: HashMap::new(),
            items: ["minecraft:redstone", "minecraft:stone", "mymod:redstone_dust"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
    #[test]
    fn an_ingredient_names_items_tags_and_lists() {
        let d = data();
        assert_eq!(d.ingredient_items(r#""stone""#), ["minecraft:stone"]);
        assert_eq!(
            d.ingredient_items(r##""#c:dusts/redstone""##),
            ["minecraft:redstone", "mymod:redstone_dust"]
        );
        assert_eq!(
            d.ingredient_items(r##"["minecraft:stone", "#c:dusts/redstone", "minecraft:stone"]"##),
            ["minecraft:stone", "minecraft:redstone", "mymod:redstone_dust"]
        );
        assert_eq!(d.ingredient_items(r#"{"item":"minecraft:stone"}"#), ["minecraft:stone"]);
        assert_eq!(
            d.ingredient_items(r#"{"tag":"c:dusts/redstone"}"#),
            ["minecraft:redstone", "mymod:redstone_dust"]
        );
    }
    #[test]
    fn a_neoforge_or_is_any_of_its_values() {
        let d = data();
        assert_eq!(
            d.ingredient_items(r##"{"type":"neoforge:or","values":["#c:dusts/redstone","minecraft:stone"]}"##),
            ["minecraft:redstone", "mymod:redstone_dust", "minecraft:stone"]
        );
    }
    #[test]
    fn intersections_and_differences_of_ingredients() {
        let d = data();
        assert_eq!(
            d.ingredient_items(r##"{"neoforge:ingredient_type":"neoforge:intersection","children":["#c:dusts/redstone",["minecraft:redstone","minecraft:stone"]]}"##),
            ["minecraft:redstone"]
        );
        assert_eq!(
            d.ingredient_items(r##"{"neoforge:ingredient_type":"neoforge:difference","base":"#c:dusts/redstone","subtracted":"minecraft:redstone"}"##),
            ["mymod:redstone_dust"]
        );
    }
    #[test]
    fn unknown_things_are_left_out() {
        let d = data();
        assert!(d.ingredient_items(r#""minecraft:nope""#).is_empty());
        assert!(d.ingredient_items(r##""#c:empty""##).is_empty());
        assert!(d.ingredient_items(r##""#c:missing""##).is_empty());
        assert!(d.ingredient_items("17").is_empty());
        assert!(d.ingredient_items("{not json").is_empty());
    }
}
