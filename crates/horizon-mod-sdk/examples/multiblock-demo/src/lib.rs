//! Multiblock Demo: the smallest user of multiblocks. `mbdemo:casing` blocks
//! that touch are a candidate; it is a real structure when it is a solid 2x2x2
//! cube of eight. Every member keeps `[valid, members]` in its data, which is
//! where a real mod keeps the structure's shared state.
use horizon_mod_sdk::server::{
    BlockEntityKind, BlockEntityUpdate, Registration, ServerMod, Structure,
};
use horizon_mod_sdk::server::MultiblockSpec;

struct Demo;

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
        Ok((Demo, Registration::new().block_entity_kind(kind)))
    }
    fn multiblock_changed(
        &mut self,
        _group: &str,
        dimension: &str,
        structures: Vec<Structure>,
    ) -> Vec<BlockEntityUpdate> {
        let mut out = Vec::new();
        for s in structures {
            let size = |a: i32, b: i32| b - a + 1;
            let valid = s.members.len() == 8
                && size(s.min.x, s.max.x) == 2
                && size(s.min.y, s.max.y) == 2
                && size(s.min.z, s.max.z) == 2;
            for m in &s.members {
                out.push(BlockEntityUpdate {
                    dimension: dimension.to_owned(),
                    pos: m.pos,
                    data: vec![u8::from(valid), s.members.len() as u8],
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
