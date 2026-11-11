//! Tank Demo: the smallest user of block tanks. `tankdemo:pump` makes 20 of
//! `minecraft:water` a tick and only gives; `tankdemo:tank` stores 1000
//! fluid, 50 a tick in and out, and never ticks; `tankdemo:gas_tank` is a
//! `gas` tank, which no fluid enters. `tankdemo:big_tank` gives a bucket a tick and more, so it fills an empty cauldron beside it.
use horizon_mod_sdk::server::{
    BlockEntity, BlockEntityKind, BlockEntityUpdate, FlowSide, Registration, ServerMod, Tank,
    TankSpec,
};

struct Demo;

fn kind(block: &str, ticks: bool, tank: TankSpec) -> BlockEntityKind {
    BlockEntityKind {
        block: block.into(),
        ticks,
        container: None,
        energy: None,
        tanks: vec![tank],
        keep_data: false,
        transmitter: None,
        multiblock: None,
    }
}
fn spec(kind: &str, side: FlowSide) -> TankSpec {
    TankSpec {
        kind: kind.into(),
        capacity: 1000,
        max_rate: 50,
        faces: vec![side; 6],
    }
}

impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        let r = Registration::new()
            .block_entity_kind(kind("tankdemo:pump", true, spec("fluid", FlowSide::Output)))
            .block_entity_kind(kind("tankdemo:tank", false, spec("fluid", FlowSide::Both)))
            .block_entity_kind(kind("tankdemo:gas_tank", false, spec("gas", FlowSide::Both)))
            // A big tank that only gives, a bucket a tick and more: it fills an empty cauldron beside it with water.
            .block_entity_kind(kind("tankdemo:big_tank", false, TankSpec { kind: "fluid".into(), capacity: 8000, max_rate: 64_000, faces: vec![FlowSide::Output; 6] }));
        Ok((Demo, r))
    }
    fn block_entities_tick(&mut self, _tick: u64, entities: Vec<BlockEntity>) -> Vec<BlockEntityUpdate> {
        entities
            .into_iter()
            .filter(|e| e.state.starts_with("tankdemo:pump"))
            .map(|e| {
                let held = e.tanks.first().map_or(0, |t| t.amount);
                BlockEntityUpdate {
                    dimension: e.dimension,
                    pos: e.pos,
                    data: e.data,
                    items: None,
                    energy: None,
                    energy_faces: None,
                    tanks: Some(vec![Tank {
                        substance: "minecraft:water".into(),
                        amount: held + 20,
                    }]),
                    tank_faces: None,
                    menu_data: None,
                    transmitter_faces: None,
                }
            })
            .collect()
    }
}

horizon_mod_sdk::export_server_mod!(Demo);
