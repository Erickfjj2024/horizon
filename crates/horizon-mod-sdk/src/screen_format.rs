//! The command buffer a screen's `render` returns (capability `screen`, API 56): one byte buffer per frame, written by
//! [`Canvas`] in the mod and read by [`decode`] in the game, so a frame costs one call into the mod however much it
//! draws. This file is the format's one source: the game includes it as it is.
//!
//! Layout, little-endian: `u16` version ([`VERSION`]), `u8` flags ([`ANIMATE`]), then commands, each a `u8` op and its
//! arguments. Coordinates are `f32` GUI pixels from the screen panel's top left; colours `u32` 0xAARRGGBB; strings a
//! `u16` byte length and UTF-8 bytes (at most [`MAX_STRING`]).

/// The buffer format's version.
pub const VERSION: u16 = 1;
/// Header flag: the screen animates, so the game calls `render` again next frame (at most 30 times a second) even
/// when nothing changed.
pub const ANIMATE: u8 = 1;
/// Commands a frame may hold at most.
pub const MAX_COMMANDS: usize = 8192;
/// Bytes a frame's buffer may hold at most.
pub const MAX_BYTES: usize = 256 * 1024;
/// Bytes a string may hold at most.
pub const MAX_STRING: usize = 1024;
/// Depth of the pose and scissor stacks.
pub const MAX_DEPTH: usize = 16;
/// Lines a tooltip may hold at most.
pub const MAX_TOOLTIP_LINES: usize = 64;

/// `TEXT` flags.
pub const TEXT_SHADOW: u8 = 1;
pub const TEXT_CENTER: u8 = 2;
pub const TEXT_RIGHT: u8 = 4;
/// `ITEM` flags: draw the count (when over 1) and the durability bar.
pub const ITEM_DECORATIONS: u8 = 1;
/// `FIELD` flags.
pub const FIELD_NO_BORDER: u8 = 1;
pub const FIELD_FOCUS: u8 = 2;

/// What an `AREA` is, for the game's own uses of it (the cursor's hover, `horizon-ctl ui`, a controller's focus).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaKind {
    Button,
    Tab,
    Row,
    Toggle,
    Other,
}
impl AreaKind {
    fn byte(self) -> u8 {
        match self {
            AreaKind::Button => 0,
            AreaKind::Tab => 1,
            AreaKind::Row => 2,
            AreaKind::Toggle => 3,
            AreaKind::Other => 4,
        }
    }
    fn of(b: u8) -> Option<Self> {
        Some(match b {
            0 => AreaKind::Button,
            1 => AreaKind::Tab,
            2 => AreaKind::Row,
            3 => AreaKind::Toggle,
            4 => AreaKind::Other,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            AreaKind::Button => "button",
            AreaKind::Tab => "tab",
            AreaKind::Row => "row",
            AreaKind::Toggle => "toggle",
            AreaKind::Other => "area",
        }
    }
}

/// What a text field takes (as the JSON `textfield`'s `validator`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldFilter {
    Any,
    Digits,
    /// Letters, digits, `_`, `:`, `/`, `.`, `-`, `#`, `*` (a tag or an id).
    Id,
}
impl FieldFilter {
    fn byte(self) -> u8 {
        match self {
            FieldFilter::Any => 0,
            FieldFilter::Digits => 1,
            FieldFilter::Id => 2,
        }
    }
    fn of(b: u8) -> Option<Self> {
        Some(match b {
            0 => FieldFilter::Any,
            1 => FieldFilter::Digits,
            2 => FieldFilter::Id,
            _ => return None,
        })
    }
}

/// A rectangle: left, top, width, height.
pub type Rect = [f32; 4];

