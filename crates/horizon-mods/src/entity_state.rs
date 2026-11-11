//! The state the game keeps for the living entities of the mods' own kinds (FORK-10, the Robit): declared in content
//! (`data/<id>/horizon/entity_kind/*.json`, [`crate::content::EntityKind`]), kept with a state of the mod's own (saved bytes, container
//! slots, energy, skin, name) that outlives the world's save. What a save looks like and how a mod's update applies are here, with
//! tests; the game's part (spawning, ticking, saving to the world's folder) is `game/mod_entities.rs`.
use crate::{ModEntityUpdate, ModItemStack, content::EntityKind};
use serde_json::{Value, json};

/// Where the saved entities are, inside the world's folder.
pub const FILE: &str = "data/horizon/mod_entities.json";

/// One entity of a mod's kind.
#[derive(Clone, Debug, PartialEq)]
pub struct ModEntityState {
    pub id: u32,
    pub dimension: String,
    pub kind: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub health: f32,
    pub data: Vec<u8>,
    pub items: Vec<Option<ModItemStack>>,
    pub energy: u64,
    pub skin: String,
    pub name: Option<String>,
    /// The entity Horizon shows it as (an `item_display` the clients draw with the skin's model), once it has one; not saved.
    pub carrier: Option<u32>,
    /// The skin and the name that carrier was made with, to make it again when either changes; not saved.
    pub shown: Option<(String, Option<String>)>,
}

/// The entities and whether the save was read.
#[derive(Default)]
pub struct ModEntities {
    /// Whether the carriers an earlier run left in the world were taken out.
    pub swept: bool,
    pub loaded: bool,
    pub list: Vec<ModEntityState>,
}

