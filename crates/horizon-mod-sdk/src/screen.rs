//! Screens drawn by code (capability `screen`, API 56; docs/mods-screen.md): a menu type whose screen says
//! `"code": true` is drawn by the mod's screens component, a second crate of the mod built into `screens.wasm`. A type
//! implementing [`ScreenMod`], exported with [`export_screen_mod!`](crate::export_screen_mod), opens with the menu,
//! draws each frame into a [`Canvas`] (methods named after `GuiGraphics`'), and hears the player's input.
//!
//! ```ignore
//! use horizon_mod_sdk::screen::{Canvas, Frame, InputEvent, OpenInfo, ScreenMod, Setup, host};
//! struct Tele;
//! impl ScreenMod for Tele {
//!     fn open(_info: &OpenInfo) -> Result<(Self, Setup), String> {
//!         Ok((Tele, Setup { width: 0, height: 0, hide_inventory: false, own_labels: true }))
//!     }
//!     fn render(&mut self, _frame: &Frame, c: &mut Canvas) {
//!         let strings = host::menu_strings();
//!         c.draw_string(strings.first().map_or("", String::as_str), 8.0, 6.0, 0xFF40_4040, false);
//!     }
//! }
//! horizon_mod_sdk::export_screen_mod!(Tele);
//! ```
//!
//! The menu's real slots stay the game's: the screen says where they are ([`Canvas::slot`]) and where their items are
//! drawn ([`Canvas::slots`]); a click on one is the game's. Text fields are the game's too ([`Canvas::field`]): it
//! keeps the focus, the cursor, the touch keyboard and the IME, and the screen hears `field-changed` and
//! `field-submitted`. [`widgets`] has Mekanism's scroll list, scroll bar, window and side tab.

wit_bindgen::generate!({
    path: "wit",
    world: "horizon-screen-mod",
    pub_export_macro: true,
    export_macro_name: "__export_horizon_screen_mod",
    default_bindings_module: "horizon_mod_sdk::screen",
    with: {
        "horizon:mods/log@0.1.0": crate::log,
    },
});

pub use crate::screen_format::{
    self as format, AreaKind, Canvas, FIELD_FOCUS, FIELD_NO_BORDER, FieldFilter, ITEM_DECORATIONS,
    Rect, TEXT_CENTER, TEXT_RIGHT, TEXT_SHADOW,
};
pub use exports::horizon::mods::screen_events::{
    Drag, FieldText, Frame, Guest as __Guest, InputEvent, Key, Modifiers, MouseButton, OpenInfo,
    Pointer, Setup, Wheel,
};
/// What the screen reads and sends: the menu as this client has it, and the menu's own paths to the server.
pub use horizon::mods::screen_host as host;

/// A mod's screens. One is open at a time: [`ScreenMod::open`] makes it for `info.menu` (match on it when the mod has
/// several code screens), [`ScreenMod::close`] ends it.
pub trait ScreenMod: Sized + 'static {
    /// The screen opens; an error closes the menu and is shown.
    fn open(info: &OpenInfo) -> Result<(Self, Setup), String>;
    /// 20 times a second, after the server's data for the tick arrived.
    fn tick(&mut self) {}
    /// The frame's drawing. The game calls it when something changed or when the last frame called
    /// [`Canvas::animate`], and draws the last frame otherwise.
    fn render(&mut self, frame: &Frame, canvas: &mut Canvas);
    /// The player's input on the screen, from the panel's top left: true when the screen took it.
    fn input(&mut self, _event: &InputEvent) -> bool {
        false
    }
    fn close(&mut self) {}
}

