//! The screen of Screen Demo's `screendemo:panel`, drawn by code (capability `screen`, API 56): what Mekanism's
//! `GuiFrequencySelector` and `GuiQIODashboard` are made of. The header (string 0) and the frequency list (strings 1
//! on, a `ScrollList`), three 38 by 20 image buttons with their icon centred (menu buttons 10 to 12, a tooltip each),
//! a text field the game owns (field 1: Enter sends the name), a Delete button (menu button 2, active with a
//! selection), an item grid (`menu-grid`, two columns with a `ScrollBar`), and a side tab at -26, 20 that opens a
//! `Window` you drag by its title; Esc closes the window before the menu.
use horizon_mod_sdk::screen::{
    AreaKind, Canvas, FieldFilter, Frame, InputEvent, MouseButton, OpenInfo, ScreenMod, Setup,
    TEXT_SHADOW, host,
    widgets::{ScrollBar, ScrollList, SideTab, Window, WindowPress, background},
};

const BASE: &str = "screendemo:gui/base";
const BUTTON: &str = "screendemo:gui/button";
const MODES: [(&str, &str); 3] = [
    ("screendemo:gui/public", "Public"),
    ("screendemo:gui/trusted", "Trusted"),
    ("screendemo:gui/private", "Private"),
];
const GRID: (i32, i32, i32, i32) = (124, 54, 2, 2);
const TAB: i32 = 9100;
const FIELD: i32 = 1;
const DELETE: i32 = 2;
const WIDTH: f32 = 176.0;

struct Panel {
    list: ScrollList,
    bar: ScrollBar,
    tab: SideTab,
    window: Option<Window>,
    /// Where the window was when it closed (`SelectedWindowData.updateLastPosition`).
    last_window: Option<(i32, i32)>,
    gui: [f32; 2],
    field: String,
}

impl Panel {
    fn rows() -> Vec<String> {
        host::menu_strings().into_iter().skip(1).collect()
    }
    fn bounds(&self) -> [f32; 4] {
        let (left, top) = (
            ((self.gui[0] - WIDTH) / 2.0).floor(),
            ((self.gui[1] - 200.0) / 2.0).floor(),
        );
        [-left, -top, self.gui[0], self.gui[1]]
    }
    fn image_button(c: &mut Canvas, i: usize, mouse: (f32, f32)) {
        let (x, y) = (8.0 + 42.0 * i as f32, 30.0);
        background(c, BUTTON, 4.0, [x, y, 38.0, 20.0], [256.0, 256.0]);
        // MekanismImageButton: the icon in the middle of the button.
        let (icon, label) = MODES[i];
        c.blit(icon, x + 10.0, y + 1.0, 0.0, 0.0, 18.0, 18.0, 18.0, 18.0);
        c.area(
            10 + i as i32,
            [x, y, 38.0, 20.0],
            AreaKind::Button,
            true,
            label,
        );
        if mouse.0 >= x && mouse.0 < x + 38.0 && mouse.1 >= y && mouse.1 < y + 20.0 {
            c.tooltip(&[(label, 0xFFFF_FFFF)]);
        }
    }
}

impl ScreenMod for Panel {
    fn open(info: &OpenInfo) -> Result<(Self, Setup), String> {
        if info.menu != "screendemo:panel" {
            return Err(format!("no screen for {}", info.menu));
        }
        let mut list = ScrollList::new(8, 54, 112, 42, 10);
        list.texture = "screendemo:gui/scroll_list";
        list.background = "screendemo:gui/inner_screen";
        let mut bar = ScrollBar::new(156, 54, 40);
        bar.texture = "screendemo:gui/scroll_bar";
        bar.holder = "screendemo:gui/element_holder";
        let mut tab = SideTab::new(TAB, -26, 20, "screendemo:gui/frequency", "Set Frequency");
        tab.holder = "screendemo:gui/holder_left";
        let panel = Panel {
            list,
            bar,
            tab,
            window: None,
            last_window: None,
            gui: [info.gui_width, info.gui_height],
            field: String::new(),
        };
        Ok((
            panel,
            Setup {
                width: 0,
                height: 0,
                hide_inventory: false,
                own_labels: true,
            },
        ))
    }