/// One drawing command, as [`decode`] reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// `GuiGraphics.fill`.
    Fill { rect: Rect, argb: u32 },
    /// `GuiGraphics.fillGradient`: top colour to bottom colour.
    Gradient { rect: Rect, top: u32, bottom: u32 },
    /// `GuiGraphics.blit` of a texture (`<ns>:gui/<path>`, the file under `assets/<ns>/textures/`): the region `uv`
    /// (u, v, width, height in texture pixels) of a texture `tex` (width, height) pixels big, drawn over `rect`.
    Blit {
        texture: String,
        rect: Rect,
        uv: Rect,
        tex: [f32; 2],
        argb: u32,
    },
    /// `GuiGraphics.blitSprite` of a GUI atlas sprite (`<ns>:<path>` under `textures/gui/sprites/`): nine-sliced when
    /// its `.mcmeta` says so, as the game draws its own.
    Sprite {
        sprite: String,
        rect: Rect,
        argb: u32,
    },
    /// Mekanism's `GuiUtils.blitNineSliced`: a texture region drawn over `rect` with its borders (left, top, right,
    /// bottom) kept and the middle stretched.
    Nine {
        texture: String,
        rect: Rect,
        borders: Rect,
        uv: Rect,
        tex: [f32; 2],
        argb: u32,
    },
    /// `GuiGraphics.drawString`, scaled by `scale` from `(x, y)`.
    Text {
        text: String,
        x: f32,
        y: f32,
        argb: u32,
        flags: u8,
        scale: f32,
    },
    /// `GuiGraphics.renderItem` (+ `renderItemDecorations` with [`ITEM_DECORATIONS`]): 16 by 16 times `scale`.
    Item {
        item: String,
        components: String,
        count: u32,
        x: f32,
        y: f32,
        flags: u8,
        scale: f32,
    },
    /// A tank's contents as Mekanism's gauges draw them: atlas sprite `sprite` (a fluid's still texture, a chemical's)
    /// tiled from the bottom of `rect` up to `fill` (0 to 1) and tinted.
    Fluid {
        sprite: String,
        rect: Rect,
        fill: f32,
        argb: u32,
    },
    /// `enableScissor`: what follows is clipped to `rect` (and to the scissors under it).
    Scissor { rect: Rect },
    /// `disableScissor`.
    Unscissor,
    /// `pose().pushPose()`.
    Push,
    /// `pose().popPose()`.
    Pop,
    /// `pose().translate(dx, dy, 0)`.
    Translate { dx: f32, dy: f32 },
    /// `pose().scale(sx, sy, 1)`.
    Scale { sx: f32, sy: f32 },
    /// The game draws the items of the menu's real slots here (after the panel, before windows over it); without it
    /// they come after everything.
    Slots,
    /// Where the menu's real slot `index` is this frame and whether it is shown and clicked (a window over it hides it).
    Slot {
        index: u32,
        x: f32,
        y: f32,
        active: bool,
    },
    /// A clickable area: the cursor's hover, `horizon-ctl ui` (`screen-<id>`) and `click`, a controller's focus. The
    /// clicks themselves reach the screen's `input`.
    Area {
        id: i32,
        rect: Rect,
        kind: AreaKind,
        active: bool,
        label: String,
    },
    /// A text field the game owns (focus, cursor, selection, the touch keyboard and IME): drawn over `rect`; its edits
    /// come back as `field-changed` / `field-submitted`. `text` is its text when it first shows (and whenever the screen
    /// changes it); `color` its text colour (0 the game's).
    Field {
        id: i32,
        rect: Rect,
        max_length: u16,
        filter: FieldFilter,
        text: String,
        flags: u8,
        color: u32,
    },
    /// A tooltip at the cursor, drawn over everything at the end of the frame: lines and their colours.
    Tooltip { lines: Vec<(String, u32)> },
}

/// A decoded buffer: its header flags and commands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub flags: u8,
    pub commands: Vec<Command>,
}

const FILL: u8 = 1;
const GRADIENT: u8 = 2;
const BLIT: u8 = 3;
const SPRITE: u8 = 4;
const NINE: u8 = 5;
const TEXT: u8 = 6;
const ITEM: u8 = 7;
const FLUID: u8 = 8;
const SCISSOR: u8 = 9;
const UNSCISSOR: u8 = 10;
const PUSH: u8 = 11;
const POP: u8 = 12;
const TRANSLATE: u8 = 13;
const SCALE: u8 = 14;
const SLOTS: u8 = 15;
const SLOT: u8 = 16;
const AREA: u8 = 17;
const FIELD: u8 = 18;
const TOOLTIP: u8 = 19;

