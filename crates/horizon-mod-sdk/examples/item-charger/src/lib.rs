//! Item Charger: the smallest user of data components. `itemcharger:charger`
//! has one slot; every tick the stack in it gains 10 of its
//! `itemcharger:energy` component, up to 100, and keeps whatever other
//! components it had. The block keeps its slot in the item it drops.
use horizon_mod_sdk::components::Components;
use horizon_mod_sdk::server::{
    BlockEntity, BlockEntityKind, BlockEntityUpdate, ContainerSpec, Registration, ServerMod,
};

struct Charger;

impl ServerMod for Charger {
    fn init() -> Result<(Self, Registration), String> {
        let kind = BlockEntityKind {
            block: "itemcharger:charger".into(),
            ticks: true,
            container: Some(ContainerSpec::open(1)),
            energy: None,
            tanks: Vec::new(),
            keep_data: true,
            transmitter: None,
            multiblock: None,
        };
        Ok((Charger, Registration::new().block_entity_kind(kind)))
    }
    fn block_entities_tick(&mut self, _tick: u64, entities: Vec<BlockEntity>) -> Vec<BlockEntityUpdate> {
        let mut out = Vec::new();
        for e in entities {
            let Some(Some(stack)) = e.items.first() else { continue };
            let Some(mut c) = Components::parse(&stack.components) else { continue };
            let have = c.get_long("itemcharger:energy").unwrap_or(0);
            if have >= 100 {
                continue;
            }
            c.set_long("itemcharger:energy", (have + 10).min(100));
            let mut stack = stack.clone();
            stack.components = c.to_snbt();
            out.push(BlockEntityUpdate {
                dimension: e.dimension,
                pos: e.pos,
                data: e.data,
                items: Some(vec![Some(stack)]),
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

horizon_mod_sdk::export_server_mod!(Charger);
