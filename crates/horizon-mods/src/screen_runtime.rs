//! Screens drawn by code (capability `screen`, API 56): a mod's `screens.wasm`, run in the player's game. One
//! [`ScreenMod`] per mod that has one; it serves the mod's menu types whose screen says `"code": true`. Each call gets
//! what the server synchronised to this client ([`HostData`]) and hands back what the screen asked for
//! ([`ScreenAction`]): the game sends them through the menu's own paths.
use crate::manifest::Capability;
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap,
    component::{Component, HasSelf, Linker},
};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "horizon-screen-mod",
    });
}
pub use bindings::exports::horizon::mods::screen_events::{
    Drag, FieldText, Frame, InputEvent, Key, Modifiers, MouseButton, OpenInfo, Pointer, Setup,
    Wheel,
};
pub use bindings::horizon::mods::screen_host::{GridEntry, Stack};
use bindings::{HorizonScreenMod, horizon::mods::log as mod_log, horizon::mods::screen_host};

/// The command buffer's format (shared with the SDK, which writes it).
#[path = "../../horizon-mod-sdk/src/screen_format.rs"]
pub mod format;

/// A screens component's linear memory cap.
const MEMORY: usize = 32 << 20;
/// Fuel per call (about one unit per WebAssembly instruction): `open` builds the screen, `render` one frame's buffer.
const FUEL_OPEN: u64 = 200_000_000;
const FUEL_RENDER: u64 = 50_000_000;
const FUEL_CALL: u64 = 50_000_000;
/// Actions one call may queue (buttons, texts, grid clicks, clicks).
const ACTIONS_PER_CALL: usize = 64;
/// Characters a text sent to the server may hold (the `<mod>:menu_text` message's room).
const TEXT_CHARS: usize = 256;

/// What the screen reads during a call: the menu as this client has it.
#[derive(Clone)]
pub struct HostData {
    pub data: Vec<i32>,
    pub strings: Vec<String>,
    pub grid: Vec<GridEntry>,
    /// The menu's slots, then the player's 36.
    pub slots: Vec<Option<Stack>>,
    pub carried: Option<Stack>,
    /// GUI font advance widths by character (`render::text-width`'s table).
    pub font_widths: Arc<[f32]>,
    /// A GUI texture's size in pixels.
    pub texture_size: Arc<dyn Fn(&str) -> Option<(u32, u32)> + Send + Sync>,
    /// A translation key in the player's language, `%s` filled.
    pub translate: Arc<dyn Fn(&str, &[String]) -> String + Send + Sync>,
}
impl Default for HostData {
    fn default() -> Self {
        HostData {
            data: Vec::new(),
            strings: Vec::new(),
            grid: Vec::new(),
            slots: Vec::new(),
            carried: None,
            font_widths: Arc::from(Vec::new()),
            texture_size: Arc::new(|_| None),
            translate: Arc::new(|key, _| key.to_owned()),
        }
    }
}

/// What a screen asked the game to do during a call.
#[derive(Clone, Debug, PartialEq)]
pub enum ScreenAction {
    /// Menu button `id` (`AbstractContainerMenu.clickMenuButton`).
    Button(i32),
    /// Text field `id`'s text to the server (`<mod>:menu_text`).
    Text(i32, String),
    /// Item grid entry `index`, button, Shift (`menu-grid-click`).
    GridClick(u32, u8, bool),
    /// The button click sound.
    Click,
    /// Close the menu.
    Close,
}
impl ScreenAction {
    /// One line for tests and `horizon-ctl`: `button 9100`, `text 8533 "Base"`, `grid 3 0`, `click`, `close`.
    pub fn describe(&self) -> String {
        match self {
            ScreenAction::Button(id) => format!("button {id}"),
            ScreenAction::Text(id, text) => format!("text {id} {text:?}"),
            ScreenAction::GridClick(index, button, shift) => {
                format!(
                    "grid {index} {button}{}",
                    if *shift { " shift" } else { "" }
                )
            }
            ScreenAction::Click => "click".into(),
            ScreenAction::Close => "close".into(),
        }
    }
}