/// Writes a frame's commands: the mod's side of the buffer, with methods named after `GuiGraphics`'.
#[derive(Clone, Debug)]
pub struct Canvas {
    bytes: Vec<u8>,
    commands: usize,
}
impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}
impl Canvas {
    pub fn new() -> Self {
        let mut bytes = Vec::with_capacity(4096);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.push(0);
        Canvas { bytes, commands: 0 }
    }
    /// Asks the game to call `render` again next frame (an animation), at most 30 times a second.
    pub fn animate(&mut self) {
        self.bytes[2] |= ANIMATE;
    }
    /// The buffer for `render` to return.
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
    /// Commands written so far.
    pub fn len(&self) -> usize {
        self.commands
    }
    pub fn is_empty(&self) -> bool {
        self.commands == 0
    }
    fn op(&mut self, op: u8) {
        self.commands += 1;
        self.bytes.push(op);
    }
    fn f(&mut self, v: f32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn u(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn i(&mut self, v: i32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn rect(&mut self, r: Rect) {
        for v in r {
            self.f(v);
        }
    }
    fn s(&mut self, text: &str) {
        // Cut at a character boundary within the cap.
        let mut end = text.len().min(MAX_STRING);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.bytes.extend_from_slice(&(end as u16).to_le_bytes());
        self.bytes.extend_from_slice(&text.as_bytes()[..end]);
    }

    /// `fill(x1, y1, x2, y2, argb)` as a rectangle.
    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        self.op(FILL);
        self.rect([x, y, w, h]);
        self.u(argb);
    }
    /// `fillGradient`: `top` at the top edge to `bottom` at the bottom edge.
    pub fn gradient(&mut self, x: f32, y: f32, w: f32, h: f32, top: u32, bottom: u32) {
        self.op(GRADIENT);
        self.rect([x, y, w, h]);
        self.u(top);
        self.u(bottom);
    }
    /// `blit(texture, x, y, u, v, w, h, texWidth, texHeight)`: the region at `(u, v)` the size of the destination.
    pub fn blit(
        &mut self,
        texture: &str,
        x: f32,
        y: f32,
        u: f32,
        v: f32,
        w: f32,
        h: f32,
        tex_w: f32,
        tex_h: f32,
    ) {
        self.blit_region(
            texture,
            [x, y, w, h],
            [u, v, w, h],
            [tex_w, tex_h],
            u32::MAX,
        );
    }
    /// `blit` of region `uv` (u, v, width, height) stretched over `rect`, tinted `argb`.
    pub fn blit_region(&mut self, texture: &str, rect: Rect, uv: Rect, tex: [f32; 2], argb: u32) {
        self.op(BLIT);
        self.s(texture);
        self.rect(rect);
        self.rect(uv);
        self.f(tex[0]);
        self.f(tex[1]);
        self.u(argb);
    }
    /// `blitSprite(sprite, x, y, w, h)`.
    pub fn blit_sprite(&mut self, sprite: &str, x: f32, y: f32, w: f32, h: f32) {
        self.blit_sprite_tinted(sprite, [x, y, w, h], u32::MAX);
    }
    pub fn blit_sprite_tinted(&mut self, sprite: &str, rect: Rect, argb: u32) {
        self.op(SPRITE);
        self.s(sprite);
        self.rect(rect);
        self.u(argb);
    }
    /// Mekanism's `blitNineSliced(texture, x, y, w, h, sliceW, sliceH, uW, vH, u, v, texW, texH)` with all four borders
    /// given.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_nine_sliced(
        &mut self,
        texture: &str,
        rect: Rect,
        borders: Rect,
        uv: Rect,
        tex: [f32; 2],
        argb: u32,
    ) {
        self.op(NINE);
        self.s(texture);
        self.rect(rect);
        self.rect(borders);
        self.rect(uv);
        self.f(tex[0]);
        self.f(tex[1]);
        self.u(argb);
    }
    /// `drawString(font, text, x, y, color, shadow)`.
    pub fn draw_string(&mut self, text: &str, x: f32, y: f32, argb: u32, shadow: bool) {
        self.text(text, x, y, argb, if shadow { TEXT_SHADOW } else { 0 }, 1.0);
    }
    /// Text with [`TEXT_SHADOW`] / [`TEXT_CENTER`] / [`TEXT_RIGHT`] flags, scaled from `(x, y)` (Mekanism's
    /// `drawScaledText`).
    pub fn text(&mut self, text: &str, x: f32, y: f32, argb: u32, flags: u8, scale: f32) {
        self.op(TEXT);
        self.s(text);
        self.f(x);
        self.f(y);
        self.u(argb);
        self.bytes.push(flags);
        self.f(scale);
    }
    /// `renderItem` + `renderItemDecorations`.
    pub fn render_item(
        &mut self,
        item: &str,
        components: &str,
        count: u32,
        x: f32,
        y: f32,
        decorations: bool,
    ) {
        self.item(
            item,
            components,
            count,
            x,
            y,
            if decorations { ITEM_DECORATIONS } else { 0 },
            1.0,
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub fn item(
        &mut self,
        item: &str,
        components: &str,
        count: u32,
        x: f32,
        y: f32,
        flags: u8,
        scale: f32,
    ) {
        self.op(ITEM);
        self.s(item);
        self.s(components);
        self.u(count);
        self.f(x);
        self.f(y);
        self.bytes.push(flags);
        self.f(scale);
    }
    /// A gauge's contents: `sprite` tiled from the bottom of `rect` up to `fill`.
    pub fn fluid(&mut self, sprite: &str, rect: Rect, fill: f32, argb: u32) {
        self.op(FLUID);
        self.s(sprite);
        self.rect(rect);
        self.f(fill);
        self.u(argb);
    }
    /// `enableScissor(x1, y1, x2, y2)` as a rectangle.
    pub fn enable_scissor(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.op(SCISSOR);
        self.rect([x, y, w, h]);
    }
    pub fn disable_scissor(&mut self) {
        self.op(UNSCISSOR);
    }
    pub fn push_pose(&mut self) {
        self.op(PUSH);
    }
    pub fn pop_pose(&mut self) {
        self.op(POP);
    }
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.op(TRANSLATE);
        self.f(dx);
        self.f(dy);
    }
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.op(SCALE);
        self.f(sx);
        self.f(sy);
    }
    /// The game draws the real slots' items here.
    pub fn slots(&mut self) {
        self.op(SLOTS);
    }
    /// Real slot `index` at `(x, y)` this frame; `active` false hides it and takes its clicks away.
    pub fn slot(&mut self, index: u32, x: f32, y: f32, active: bool) {
        self.op(SLOT);
        self.u(index);
        self.f(x);
        self.f(y);
        self.bytes.push(u8::from(active));
    }
    /// A clickable area with the id the screen knows it by.
    pub fn area(&mut self, id: i32, rect: Rect, kind: AreaKind, active: bool, label: &str) {
        self.op(AREA);
        self.i(id);
        self.rect(rect);
        self.bytes.push(kind.byte());
        self.bytes.push(u8::from(active));
        self.s(label);
    }
    /// A text field the game owns.
    #[allow(clippy::too_many_arguments)]
    pub fn field(
        &mut self,
        id: i32,
        rect: Rect,
        max_length: u16,
        filter: FieldFilter,
        text: &str,
        flags: u8,
        color: u32,
    ) {
        self.op(FIELD);
        self.i(id);
        self.rect(rect);
        self.bytes.extend_from_slice(&max_length.to_le_bytes());
        self.bytes.push(filter.byte());
        self.s(text);
        self.bytes.push(flags);
        self.u(color);
    }
    /// `renderComponentTooltip`: lines with their colours, at the cursor.
    pub fn tooltip(&mut self, lines: &[(&str, u32)]) {
        self.op(TOOLTIP);
        let n = lines.len().min(MAX_TOOLTIP_LINES);
        self.bytes.push(n as u8);
        for (text, argb) in &lines[..n] {
            self.s(text);
            self.u(*argb);
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|e| *e <= self.bytes.len())
            .ok_or("the buffer ends inside a command")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    /// A finite number; NaN and infinities are 0, so no command carries one into the renderer.
    fn f32(&mut self) -> Result<f32, String> {
        let v = f32::from_bits(self.u32()?);
        Ok(if v.is_finite() {
            v.clamp(-1.0e6, 1.0e6)
        } else {
            0.0
        })
    }
    fn rect(&mut self) -> Result<Rect, String> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }
    fn string(&mut self) -> Result<String, String> {
        let n = usize::from(self.u16()?);
        if n > MAX_STRING {
            return Err(format!("a string of {n} bytes (at most {MAX_STRING})"));
        }
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| "a string that is not UTF-8".to_owned())
    }
}