/// Exports a [`ScreenMod`] as the screens component's `screen-events`.
#[macro_export]
macro_rules! export_screen_mod {
    ($t:ty) => {
        const _: () = {
            use ::std::cell::RefCell;
            ::std::thread_local! {
                static STATE: RefCell<Option<$t>> = const { RefCell::new(None) };
            }
            struct Export;
            impl $crate::screen::__Guest for Export {
                fn open(info: $crate::screen::OpenInfo) -> Result<$crate::screen::Setup, String> {
                    let (s, setup) = <$t as $crate::screen::ScreenMod>::open(&info)?;
                    STATE.with(|st| *st.borrow_mut() = Some(s));
                    Ok(setup)
                }
                fn tick() {
                    STATE.with(|st| {
                        if let Some(s) = st.borrow_mut().as_mut() {
                            $crate::screen::ScreenMod::tick(s)
                        }
                    })
                }
                fn render(frame: $crate::screen::Frame) -> Vec<u8> {
                    let mut canvas = $crate::screen::Canvas::new();
                    STATE.with(|st| {
                        if let Some(s) = st.borrow_mut().as_mut() {
                            $crate::screen::ScreenMod::render(s, &frame, &mut canvas)
                        }
                    });
                    canvas.finish()
                }
                fn input(event: $crate::screen::InputEvent) -> bool {
                    STATE.with(|st| {
                        st.borrow_mut()
                            .as_mut()
                            .is_some_and(|s| $crate::screen::ScreenMod::input(s, &event))
                    })
                }
                fn close() {
                    STATE.with(|st| {
                        if let Some(mut s) = st.borrow_mut().take() {
                            $crate::screen::ScreenMod::close(&mut s)
                        }
                    })
                }
            }
            #[cfg(target_arch = "wasm32")]
            $crate::screen::__export_horizon_screen_mod!(Export);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = <Export as $crate::screen::__Guest>::open;
        };
    };
}

/// Mekanism's GUI elements, with its integer arithmetic, from the panel's top left (Mekanism's `getGuiLeft()` and
/// `getGuiTop()` are 0 here). Textures are named as the mod's atlas has them (`mekanism:gui/scroll_list` for the file
/// `assets/mekanism/textures/gui/scroll_list.png`): put Mekanism's PNGs there.
pub mod widgets {
    use super::{AreaKind, Canvas, Rect};

    /// `GuiUtils.renderBackgroundTexture`: `texture` (`tex` pixels big) nine-sliced over `rect` with `side`-pixel
    /// borders, its edges and middle repeated, as `blitNineSlicedSized` does.
    pub fn background(c: &mut Canvas, texture: &str, side: f32, rect: Rect, tex: [f32; 2]) {
        c.blit_nine_sliced(
            texture,
            rect,
            [side, side, side, side],
            [0.0, 0.0, tex[0], tex[1]],
            tex,
            u32::MAX,
        );
    }

    fn clamp(v: i32, lo: i32, hi: i32) -> i32 {
        v.max(lo).min(hi)
    }

