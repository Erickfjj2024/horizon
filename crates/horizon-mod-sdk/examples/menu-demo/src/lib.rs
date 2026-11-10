//! Menu Demo: the smallest user of menus. `menudemo:grinder` has two slots
//! and a menu (see `screen-example/grinder.json`, the menu type that draws
//! it). Its menu data are `[progress, paused]`; the progress counts to 100
//! and starts over, unless paused with menu button 1.
use horizon_mod_sdk::server::{
    BlockEntity, BlockEntityKind, BlockEntityUpdate, BlockPos, ContainerSpec, Player, Registration,
    ServerMod,
};

struct Grinder;

fn update(e: &BlockEntity, menu_data: Vec<i32>) -> BlockEntityUpdate {
    BlockEntityUpdate {
        dimension: e.dimension.clone(),
        pos: e.pos,
        data: e.data.clone(),
        items: None,
        energy: None,
        energy_faces: None,
        tanks: None,
        tank_faces: None,
        menu_data: Some(menu_data),
        transmitter_faces: None,
    }
}

impl ServerMod for Grinder {
    fn init() -> Result<(Self, Registration), String> {
        let mut container = ContainerSpec::open(2);
        container.menu = true;
        let kind = BlockEntityKind {
            block: "menudemo:grinder".into(),
            ticks: true,
            container: Some(container),
            energy: None,
            tanks: Vec::new(),
            keep_data: false,
            transmitter: None,
            multiblock: None,
        };
        Ok((Grinder, Registration::new().block_entity_kind(kind)))
    }
    fn block_entities_tick(&mut self, _tick: u64, entities: Vec<BlockEntity>) -> Vec<BlockEntityUpdate> {
        entities
            .iter()
            .filter_map(|e| {
                let progress = e.menu_data.first().copied().unwrap_or(0);
                let paused = e.menu_data.get(1).copied().unwrap_or(0);
                (paused == 0).then(|| update(e, vec![(progress + 1) % 100, 0]))
            })
            .collect()
    }
    fn menu_button(
        &mut self,
        _player: &Player,
        _pos: BlockPos,
        id: i32,
        entity: BlockEntity,
    ) -> Option<BlockEntityUpdate> {
        (id == 1).then(|| {
            let progress = entity.menu_data.first().copied().unwrap_or(0);
            let paused = entity.menu_data.get(1).copied().unwrap_or(0);
            update(&entity, vec![progress, 1 - paused])
        })
    }
}

horizon_mod_sdk::export_server_mod!(Grinder);