/// Reads a buffer, checking its version and caps.
pub fn decode(bytes: &[u8]) -> Result<Frame, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "a buffer of {} bytes (at most {MAX_BYTES})",
            bytes.len()
        ));
    }
    let mut r = Reader { bytes, at: 0 };
    let version = r.u16()?;
    if version != VERSION {
        return Err(format!(
            "buffer version {version}; this game reads {VERSION}"
        ));
    }
    let flags = r.u8()?;
    let mut commands = Vec::new();
    while r.at < bytes.len() {
        if commands.len() == MAX_COMMANDS {
            return Err(format!("more than {MAX_COMMANDS} commands"));
        }
        let op = r.u8()?;
        let c = match op {
            FILL => Command::Fill {
                rect: r.rect()?,
                argb: r.u32()?,
            },
            GRADIENT => Command::Gradient {
                rect: r.rect()?,
                top: r.u32()?,
                bottom: r.u32()?,
            },
            BLIT => Command::Blit {
                texture: r.string()?,
                rect: r.rect()?,
                uv: r.rect()?,
                tex: [r.f32()?, r.f32()?],
                argb: r.u32()?,
            },
            SPRITE => Command::Sprite {
                sprite: r.string()?,
                rect: r.rect()?,
                argb: r.u32()?,
            },
            NINE => Command::Nine {
                texture: r.string()?,
                rect: r.rect()?,
                borders: r.rect()?,
                uv: r.rect()?,
                tex: [r.f32()?, r.f32()?],
                argb: r.u32()?,
            },
            TEXT => Command::Text {
                text: r.string()?,
                x: r.f32()?,
                y: r.f32()?,
                argb: r.u32()?,
                flags: r.u8()?,
                scale: r.f32()?,
            },
            ITEM => Command::Item {
                item: r.string()?,
                components: r.string()?,
                count: r.u32()?,
                x: r.f32()?,
                y: r.f32()?,
                flags: r.u8()?,
                scale: r.f32()?,
            },
            FLUID => Command::Fluid {
                sprite: r.string()?,
                rect: r.rect()?,
                fill: r.f32()?.clamp(0.0, 1.0),
                argb: r.u32()?,
            },
            SCISSOR => Command::Scissor { rect: r.rect()? },
            UNSCISSOR => Command::Unscissor,
            PUSH => Command::Push,
            POP => Command::Pop,
            TRANSLATE => Command::Translate {
                dx: r.f32()?,
                dy: r.f32()?,
            },
            SCALE => Command::Scale {
                sx: r.f32()?,
                sy: r.f32()?,
            },
            SLOTS => Command::Slots,
            SLOT => Command::Slot {
                index: r.u32()?,
                x: r.f32()?,
                y: r.f32()?,
                active: r.u8()? != 0,
            },
            AREA => {
                let id = r.i32()?;
                let rect = r.rect()?;
                let kind = AreaKind::of(r.u8()?).ok_or("an area of an unknown kind")?;
                Command::Area {
                    id,
                    rect,
                    kind,
                    active: r.u8()? != 0,
                    label: r.string()?,
                }
            }
            FIELD => {
                let id = r.i32()?;
                let rect = r.rect()?;
                let max_length = r.u16()?;
                let filter = FieldFilter::of(r.u8()?).ok_or("a field of an unknown filter")?;
                Command::Field {
                    id,
                    rect,
                    max_length,
                    filter,
                    text: r.string()?,
                    flags: r.u8()?,
                    color: r.u32()?,
                }
            }
            TOOLTIP => {
                let n = usize::from(r.u8()?);
                if n > MAX_TOOLTIP_LINES {
                    return Err(format!(
                        "a tooltip of {n} lines (at most {MAX_TOOLTIP_LINES})"
                    ));
                }
                let mut lines = Vec::with_capacity(n);
                for _ in 0..n {
                    lines.push((r.string()?, r.u32()?));
                }
                Command::Tooltip { lines }
            }
            other => return Err(format!("unknown command {other}")),
        };
        commands.push(c);
    }
    Ok(Frame { flags, commands })
}

