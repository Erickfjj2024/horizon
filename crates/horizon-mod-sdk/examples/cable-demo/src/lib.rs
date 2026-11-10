//! Cable Demo: the smallest users of transmitter networks.
//! `cabledemo:generator` makes 10 energy a tick and only gives;
//! `cabledemo:cable` joins what touches it (throughput 30);
//! `cabledemo:machine` stores up to 100 and only takes. And fluids:
//! `cabledemo:pump` makes 20 of `minecraft:water` a tick, `cabledemo:pipe`
//! carries it (throughput 15) and `cabledemo:tank` stores up to 1000.
use horizon_mod_sdk::server::{
    BlockEntity, BlockEntityKind, BlockEntityUpdate, EnergySpec, FlowSide, Registration,
    ServerMod, Tank, TankSpec, TransmitterMedium, TransmitterSpec,
};

struct Demo;

fn kind(block: &str, ticks: bool) -> BlockEntityKind {
    BlockEntityKind {
        block: block.into(),
        ticks,
        container: None,
        energy: None,
        tanks: Vec::new(),
        keep_data: false,
        transmitter: None,
        multiblock: None,
    }
}
fn tank(side: FlowSide) -> TankSpec {
    TankSpec {
        kind: "fluid".into(),
        capacity: 1000,
        max_rate: 100,
        faces: vec![side; 6],
    }
}

impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        let mut generator = kind("cabledemo:generator", true);
        generator.energy = Some(EnergySpec::open(100, 50, FlowSide::Output));
        let mut cable = kind("cabledemo:cable", false);
        cable.transmitter = Some(TransmitterSpec {
            medium: TransmitterMedium::Energy,
            throughput: 30,
        });
        let mut machine = kind("cabledemo:machine", false);
        machine.energy = Some(EnergySpec::open(100, 50, FlowSide::Input));
        let mut pump = kind("cabledemo:pump", true);
        pump.tanks = vec![tank(FlowSide::Output)];
        let mut pipe = kind("cabledemo:pipe", false);
        pipe.transmitter = Some(TransmitterSpec {
            medium: TransmitterMedium::Tank("fluid".into()),
            throughput: 15,
        });
        let mut store = kind("cabledemo:tank", false);
        store.tanks = vec![tank(FlowSide::Input)];
        let r = [generator, cable, machine, pump, pipe, store]
            .into_iter()
            .fold(Registration::new(), Registration::block_entity_kind);
        Ok((Demo, r))
    }
    fn block_entities_tick(&mut self, _tick: u64, entities: Vec<BlockEntity>) -> Vec<BlockEntityUpdate> {
        entities
            .into_iter()
            .filter_map(|e| {
                let mut u = BlockEntityUpdate {
                    dimension: e.dimension.clone(),
                    pos: e.pos,
                    data: e.data.clone(),
                    items: None,
                    energy: None,
                    energy_faces: None,
                    tanks: None,
                    tank_faces: None,
                    menu_data: None,
                    transmitter_faces: None,
                };
                if e.state.starts_with("cabledemo:generator") {
                    u.energy = Some(e.energy + 10);
                } else if e.state.starts_with("cabledemo:pump") {
                    let held = e.tanks.first().map_or(0, |t| t.amount);
                    u.tanks = Some(vec![Tank {
                        substance: "minecraft:water".into(),
                        amount: held + 20,
                    }]);
                } else {
                    return None;
                }
                Some(u)
            })
            .collect()
    }
}

horizon_mod_sdk::export_server_mod!(Demo);
