//! Energy Demo: the smallest user of block energy.
//! `energydemo:generator` makes 10 a tick and only gives; `energydemo:battery`
//! stores up to 1000, 50 a tick in and out, and never ticks (the game moves
//! its energy); `energydemo:consumer` burns 5 a tick and only takes.
use horizon_mod_sdk::server::{
    BlockEntity, BlockEntityKind, BlockEntityUpdate, FlowSide, EnergySpec, Registration,
    ServerMod,
};

struct Demo;

fn kind(block: &str, ticks: bool, energy: EnergySpec) -> BlockEntityKind {
    BlockEntityKind {
        block: block.into(),
        ticks,
        container: None,
        energy: Some(energy),
        tanks: Vec::new(),
        keep_data: false,
        transmitter: None,
        multiblock: None,
    }
}

impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        let r = Registration::new()
            .block_entity_kind(kind(
                "energydemo:generator",
                true,
                EnergySpec::open(100, 50, FlowSide::Output),
            ))
            .block_entity_kind(kind(
                "energydemo:battery",
                false,
                EnergySpec::open(1000, 50, FlowSide::Both),
            ))
            .block_entity_kind(kind(
                "energydemo:consumer",
                true,
                EnergySpec::open(100, 50, FlowSide::Input),
            ));
        Ok((Demo, r))
    }
    fn block_entities_tick(&mut self, _tick: u64, entities: Vec<BlockEntity>) -> Vec<BlockEntityUpdate> {
        entities
            .into_iter()
            .filter_map(|e| {
                let energy = if e.state.starts_with("energydemo:generator") {
                    e.energy + 10
                } else if e.state.starts_with("energydemo:consumer") {
                    e.energy.saturating_sub(5)
                } else {
                    return None;
                };
                Some(BlockEntityUpdate {
                    dimension: e.dimension,
                    pos: e.pos,
                    data: e.data,
                    items: None,
                    energy: Some(energy),
                    energy_faces: None,
                    tanks: None,
                    tank_faces: None,
                    menu_data: None,
                    transmitter_faces: None,
                })
            })
            .collect()
    }
}

horizon_mod_sdk::export_server_mod!(Demo);