    /// `GuiScrollableElement`: a scroll position from 0 to 1 and the bar's thumb.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Scrollable {
        pub x: i32,
        pub y: i32,
        pub width: i32,
        pub height: i32,
        pub bar_x_shift: i32,
        pub bar_x: i32,
        pub bar_y: i32,
        pub bar_width: i32,
        pub bar_height: i32,
        pub max_bar_height: i32,
        /// 0 at the top, 1 at the bottom.
        pub scroll: f64,
        drag_offset: i32,
        pub dragging: bool,
    }
    impl Scrollable {
        #[allow(clippy::too_many_arguments)]
        pub fn new(
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            bar_x_shift: i32,
            bar_y_shift: i32,
            bar_width: i32,
            bar_height: i32,
            max_bar_height: i32,
        ) -> Self {
            Scrollable {
                x,
                y,
                width,
                height,
                bar_x_shift,
                bar_x: x + bar_x_shift,
                bar_y: y + bar_y_shift,
                bar_width,
                bar_height,
                max_bar_height,
                scroll: 0.0,
                drag_offset: 0,
                dragging: false,
            }
        }
        fn max(&self) -> i32 {
            self.max_bar_height - self.bar_height
        }
        /// `getScroll`: the thumb's offset down the bar.
        pub fn thumb(&self) -> i32 {
            let max = self.max();
            clamp((self.scroll * f64::from(max)) as i32, 0, max)
        }
        /// `needsScrollBars`.
        pub fn needs_bar(max_elements: i32, focused: i32) -> bool {
            max_elements > focused
        }
        /// `getCurrentSelection`: the first element shown.
        pub fn first(&self, max_elements: i32, focused: i32) -> i32 {
            if Self::needs_bar(max_elements, focused) {
                ((f64::from(max_elements - focused) + 0.5) * self.scroll) as i32
            } else {
                0
            }
        }
        /// `adjustScroll`: a wheel step (sign of `delta`), `scaler` elements a step; true when it scrolled.
        pub fn wheel(&mut self, delta: f64, max_elements: i32, focused: i32, scaler: i32) -> bool {
            if delta != 0.0 && Self::needs_bar(max_elements, focused) {
                let elements =
                    (f64::from(max_elements - focused) / f64::from(scaler.max(1))).ceil() as i32;
                if elements > 0 {
                    let delta = if delta > 0.0 { 1.0 } else { -1.0 };
                    self.scroll = (self.scroll - delta / f64::from(elements)).clamp(0.0, 1.0);
                    return true;
                }
            }
            false
        }
        /// `onClick`: a press on the thumb starts dragging it (or, with nothing to scroll, puts the list at the top).
        pub fn press(&mut self, mx: f64, my: f64, max_elements: i32, focused: i32) -> bool {
            let scroll = self.thumb();
            let (x, y) = (f64::from(self.bar_x), f64::from(self.bar_y));
            if mx >= x
                && mx <= x + f64::from(self.bar_width)
                && my >= y + f64::from(scroll)
                && my <= y + f64::from(scroll + self.bar_height)
            {
                if Self::needs_bar(max_elements, focused) {
                    self.drag_offset = (my - f64::from(scroll + self.bar_y)) as i32;
                    self.dragging = true;
                } else {
                    self.scroll = 0.0;
                }
                return true;
            }
            false
        }
        /// `onDrag`.
        pub fn drag(&mut self, my: f64, max_elements: i32, focused: i32) {
            if self.dragging && Self::needs_bar(max_elements, focused) {
                let max = f64::from(self.max().max(1));
                self.scroll = ((my - f64::from(self.bar_y) - f64::from(self.drag_offset)) / max)
                    .clamp(0.0, 1.0);
            }
        }
        /// `onRelease`.
        pub fn release(&mut self) {
            self.drag_offset = 0;
            self.dragging = false;
        }
        pub fn is_over(&self, mx: f64, my: f64) -> bool {
            mx >= f64::from(self.x)
                && mx < f64::from(self.x + self.width)
                && my >= f64::from(self.y)
                && my < f64::from(self.y + self.height)
        }
        /// `drawScrollBar`: the borders and the thumb from a `tex_w` by `tex_h` texture (`scroll_list.png`).
        pub fn draw_bar(&self, c: &mut Canvas, texture: &str, tex_w: i32, tex_h: i32) {
            let (tw, th) = (tex_w as f32, tex_h as f32);
            let (bx, by) = (self.bar_x as f32, self.bar_y as f32);
            c.blit(texture, bx - 1.0, by - 1.0, 0.0, 0.0, tw, 1.0, tw, th);
            c.blit_region(
                texture,
                [bx - 1.0, by, tw, self.max_bar_height as f32],
                [0.0, 1.0, tw, 1.0],
                [tw, th],
                u32::MAX,
            );
            c.blit(
                texture,
                bx - 1.0,
                (self.y + self.max_bar_height + 2) as f32,
                0.0,
                0.0,
                tw,
                1.0,
                tw,
                th,
            );
            c.blit(
                texture,
                bx,
                by + self.thumb() as f32,
                0.0,
                2.0,
                self.bar_width as f32,
                self.bar_height as f32,
                tw,
                th,
            );
        }
    }

    /// `GuiScrollList`: rows `element_height` high over a background, with the scroll bar at the right (6 pixels).
    #[derive(Clone, Debug, PartialEq)]
    pub struct ScrollList {
        pub scroll: Scrollable,
        pub element_height: i32,
        /// `scroll_list.png` (6 by 6).
        pub texture: &'static str,
        /// The background (`GuiInnerScreen.SCREEN`, 32-pixel sides, 256 by 256).
        pub background: &'static str,
        pub background_side: i32,
        /// The selected row, if any.
        pub selected: Option<usize>,
    }
    impl ScrollList {
        pub const TEXTURE_SIZE: i32 = 6;
        pub fn new(x: i32, y: i32, width: i32, height: i32, element_height: i32) -> Self {
            ScrollList {
                scroll: Scrollable::new(x, y, width, height, width - 6, 2, 4, 4, height - 4),
                element_height,
                texture: "mekanism:gui/scroll_list",
                background: "mekanism:gui/inner_screen",
                background_side: 32,
                selected: None,
            }
        }
        /// `getFocusedElements`: rows that fit.
        pub fn focused(&self) -> i32 {
            (self.scroll.height - 2) / self.element_height
        }
        /// The first row shown.
        pub fn first(&self, rows: usize) -> usize {
            self.scroll.first(rows as i32, self.focused()) as usize
        }
        /// `drawBackground`: the background and the bar; then the rows are the screen's to draw (with
        /// [`ScrollList::draw_selection`] for a text list).
        pub fn draw_background(&self, c: &mut Canvas) {
            let s = &self.scroll;
            let side = self.background_side as f32;
            background(
                c,
                self.background,
                side,
                [s.x as f32, s.y as f32, s.width as f32, s.height as f32],
                [256.0, 256.0],
            );
            s.draw_bar(c, self.texture, Self::TEXTURE_SIZE, Self::TEXTURE_SIZE);
        }
        /// `GuiTextScrollList.renderElements`: the selected row's highlight.
        pub fn draw_selection(&self, c: &mut Canvas, rows: usize) {
            let first = self.first(rows);
            if let Some(sel) = self.selected
                && sel >= first
                && (sel as i32) < first as i32 + self.focused()
            {
                let s = &self.scroll;
                let ts = Self::TEXTURE_SIZE as f32;
                c.blit_region(
                    self.texture,
                    [
                        (s.x + 1) as f32,
                        (s.y + 1 + (sel - first) as i32 * self.element_height) as f32,
                        (s.bar_x_shift - 2) as f32,
                        self.element_height as f32,
                    ],
                    [4.0, 2.0, 2.0, 2.0],
                    [ts, ts],
                    u32::MAX,
                );
            }
        }
        /// Where row `i` of those shown is: its top left.
        pub fn row_at(&self, i: usize) -> (f32, f32) {
            (
                (self.scroll.x + 1) as f32,
                (self.scroll.y + 1 + i as i32 * self.element_height) as f32,
            )
        }
        /// `GuiTextScrollList.renderForeground`'s rows: `(index, x, y)` of each row shown, for the screen to draw.
        pub fn shown(&self, rows: usize) -> Vec<(usize, f32, f32)> {
            let first = self.first(rows);
            (0..self.focused() as usize)
                .map(|i| first + i)
                .take_while(|&i| i < rows)
                .enumerate()
                .map(|(n, i)| {
                    let (x, y) = self.row_at(n);
                    (i, x, y)
                })
                .collect()
        }
        /// `onClick`: the thumb, or a row (`Some(Some(row))`), or the empty part of the list, which clears the selection
        /// (`Some(None)`); None when the press is elsewhere.
        pub fn press(&mut self, mx: f64, my: f64, rows: usize) -> Option<Option<usize>> {
            let focused = self.focused();
            if self.scroll.press(mx, my, rows as i32, focused) {
                return Some(self.selected);
            }
            let s = &self.scroll;
            if mx >= f64::from(s.x + 1)
                && mx < f64::from(s.x + s.bar_x_shift - 1)
                && my >= f64::from(s.y + 1)
                && my < f64::from(s.y + s.height - 1)
            {
                let first = self.first(rows);
                for i in 0..focused as usize {
                    if first + i >= rows {
                        break;
                    }
                    let top = f64::from(s.y + 1 + self.element_height * i as i32);
                    if my >= top && my <= top + f64::from(self.element_height) {
                        self.selected = Some(first + i);
                        return Some(self.selected);
                    }
                }
                self.selected = None;
                return Some(None);
            }
            None
        }
        pub fn drag(&mut self, my: f64, rows: usize) {
            let focused = self.focused();
            self.scroll.drag(my, rows as i32, focused);
        }
        pub fn release(&mut self) {
            self.scroll.release();
        }
        /// `mouseScrolled`: over the list, a wheel step.
        pub fn wheel(&mut self, mx: f64, my: f64, delta: f64, rows: usize) -> bool {
            let focused = self.focused();
            self.scroll.is_over(mx, my) && self.scroll.wheel(delta, rows as i32, focused, 1)
        }
        /// `GuiTextScrollList.setText`: the selection past the new rows goes, and the list goes to the top when it no
        /// longer needs a bar.
        pub fn set_rows(&mut self, rows: usize) {
            if self.selected.is_some_and(|s| s >= rows) {
                self.selected = None;
            }
            if !Scrollable::needs_bar(rows as i32, self.focused()) {
                self.scroll.scroll = 0.0;
            }
        }
    }

    /// `GuiScrollBar`: a stand-alone bar, 14 wide, for a list the screen lays out itself (the QIO grid).
    #[derive(Clone, Debug, PartialEq)]
    pub struct ScrollBar {
        pub scroll: Scrollable,
        /// `scroll_bar.png` (24 by 15: the thumb, then the greyed thumb).
        pub texture: &'static str,
        /// `GuiElementHolder.HOLDER` (32-pixel sides, 256 by 256).
        pub holder: &'static str,
    }
    impl ScrollBar {
        pub const TEXTURE_WIDTH: i32 = 24;
        pub const TEXTURE_HEIGHT: i32 = 15;
        pub fn new(x: i32, y: i32, height: i32) -> Self {
            ScrollBar {
                scroll: Scrollable::new(
                    x,
                    y,
                    14,
                    height,
                    1,
                    1,
                    Self::TEXTURE_WIDTH / 2,
                    Self::TEXTURE_HEIGHT,
                    height - 2,
                ),
                texture: "mekanism:gui/scroll_bar",
                holder: "mekanism:gui/element_holder",
            }
        }
        /// `drawBackground`: the holder and the thumb, greyed when there is nothing to scroll.
        pub fn draw(&self, c: &mut Canvas, max_elements: i32, focused: i32) {
            let s = &self.scroll;
            background(
                c,
                self.holder,
                32.0,
                [
                    s.x as f32,
                    s.y as f32,
                    (s.bar_width + 2) as f32,
                    s.height as f32,
                ],
                [256.0, 256.0],
            );
            let u = if Scrollable::needs_bar(max_elements, focused) {
                0.0
            } else {
                s.bar_width as f32
            };
            c.blit(
                self.texture,
                s.bar_x as f32,
                (s.bar_y + s.thumb()) as f32,
                u,
                0.0,
                s.bar_width as f32,
                s.bar_height as f32,
                Self::TEXTURE_WIDTH as f32,
                Self::TEXTURE_HEIGHT as f32,
            );
        }
        pub fn first(&self, max_elements: i32, focused: i32) -> i32 {
            self.scroll.first(max_elements, focused)
        }
        pub fn press(&mut self, mx: f64, my: f64, max_elements: i32, focused: i32) -> bool {
            self.scroll.press(mx, my, max_elements, focused)
        }
        pub fn drag(&mut self, my: f64, max_elements: i32, focused: i32) {
            self.scroll.drag(my, max_elements, focused);
        }
        pub fn release(&mut self) {
            self.scroll.release();
        }
        /// A wheel step anywhere the screen routes it (the QIO grid scrolls by rows: `scaler` 1).
        pub fn wheel(&mut self, delta: f64, max_elements: i32, focused: i32) -> bool {
            self.scroll.wheel(delta, max_elements, focused, 1)
        }
    }

    /// What a press did to a [`Window`].
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum WindowPress {
        /// Its close button: the window closed.
        Closed,
        /// Its title band (18 pixels): dragging started.
        Dragging,
        /// Somewhere else on it: the press is the window's (its own widgets may take it).
        Inside,
        /// Outside it, over the player's inventory: the game's slots take it (`InteractionStrategy.CONTAINER`).
        Inventory,
        /// Outside it and not over the inventory: taken by the window, nothing behind gets it.
        Blocked,
    }

    /// `GuiWindow`: a panel over the screen with Mekanism's shadow, a close button at 6, 6 and a title band that drags
    /// it. While it is open, the screen draws it after [`Canvas::slots`] and hides the slots under it
    /// ([`Window::covers`]).
    #[derive(Clone, Debug, PartialEq)]
    pub struct Window {
        pub x: i32,
        pub y: i32,
        pub width: i32,
        pub height: i32,
        pub title: String,
        /// `GuiMekanism.BASE_BACKGROUND` (4-pixel sides, 256 by 256).
        pub background: &'static str,
        /// `GuiMekanism.SHADOW`.
        pub shadow: &'static str,
        /// `GuiCloseButton`'s `button/close.png`, drawn 8 by 12.
        pub close_texture: &'static str,
        drag_from: Option<(f64, f64)>,
        prev: (i32, i32),
    }
    impl Window {
        pub fn new(x: i32, y: i32, width: i32, height: i32, title: &str) -> Self {
            Window {
                x,
                y,
                width,
                height,
                title: title.to_owned(),
                background: "mekanism:gui/base",
                shadow: "mekanism:gui/shadow",
                close_texture: "mekanism:gui/button/close",
                drag_from: None,
                prev: (0, 0),
            }
        }
        /// `calculateOpenPosition`: a window reopened where it was (`last`), kept on the screen (`bounds`: the GUI from
        /// the panel's top left, `[-left, -top, gui width, gui height]`).
        pub fn place(&mut self, last: Option<(i32, i32)>, bounds: Rect) {
            if let Some((x, y)) = last {
                let (min_x, min_y) = (bounds[0] as i32, bounds[1] as i32);
                let (max_x, max_y) = (
                    (bounds[0] + bounds[2]) as i32 - self.width,
                    (bounds[1] + bounds[3]) as i32 - self.height,
                );
                self.x = x.max(min_x).min(max_x.max(min_x));
                self.y = y.max(min_y).min(max_y.max(min_y));
            }
        }
        pub fn rect(&self) -> Rect {
            [
                self.x as f32,
                self.y as f32,
                self.width as f32,
                self.height as f32,
            ]
        }
        pub fn is_over(&self, mx: f64, my: f64) -> bool {
            mx >= f64::from(self.x)
                && mx < f64::from(self.x + self.width)
                && my >= f64::from(self.y)
                && my < f64::from(self.y + self.height)
        }
        /// Whether a slot at `(x, y)` (16 by 16) is under the window: hide it ([`Canvas::slot`] with `active` false).
        pub fn covers(&self, x: f32, y: f32) -> bool {
            x + 16.0 > self.x as f32
                && x < (self.x + self.width) as f32
                && y + 16.0 > self.y as f32
                && y < (self.y + self.height) as f32
        }
        /// `renderBackgroundOverlay`: the shadow at three quarters, then the panel. Then the screen draws the title
        /// ([`Window::draw_title`]) and the window's own widgets.
        pub fn draw_background(&self, c: &mut Canvas) {
            let r = self.rect();
            c.blit_nine_sliced(
                self.shadow,
                [r[0] - 3.0, r[1] - 3.0, r[2] + 6.0, r[3] + 6.0],
                [4.0; 4],
                [0.0, 0.0, 256.0, 256.0],
                [256.0, 256.0],
                0xBFFF_FFFF,
            );
            background(c, self.background, 4.0, r, [256.0, 256.0]);
            c.blit(
                self.close_texture,
                r[0] + 6.0,
                r[1] + 6.0,
                0.0,
                0.0,
                8.0,
                12.0,
                8.0,
                12.0,
            );
            c.area(
                -1,
                [r[0] + 6.0, r[1] + 6.0, 8.0, 12.0],
                AreaKind::Button,
                true,
                "Close",
            );
        }
        /// `drawTitleTextTextWithOffset(text, 12, 6, width)`: the title centred between the close button and the edge.
        pub fn draw_title(&self, c: &mut Canvas, width_of: impl Fn(&str) -> f32, argb: u32) {
            let start = 12.0;
            let room = self.width as f32 - start;
            let w = width_of(&self.title);
            let x = self.x as f32 + start + ((room - w) / 2.0).max(0.0);
            c.draw_string(&self.title, x.floor(), (self.y + 6) as f32, argb, false);
        }
        /// `mouseClicked`: the close button, the title band, the window itself, or outside it.
        pub fn press(
            &mut self,
            mx: f64,
            my: f64,
            inventory_top: Option<f64>,
            panel_width: f64,
        ) -> WindowPress {
            let (x, y) = (f64::from(self.x), f64::from(self.y));
            if mx >= x + 6.0 && mx < x + 14.0 && my >= y + 6.0 && my < y + 18.0 {
                return WindowPress::Closed;
            }
            if self.is_over(mx, my) {
                if my < y + 18.0 {
                    self.drag_from = Some((mx, my));
                    self.prev = (0, 0);
                    return WindowPress::Dragging;
                }
                return WindowPress::Inside;
            }
            match inventory_top {
                Some(top) if mx >= 0.0 && mx < panel_width && my >= top => WindowPress::Inventory,
                _ => WindowPress::Blocked,
            }
        }
        /// `onDrag`: follows the cursor, kept within `bounds` (as [`Window::place`]).
        pub fn drag(&mut self, mx: f64, my: f64, bounds: Rect) {
            let Some((fx, fy)) = self.drag_from else {
                return;
            };
            let (ndx, ndy) = ((mx - fx).round() as i32, (my - fy).round() as i32);
            let (min_x, min_y) = (bounds[0] as i32, bounds[1] as i32);
            let (max_x, max_y) = (
                (bounds[0] + bounds[2]) as i32,
                (bounds[1] + bounds[3]) as i32,
            );
            let cx = clamp(
                ndx - self.prev.0,
                min_x - self.x,
                max_x - (self.x + self.width),
            );
            let cy = clamp(
                ndy - self.prev.1,
                min_y - self.y,
                max_y - (self.y + self.height),
            );
            self.prev = (ndx, ndy);
            self.x += cx;
            self.y += cy;
        }
        pub fn release(&mut self) {
            self.drag_from = None;
        }
        pub fn dragging(&self) -> bool {
            self.drag_from.is_some()
        }
    }

    /// A tab at the panel's side that opens a [`Window`] (`GuiInsetElement` tabs: the QIO frequency tab at -26, 20), and
    /// is inactive while its window is open (`setListenerTab`).
    #[derive(Clone, Debug, PartialEq)]
    pub struct SideTab {
        pub id: i32,
        pub x: i32,
        pub y: i32,
        pub width: i32,
        pub height: i32,
        /// The tab's background (`holder_left.png` / `holder_right.png`, 26 by 18 in Mekanism's insets).
        pub holder: &'static str,
        /// The icon drawn in the middle.
        pub icon: &'static str,
        pub icon_size: [f32; 2],
        pub label: String,
    }
    impl SideTab {
        pub fn new(id: i32, x: i32, y: i32, icon: &'static str, label: &str) -> Self {
            SideTab {
                id,
                x,
                y,
                width: 26,
                height: 18,
                holder: "mekanism:gui/holder_left",
                icon,
                icon_size: [18.0, 18.0],
                label: label.to_owned(),
            }
        }
        pub fn rect(&self) -> Rect {
            [
                self.x as f32,
                self.y as f32,
                self.width as f32,
                self.height as f32,
            ]
        }
        /// The holder, the icon centred, and its area (inactive while `open`).
        pub fn draw(&self, c: &mut Canvas, open: bool) {
            let r = self.rect();
            c.blit(self.holder, r[0], r[1], 0.0, 0.0, r[2], r[3], r[2], r[3]);
            let [iw, ih] = self.icon_size;
            let tint = if open { 0xFF80_8080 } else { u32::MAX };
            c.blit_region(
                self.icon,
                [
                    r[0] + ((r[2] - iw) / 2.0).floor(),
                    r[1] + ((r[3] - ih) / 2.0).floor(),
                    iw,
                    ih,
                ],
                [0.0, 0.0, iw, ih],
                [iw, ih],
                tint,
            );
            c.area(self.id, r, AreaKind::Tab, !open, &self.label);
        }
        pub fn is_over(&self, mx: f64, my: f64) -> bool {
            mx >= f64::from(self.x)
                && mx < f64::from(self.x + self.width)
                && my >= f64::from(self.y)
                && my < f64::from(self.y + self.height)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_scroll_list_shows_and_picks_rows_as_gui_scroll_list() {
            // A 162 by 42 list of 10-pixel rows (the frequency list): 4 rows shown.
            let mut list = ScrollList::new(7, 29, 162, 42, 10);
            assert_eq!(list.focused(), 4);
            assert_eq!(list.first(3), 0);
            assert_eq!(list.shown(3).len(), 3);
            // Row 2 of 3: y from 29 + 1 + 20 to 29 + 1 + 30.
            assert_eq!(list.press(20.0, 52.0, 3), Some(Some(2)));
            // Below the last row, inside the list: the selection clears.
            assert_eq!(list.press(20.0, 65.0, 3), Some(None));
            assert_eq!(list.press(300.0, 65.0, 3), None);
            // Ten rows: the wheel moves one of the six hidden at a time.
            assert!(list.wheel(10.0, 40.0, -1.0, 10));
            assert!((list.scroll.scroll - 1.0 / 6.0).abs() < 1e-9);
            assert_eq!(list.first(10), 1);
            // Fewer rows than fit: back to the top.
            list.set_rows(2);
            assert_eq!(list.scroll.scroll, 0.0);
        }

        #[test]
        fn the_thumb_drags_with_the_cursor_and_stops_at_the_ends() {
            let mut bar = ScrollBar::new(156, 18, 74);
            // 100 rows of 8 shown: the thumb at the top (bar_y 19, 15 high) takes the press.
            assert!(bar.press(160.0, 25.0, 100, 8));
            bar.drag(200.0, 100, 8);
            assert_eq!(bar.scroll.scroll, 1.0);
            assert_eq!(bar.first(100, 8), 92);
            bar.drag(0.0, 100, 8);
            assert_eq!(bar.scroll.scroll, 0.0);
            bar.release();
            assert!(!bar.scroll.dragging);
        }

        #[test]
        fn a_window_drags_by_its_title_and_stays_on_screen() {
            let mut w = Window::new(10, 20, 100, 60, "Set Frequency");
            let bounds = [-50.0, -30.0, 400.0, 240.0];
            assert_eq!(
                w.press(50.0, 25.0, Some(84.0), 176.0),
                WindowPress::Dragging
            );
            w.drag(80.0, 35.0, bounds);
            assert_eq!((w.x, w.y), (40, 30));
            // Far to the left: it stops at the GUI's edge.
            w.drag(-500.0, 35.0, bounds);
            assert_eq!(w.x, -50);
            w.release();
            assert_eq!(
                w.press(f64::from(w.x) + 8.0, f64::from(w.y) + 8.0, None, 176.0),
                WindowPress::Closed
            );
            assert_eq!(
                w.press(150.0, 100.0, Some(84.0), 176.0),
                WindowPress::Inventory
            );
            assert_eq!(
                w.press(150.0, 50.0, Some(84.0), 176.0),
                WindowPress::Blocked
            );
            assert!(w.covers(w.x as f32, w.y as f32));
        }
    }
}