struct State {
    id: String,
    host: HostData,
    actions: Vec<ScreenAction>,
    limits: StoreLimits,
}

impl mod_log::Host for State {
    fn write(&mut self, level: mod_log::Level, message: String) {
        let level = match level {
            mod_log::Level::Debug => log::Level::Debug,
            mod_log::Level::Info => log::Level::Info,
            mod_log::Level::Warn => log::Level::Warn,
            mod_log::Level::Error => log::Level::Error,
        };
        let message: String = message.chars().take(256).collect();
        log::log!(level, "[mod {} screens] {message}", self.id);
    }
}
impl State {
    fn act(&mut self, a: ScreenAction) {
        if self.actions.len() < ACTIONS_PER_CALL {
            self.actions.push(a);
        }
    }
}
impl screen_host::Host for State {
    fn menu_data(&mut self) -> Vec<i32> {
        self.host.data.clone()
    }
    fn menu_strings(&mut self) -> Vec<String> {
        self.host.strings.clone()
    }
    fn menu_grid(&mut self) -> Vec<GridEntry> {
        self.host.grid.clone()
    }
    fn slot(&mut self, index: u32) -> Option<Stack> {
        self.host.slots.get(index as usize).cloned().flatten()
    }
    fn carried(&mut self) -> Option<Stack> {
        self.host.carried.clone()
    }
    fn text_width(&mut self, text: String) -> f32 {
        text_width(&self.host.font_widths, &text)
    }
    fn texture_size(&mut self, texture: String) -> Option<(u32, u32)> {
        (self.host.texture_size)(&texture)
    }
    fn translate(&mut self, key: String, args: Vec<String>) -> String {
        (self.host.translate)(&key, &args)
    }
    fn send_button(&mut self, id: i32) {
        self.act(ScreenAction::Button(id));
    }
    fn send_text(&mut self, id: i32, text: String) {
        self.act(ScreenAction::Text(
            id,
            text.chars().take(TEXT_CHARS).collect(),
        ));
    }
    fn send_grid_click(&mut self, index: u32, button: u8, shift: bool) {
        self.act(ScreenAction::GridClick(index, button.min(1), shift));
    }
    fn play_click(&mut self) {
        self.act(ScreenAction::Click);
    }
    fn close(&mut self) {
        self.act(ScreenAction::Close);
    }
}

/// `text-width`: the GUI font's advances, 6 for a character the table lacks.
pub fn text_width(widths: &[f32], text: &str) -> f32 {
    text.chars()
        .map(|c| widths.get(c as usize).copied().unwrap_or(6.0))
        .sum::<f32>()
        .ceil()
}

fn engine() -> Result<&'static (Engine, Linker<State>), String> {
    static ENGINE: OnceLock<Result<(Engine, Linker<State>), String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let mut config = Config::new();
            config.wasm_component_model(true).consume_fuel(true);
            let engine = Engine::new(&config).map_err(|e| e.to_string())?;
            let mut linker = Linker::new(&engine);
            HorizonScreenMod::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            Ok((engine, linker))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn reason(e: &wasmtime::Error) -> String {
    match e.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => "it took too long (an endless loop?)".into(),
        Some(Trap::UnreachableCodeReached) => "it crashed (a panic)".into(),
        Some(trap) => format!("it crashed: {trap}"),
        None => format!("{e:#}"),
    }
}

/// A mod's screens component, instantiated.
pub struct ScreenMod {
    pub id: String,
    store: Store<State>,
    bindings: HorizonScreenMod,
    /// A call failed: the component is not called again (its state may be broken).
    pub failed: Option<String>,
    /// How long the last `render` took, buffer decoding included.
    pub last_render: Duration,
}

/// What `render` gave: the decoded frame (or why it was refused), the actions the call queued and its time.
pub struct Rendered {
    pub frame: Result<format::Frame, String>,
    pub actions: Vec<ScreenAction>,
    pub took: Duration,
}

