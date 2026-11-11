//! World Data Probe (FORK-36, `world-data`): each tick it adds one to `wdprobe:counter` (a little-endian u32 kept in the world),
//! writes the list of its keys under `wdprobe:frequencies/` into `wdprobe:list`, and on its first tick tries what the limits refuse
//! (another mod's key, a value over 256 KiB) and writes the errors' count into `wdprobe:refused`. `wdprobe:drop` is removed when seen.
use horizon_mod_sdk::server::{Registration, ServerMod, world_data};

struct Probe {
    tried: bool,
}

impl ServerMod for Probe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Probe { tried: false }, Registration::new().on_tick()))
    }
    fn tick(&mut self, _tick: u64) {
        let count = world_data::get("wdprobe:counter").and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes);
        let _ = world_data::set("wdprobe:counter", &(count + 1).to_le_bytes());
        let keys = world_data::keys("wdprobe:frequencies/");
        let _ = world_data::set("wdprobe:list", keys.join(",").as_bytes());
        if world_data::get("wdprobe:drop").is_some() {
            world_data::remove("wdprobe:drop");
        }
        if !self.tried {
            self.tried = true;
            let refused = [world_data::set("mekanism:owners", &[1]), world_data::set("wdprobe:big", &vec![0; 256 * 1024 + 1])]
                .iter()
                .filter(|r| r.is_err())
                .count() as u8;
            let _ = world_data::set("wdprobe:refused", &[refused]);
        }
    }
}

horizon_mod_sdk::export_server_mod!(Probe);
