//! Builds the content root for one or more extracted content mods, as the game does at its start, and says how long
//! and (with `/usr/bin/time -v`) how much memory it took; then loads the tables the game loads from it.
//!
//! cargo run -p horizon-client --example mod_build_probe -- BASE OUT MOD_DIR...
use horizon_client::{
    Result,
    assets::Assets,
    blocks::BlockFacts,
    mod_content::{self, ContentMod},
};
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [base, out, dirs @ ..] = args.as_slice() else {
        return Err("usage: mod_build_probe BASE OUT MOD_DIR...".into());
    };
    let mut mods = Vec::new();
    for dir in dirs {
        let package = horizon_mods::Package::read(Path::new(dir))?;
        let content = horizon_mods::Content::read(Path::new(dir), &package.manifest)?;
        mods.push(ContentMod { id: package.manifest.id.clone(), content });
    }
    mods.sort_by(|a, b| a.id.cmp(&b.id));
    // PHASE=build stops after the build, PHASE=load only loads what an earlier run built.
    let phase = std::env::var("PHASE").unwrap_or_default();
    if phase != "load" {
        let began = std::time::Instant::now();
        let warnings = mod_content::build(Path::new(base), &mods, Path::new(out))?;
        println!("built in {:.1}s ({} warnings)", began.elapsed().as_secs_f64(), warnings.len());
    }
    if phase == "build" {
        return Ok(());
    }
    let began = std::time::Instant::now();
    let assets = Assets::load(Path::new(out))?;
    let facts = BlockFacts::load(Path::new(out), assets.registry)?;
    println!("loaded {} states in {:.1}s", facts.states.len(), began.elapsed().as_secs_f64());
    // CHECK=block,block: the state each block has in the registry of the root.
    // What the game loads from the root besides the registry and the block facts.
    let items = horizon_client::items::Items::load(
        Path::new(out),
        assets.registry,
        &facts,
        &horizon_world::loot::PackOverlay::NONE,
    )?;
    let collisions = horizon_client::collisions::load(Path::new(out), assets.registry)?;
    let _ = (&items, &collisions);
    println!("items and collisions loaded");
    let checks: Vec<String> = std::env::var("CHECK")
        .unwrap_or_default()
        .split(',')
        .filter(|b| !b.is_empty())
        .map(str::to_owned)
        .collect();
    if !checks.is_empty() {
        // The models the game would bake for the root: how many each checked block's state selects.
        let facts: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_slice(&std::fs::read(Path::new(out).join("model-states.json"))?)?;
        let blocks: Vec<&str> = facts
            .keys()
            .filter(|name| assets.registry.default_state(name).is_some())
            .map(|name| name.strip_prefix("minecraft:").unwrap_or(name))
            .collect();
        let mut sprites = horizon_client_render::sprites::Sprites::default();
        let catalog = horizon_client_render::models::catalog::StateModels::from_facts(
            Path::new(out),
            assets.registry,
            &blocks,
            &mut sprites,
            &facts,
        )?;
        for block in &checks {
            let state = assets.registry.default_state(block);
            let models = state
                .map(|s| catalog.collect(s, horizon_core::position::BlockPos::new(0, 0, 0)).map(|m| m.len()))
                .transpose();
            println!("{block}: {state:?} models {models:?}");
        }
    }
    Ok(())
}
