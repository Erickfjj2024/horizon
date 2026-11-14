//! Menu screens drawn by code (capability `screen`, API 56; docs/mods-screen.md): a menu type whose screen says
//! `"code": true` is drawn by its mod's `screens.wasm` instead of its JSON widgets. The component runs here, in the
//! window's game, also on Android. Each frame draws the last command buffer; `render` is called again only when
//! something changed (a tick, an input, the cursor moving to another area) or the screen animates. The hooks of the
//! JSON screens (`menu_screens.rs`) call in here first, so a JSON screen is untouched.
//!
//! Without the `mods` feature (no WebAssembly runtime) every method here does nothing.
use super::Game;
#[cfg(not(feature = "mods"))]
use horizon_client::hud::HudBatch;

/// A press the code screen took: no menu button to send, no click sound of the game's (the screen plays its own).
pub(super) const CODE_PRESS: i32 = i32::MIN + 1;

/// A clickable area or a text field of the code screen, in GUI pixels: what `horizon-ctl ui` lists.
#[derive(Clone, Debug)]
pub(super) struct CodeControl {
    pub id: i32,
    /// `button`, `tab`, `row`, `toggle`, `area`, or `field`.
    pub kind: &'static str,
    pub rect: [f32; 4],
    pub active: bool,
    pub label: String,
}

/// How the open code screen is doing, for `horizon-ctl state`.
#[derive(Clone, Debug, Default)]
pub(super) struct CodeStats {
    pub menu: String,
    pub last_render_ms: f64,
    pub average_render_ms: f64,
    pub renders: u64,
    pub fps: f64,
    pub commands: usize,
    pub error: Option<String>,
}