    fn render(&mut self, frame: &Frame, c: &mut Canvas) {
        let mouse = (frame.mouse_x, frame.mouse_y);
        let strings = host::menu_strings();
        let rows = Self::rows();
        self.list.set_rows(rows.len());
        let data = host::menu_data();
        background(c, BASE, 4.0, [0.0, 0.0, WIDTH, 200.0], [256.0, 256.0]);
        let title = "Screen Demo";
        c.draw_string(
            title,
            ((WIDTH - host::text_width(title)) / 2.0).floor(),
            6.0,
            0xFF40_4040,
            false,
        );
        c.text(
            strings.first().map_or("", String::as_str),
            8.0,
            18.0,
            0xFF3C_FE9A,
            TEXT_SHADOW,
            1.0,
        );
        for i in 0..3 {
            Self::image_button(c, i, mouse);
        }
        // The frequency list.
        self.list.selected = data.get(2).copied().filter(|s| *s >= 0).map(|s| s as usize);
        self.list.draw_background(c);
        self.list.draw_selection(c, rows.len());
        for (i, x, y) in self.list.shown(rows.len()) {
            c.text(&rows[i], x + 1.0, y + 1.0, 0xFF3C_FE9A, 0, 0.8);
            c.area(
                100 + i as i32,
                [x, y, 104.0, 10.0],
                AreaKind::Row,
                true,
                &rows[i],
            );
        }
        // The name box and Delete.
        c.field(
            FIELD,
            [8.0, 100.0, 100.0, 12.0],
            16,
            FieldFilter::Any,
            &self.field,
            0,
            0,
        );
        let selected = self.list.selected.is_some();
        background(c, BUTTON, 4.0, [112.0, 98.0, 56.0, 16.0], [256.0, 256.0]);
        c.text(
            "Delete",
            140.0,
            102.0,
            if selected { 0xFFFF_5555 } else { 0xFF80_8080 },
            horizon_mod_sdk::screen::TEXT_CENTER,
            1.0,
        );
        c.area(
            DELETE,
            [112.0, 98.0, 56.0, 16.0],
            AreaKind::Button,
            selected,
            "Delete",
        );
        // The item grid, two columns, with its bar.
        let grid = host::menu_grid();
        let (gx, gy, cols, shown_rows) = GRID;
        let lines = (grid.len() as i32 + cols - 1) / cols;
        let first = self.bar.first(lines, shown_rows);
        for r in 0..shown_rows {
            for col in 0..cols {
                let index = ((first + r) * cols + col) as usize;
                let (x, y) = ((gx + col * 18) as f32, (gy + r * 18) as f32);
                c.fill(x, y, 18.0, 18.0, 0xFF37_3737);
                c.fill(x + 1.0, y + 1.0, 16.0, 16.0, 0xFF8B_8B8B);
                if let Some(e) = grid.get(index) {
                    c.render_item(
                        &e.item,
                        &e.components,
                        e.count.min(u64::from(u32::MAX)) as u32,
                        x + 1.0,
                        y + 1.0,
                        true,
                    );
                }
            }
        }
        self.bar.draw(c, lines, shown_rows);
        // The game draws the inventory's items here: the window comes over them.
        c.slots();
        self.tab.draw(c, self.window.is_some());
        if let Some(w) = &self.window {
            w.draw_background(c);
            w.draw_title(c, host::text_width, 0xFF40_4040);
            let header = strings.first().map_or("", String::as_str);
            c.draw_string(
                header,
                (w.x + 8) as f32,
                (w.y + 24) as f32,
                0xFF40_4040,
                false,
            );
        }
    }