fn n(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
fn r(rect: &Rect) -> String {
    rect.iter().map(|v| n(*v)).collect::<Vec<_>>().join(" ")
}

/// One line for a command, for tests and `horizon-ctl ui`: the op's name and its main arguments
/// (`TEXT "No frequency" 8 6`, `AREA 9100 tab 98 20 26 18`).
pub fn describe(c: &Command) -> String {
    match c {
        Command::Fill { rect, argb } => format!("FILL {} {argb:08x}", r(rect)),
        Command::Gradient { rect, top, bottom } => {
            format!("GRADIENT {} {top:08x} {bottom:08x}", r(rect))
        }
        Command::Blit {
            texture, rect, uv, ..
        } => format!("BLIT {texture} {} {}", r(rect), r(uv)),
        Command::Sprite { sprite, rect, .. } => format!("SPRITE {sprite} {}", r(rect)),
        Command::Nine { texture, rect, .. } => format!("NINE {texture} {}", r(rect)),
        Command::Text { text, x, y, .. } => format!("TEXT {text:?} {} {}", n(*x), n(*y)),
        Command::Item {
            item, count, x, y, ..
        } => format!("ITEM {item}*{count} {} {}", n(*x), n(*y)),
        Command::Fluid {
            sprite, rect, fill, ..
        } => format!("FLUID {sprite} {} {}", r(rect), n(*fill)),
        Command::Scissor { rect } => format!("SCISSOR {}", r(rect)),
        Command::Unscissor => "UNSCISSOR".into(),
        Command::Push => "PUSH".into(),
        Command::Pop => "POP".into(),
        Command::Translate { dx, dy } => format!("TRANSLATE {} {}", n(*dx), n(*dy)),
        Command::Scale { sx, sy } => format!("SCALE {} {}", n(*sx), n(*sy)),
        Command::Slots => "SLOTS".into(),
        Command::Slot {
            index,
            x,
            y,
            active,
        } => format!(
            "SLOT {index} {} {}{}",
            n(*x),
            n(*y),
            if *active { "" } else { " off" }
        ),
        Command::Area {
            id,
            rect,
            kind,
            active,
            ..
        } => {
            format!(
                "AREA {id} {} {}{}",
                kind.name(),
                r(rect),
                if *active { "" } else { " off" }
            )
        }
        Command::Field { id, rect, text, .. } => format!("FIELD {id} {} {text:?}", r(rect)),
        Command::Tooltip { lines } => {
            format!(
                "TOOLTIP {}",
                lines
                    .iter()
                    .map(|(t, _)| format!("{t:?}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_round_trips_and_describes() {
        let mut c = Canvas::new();
        c.fill(1.0, 2.0, 3.0, 4.0, 0xFF00_00FF);
        c.gradient(0.0, 0.0, 10.0, 10.0, 0xFF00_0000, 0xFFFF_FFFF);
        c.blit(
            "mekanism:gui/qio",
            0.0,
            0.0,
            0.5,
            0.0,
            16.0,
            16.0,
            256.0,
            256.0,
        );
        c.blit_sprite("mekanism:button/frequency", 4.0, 4.0, 38.0, 20.0);
        c.blit_nine_sliced(
            "mekanism:gui/base",
            [0.0, 0.0, 176.0, 166.0],
            [4.0; 4],
            [0.0, 0.0, 16.0, 16.0],
            [16.0, 16.0],
            u32::MAX,
        );
        c.text(
            "Frequency: Base (0/0)",
            8.0,
            6.0,
            0xFF40_4040,
            TEXT_CENTER | TEXT_SHADOW,
            0.75,
        );
        c.render_item("minecraft:stone", "", 64, 8.0, 18.0, true);
        c.fluid(
            "minecraft:block/water_still",
            [7.0, 16.0, 18.0, 60.0],
            0.5,
            0xFF3F_76E4,
        );
        c.enable_scissor(9.0, 31.0, 158.0, 40.0);
        c.push_pose();
        c.translate(-26.0, 0.0);
        c.scale(0.5, 0.5);
        c.pop_pose();
        c.disable_scissor();
        c.slots();
        c.slot(0, 8.0, 84.0, false);
        c.area(
            9100,
            [-26.0, 20.0, 26.0, 18.0],
            AreaKind::Tab,
            true,
            "Set Frequency",
        );
        c.field(
            8533,
            [10.0, 120.0, 100.0, 12.0],
            16,
            FieldFilter::Any,
            "Base",
            FIELD_FOCUS,
            0,
        );
        c.tooltip(&[("Diamond", 0xFFFF_FFFF), ("64", 0xFFAA_AAAA)]);
        c.animate();
        assert_eq!(c.len(), 19);
        let frame = decode(&c.finish()).unwrap();
        assert_eq!(frame.flags, ANIMATE);
        assert_eq!(frame.commands.len(), 19);
        let lines: Vec<String> = frame.commands.iter().map(describe).collect();
        assert_eq!(lines[5], "TEXT \"Frequency: Base (0/0)\" 8 6");
        assert_eq!(lines[6], "ITEM minecraft:stone*64 8 18");
        assert_eq!(lines[15], "SLOT 0 8 84 off");
        assert_eq!(lines[16], "AREA 9100 tab -26 20 26 18");
        assert_eq!(lines[17], "FIELD 8533 10 120 100 12 \"Base\"");
        assert!(matches!(
            frame.commands[2],
            Command::Blit {
                uv: [0.5, 0.0, 16.0, 16.0],
                ..
            }
        ));
    }

    #[test]
    fn bad_buffers_are_refused_not_drawn() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[2, 0, 0]).unwrap_err().contains("version 2"));
        // A command cut short.
        let mut c = Canvas::new();
        c.fill(0.0, 0.0, 1.0, 1.0, 0);
        let mut bytes = c.finish();
        bytes.pop();
        assert!(decode(&bytes).is_err());
        // An unknown op.
        assert!(
            decode(&[1, 0, 0, 200])
                .unwrap_err()
                .contains("unknown command 200")
        );
        // Too many commands.
        let mut c = Canvas::new();
        for _ in 0..=MAX_COMMANDS {
            c.push_pose();
        }
        assert!(decode(&c.finish()).unwrap_err().contains("more than"));
        // A non-finite number reads as 0.
        let mut c = Canvas::new();
        c.translate(f32::NAN, f32::INFINITY);
        assert_eq!(
            decode(&c.finish()).unwrap().commands,
            [Command::Translate { dx: 0.0, dy: 0.0 }]
        );
        // A long string is cut at a character boundary, not refused.
        let mut c = Canvas::new();
        c.draw_string(&"é".repeat(MAX_STRING), 0.0, 0.0, 0, false);
        let Command::Text { text, .. } = &decode(&c.finish()).unwrap().commands[0] else {
            panic!()
        };
        assert!(text.len() <= MAX_STRING && text.chars().all(|c| c == 'é'));
    }
}