#[cfg(feature = "mods")]
mod imp {
    use super::{CodeControl, CodeStats, Game};
    use horizon_client::hud::{HudBatch, rgba};
    use horizon_mods::screen_runtime::{
        self as rt, Frame, HostData, InputEvent, Modifiers, MouseButton, OpenInfo, ScreenAction,
        ScreenMod,
        format::{self, Command, Rect},
    };
    use std::{
        collections::{BTreeMap, HashMap, VecDeque},
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    /// The screens components of the mods whose content is loaded, by mod id (set with the content).
    pub(in crate::game) static WASM: Mutex<BTreeMap<String, Arc<Vec<u8>>>> =
        Mutex::new(BTreeMap::new());
    /// The instantiated components, kept between screens.
    static INSTANCES: Mutex<BTreeMap<String, ScreenMod>> = Mutex::new(BTreeMap::new());
    /// The open screen.
    static OPEN: Mutex<Option<Open>> = Mutex::new(None);
    /// The menu whose code screen could not run, and why: logged once, shown on the JSON screen drawn instead and in
    /// `horizon-ctl state`'s `screen_code`.
    static FAILED: Mutex<Option<(String, String)>> = Mutex::new(None);

    /// A code screen that could not run: warned once per menu and reason, kept for the panel and `state`.
    fn fail(menu: &str, reason: String) {
        let Ok(mut f) = FAILED.lock() else { return };
        if f.as_ref().is_some_and(|(m, r)| m == menu && *r == reason) {
            return;
        }
        log::warn!(
            "Horizon: the code screen of {menu} could not run, its JSON screen is shown instead: {reason}"
        );
        *f = Some((menu.to_owned(), reason));
    }

    /// Why the code screen of `menu` could not run, if it could not.
    pub(in crate::game) fn failure(menu: &str) -> Option<String> {
        FAILED
            .lock()
            .ok()?
            .as_ref()
            .filter(|(m, _)| m == menu)
            .map(|(_, r)| r.clone())
    }
    /// What the screen asked for, waiting for a place with the game mutable to send it.
    static ACTIONS: Mutex<Vec<ScreenAction>> = Mutex::new(Vec::new());
    /// Renders a second at most for an animated screen.
    const ANIMATION_GAP: Duration = Duration::from_millis(33);
    /// The GUI font's first characters' advances, as `render::text-width` reads them.
    const FONT_CHARS: u32 = 0x250;

    /// A command placed: its index and the pose and clip it is drawn under.
    struct Placed {
        index: usize,
        pose: [f32; 4],
        clip: Option<Rect>,
    }

    struct Open {
        menu: String,
        at: [i32; 3],
        screen: ScreenMod,
        commands: Vec<Command>,
        /// The commands before `SLOTS` and after it (all before when there is none).
        before: Vec<Placed>,
        after: Vec<Placed>,
        animate: bool,
        dirty: bool,
        last_render: Option<Instant>,
        opened: Instant,
        areas: Vec<CodeControl>,
        fields: Vec<(i32, Rect, u16, format::FieldFilter, String, u8, u32)>,
        /// The text each field was last given by the screen: a change sets the box.
        field_texts: HashMap<i32, String>,
        slots: HashMap<u32, (f32, f32, bool)>,
        tooltip: Vec<(String, u32)>,
        hover: Option<i32>,
        pressed: Option<(MouseButton, [f32; 2])>,
        error: Option<String>,
        renders: u64,
        render_total: Duration,
        frames: VecDeque<Instant>,
    }

    fn pack(argb: u32) -> u32 {
        let [a, r, g, b] = argb.to_be_bytes();
        rgba(r, g, b, a)
    }

    /// The pose `[sx, sy, tx, ty]` applied to a point.
    fn apply(p: [f32; 4], x: f32, y: f32) -> (f32, f32) {
        (p[2] + p[0] * x, p[3] + p[1] * y)
    }

    fn intersect(a: Rect, b: Rect) -> Rect {
        let (x0, y0) = (a[0].max(b[0]), a[1].max(b[1]));
        let (x1, y1) = (
            (a[0] + a[2]).min(b[0] + b[2]),
            (a[1] + a[3]).min(b[1] + b[3]),
        );
        [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
    }

    /// Lays the frame's commands out: what is drawn before and after the slots under which pose and clip, and the
    /// areas, fields, slots and tooltip the frame declares (in panel pixels).
    fn layout(o: &mut Open, frame: format::Frame) {
        o.animate = frame.flags & format::ANIMATE != 0;
        o.commands = frame.commands;
        o.before.clear();
        o.after.clear();
        o.areas.clear();
        o.fields.clear();
        o.slots.clear();
        o.tooltip.clear();
        let mut poses = vec![[1.0f32, 1.0, 0.0, 0.0]];
        let mut clips: Vec<Rect> = Vec::new();
        let mut after = false;
        for (index, c) in o.commands.iter().enumerate() {
            let pose = *poses.last().unwrap_or(&[1.0, 1.0, 0.0, 0.0]);
            let rect = |r: &Rect| {
                let (x, y) = apply(pose, r[0], r[1]);
                [x, y, r[2] * pose[0], r[3] * pose[1]]
            };
            match c {
                Command::Push if poses.len() < format::MAX_DEPTH => poses.push(pose),
                Command::Pop if poses.len() > 1 => {
                    poses.pop();
                }
                Command::Translate { dx, dy } => {
                    if let Some(p) = poses.last_mut() {
                        p[2] += p[0] * dx;
                        p[3] += p[1] * dy;
                    }
                }
                Command::Scale { sx, sy } => {
                    if let Some(p) = poses.last_mut() {
                        p[0] *= sx;
                        p[1] *= sy;
                    }
                }
                Command::Scissor { rect: r } if clips.len() < format::MAX_DEPTH => {
                    let r = rect(r);
                    let r = clips.last().map_or(r, |top| intersect(*top, r));
                    clips.push(r);
                }
                Command::Unscissor => {
                    clips.pop();
                }
                Command::Slots => after = true,
                Command::Slot {
                    index,
                    x,
                    y,
                    active,
                } => {
                    let (x, y) = apply(pose, *x, *y);
                    o.slots.insert(*index, (x, y, *active));
                }
                Command::Area {
                    id,
                    rect: r,
                    kind,
                    active,
                    label,
                } => o.areas.push(CodeControl {
                    id: *id,
                    kind: kind.name(),
                    rect: rect(r),
                    active: *active,
                    label: label.clone(),
                }),
                Command::Field {
                    id,
                    rect: r,
                    max_length,
                    filter,
                    text,
                    flags,
                    color,
                } => {
                    o.fields.push((
                        *id,
                        rect(r),
                        *max_length,
                        *filter,
                        text.clone(),
                        *flags,
                        *color,
                    ));
                }
                Command::Tooltip { lines } => o.tooltip.clone_from(lines),
                _ => {}
            }
            let placed = Placed {
                index,
                pose,
                clip: clips.last().copied(),
            };
            if after {
                o.after.push(placed)
            } else {
                o.before.push(placed)
            }
        }
    }

    fn take_actions(actions: Vec<ScreenAction>) {
        if let Ok(mut a) = ACTIONS.lock() {
            a.extend(actions);
        }
    }

    impl Game<'_> {
        /// The open menu's code screen, if its menu type has one: its menu type, data and origin.
        fn code_target(&self) -> Option<(String, Vec<i32>, [f32; 2])> {
            let (block, data, origin) = self.native_screen_origin()?;
            super::super::menu_screens::screen_of(&block).filter(|s| s.code)?;
            Some((block, data, origin))
        }

        /// What the screen reads: the menu as this client has it.
        fn code_host(&self, data: Vec<i32>) -> HostData {
            use std::sync::OnceLock;
            static FONT: OnceLock<Arc<[f32]>> = OnceLock::new();
            let atlas = &self.gui.atlas;
            let font = FONT
                .get_or_init(|| {
                    (0..FONT_CHARS)
                        .map(|c| char::from_u32(c).map_or(6.0, |c| atlas.glyph_cell(c).1))
                        .collect::<Vec<f32>>()
                        .into()
                })
                .clone();
            // The mods' GUI textures' sizes, kept until the atlas changes (a resource reload).
            static SIZES: Mutex<Option<(usize, Arc<HashMap<String, (u32, u32)>>)>> =
                Mutex::new(None);
            let sizes = {
                let mut cache = SIZES.lock().unwrap_or_else(|e| e.into_inner());
                match cache.as_ref() {
                    Some((n, s)) if *n == atlas.sprites.len() => s.clone(),
                    _ => {
                        let s: Arc<HashMap<String, (u32, u32)>> = Arc::new(
                            atlas
                                .sprites
                                .iter()
                                .filter(|(k, _)| k.contains(':'))
                                .map(|(k, r)| (k.clone(), (r.w, r.h)))
                                .collect(),
                        );
                        *cache = Some((atlas.sprites.len(), s.clone()));
                        s
                    }
                }
            };
            let names = self.pack.names.clone();
            let stack = |s: &horizon_client::items::ItemStack| {
                let m = Self::mod_stack(s);
                rt::Stack {
                    item: m.item,
                    components: m.components,
                    count: m.count,
                }
            };
            let mut slots: Vec<Option<rt::Stack>> = match self
                .container
                .and_then(|k| self.world.block_entities.get(&k))
            {
                Some(horizon_client::containers::BlockEntity::Mod(m)) => {
                    m.items.iter().map(|s| s.as_ref().map(stack)).collect()
                }
                _ => Vec::new(),
            };
            slots.extend(
                self.inventory
                    .iter()
                    .take(36)
                    .map(|s| s.as_ref().map(stack)),
            );
            HostData {
                data,
                strings: super::super::menu_screens::menu_strings(),
                grid: super::super::menu_screens::menu_grid()
                    .into_iter()
                    .map(|e| rt::GridEntry {
                        item: e.item,
                        components: e.components,
                        count: e.count,
                    })
                    .collect(),
                slots,
                carried: self.carried.as_ref().map(stack),
                font_widths: font,
                texture_size: Arc::new(move |t| sizes.get(t).copied()),
                translate: Arc::new(move |key, args| {
                    let mut text = names.get(key).unwrap_or(key).to_owned();
                    for (i, a) in args.iter().enumerate() {
                        text = text.replacen(&format!("%{}$s", i + 1), a, 1);
                    }
                    for a in args {
                        text = text.replacen("%s", a, 1);
                    }
                    text
                }),
            }
        }

        /// The open screen for this menu, opened (or reopened for another menu) when it isn't yet.
        fn code_open<'a>(
            &self,
            open: &'a mut Option<Open>,
            menu: &str,
            data: &[i32],
        ) -> Result<&'a mut Open, String> {
            let at = self.native_screen_pos().unwrap_or([0; 3]);
            if open.as_ref().is_some_and(|o| o.menu != menu || o.at != at) {
                close(open);
            }
            if open.is_none() {
                let namespace = menu.split_once(':').map_or(menu, |(ns, _)| ns);
                let host = self.code_host(data.to_vec());
                let mut error = None;
                let mut screen = INSTANCES.lock().ok().and_then(|mut i| i.remove(namespace));
                if screen.is_none() {
                    let wasm = WASM.lock().ok().and_then(|w| w.get(namespace).cloned());
                    match wasm {
                        Some(w) => match ScreenMod::load(namespace, &w) {
                            Ok(s) => screen = Some(s),
                            Err(e) => {
                                error = Some(format!("mod {namespace}'s screens did not load: {e}"))
                            }
                        },
                        None => {
                            error = Some(format!("mod {namespace} has no screens.wasm for {menu}"))
                        }
                    }
                }
                let size = self
                    .gui_scale()
                    .map_or([427.0, 240.0], |b| [b.width, b.height]);
                let info = OpenInfo {
                    menu: menu.to_owned(),
                    gui_width: size[0],
                    gui_height: size[1],
                    touch: cfg!(target_os = "android"),
                };
                let screen = match screen {
                    Some(mut s) => {
                        match s.open(&host, &info) {
                            Ok((_, actions)) => take_actions(actions),
                            Err(e) => error = Some(e),
                        }
                        s
                    }
                    None => {
                        return Err(error
                            .unwrap_or_else(|| format!("mod {namespace}'s screens did not load")));
                    }
                };
                // `open` refused: the JSON screen instead, and the component kept for the next menu.
                if let Some(e) = error {
                    if screen.failed.is_none()
                        && let Ok(mut i) = INSTANCES.lock()
                    {
                        i.insert(screen.id.clone(), screen);
                    }
                    return Err(e);
                }
                if let Ok(mut f) = FAILED.lock() {
                    *f = None;
                }
                log::info!("Horizon: the screen of {menu} is drawn by mod {namespace}'s code");
                *open = Some(Open {
                    menu: menu.to_owned(),
                    at,
                    screen,
                    commands: Vec::new(),
                    before: Vec::new(),
                    after: Vec::new(),
                    animate: false,
                    dirty: true,
                    last_render: None,
                    opened: Instant::now(),
                    areas: Vec::new(),
                    fields: Vec::new(),
                    field_texts: HashMap::new(),
                    slots: HashMap::new(),
                    tooltip: Vec::new(),
                    hover: None,
                    pressed: None,
                    error,
                    renders: 0,
                    render_total: Duration::ZERO,
                    frames: VecDeque::new(),
                });
            }
            open.as_mut().ok_or_else(|| "the screen closed".to_owned())
        }