impl ModEntityState {
    /// A new entity of `kind` with `data`: full health, the kind's first skin, an empty container.
    pub fn new(id: u32, dimension: &str, kind: &EntityKind, pos: [f64; 3], data: Vec<u8>) -> Self {
        Self {
            id,
            dimension: dimension.to_owned(),
            kind: kind.id.clone(),
            pos,
            yaw: 0.0,
            health: kind.max_health,
            data,
            items: vec![None; usize::from(kind.container_slots)],
            energy: 0,
            skin: kind.skins.first().map(|s| s.id.clone()).unwrap_or_default(),
            name: None,
            carrier: None,
            shown: None,
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (text.len() % 2 == 0)
        .then(|| (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect())
        .flatten()
}

/// The save of `entities`.
pub fn encode(entities: &[ModEntityState]) -> String {
    let list: Vec<Value> = entities
        .iter()
        .map(|e| {
            json!({
                "id": e.id, "dimension": e.dimension, "kind": e.kind, "pos": e.pos, "yaw": e.yaw, "health": e.health,
                "data": hex(&e.data), "energy": e.energy, "skin": e.skin, "name": e.name,
                "items": e.items.iter().map(|s| s.as_ref().map(|s| json!({"item": s.item, "count": s.count, "components": s.components}))).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "version": 1, "entities": list }).to_string()
}

/// The entities a save holds; a row that is not one is left out.
pub fn decode(text: &str) -> Vec<ModEntityState> {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(rows) = root["entities"].as_array() else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|r| {
            let pos = r["pos"].as_array()?;
            Some(ModEntityState {
                id: u32::try_from(r["id"].as_u64()?).ok()?,
                dimension: r["dimension"].as_str()?.to_owned(),
                kind: r["kind"].as_str()?.to_owned(),
                pos: [pos.first()?.as_f64()?, pos.get(1)?.as_f64()?, pos.get(2)?.as_f64()?],
                yaw: r["yaw"].as_f64().unwrap_or(0.0) as f32,
                health: r["health"].as_f64()? as f32,
                data: unhex(r["data"].as_str().unwrap_or(""))?,
                energy: r["energy"].as_u64().unwrap_or(0),
                skin: r["skin"].as_str().unwrap_or("").to_owned(),
                name: r["name"].as_str().map(str::to_owned),
                carrier: None,
                shown: None,
                items: r["items"]
                    .as_array()?
                    .iter()
                    .map(|s| {
                        s.as_object().and_then(|s| {
                            Some(ModItemStack {
                                item: s.get("item")?.as_str()?.to_owned(),
                                count: u32::try_from(s.get("count")?.as_u64()?).ok()?,
                                components: s.get("components").and_then(Value::as_str).unwrap_or("").to_owned(),
                            })
                        })
                    })
                    .collect(),
            })
        })
        .collect()
}

/// The compound (SNBT) of the `item_display` that stands for an entity of `kind` with `skin` and `name`: it shows a stick whose
/// `item_model` is the skin's model (`<ns>:<path>`, `assets/<ns>/items/<path>.json`), faces the same way whatever the viewer does,
/// eases moves over 2 ticks, and carries a tag the game finds its own by.
pub fn carrier_snbt(kind: &EntityKind, skin: &str, name: Option<&str>) -> String {
    let components = kind
        .skins
        .iter()
        .find(|s| s.id == skin)
        .and_then(|s| s.model.as_deref())
        .map_or_else(String::new, |m| format!(",components:{{\"minecraft:item_model\":\"{m}\"}}"));
    let name = name.map_or_else(String::new, |n| {
        let json = serde_json::to_string(&json!({ "text": n })).unwrap_or_default();
        format!(",CustomName:'{}',CustomNameVisible:1b", json.replace('\\', "\\\\").replace('\'', "\\'"))
    });
    format!(
        "{{item:{{id:\"minecraft:stick\",count:1{components}}},teleport_duration:2,Tags:[\"{CARRIER_TAG}\"]{name}}}"
    )
}

/// The tag every carrier has, for the game to tell them from other displays.
pub const CARRIER_TAG: &str = "horizon_mod_entity";

/// An update the mod made (`mod-entity-update`) applied to `e` (`kind` is its kind): what it changes, whether it asks for the
/// entity to be taken out. A skin the kind does not have, and slots past its container, are dropped.
pub fn apply(e: &mut ModEntityState, kind: &EntityKind, u: &ModEntityUpdate) -> bool {
    if let Some(d) = &u.data {
        e.data.clone_from(d);
    }
    if let Some(items) = &u.items {
        let mut items = items.clone();
        items.resize(usize::from(kind.container_slots), None);
        e.items = items;
    }
    if let Some(energy) = u.energy {
        e.energy = energy;
    }
    if let Some(skin) = &u.skin
        && kind.skins.iter().any(|s| &s.id == skin)
    {
        e.skin.clone_from(skin);
    }
    if let Some(name) = &u.name {
        e.name.clone_from(name);
    }
    if let Some((x, y, z)) = u.teleport {
        e.pos = [x, y, z];
    }
    u.remove
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::EntitySkin;

    fn kind() -> EntityKind {
        EntityKind {
            id: "robits:robit".into(),
            width: 0.6,
            height: 0.65,
            eye_height: None,
            max_health: 1.0,
            speed: 0.3,
            persistent: true,
            skins: vec![
                EntitySkin { id: "default".into(), texture: "robits:entity/robit".into(), model: None },
                EntitySkin { id: "red".into(), texture: "robits:entity/red".into(), model: None },
            ],
            container_slots: 3,
        }
    }

    fn update(id: u32) -> ModEntityUpdate {
        ModEntityUpdate {
            dimension: "minecraft:overworld".into(),
            id,
            data: None,
            items: None,
            energy: None,
            skin: None,
            name: None,
            teleport: None,
            remove: false,
        }
    }

    #[test]
    fn the_carrier_shows_the_skins_model_and_the_name() {
        let mut k = kind();
        k.skins[1].model = Some("robits:robit_red".into());
        // A skin without a model shows the plain stick; one with it names the model in the item's components.
        assert_eq!(
            carrier_snbt(&k, "default", None),
            r#"{item:{id:"minecraft:stick",count:1},teleport_duration:2,Tags:["horizon_mod_entity"]}"#
        );
        assert_eq!(
            carrier_snbt(&k, "red", Some("Bo'lt")),
            r#"{item:{id:"minecraft:stick",count:1,components:{"minecraft:item_model":"robits:robit_red"}},teleport_duration:2,Tags:["horizon_mod_entity"],CustomName:'{"text":"Bo\'lt"}',CustomNameVisible:1b}"#
        );
    }

    #[test]
    fn a_new_entity_has_full_health_the_first_skin_and_an_empty_container() {
        let e = ModEntityState::new(7, "minecraft:overworld", &kind(), [1.0, 64.0, 2.0], vec![9]);
        assert_eq!((e.health, e.skin.as_str(), e.items.len(), e.data.clone()), (1.0, "default", 3, vec![9]));
    }

    #[test]
    fn a_save_gives_back_what_was_kept() {
        let mut e = ModEntityState::new(7, "minecraft:overworld", &kind(), [1.5, 64.0, -2.25], vec![0, 255, 16]);
        e.energy = 123_456_789_012;
        e.name = Some("Bolt".into());
        e.yaw = 90.0;
        e.items[1] = Some(ModItemStack { item: "minecraft:coal".into(), count: 5, components: "{a:1}".into() });
        let again = decode(&encode(&[e.clone(), e.clone()]));
        assert_eq!(again, vec![e.clone(), e]);
        // A broken row is left out, a broken file is empty.
        assert!(decode(r#"{"entities":[{"id":1}]}"#).is_empty());
        assert!(decode("not json").is_empty());
    }

    #[test]
    fn an_update_changes_what_it_names_and_drops_what_does_not_fit() {
        let k = kind();
        let mut e = ModEntityState::new(1, "minecraft:overworld", &k, [0.0; 3], vec![1]);
        let mut u = update(1);
        u.data = Some(vec![2, 3]);
        u.energy = Some(40);
        u.skin = Some("red".into());
        u.name = Some(Some("Bolt".into()));
        u.teleport = Some((5.0, 70.0, 5.0));
        u.items = Some(vec![Some(ModItemStack { item: "minecraft:stick".into(), count: 1, components: String::new() }); 5]);
        assert!(!apply(&mut e, &k, &u));
        assert_eq!((e.data.clone(), e.energy, e.skin.as_str(), e.pos), (vec![2, 3], 40, "red", [5.0, 70.0, 5.0]));
        assert_eq!(e.name.as_deref(), Some("Bolt"));
        assert_eq!(e.items.len(), 3, "the container keeps its 3 slots");
        // An unknown skin is ignored, `some(none)` takes the name away, remove is told back.
        let mut u = update(1);
        u.skin = Some("gold".into());
        u.name = Some(None);
        u.remove = true;
        assert!(apply(&mut e, &k, &u));
        assert_eq!((e.skin.as_str(), e.name.clone()), ("red", None));
    }
}
