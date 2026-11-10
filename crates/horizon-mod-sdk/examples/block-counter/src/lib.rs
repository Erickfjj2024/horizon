//! Block Counter: the smallest user of block entities. Every
//! `blockcounter:counter` block keeps a little-endian `u32` of the ticks it
//! has been loaded; the game saves the bytes with the chunk, so the count
//! survives closing the world.
use horizon_mod_sdk::server::{BlockEntity, BlockEntityUpdate, Registration, ServerMod};

struct Counter;

impl ServerMod for Counter {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Counter, Registration::new().block_entity("blockcounter:counter", true)))
    }
    fn block_entities_tick(&mut self, _tick: u64, entities: Vec<BlockEntity>) -> Vec<BlockEntityUpdate> {
        entities
            .into_iter()
            .map(|e| {
                let n = e
                    .data
                    .get(..4)
                    .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                BlockEntityUpdate {
                    dimension: e.dimension,
                    pos: e.pos,
                    data: (n + 1).to_le_bytes().to_vec(),
                    items: None,
                    energy: None,
                    energy_faces: None,
                    tanks: None,
                    tank_faces: None,
                    menu_data: None,
                    transmitter_faces: None,
                }
            })
            .collect()
    }
}

horizon_mod_sdk::export_server_mod!(Counter);