        /// Renders when the frame is stale.
        fn code_render(&self, o: &mut Open, data: Vec<i32>, origin: [f32; 2]) {
            let stale = o.dirty
                || (o.animate && o.last_render.is_none_or(|t| t.elapsed() >= ANIMATION_GAP));
            if !stale || o.error.is_some() {
                return;
            }
            let host = self.code_host(data);
            let frame = Frame {
                mouse_x: self.cursor.0 - origin[0],
                mouse_y: self.cursor.1 - origin[1],
                partial_tick: 0.0,
                time_ms: o.opened.elapsed().as_millis() as u64,
            };
            match o.screen.render(&host, frame) {
                Ok(r) => {
                    take_actions(r.actions);
                    o.renders += 1;
                    o.render_total += r.took;
                    match r.frame {
                        Ok(frame) => layout(o, frame),
                        Err(e) => {
                            log::warn!(
                                "Horizon: mod {}'s screen drew a bad frame: {e}",
                                o.screen.id
                            );
                            o.error = Some(format!("bad frame: {e}"));
                        }
                    }
                }
                Err(e) => o.error = Some(e),
            }
            o.dirty = false;
            o.last_render = Some(Instant::now());
        }

        /// Draws one command at `origin` under its pose and clip.
        fn code_draw(&self, batch: &mut HudBatch, origin: [f32; 2], o: &Open, p: &Placed) {
            let [ox, oy] = origin;
            let atlas = &self.gui.atlas;
            let start = batch.vertices.len();
            match &o.commands[p.index] {
                Command::Fill { rect, argb } => {
                    self.native_fill(batch, [ox + rect[0], oy + rect[1], rect[2], rect[3]], *argb)
                }
                Command::Gradient { rect, top, bottom } => {
                    let steps = rect[3].ceil().clamp(1.0, 32.0) as u32;
                    let h = rect[3] / steps as f32;
                    for i in 0..steps {
                        let t = (i as f32 + 0.5) / steps as f32;
                        let mix = |a: u32, b: u32, s: u32| {
                            let (a, b) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
                            ((a + (b - a) * t).round() as u32) << s
                        };
                        let c = mix(*top, *bottom, 24)
                            | mix(*top, *bottom, 16)
                            | mix(*top, *bottom, 8)
                            | mix(*top, *bottom, 0);
                        self.native_fill(
                            batch,
                            [ox + rect[0], oy + rect[1] + h * i as f32, rect[2], h],
                            c,
                        );
                    }
                }
                Command::Blit {
                    texture,
                    rect,
                    uv,
                    tex,
                    argb,
                } => {
                    self.code_blit(
                        batch,
                        texture,
                        [ox + rect[0], oy + rect[1], rect[2], rect[3]],
                        *uv,
                        *tex,
                        *argb,
                    );
                }
                Command::Sprite { sprite, rect, argb } => {
                    let name = match sprite.split_once(':') {
                        None => format!("gui/sprites/{sprite}"),
                        Some(("minecraft", path)) => format!("gui/sprites/{path}"),
                        Some((ns, path)) => format!("{ns}:gui/sprites/{path}"),
                    };
                    horizon_client::ui::blit_sprite(
                        batch,
                        atlas,
                        &name,
                        ox + rect[0],
                        oy + rect[1],
                        rect[2],
                        rect[3],
                        pack(*argb),
                    );
                }
                Command::Nine {
                    texture,
                    rect,
                    borders,
                    uv,
                    tex,
                    argb,
                } => {
                    for (dst, src) in nine_slices(*rect, *borders, *uv) {
                        self.code_blit(
                            batch,
                            texture,
                            [ox + dst[0], oy + dst[1], dst[2], dst[3]],
                            src,
                            *tex,
                            *argb,
                        );
                    }
                }
                Command::Text {
                    text,
                    x,
                    y,
                    argb,
                    flags,
                    scale,
                } => {
                    let w = HudBatch::text_width(atlas, text) * scale;
                    let x = if flags & format::TEXT_CENTER != 0 {
                        x - w / 2.0
                    } else if flags & format::TEXT_RIGHT != 0 {
                        x - w
                    } else {
                        *x
                    };
                    let at = [ox + x, oy + y];
                    let first = batch.vertices.len();
                    batch.text_styled(
                        atlas,
                        &horizon_client::text::Text::literal(text.as_str()),
                        at[0],
                        at[1],
                        pack(*argb),
                        flags & format::TEXT_SHADOW != 0,
                    );
                    if (*scale - 1.0).abs() > f32::EPSILON {
                        batch.scale_since(first, at[0], at[1], *scale, *scale);
                    }
                }
                Command::Item {
                    item,
                    count,
                    x,
                    y,
                    flags,
                    scale,
                    ..
                } => {
                    let at = [ox + x, oy + y];
                    let first = batch.vertices.len();
                    if self.native_item(batch, item, at, 16.0) {
                        if flags & format::ITEM_DECORATIONS != 0 {
                            let stack = horizon_client::items::ItemStack::new(item, *count);
                            self.native_item_decorations(&stack, at[0], at[1], batch);
                        }
                        if (*scale - 1.0).abs() > f32::EPSILON {
                            batch.scale_since(first, at[0], at[1], *scale, *scale);
                        }
                    }
                }
                Command::Fluid {
                    sprite,
                    rect,
                    fill,
                    argb,
                } => {
                    self.draw_tiled_bottom_up(
                        batch,
                        sprite,
                        [ox + rect[0], oy + rect[1], rect[2], rect[3]],
                        *fill,
                        pack(*argb),
                    );
                }
                Command::Field {
                    id,
                    rect,
                    max_length,
                    text,
                    flags,
                    color,
                    ..
                } => {
                    let r = [ox + rect[0], oy + rect[1], rect[2], rect[3]];
                    let mut drawn = false;
                    if let Ok(mut f) = super::super::menu_screens::FIELD.lock()
                        && let Some(field) = f.as_mut().filter(|f| f.id == *id)
                    {
                        let edit = &mut field.edit;
                        [edit.x, edit.y, edit.width, edit.height] = r;
                        edit.focused = true;
                        edit.bordered = flags & format::FIELD_NO_BORDER == 0;
                        if *color != 0 {
                            edit.text_color = pack(*color);
                        }
                        edit.draw(batch, atlas, None);
                        drawn = true;
                    }
                    if !drawn {
                        let mut edit = horizon_client::ui::EditBox::new(r[0], r[1], r[2], r[3]);
                        edit.max_length = usize::from(*max_length);
                        edit.bordered = flags & format::FIELD_NO_BORDER == 0;
                        if *color != 0 {
                            edit.text_color = pack(*color);
                        }
                        edit.set_value(text);
                        edit.draw(batch, atlas, None);
                    }
                }
                _ => {}
            }
            let pose = p.pose;
            if pose != [1.0, 1.0, 0.0, 0.0] {
                for v in &mut batch.vertices[start..] {
                    let (x, y) = apply(pose, v.pos[0] - ox, v.pos[1] - oy);
                    v.pos = [ox + x, oy + y];
                }
            }
            if let Some(c) = p.clip {
                batch.clip_since(
                    start,
                    ox + c[0],
                    oy + c[1],
                    ox + c[0] + c[2],
                    oy + c[1] + c[3],
                );
            }
        }