impl ScreenMod {
    /// Instantiates `screens.wasm` of mod `id`. Its imports must be `log` and `screen-host` only, and its API level one
    /// this game serves.
    pub fn load(id: &str, wasm: &[u8]) -> Result<Self, String> {
        let (engine, linker) = engine()?;
        let component = Component::new(engine, wasm).map_err(|e| format!("screens.wasm: {e:#}"))?;
        for (name, _) in component.component_type().imports(engine) {
            match Capability::of_import(name)? {
                None | Some(Capability::Screen) => {}
                Some(c) => {
                    return Err(format!(
                        "screens.wasm imports {name} (capability {c}): a screen may use log and screen-host only"
                    ));
                }
            }
        }
        let functions = crate::api_level::imports(wasm)?;
        let (level, function) = crate::api_level::needed(functions.iter().map(String::as_str));
        if let Some(why) = crate::api_level::too_new(level, function) {
            return Err(why);
        }
        let state = State {
            id: id.to_owned(),
            host: HostData::default(),
            actions: Vec::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY)
                .instances(16)
                .build(),
        };
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(FUEL_OPEN).map_err(|e| e.to_string())?;
        let bindings = HorizonScreenMod::instantiate(&mut store, &component, linker)
            .map_err(|e| reason(&e))?;
        Ok(ScreenMod {
            id: id.to_owned(),
            store,
            bindings,
            failed: None,
            last_render: Duration::ZERO,
        })
    }

    fn call<T>(
        &mut self,
        host: &HostData,
        fuel: u64,
        f: impl FnOnce(&HorizonScreenMod, &mut Store<State>) -> wasmtime::Result<T>,
    ) -> Result<(T, Vec<ScreenAction>), String> {
        if let Some(why) = &self.failed {
            return Err(why.clone());
        }
        {
            let s = self.store.data_mut();
            s.host = host.clone();
            s.actions.clear();
        }
        let result = self
            .store
            .set_fuel(fuel)
            .and_then(|()| f(&self.bindings, &mut self.store));
        let actions = std::mem::take(&mut self.store.data_mut().actions);
        match result {
            Ok(v) => Ok((v, actions)),
            Err(e) => {
                let why = format!("mod {}'s screen stopped: {}", self.id, reason(&e));
                log::warn!("Horizon: {why}");
                self.failed = Some(why.clone());
                Err(why)
            }
        }
    }

    /// `open`: the screen's setup, or why it would not open.
    pub fn open(
        &mut self,
        host: &HostData,
        info: &OpenInfo,
    ) -> Result<(Setup, Vec<ScreenAction>), String> {
        let (r, actions) = self.call(host, FUEL_OPEN, |b, s| {
            b.horizon_mods_screen_events().call_open(s, info)
        })?;
        r.map(|setup| (setup, actions))
    }
    pub fn tick(&mut self, host: &HostData) -> Result<Vec<ScreenAction>, String> {
        self.call(host, FUEL_CALL, |b, s| {
            b.horizon_mods_screen_events().call_tick(s)
        })
        .map(|((), a)| a)
    }
    /// `render` and the buffer decoded; the time covers both.
    pub fn render(&mut self, host: &HostData, frame: Frame) -> Result<Rendered, String> {
        let start = Instant::now();
        let (bytes, actions) = self.call(host, FUEL_RENDER, |b, s| {
            b.horizon_mods_screen_events().call_render(s, frame)
        })?;
        let frame = format::decode(&bytes);
        let took = start.elapsed();
        self.last_render = took;
        Ok(Rendered {
            frame,
            actions,
            took,
        })
    }
    pub fn input(
        &mut self,
        host: &HostData,
        event: &InputEvent,
    ) -> Result<(bool, Vec<ScreenAction>), String> {
        self.call(host, FUEL_CALL, |b, s| {
            b.horizon_mods_screen_events().call_input(s, event)
        })
    }
    pub fn close(&mut self, host: &HostData) -> Vec<ScreenAction> {
        self.call(host, FUEL_CALL, |b, s| {
            b.horizon_mods_screen_events().call_close(s)
        })
        .map(|((), a)| a)
        .unwrap_or_default()
    }
}
