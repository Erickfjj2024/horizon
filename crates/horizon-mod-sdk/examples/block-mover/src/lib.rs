//! Block Mover: the smallest user of block entity slots. `blockmover:mover`
//! has two slots; every tick it moves one item from slot 0 to slot 1 when
//! slot 1 is empty or holds the same item (and has room: 64).
use horizon_mod_sdk::server::{
    BlockEntity, BlockEntityUpdate, ContainerSpec, Registration, ServerMod,
};

struct Mover;

impl ServerMod for Mover {
    fn init() -> Result<(Self, Registration), String> {
        let slots = ContainerSpec {
            slots: 2,
            // Hoppers above feed slot 0; the rest of the faces take from slot 1.
            faces: vec![vec![1], vec![0], vec![1], vec![1], vec![1], vec![1]],
            inputs: vec![0],
            outputs: vec![1],
            menu: false,
        };
        Ok((
            Mover,
            Registration::new().block_entity_with_slots("blockmover:mover", true, slots),
        ))
    }
    fn block_entities_tick(&mut self, _tick: u64, entities: Vec<BlockEntity>) -> Vec<BlockEntityUpdate> {
        let mut out = Vec::new();
        for e in entities {
            let mut items = e.items.clone();
            items.resize(2, None);
            let Some(src) = items[0].clone() else { continue };
            let room = match &items[1] {
                None => true,
                Some(d) => d.item == src.item && d.components == src.components && d.count < 64,
            };
            if !room {
                continue;
            }
            match &mut items[1] {
                Some(d) => d.count += 1,
                slot => {
                    let mut one = src.clone();
                    one.count = 1;
                    *slot = Some(one);
                }
            }
            if src.count <= 1 {
                items[0] = None;
            } else if let Some(s) = &mut items[0] {
                s.count -= 1;
            }
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

horizon_mod_sdk::export_server_mod!(Mover);
