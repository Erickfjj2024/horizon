//! Item stacks in NBT (`ItemStack.CODEC`): id, count and components, alone or in
//! slot lists.
use horizon_client_core::nbt_value::{number, string, text};
use horizon_world::nbt::Tag;
use std::sync::Arc;

pub fn encode_item(stack: &crate::items::ItemStack) -> Vec<(&'static str, Tag)> {
    let mut entries = vec![
        ("id", string(&stack.item)),
        ("count", Tag::Int(stack.count as i32)),
    ];
    if let Some(components) = crate::item_data::encode_components(stack) {
        entries.push(("components", components));
    }
    entries
}
/// ItemStack.CODEC (id, count, components).
pub fn item_to_nbt(stack: &crate::items::ItemStack) -> Tag {
    Tag::compound(encode_item(stack))
}
pub fn item_from_nbt(tag: &Tag) -> Option<crate::items::ItemStack> {
    let id = text(tag.get("id"))?;
    if id == "minecraft:air" {
        return None;
    }
    let count = number(tag.get("count"))
        .or_else(|| number(tag.get("Count")))
        .unwrap_or(1)
        .max(1) as u32;
    let (damage, extra) = crate::item_data::decode_components(tag.get("components"));
    Some(crate::items::ItemStack {
        item: Arc::from(id.as_str()),
        count,
        damage,
        extra,
    })
}
pub fn encode_items(slots: &[Option<crate::items::ItemStack>]) -> Tag {
    Tag::List(
        slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let mut entries = vec![("Slot", Tag::Byte(i as i8))];
                entries.extend(encode_item(s.as_ref()?));
                Some(Tag::compound(entries))
            })
            .collect(),
    )
}
pub fn decode_items(tag: Option<&Tag>, size: usize) -> Vec<Option<crate::items::ItemStack>> {
    let mut slots = vec![None; size];
    if let Some(Tag::List(items)) = tag {
        for it in items {
            let slot = number(it.get("Slot")).unwrap_or(0) as u8 as usize;
            if let Some(cell) = slots.get_mut(slot) {
                *cell = item_from_nbt(it);
            }
        }
    }
    slots
}

#[cfg(test)]
mod mod_component_tests {
    use super::*;
    use horizon_world::nbt::parse_snbt;

    /// A component of a mod's own (`mymod:energy`) rides on a stack through
    /// NBT and back untouched, next to a vanilla one (API block 5).
    #[test]
    fn a_mod_component_round_trips() {
        let (components, _) =
            parse_snbt(r#"{"minecraft:damage":3,"mymod:energy":1500L,"mymod:config":{side:"left",on:1b}}"#)
                .unwrap();
        let tag = Tag::compound([
            ("id", Tag::String("minecraft:stick".into())),
            ("count", Tag::Int(2)),
            ("components", components.clone()),
        ]);
        let stack = item_from_nbt(&tag).unwrap();
        assert_eq!(stack.damage, 3);
        let back = item_to_nbt(&stack);
        let got = back.get("components").unwrap();
        assert_eq!(got.get("mymod:energy"), components.get("mymod:energy"));
        assert_eq!(got.get("mymod:config"), components.get("mymod:config"));
        assert_eq!(got.get("minecraft:damage"), components.get("minecraft:damage"));
        // And the slot list a container saves.
        let slots = vec![Some(stack.clone()), None];
        let list = encode_items(&slots);
        assert_eq!(decode_items(Some(&list), 2)[0], Some(stack));
    }
}