        /// `blit` of texture region `uv` (in a texture `tex` pixels big) over `dst`.
        fn code_blit(
            &self,
            batch: &mut HudBatch,
            texture: &str,
            dst: Rect,
            uv: Rect,
            tex: [f32; 2],
            argb: u32,
        ) {
            let Some([sw, sh]) = self.native_sprite_size(texture) else {
                return;
            };
            let (fx, fy) = (sw / tex[0].max(1.0), sh / tex[1].max(1.0));
            self.native_sprite_tinted(
                batch,
                texture,
                [uv[0] * fx, uv[1] * fy, uv[2] * fx, uv[3] * fy],
                dst,
                pack(argb),
            );
        }

        /// The panel's commands up to `SLOTS` (all of them without one) of the code screen of menu `block`; false
        /// when it could not run (the reason is logged once and kept: the caller draws the JSON screen instead).
        pub(in crate::game) fn code_screen_draw(
            &self,
            block: &str,
            batch: &mut HudBatch,
            origin: [f32; 2],
        ) -> bool {
            let Some((menu, data, _)) = self.code_target() else {
                fail(
                    block,
                    "the open menu's panel was not found (no screen shown or no renderer)".into(),
                );
                return false;
            };
            let Ok(mut guard) = OPEN.lock() else {
                return false;
            };
            let o = match self.code_open(&mut guard, &menu, &data) {
                Ok(o) => o,
                Err(e) => {
                    drop(guard);
                    fail(&menu, e);
                    return false;
                }
            };
            self.code_render(o, data, origin);
            o.frames.push_back(Instant::now());
            while o
                .frames
                .front()
                .is_some_and(|t| t.elapsed() > Duration::from_secs(1))
            {
                o.frames.pop_front();
            }
            for p in &o.before {
                self.code_draw(batch, origin, o, p);
            }
            let frames = o.slots.is_empty();
            let error = o.error.clone();
            // Unlocked first: the frames ask where the slots are (`code_screen_slot`), which locks the screen too.
            drop(guard);
            // The slots' frames, where the items will be: the game's own unless the screen places its slots
            // (`SLOT`) and draws their frames itself, as Mekanism's `GuiSlot`s do.
            if frames {
                self.draw_native_slot_frames(batch, origin[0], origin[1]);
            }
            if let Some(e) = &error {
                let text = horizon_client::text::Text::literal(format!("Screen error: {e}"));
                batch.text_styled(
                    &self.gui.atlas,
                    &text,
                    origin[0] + 4.0,
                    origin[1] + 4.0,
                    rgba(255, 85, 85, 255),
                    true,
                );
            }
            true
        }

