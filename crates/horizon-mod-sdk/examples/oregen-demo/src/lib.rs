//! Ore Gen: the smallest user of `chunk-loaded`. In every chunk that was just
//! generated it replaces stone with `oregen:ore` along a short vein at
//! y = 12 starting at the chunk's centre (a real mod picks its places with its
//! own random numbers from the chunk's coordinates), and it keeps chunk 0, 0
//! loaded. `set_blocks` is made at the end of the tick, with the chunk there.
use horizon_mod_sdk::config::Config;
use horizon_mod_sdk::server::{BlockChange, Registration, ServerMod, chunks, world};

struct OreGen {
    /// How long a vein is, from the mod's config (default 4).
    vein: i64,
}

impl ServerMod for OreGen {
    fn init() -> Result<(Self, Registration), String> {
        let config = Config::load("config.txt");
        let vein = config.get_i64("vein_length", 4, 1, 16);
        Ok((OreGen { vein }, Registration::new().on_chunks()))
    }
    fn chunk_loaded(&mut self, dimension: &str, chunk_x: i32, chunk_z: i32, new: bool) {
        if chunk_x == 0 && chunk_z == 0 {
            let _ = chunks::set_forced(dimension, 0, 0, true);
        }
        if !new {
            return;
        }
        let (x0, z0) = (chunk_x * 16 + 8, chunk_z * 16 + 8);
        let mut changes = Vec::new();
        for i in 0..self.vein as i32 {
            let (x, y, z) = (x0 + i, 12, z0);
            if world::get_block(dimension, x, y, z).as_deref() == Some("minecraft:stone") {
                changes.push(BlockChange {
                    x,
                    y,
                    z,
                    state: "oregen:ore".into(),
                });
            }
        }
        if !changes.is_empty() {
            let _ = world::set_blocks(dimension, &changes);
        }
    }
}

horizon_mod_sdk::export_server_mod!(OreGen);
