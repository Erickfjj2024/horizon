//! Multiblock Demo: the smallest user of multiblocks. `mbdemo:casing` blocks
//! that touch are a candidate; it is a real structure when it is a solid 2x2x2
//! cube of eight. Every member keeps `[valid, members]` in its data, which is
//! where a real mod keeps the structure's shared state.
use horizon_mod_sdk::server::{
    BlockEntityKind, BlockEntityUpdate, Registration, ServerMod, Structure,
};
use horizon_mod_sdk::server::MultiblockSpec;

#[derive(Default)]
struct Demo {
    calls: u8,
}

impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        let kind = BlockEntityKind {
            block: "mbdemo:casing".into(),
            ticks: false,
            container: None,
            energy: None,
            tanks: Vec::new(),
            keep_data: false,
            transmitter: None,
            multiblock: Some(MultiblockSpec {
                group: "mbdemo:cube".into(),
            }),
        };
        // FORK-33: the glass is part of two groups at once, as a pressure disperser is of boilers and turbines.
        let glass = BlockEntityKind {
            block: "mbdemo:glass".into(),
            multiblock: Some(MultiblockSpec { group: "mbdemo:cube,mbdemo:line".into() }),
            ..kind.clone()
        };
        Ok((Demo::default(), Registration::new().block_entity_kind(kind).block_entity_kind(glass)))
    }
    fn multiblock_changed(
        &mut self,
        group: &str,
        dimension: &str,
        structures: Vec<Structure>,
    ) -> Vec<BlockEntityUpdate> {
        let mut out = Vec::new();
        self.calls += 1;
        let calls = self.calls;
        let group_id = if group == "mbdemo:cube" { 1 } else { 2 };
        for s in structures {
            let size = |a: i32, b: i32| b - a + 1;
            let valid = group == "mbdemo:cube" && s.members.len() == 8
                && size(s.min.x, s.max.x) == 2
                && size(s.min.y, s.max.y) == 2
                && size(s.min.z, s.max.z) == 2;
            for m in &s.members {
                out.push(BlockEntityUpdate {
                    dimension: dimension.to_owned(),
                    pos: m.pos,
                    data: if m.state.starts_with("mbdemo:glass") { vec![u8::from(valid), s.members.len() as u8, group_id, calls] } else { vec![u8::from(valid), s.members.len() as u8] },
                    items: None,
                    energy: None,
                    energy_faces: None,
                    tanks: None,
                    tank_faces: None,
                    menu_data: None,
                    transmitter_faces: None,
                });
            }
        }
        out
    }
}

horizon_mod_sdk::export_server_mod!(Demo);