    fn input(&mut self, event: &InputEvent) -> bool {
        let rows = Self::rows().len();
        let grid = host::menu_grid().len() as i32;
        let lines = (grid + GRID.2 - 1) / GRID.2;
        match event {
            InputEvent::MousePressed(p) if p.button == MouseButton::Left => {
                let (mx, my) = (f64::from(p.x), f64::from(p.y));
                if let Some(w) = &mut self.window {
                    match w.press(mx, my, Some(118.0), f64::from(WIDTH)) {
                        WindowPress::Closed => {
                            self.last_window = Some((w.x, w.y));
                            self.window = None;
                            host::play_click();
                            return true;
                        }
                        WindowPress::Dragging | WindowPress::Inside | WindowPress::Blocked => {
                            return true;
                        }
                        WindowPress::Inventory => return false,
                    }
                }
                if self.tab.is_over(mx, my) {
                    let mut w = Window::new(20, 20, 136, 80, "Set Frequency");
                    w.background = BASE;
                    w.shadow = "screendemo:gui/shadow";
                    w.close_texture = "screendemo:gui/close";
                    w.place(self.last_window, self.bounds());
                    self.window = Some(w);
                    host::play_click();
                    return true;
                }
                for i in 0..3 {
                    let x = 8.0 + 42.0 * i as f32;
                    if p.x >= x && p.x < x + 38.0 && p.y >= 30.0 && p.y < 50.0 {
                        host::send_button(10 + i);
                        host::play_click();
                        return true;
                    }
                }
                if p.x >= 112.0 && p.x < 168.0 && p.y >= 98.0 && p.y < 114.0 {
                    if self.list.selected.is_some() {
                        host::send_button(DELETE);
                        host::play_click();
                    }
                    return true;
                }
                if self.bar.press(mx, my, lines, GRID.3) {
                    return true;
                }
                let (gx, gy, cols, shown) = GRID;
                if p.x >= gx as f32
                    && p.x < (gx + cols * 18) as f32
                    && p.y >= gy as f32
                    && p.y < (gy + shown * 18) as f32
                {
                    let first = self.bar.first(lines, shown);
                    let index = (first + ((p.y as i32 - gy) / 18)) * cols + (p.x as i32 - gx) / 18;
                    let index = if index < grid { index as u32 } else { u32::MAX };
                    host::send_grid_click(index, 0, p.modifiers.shift);
                    return true;
                }
                match self.list.press(mx, my, rows) {
                    Some(Some(row)) => {
                        host::send_button(100 + row as i32);
                        true
                    }
                    Some(None) => true,
                    None => false,
                }
            }
            InputEvent::MouseDragged(d) => {
                let bounds = self.bounds();
                if let Some(w) = &mut self.window
                    && w.dragging()
                {
                    w.drag(f64::from(d.x), f64::from(d.y), bounds);
                    return true;
                }
                self.bar.drag(f64::from(d.y), lines, GRID.3);
                self.list.drag(f64::from(d.y), rows);
                true
            }
            InputEvent::MouseReleased(_) => {
                if let Some(w) = &mut self.window {
                    w.release();
                }
                self.bar.release();
                self.list.release();
                false
            }
            InputEvent::MouseScrolled(s) => {
                let (mx, my) = (f64::from(s.x), f64::from(s.y));
                self.list.wheel(mx, my, f64::from(s.delta), rows)
                    || self.bar.wheel(f64::from(s.delta), lines, GRID.3)
            }
            // GuiWindow.keyPressed: Esc closes the window, not the menu.
            InputEvent::KeyPressed(k)
                if k.key == "key.keyboard.escape" && self.window.is_some() =>
            {
                self.last_window = self.window.take().map(|w| (w.x, w.y));
                true
            }
            InputEvent::FieldChanged(f) if f.id == FIELD => {
                self.field.clone_from(&f.text);
                true
            }
            InputEvent::FieldSubmitted(f) if f.id == FIELD => {
                host::send_text(FIELD, &f.text);
                self.field.clear();
                true
            }
            _ => false,
        }
    }
}

horizon_mod_sdk::export_screen_mod!(Panel);
