//! Measures screens drawn by code (API 56): `render` of a real screen (the example screen-demo's), the decoding of a
//! frame the size of Mekanism's QIO Dashboard (600 commands), and what one call into a component costs, which a call
//! per command would pay per command.
//!
//!     cargo run --release -p horizon-mods --features runtime --example screen_bench -- <screendemo.hmod>
use horizon_mods::screen_runtime::{Frame, HostData, OpenInfo, ScreenMod, format};
use std::time::Instant;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("screen_bench <screendemo.hmod>");
    let package = horizon_mods::Package::read(std::path::Path::new(&path)).expect("a .hmod");
    let wasm = package.screens.expect("the mod has a screens.wasm");
    let mut screen = ScreenMod::load(&package.manifest.id, &wasm).expect("it loads");
    let host = HostData {
        data: vec![0, 0, 1],
        strings: vec![
            "Frequency: Mine".into(),
            "Base".into(),
            "Mine".into(),
            "Home".into(),
        ],
        font_widths: (0..256)
            .map(|c| if c == 32 { 4.0 } else { 6.0 })
            .collect::<Vec<f32>>()
            .into(),
        ..Default::default()
    };
    screen
        .open(
            &host,
            &OpenInfo {
                menu: "screendemo:panel".into(),
                gui_width: 427.0,
                gui_height: 240.0,
                touch: false,
            },
        )
        .expect("it opens");
    let frame = Frame {
        mouse_x: 60.0,
        mouse_y: 40.0,
        partial_tick: 0.0,
        time_ms: 0,
    };
    let n = 2000;
    // Warm up, then time `render` with the buffer decoded (what the game does when a frame is stale).
    for _ in 0..100 {
        let _ = screen.render(&host, frame);
    }
    let start = Instant::now();
    let mut commands = 0;
    for _ in 0..n {
        commands = screen
            .render(&host, frame)
            .expect("render")
            .frame
            .expect("a good frame")
            .commands
            .len();
    }
    let render = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
    // One call into the component that does almost nothing (`tick` of the demo): the toll a call per command pays.
    let start = Instant::now();
    for _ in 0..n {
        screen.tick(&host).expect("tick");
    }
    let call = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
    // Decoding a QIO-sized frame: 48 cells, each a fill, an item and its count, and the panel around them.
    let mut c = format::Canvas::new();
    for i in 0..48 {
        let (x, y) = ((i % 8) as f32 * 18.0, (i / 8) as f32 * 18.0);
        c.fill(x, y, 18.0, 18.0, 0xFF37_3737);
        c.fill(x + 1.0, y + 1.0, 16.0, 16.0, 0xFF8B_8B8B);
        c.render_item(
            "minecraft:cobblestone",
            "",
            1_500_000,
            x + 1.0,
            y + 1.0,
            false,
        );
        c.text(
            "1.5M",
            x + 17.0,
            y + 9.0,
            0xFFFF_FFFF,
            format::TEXT_RIGHT | format::TEXT_SHADOW,
            0.5,
        );
        c.area(i, [x, y, 18.0, 18.0], format::AreaKind::Button, true, "");
    }
    while c.len() < 600 {
        c.blit(
            "mekanism:gui/qio",
            0.0,
            0.0,
            0.0,
            0.0,
            16.0,
            16.0,
            256.0,
            256.0,
        );
    }
    let bytes = c.finish();
    let start = Instant::now();
    for _ in 0..n {
        let _ = format::decode(&bytes).expect("decodes");
    }
    let decode = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
    println!("screen-demo render + decode: {render:.1} us for {commands} commands");
    println!("one call into the component: {call:.2} us");
    println!(
        "decoding a 600-command frame ({} bytes): {decode:.1} us",
        bytes.len()
    );
    println!(
        "a call per command instead, for 600 commands: about {:.0} us of calls alone ({:.0} times the buffer's decode)",
        600.0 * call,
        600.0 * call / decode.max(0.01)
    );
}