        /// The commands after `SLOTS` (windows over the slots' items).
        pub(in crate::game) fn code_screen_draw_after_slots(&self, batch: &mut HudBatch) {
            let Some((_, _, origin)) = self.code_target() else {
                return;
            };
            let Ok(guard) = OPEN.lock() else { return };
            let Some(o) = guard.as_ref() else { return };
            for p in &o.after {
                self.code_draw(batch, origin, o, p);
            }
        }

        /// The frame's tooltip, at the cursor; true when the code screen had one.
        pub(in crate::game) fn code_screen_tooltip(&self, batch: &mut HudBatch) -> bool {
            if self.code_target().is_none() {
                return false;
            }
            let lines: Vec<(horizon_client::text::Text, u32)> = OPEN
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|o| o.tooltip.clone()))
                .unwrap_or_default()
                .into_iter()
                .map(|(t, c)| (horizon_client::text::Text::literal(t), pack(c)))
                .collect();
            if !lines.is_empty() {
                self.draw_tooltip_lines(batch, &lines, self.cursor.0, self.cursor.1);
            }
            true
        }

        fn code_input(&self, event: InputEvent) -> Option<bool> {
            let (menu, data, _) = self.code_target()?;
            let mut guard = OPEN.lock().ok()?;
            let o = guard.as_mut().filter(|o| o.menu == menu)?;
            if o.error.is_some() {
                return Some(false);
            }
            let host = self.code_host(data);
            let taken = match o.screen.input(&host, &event) {
                Ok((taken, actions)) => {
                    take_actions(actions);
                    taken
                }
                Err(e) => {
                    o.error = Some(e);
                    false
                }
            };
            o.dirty = true;
            Some(taken)
        }

        fn modifiers(&self) -> Modifiers {
            Modifiers {
                shift: self.has_shift_down(),
                ctrl: self.has_control_down(),
                alt: false,
            }
        }

        /// A press at `at` (from the panel's top left): a text field takes the focus, else the screen hears it. None
        /// when the open menu has no code screen; Some(true) when the screen took it (no slot click then).
        pub(in crate::game) fn code_screen_press(&self, at: [f32; 2], right: bool) -> Option<bool> {
            self.code_target()?;
            let field = OPEN.lock().ok().and_then(|g| {
                g.as_ref()?
                    .fields
                    .iter()
                    .find(|f| {
                        at[0] >= f.1[0]
                            && at[0] < f.1[0] + f.1[2]
                            && at[1] >= f.1[1]
                            && at[1] < f.1[1] + f.1[3]
                    })
                    .cloned()
            });
            if let Some((id, _, max, filter, text, _, _)) = field {
                focus_field(id, max, filter, &text, true);
                return Some(true);
            }
            super::super::menu_screens::drop_field();
            let button = if right {
                MouseButton::Right
            } else {
                MouseButton::Left
            };
            if let Ok(mut g) = OPEN.lock()
                && let Some(o) = g.as_mut()
            {
                o.pressed = Some((button, at));
            }
            self.code_input(InputEvent::MousePressed(rt::Pointer {
                x: at[0],
                y: at[1],
                button,
                modifiers: self.modifiers(),
            }))
        }

        /// The left button came up.
        pub(in crate::game) fn code_screen_release(&self) {
            let Some((_, _, origin)) = self.code_target() else {
                return;
            };
            let pressed = OPEN
                .lock()
                .ok()
                .and_then(|mut g| g.as_mut()?.pressed.take());
            let Some((button, _)) = pressed else { return };
            let at = [self.cursor.0 - origin[0], self.cursor.1 - origin[1]];
            self.code_input(InputEvent::MouseReleased(rt::Pointer {
                x: at[0],
                y: at[1],
                button,
                modifiers: self.modifiers(),
            }));
        }

        /// The cursor moved: a held button drags; another area under it redraws.
        pub(in crate::game) fn code_screen_motion(&self) {
            let Some((_, _, origin)) = self.code_target() else {
                return;
            };
            let at = [self.cursor.0 - origin[0], self.cursor.1 - origin[1]];
            let drag = {
                let Ok(mut g) = OPEN.lock() else { return };
                let Some(o) = g.as_mut() else { return };
                let hover = o
                    .areas
                    .iter()
                    .rev()
                    .find(|a| {
                        at[0] >= a.rect[0]
                            && at[0] < a.rect[0] + a.rect[2]
                            && at[1] >= a.rect[1]
                            && at[1] < a.rect[1] + a.rect[3]
                    })
                    .map(|a| a.id);
                if hover != o.hover || !o.tooltip.is_empty() {
                    o.hover = hover;
                    o.dirty = true;
                }
                match &mut o.pressed {
                    Some((button, last)) if *last != at => {
                        let d = (*button, at[0] - last[0], at[1] - last[1]);
                        *last = at;
                        Some(d)
                    }
                    _ => None,
                }
            };
            if let Some((button, dx, dy)) = drag {
                self.code_input(InputEvent::MouseDragged(rt::Drag {
                    x: at[0],
                    y: at[1],
                    button,
                    dx,
                    dy,
                }));
            }
        }

        /// The wheel over the screen: true when the screen took it.
        pub(in crate::game) fn code_screen_scroll(&self, delta: f32) -> Option<bool> {
            let (_, _, origin) = self.code_target()?;
            let at = [self.cursor.0 - origin[0], self.cursor.1 - origin[1]];
            self.code_input(InputEvent::MouseScrolled(rt::Wheel {
                x: at[0],
                y: at[1],
                delta,
            }))
        }

        /// A key on the screen (not while one of its text fields has the focus): true when the screen took it (Esc
        /// then doesn't close the menu).
        pub(in crate::game) fn code_screen_key(&self, key: &str) -> bool {
            if self.mod_field_typing() {
                return false;
            }
            self.code_input(InputEvent::KeyPressed(rt::Key {
                key: key.to_owned(),
                modifiers: self.modifiers(),
            })) == Some(true)
        }

        /// A character typed on the screen (not into a field of it).
        pub(in crate::game) fn code_screen_char(&self, c: char) -> bool {
            if self.mod_field_typing() {
                return false;
            }
            self.code_input(InputEvent::CharTyped(c.to_string())) == Some(true)
        }

        /// The focused field of the code screen changed (`submitted`: Enter): the screen hears it. False when the
        /// open menu has no code screen.
        pub(in crate::game) fn code_screen_field(
            &self,
            id: i32,
            text: &str,
            submitted: bool,
        ) -> bool {
            if self.code_target().is_none() {
                return false;
            }
            let t = rt::FieldText {
                id,
                text: text.to_owned(),
            };
            self.code_input(if submitted {
                InputEvent::FieldSubmitted(t)
            } else {
                InputEvent::FieldChanged(t)
            });
            true
        }

        /// Focuses the code screen's text field `id` as a press would (`horizon-ctl type --menu-field`); its text, or
        /// None when the open screen has no such field.
        pub(in crate::game) fn code_screen_focus_field(&self, id: i32) -> Option<()> {
            let field = OPEN
                .lock()
                .ok()?
                .as_ref()?
                .fields
                .iter()
                .find(|f| f.0 == id)
                .cloned()?;
            focus_field(id, field.2, field.3, &field.4, false);
            Some(())
        }

        /// The open code screen's areas and fields in GUI pixels; None when the open menu has no code screen.
        pub(in crate::game) fn code_screen_controls(&self) -> Option<Vec<CodeControl>> {
            let (_, _, [ox, oy]) = self.code_target()?;
            let g = OPEN.lock().ok()?;
            let o = g.as_ref()?;
            let shift = |r: Rect| [r[0] + ox, r[1] + oy, r[2], r[3]];
            let mut out: Vec<CodeControl> = o
                .areas
                .iter()
                .map(|a| CodeControl {
                    rect: shift(a.rect),
                    ..a.clone()
                })
                .collect();
            out.extend(o.fields.iter().map(|f| CodeControl {
                id: f.0,
                kind: "field",
                rect: shift(f.1),
                active: true,
                label: f.4.clone(),
            }));
            Some(out)
        }

        /// The last frame's commands, one line each (`format::describe`).
        pub(in crate::game) fn code_screen_commands(&self) -> Option<Vec<String>> {
            self.code_target()?;
            let g = OPEN.lock().ok()?;
            Some(g.as_ref()?.commands.iter().map(format::describe).collect())
        }

        /// The texts the last frame drew, with their place in GUI pixels.
        pub(in crate::game) fn code_screen_texts(&self) -> Option<Vec<(String, [f32; 2])>> {
            let (_, _, [ox, oy]) = self.code_target()?;
            let g = OPEN.lock().ok()?;
            let o = g.as_ref()?;
            Some(
                o.before
                    .iter()
                    .chain(&o.after)
                    .filter_map(|p| match &o.commands[p.index] {
                        Command::Text { text, x, y, .. } => {
                            let (x, y) = apply(p.pose, *x, *y);
                            Some((text.clone(), [ox + x, oy + y]))
                        }
                        _ => None,
                    })
                    .collect(),
            )
        }

        /// Where the code screen puts container slot `slot` and whether it shows: None when it says nothing of it.
        pub(in crate::game) fn code_screen_slot(&self, slot: usize) -> Option<(f32, f32, bool)> {
            let g = OPEN.lock().ok()?;
            g.as_ref()?.slots.get(&(slot as u32)).copied()
        }

        /// Whether the open menu's code screen runs (else its JSON screen is drawn and pressed).
        pub(in crate::game) fn code_screen_running(&self) -> bool {
            let Some((menu, _, _)) = self.code_target() else {
                return false;
            };
            OPEN.lock()
                .ok()
                .is_some_and(|g| g.as_ref().is_some_and(|o| o.menu == menu))
        }

        pub(in crate::game) fn code_screen_stats(&self) -> Option<CodeStats> {
            let g = OPEN.lock().ok()?;
            let Some(o) = g.as_ref() else {
                // A code screen that could not run: its menu and why.
                let block = self.native_open_block()?;
                let reason = failure(&block)?;
                return Some(CodeStats {
                    menu: block,
                    error: Some(reason),
                    ..Default::default()
                });
            };
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            Some(CodeStats {
                menu: o.menu.clone(),
                last_render_ms: ms(o.screen.last_render),
                average_render_ms: if o.renders == 0 {
                    0.0
                } else {
                    ms(o.render_total) / o.renders as f64
                },
                renders: o.renders,
                fps: o.frames.len() as f64,
                commands: o.commands.len(),
                error: o.error.clone(),
            })
        }

        /// Once a tick: the screen's tick (and a redraw), a screen whose menu closed is closed, and what the screen
        /// asked for goes out.
        pub(in crate::game) fn tick_code_screen(&mut self) {
            let target = self.code_target();
            if let Ok(mut g) = OPEN.lock() {
                match (&target, g.as_mut()) {
                    (Some((menu, data, _)), Some(o)) if *menu == o.menu => {
                        if o.error.is_none() {
                            let host = self.code_host(data.clone());
                            match o.screen.tick(&host) {
                                Ok(actions) => take_actions(actions),
                                Err(e) => o.error = Some(e),
                            }
                        }
                        // The field the screen gave a new text to shows it.
                        for f in &o.fields {
                            if o.field_texts.get(&f.0) != Some(&f.4) {
                                o.field_texts.insert(f.0, f.4.clone());
                                if let Ok(mut field) = super::super::menu_screens::FIELD.lock()
                                    && let Some(field) = field.as_mut().filter(|x| x.id == f.0)
                                    && field.edit.value != f.4
                                {
                                    field.edit.set_value(&f.4);
                                    field.edit.move_cursor_to_end(false);
                                }
                            }
                        }
                        o.dirty = true;
                    }
                    (_, Some(_)) => close(&mut g),
                    _ => {}
                }
            }
            self.code_screen_actions();
        }

        /// Sends what the screen asked for: menu buttons, field texts, grid clicks, the click sound, closing.
        pub(in crate::game) fn code_screen_actions(&mut self) {
            let actions = ACTIONS
                .lock()
                .map(|mut a| std::mem::take(&mut *a))
                .unwrap_or_default();
            for a in actions {
                match a {
                    ScreenAction::Button(id) => self.native_menu_button_send(id),
                    ScreenAction::Text(id, text) => {
                        let (Some(at), Some(block)) =
                            (self.native_screen_pos(), self.native_open_block())
                        else {
                            continue;
                        };
                        let Some((namespace, _)) = block.split_once(':') else {
                            continue;
                        };
                        let data = horizon_mods::screen::encode_menu_input(at, id, &text);
                        let _ = self
                            .native_net_send_to_server(&format!("{namespace}:menu_text"), &data);
                    }
                    ScreenAction::GridClick(index, button, shift) => {
                        let _ = self.native_net_send_game_to_server(
                            horizon_mods::screen::GRID_CLICK_CHANNEL,
                            &horizon_mods::screen::encode_grid_click(index, button, shift),
                        );
                    }
                    ScreenAction::Click => self.audio.play_button_click(),
                    ScreenAction::Close => self.close_container(),
                }
            }
        }
    }

    /// Focuses a code screen's field with `text` (`keyboard`: a press, which opens a touch keyboard).
    fn focus_field(id: i32, max: u16, filter: format::FieldFilter, text: &str, keyboard: bool) {
        use horizon_mods::content::Validator;
        let validator = match filter {
            format::FieldFilter::Any => Validator::Any,
            format::FieldFilter::Digits => Validator::Digit,
            format::FieldFilter::Id => Validator::Id,
        };
        if let Ok(mut f) = super::super::menu_screens::FIELD.lock() {
            if f.as_ref().is_some_and(|f| f.id == id) {
                return;
            }
            let mut edit = horizon_client::ui::EditBox::new(0.0, 0.0, 100.0, 20.0);
            edit.max_length = usize::from(max);
            edit.set_value(text);
            edit.move_cursor_to_end(false);
            *f = Some(super::super::menu_screens::ModField {
                id,
                validator,
                edit,
            });
            if keyboard {
                super::super::menu_screens::FIELD_KEYBOARD
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    /// Closes the open screen, its component kept for the next.
    fn close(open: &mut Option<Open>) {
        if let Some(mut o) = open.take() {
            take_actions(o.screen.close(&HostData::default()));
            if o.screen.failed.is_none()
                && let Ok(mut i) = INSTANCES.lock()
            {
                i.insert(o.screen.id.clone(), o.screen);
            }
        }
    }

    /// A new screen opened: the code screen open (if any) closes.
    pub(in crate::game) fn reset() {
        if let Ok(mut g) = OPEN.lock() {
            close(&mut g);
        }
    }

    /// Mekanism's `blitNineSlicedSized`: corners as they are, the edges and the middle repeated (each piece centred
    /// in its source, as `blitRepeating` cuts it). `(destination, source)` pairs, both from their own origins.
    pub(super) fn nine_slices(rect: Rect, borders: Rect, uv: Rect) -> Vec<(Rect, Rect)> {
        let [x, y, w, h] = rect.map(|v| v.round());
        let [u, v, uw, vh] = uv;
        let (l, t, r, b) = (
            borders[0].min((w / 2.0).floor()),
            borders[1].min((h / 2.0).floor()),
            borders[2].min((w / 2.0).floor()),
            borders[3].min((h / 2.0).floor()),
        );
        let mut out = Vec::new();
        // `blitRepeating`: the target cut into source-sized pieces, each from the middle of the source.
        let mut repeat =
            |dx: f32, dy: f32, dw: f32, dh: f32, su: f32, sv: f32, sw: f32, sh: f32| {
                if dw <= 0.0 || dh <= 0.0 || sw <= 0.0 || sh <= 0.0 {
                    return;
                }
                let mut i = 0.0;
                while i < dw {
                    let pw = sw.min(dw - i);
                    let k = ((sw - pw) / 2.0).floor();
                    let mut j = 0.0;
                    while j < dh {
                        let ph = sh.min(dh - j);
                        let m = ((sh - ph) / 2.0).floor();
                        out.push(([dx + i, dy + j, pw, ph], [su + k, sv + m, pw, ph]));
                        j += ph;
                    }
                    i += pw;
                }
            };
        if w == uw && h == vh {
            repeat(x, y, w, h, u, v, uw, vh);
            return out;
        }
        let (mw, mh) = (uw - l - r, vh - t - b);
        if h == vh {
            repeat(x, y, l, h, u, v, l, h);
            repeat(x + l, y, w - l - r, h, u + l, v, mw, vh);
            repeat(x + w - r, y, r, h, u + uw - r, v, r, h);
            return out;
        }
        if w == uw {
            repeat(x, y, w, t, u, v, w, t);
            repeat(x, y + t, w, h - t - b, u, v + t, uw, mh);
            repeat(x, y + h - b, w, b, u, v + vh - b, w, b);
            return out;
        }
        repeat(x, y, l, t, u, v, l, t);
        repeat(x + l, y, w - l - r, t, u + l, v, mw, t);
        repeat(x + w - r, y, r, t, u + uw - r, v, r, t);
        repeat(x, y + h - b, l, b, u, v + vh - b, l, b);
        repeat(x + l, y + h - b, w - l - r, b, u + l, v + vh - b, mw, b);
        repeat(x + w - r, y + h - b, r, b, u + uw - r, v + vh - b, r, b);
        repeat(x, y + t, l, h - t - b, u, v + t, l, mh);
        repeat(x + l, y + t, w - l - r, h - t - b, u + l, v + t, mw, mh);
        repeat(x + w - r, y + t, r, h - t - b, u + uw - r, v + t, r, mh);
        out
    }
}

impl Game<'_> {
    /// `horizon-ctl state`'s `screen_code`: the open code screen's menu, its last and average render time in
    /// milliseconds, the frames drawn in the last second with it open, its renders, the last frame's commands and its
    /// error if it stopped; null when no menu drawn by code is open.
    pub(in crate::game) fn code_screen_state(&self) -> serde_json::Value {
        match self.code_screen_stats() {
            Some(s) => serde_json::json!({
                "menu": s.menu,
                "last_render_ms": s.last_render_ms,
                "average_render_ms": s.average_render_ms,
                "renders": s.renders,
                "fps": s.fps,
                "commands": s.commands,
                "error": s.error,
            }),
            None => serde_json::Value::Null,
        }
    }
}

#[cfg(feature = "mods")]
pub(super) use imp::{WASM, failure, reset};

#[cfg(not(feature = "mods"))]
pub(super) fn reset() {}

/// Without the WebAssembly runtime a code screen never runs: its JSON screen is drawn, and this says why.
#[cfg(not(feature = "mods"))]
pub(super) fn failure(_menu: &str) -> Option<String> {
    Some("this build runs no mod code (no mods feature)".into())
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(in crate::game) fn code_screen_draw(
        &self,
        _block: &str,
        _batch: &mut HudBatch,
        _origin: [f32; 2],
    ) -> bool {
        false
    }
    pub(in crate::game) fn code_screen_running(&self) -> bool {
        false
    }
    pub(in crate::game) fn code_screen_draw_after_slots(&self, _batch: &mut HudBatch) {}
    pub(in crate::game) fn code_screen_tooltip(&self, _batch: &mut HudBatch) -> bool {
        false
    }
    pub(in crate::game) fn code_screen_press(&self, _at: [f32; 2], _right: bool) -> Option<bool> {
        None
    }
    pub(in crate::game) fn code_screen_release(&self) {}
    pub(in crate::game) fn code_screen_motion(&self) {}
    pub(in crate::game) fn code_screen_scroll(&self, _delta: f32) -> Option<bool> {
        None
    }
    pub(in crate::game) fn code_screen_key(&self, _key: &str) -> bool {
        false
    }
    pub(in crate::game) fn code_screen_char(&self, _c: char) -> bool {
        false
    }
    pub(in crate::game) fn code_screen_field(
        &self,
        _id: i32,
        _text: &str,
        _submitted: bool,
    ) -> bool {
        false
    }
    pub(in crate::game) fn code_screen_focus_field(&self, _id: i32) -> Option<()> {
        None
    }
    pub(in crate::game) fn code_screen_controls(&self) -> Option<Vec<CodeControl>> {
        None
    }
    pub(in crate::game) fn code_screen_commands(&self) -> Option<Vec<String>> {
        None
    }
    pub(in crate::game) fn code_screen_texts(&self) -> Option<Vec<(String, [f32; 2])>> {
        None
    }
    pub(in crate::game) fn code_screen_slot(&self, _slot: usize) -> Option<(f32, f32, bool)> {
        None
    }
    pub(in crate::game) fn code_screen_stats(&self) -> Option<CodeStats> {
        None
    }
    pub(in crate::game) fn tick_code_screen(&mut self) {}
    pub(in crate::game) fn code_screen_actions(&mut self) {}
}

#[cfg(all(test, feature = "mods"))]
mod tests {
    use super::imp::nine_slices;

    #[test]
    fn nine_slicing_keeps_corners_and_repeats_the_rest_as_mekanism() {
        // A 256 by 256 base with 4-pixel borders over a 20 by 10 panel: the corners 4 by 4 from the texture's
        // corners, the top edge 12 wide from the middle of the source's top band.
        let pieces = nine_slices([0.0, 0.0, 20.0, 10.0], [4.0; 4], [0.0, 0.0, 256.0, 256.0]);
        assert!(pieces.contains(&([0.0, 0.0, 4.0, 4.0], [0.0, 0.0, 4.0, 4.0])));
        assert!(pieces.contains(&([16.0, 6.0, 4.0, 4.0], [252.0, 252.0, 4.0, 4.0])));
        // The top edge: 12 pixels from the middle of the 248-wide band (k = (248 - 12) / 2 = 118, from u 4).
        assert!(pieces.contains(&([4.0, 0.0, 12.0, 4.0], [122.0, 0.0, 12.0, 4.0])));
        // Every destination pixel is covered once.
        let area: f32 = pieces.iter().map(|(d, _)| d[2] * d[3]).sum();
        assert_eq!(area, 200.0);
        // A destination the source's size is one blit.
        assert_eq!(
            nine_slices([5.0, 5.0, 6.0, 6.0], [2.0; 4], [0.0, 0.0, 6.0, 6.0]),
            vec![([5.0, 5.0, 6.0, 6.0], [0.0, 0.0, 6.0, 6.0])]
        );
        // A target larger than the source repeats it: a 6-wide middle over 20 pixels is 6 + 6 + 6 + 2.
        let long = nine_slices([0.0, 0.0, 24.0, 6.0], [2.0; 4], [0.0, 0.0, 10.0, 6.0]);
        let middle: Vec<_> = long
            .iter()
            .filter(|(d, _)| d[0] >= 2.0 && d[0] < 22.0)
            .collect();
        assert_eq!(
            middle.iter().map(|(d, _)| d[2]).collect::<Vec<_>>(),
            [6.0, 6.0, 6.0, 2.0]
        );
        // The last piece (2 wide) comes from the middle of its 6-pixel source: u 2 + 2.
        assert_eq!(middle[3].1, [4.0, 0.0, 2.0, 6.0]);
    }
}
