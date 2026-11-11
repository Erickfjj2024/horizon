//! What a declared menu screen computes without drawing (API block 7): the
//! number a widget reads, how much of a bar or arrow is filled, which button
//! a press hits and a tooltip's or a label's text. The game draws with these.
use crate::content::{Direction, LocalAction, MenuScreen, Value, When, Widget, Window};
use std::collections::BTreeMap;

/// What a widget reads from the menu's data slots `data`: a constant, a data
/// slot (16 bits on the wire) or two read as low and high halves.
pub fn read(value: &Value, data: &[i32]) -> i64 {
    let slot = |i: u16| i64::from(data.get(usize::from(i)).copied().unwrap_or(0)) & 0xFFFF;
    match value {
        Value::Const(c) => *c,
        Value::Data(i) => slot(*i),
        Value::Pair([lo, hi]) => (slot(*hi) << 16) | slot(*lo),
        Value::Nibble([i, which]) => (slot(*i) >> (4 * u32::from(*which & 3))) & 0xF,
    }
}

/// What a screen keeps on this client alone: the window open and its variables (a selected tab).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Locals {
    pub window: Option<String>,
    pub vars: BTreeMap<String, i64>,
    /// How far the open window was dragged from where the screen puts it.
    pub offset: [f32; 2],
}

/// The variable a list that starts at string `first` keeps its scroll in.
pub fn scroll_var(first: u16) -> String {
    format!("@scroll:{first}")
}

/// How many rows a list shows at once.
pub fn list_rows(h: i32, row_height: i32) -> i64 {
    i64::from((h / row_height.max(1)).max(1))
}

impl Locals {
    /// A list's first row showing: `count` rows of which `rows` show, so 0 up to `count - rows`.
    pub fn scroll(&self, first: u16, count: i64, rows: i64) -> i64 {
        self.var(&scroll_var(first)).clamp(0, (count - rows).max(0))
    }
    /// A variable, 0 when it was never set.
    pub fn var(&self, name: &str) -> i64 {
        self.vars.get(name).copied().unwrap_or(0)
    }
}

/// Whether a condition holds now.
pub fn shown(when: &When, data: &[i32], locals: &Locals) -> bool {
    match when {
        When::Local { var, eq } => locals.var(var) == *eq,
        When::Data { value, eq } => read(value, data) == *eq,
    }
}

/// The widgets showing now, in drawing order, with the conditional groups opened.
pub fn showing<'a>(widgets: &'a [Widget], data: &[i32], locals: &Locals) -> Vec<&'a Widget> {
    let mut out = Vec::new();
    for w in widgets {
        match w {
            Widget::Show { when, widgets } => {
                if shown(when, data, locals) {
                    out.extend(showing(widgets, data, locals));
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// The window open now, if the screen has it.
pub fn open_window<'a>(screen: &'a MenuScreen, locals: &Locals) -> Option<&'a Window> {
    let id = locals.window.as_deref()?;
    screen.windows.iter().find(|w| w.id == id)
}

/// Where the open window's top left is: its place on the screen and how far it was dragged.
pub fn window_origin(window: &Window, locals: &Locals) -> [f32; 2] {
    [window.x as f32 + locals.offset[0], window.y as f32 + locals.offset[1]]
}

/// The step `[dx, dy, dz]` from a machine facing `facing` to the block beside its relative `side` (0 front, 1 left, 2 right,
/// 3 back, 4 top, 5 bottom; the same mapping as Mekanism's `RelativeSide.getDirection`: the left is clockwise of the front
/// seen from above, the right counterclockwise).
pub fn neighbor_step(facing: &str, side: u8) -> [i32; 3] {
    // North, east, south, west in clockwise order, as the steps along x and z.
    const AROUND: [(&str, [i32; 3]); 4] =
        [("north", [0, 0, -1]), ("east", [1, 0, 0]), ("south", [0, 0, 1]), ("west", [-1, 0, 0])];
    let front = AROUND.iter().position(|(n, _)| *n == facing).unwrap_or(0);
    let turn = |by: usize| AROUND[(front + by) % 4].1;
    match side {
        0 => turn(0),
        1 => turn(1),
        2 => turn(3),
        3 => turn(2),
        4 => [0, 1, 0],
        _ => [0, -1, 0],
    }
}

/// The height of a window's top edge that takes a drag (Mekanism's windows drag by their title).
pub const DRAG_HEIGHT: f32 = 18.0;

/// What a press did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// The text box `id` was pressed (FORK-20): it takes the focus.
    Field(i32),
    /// A row of the list that shows menu text from string `first` was pressed (FORK-20): menu button `button` goes to the server
    /// when the press ends without a drag.
    Row { button: i32, first: u16 },
    /// Menu button `id`, for the server.
    Send(i32),
    /// Something here: a local action, or a press a window took.
    Local,
}

fn inside(at: [f32; 2], x: i32, y: i32, w: i32, h: i32) -> bool {
    at[0] >= x as f32 && at[1] >= y as f32 && at[0] < (x + w) as f32 && at[1] < (y + h) as f32
}

/// The first button showing under `at` among `widgets` (at their origin) and what pressing it does.
fn press_in(
    widgets: &[Widget],
    data: &[i32],
    locals: &mut Locals,
    at: [f32; 2],
    alt: bool,
    shift: bool,
) -> Option<Press> {
    if let Some(row) = showing(widgets, data, locals).into_iter().find_map(|w| match w {
        Widget::List { x, y, w, h, row_height, first, count, on_click, .. } if inside(at, *x, *y, *w, *h) => {
            let rows = list_rows(*h, *row_height);
            let row = ((at[1] - *y as f32) / *row_height as f32) as i64;
            let index = locals.scroll(*first, read(count, data), rows) + row;
            (row < rows && index < read(count, data)).then(|| Press::Row { button: *on_click + index as i32, first: *first })
        }
        _ => None,
    }) {
        return Some(row);
    }
    if let Some(id) = showing(widgets, data, locals).into_iter().find_map(|w| match w {
        Widget::TextField { x, y, w, h, id, .. } if inside(at, *x, *y, *w, *h) => Some(*id),
        _ => None,
    }) {
        return Some(Press::Field(id));
    }
    let hit = showing(widgets, data, locals).into_iter().find_map(|w| match w {
        Widget::Button { x, y, w, h, id, alt_id, shift_id, local, .. } if inside(at, *x, *y, *w, *h) => {
            Some((*id, *alt_id, *shift_id, local.clone()))
        }
        _ => None,
    })?;
    let (id, alt_id, shift_id, local) = hit;
    if let Some(action) = local {
        match action {
            LocalAction::Open(window) => {
                // The button of the window open closes it, as a tab does.
                locals.window = if locals.window.as_deref() == Some(window.as_str()) { None } else { Some(window) };
            }
            LocalAction::Close => locals.window = None,
            LocalAction::Set { var, value } => {
                locals.vars.insert(var, value);
            }
        }
        return Some(Press::Local);
    }
    if let (true, Some(shifted)) = (shift, shift_id) {
        return Some(Press::Send(shifted));
    }
    if alt {
        return alt_id.map(Press::Send);
    }
    Some(Press::Send(id))
}

/// What a press at `at` (from the screen's top left) does: the open window takes what lands on it (a press on none of its
/// buttons is its own), anything else goes to the screen's buttons. `alt` is a right press or a long one.
pub fn press(
    screen: &MenuScreen,
    data: &[i32],
    locals: &mut Locals,
    at: [f32; 2],
    alt: bool,
) -> Option<Press> {
    press_with(screen, data, locals, at, alt, false)
}

/// `press` with the Shift key's state: a button with a `shift_id` sends it while Shift is held.
pub fn press_with(
    screen: &MenuScreen,
    data: &[i32],
    locals: &mut Locals,
    at: [f32; 2],
    alt: bool,
    shift: bool,
) -> Option<Press> {
    if let Some(window) = open_window(screen, locals).cloned() {
        // A window's own widgets are hit wherever they are, outside it too (its tabs sit at its side); a press that
        // lands on the window and on none of them is the window's.
        let origin = window_origin(&window, locals);
        let rel = [at[0] - origin[0], at[1] - origin[1]];
        if let Some(pressed) = press_in(&window.widgets, data, locals, rel, alt, shift) {
            return Some(pressed);
        }
        if inside(at, origin[0] as i32, origin[1] as i32, window.width, window.height) {
            return Some(Press::Local);
        }
    }
    press_in(&screen.widgets, data, locals, at, alt, shift)
}

/// The wheel over a list: `notches` (positive up) scroll the first list showing under `at` (the open window's, else the screen's)
/// by that many rows. True when a list took it.
pub fn scroll_lists(screen: &MenuScreen, data: &[i32], locals: &mut Locals, at: [f32; 2], notches: i64) -> bool {
    let window = open_window(screen, locals).cloned();
    let (widgets, rel): (&[Widget], [f32; 2]) = match &window {
        Some(w) => {
            let o = window_origin(w, locals);
            (&w.widgets, [at[0] - o[0], at[1] - o[1]])
        }
        None => (&screen.widgets, at),
    };
    let hit = showing(widgets, data, locals).into_iter().find_map(|w| match w {
        Widget::List { x, y, w, h, row_height, first, count, .. } if inside(rel, *x, *y, *w, *h) => {
            Some((*first, read(count, data), list_rows(*h, *row_height)))
        }
        _ => None,
    });
    let Some((first, count, rows)) = hit else {
        return false;
    };
    let now = locals.scroll(first, count, rows);
    let next = (now - notches).clamp(0, (count - rows).max(0));
    locals.vars.insert(scroll_var(first), next);
    true
}

/// `value` of `max` as 0 to 1; 0 when `max` is not over 0.
pub fn fraction(value: i64, max: i64) -> f32 {
    if max <= 0 {
        return 0.0;
    }
    (value.clamp(0, max) as f64 / max as f64) as f32
}

/// The part of `[x, y, w, h]` a fill of `frac` covers when it grows from the
/// side `direction` says (`up` grows from the bottom).
pub fn filled(rect: [f32; 4], frac: f32, direction: Direction) -> [f32; 4] {
    let [x, y, w, h] = rect;
    match direction {
        Direction::Up => [x, y + h - h * frac, w, h * frac],
        Direction::Down => [x, y, w, h * frac],
        Direction::Right => [x, y, w * frac, h],
        Direction::Left => [x + w - w * frac, y, w * frac, h],
    }
}

/// A tooltip's lines with `{value}`, `{max}` and `{percent}` filled in.
pub fn tooltip_lines(text: &str, value: i64, max: i64) -> Vec<String> {
    let percent = (fraction(value, max) * 100.0).round() as i64;
    text.replace("{value}", &value.to_string())
        .replace("{max}", &max.to_string())
        .replace("{percent}", &percent.to_string())
        .split('\n')
        .map(str::to_owned)
        .collect()
}

/// `{0}`, `{1}`... in a text, filled from the data slots.
pub fn fill_text(text: &str, data: &[i32]) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').and_then(|close| {
            let inner = &after[..close];
            // `{3+4}`: a number of 32 bits kept in two data slots, the low 16 bits first.
            if let Some((low, high)) = inner.split_once('+') {
                let (low, high) = (low.parse::<u16>().ok()?, high.parse::<u16>().ok()?);
                return Some((close, read(&Value::Pair([low, high]), data)));
            }
            let n = inner.parse::<u16>().ok()?;
            Some((close, read(&Value::Data(n), data)))
        }) {
            Some((close, v)) => {
                out.push_str(&v.to_string());
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A widget's tooltip under `at` among `widgets` (at their origin).
fn hover_in(widgets: &[Widget], data: &[i32], locals: &Locals, at: [f32; 2]) -> Option<Vec<String>> {
    showing(widgets, data, locals).into_iter().rev().find_map(|w| match w {
        Widget::Bar { x, y, w, h, value, max, tooltip: Some(t), .. } if inside(at, *x, *y, *w, *h) => {
            Some(tooltip_lines(t, read(value, data), read(max, data)))
        }
        Widget::Sprite { x, y, w, h, value, max, tooltip: Some(t), .. } if inside(at, *x, *y, *w, *h) => {
            let v = value.as_ref().map_or(0, |v| read(v, data));
            let m = max.as_ref().map_or(0, |m| read(m, data));
            Some(tooltip_lines(t, v, m))
        }
        Widget::Button { x, y, w, h, tooltip: Some(t), .. } if inside(at, *x, *y, *w, *h) => {
            // `{0}`, `{1}`... are the screen's data slots, as in a text.
            Some(tooltip_lines(&fill_text(t, data), 0, 0))
        }
        Widget::Gauge { x, y, w, h, value, max, kind, types, tooltip, empty, .. } if inside(at, *x, *y, *w, *h) => {
            let k = read(kind, data).max(0) as usize;
            if k == 0 {
                return empty.as_ref().map(|e| tooltip_lines(e, 0, 0));
            }
            let label = types.get(k - 1).map_or("", |t| t.label.as_str());
            tooltip.as_ref().map(|t| tooltip_lines(&t.replace("{label}", label), read(value, data), read(max, data)))
        }
        Widget::Graph { x, y, w, h, tooltip: Some(t), .. } if inside(at, *x, *y, *w, *h) => Some(tooltip_lines(&fill_text(t, data), 0, 0)),
        Widget::Swatch { x, y, w, h, value, labels, tooltip: Some(t), .. } if inside(at, *x, *y, *w, *h) => {
            let v = read(value, data).max(0) as usize;
            let label = labels.get(v).or(labels.last()).map_or("", String::as_str);
            Some(tooltip_lines(&t.replace("{label}", label), v as i64, 0))
        }
        _ => None,
    })
}

/// The tooltip of what is under `at`: the open window's widgets first, then the screen's.
pub fn hovered(screen: &MenuScreen, data: &[i32], locals: &Locals, at: [f32; 2]) -> Option<Vec<String>> {
    if let Some(window) = open_window(screen, locals) {
        let origin = window_origin(window, locals);
        let rel = [at[0] - origin[0], at[1] - origin[1]];
        if let Some(lines) = hover_in(&window.widgets, data, locals, rel) {
            return Some(lines);
        }
    }
    hover_in(&screen.widgets, data, locals, at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_sends_the_shift_id() {
        let s: MenuScreen = serde_json::from_str(
            r##"{"width":100,"height":100,"slots":[],"widgets":[
                {"type":"button","x":0,"y":0,"w":10,"h":10,"id":1,"alt_id":2,"shift_id":3,"hover":"#40FFFFFF"}]}"##,
        )
        .unwrap();
        let mut l = Locals::default();
        assert_eq!(press_with(&s, &[], &mut l, [5.0, 5.0], false, true), Some(Press::Send(3)));
        assert_eq!(press_with(&s, &[], &mut l, [5.0, 5.0], false, false), Some(Press::Send(1)));
        assert_eq!(press_with(&s, &[], &mut l, [5.0, 5.0], true, false), Some(Press::Send(2)));
    }

    #[test]
    fn values_read_constants_slots_and_pairs() {
        let data = [7, -1, 0x1234, 0x0002];
        assert_eq!(read(&Value::Const(50), &data), 50);
        assert_eq!(read(&Value::Data(0), &data), 7);
        // A data slot is 16 bits on the wire: -1 is 65535.
        assert_eq!(read(&Value::Data(1), &data), 0xFFFF);
        assert_eq!(read(&Value::Pair([2, 3]), &data), 0x2_1234);
        assert_eq!(read(&Value::Data(9), &data), 0);
    }
    #[test]
    fn fractions_and_fills() {
        assert_eq!(fraction(50, 100), 0.5);
        assert_eq!(fraction(500, 100), 1.0);
        assert_eq!(fraction(-3, 100), 0.0);
        assert_eq!(fraction(5, 0), 0.0);
        let r = [10.0, 20.0, 6.0, 40.0];
        assert_eq!(filled(r, 0.5, Direction::Up), [10.0, 40.0, 6.0, 20.0]);
        assert_eq!(filled(r, 0.5, Direction::Down), [10.0, 20.0, 6.0, 20.0]);
        assert_eq!(filled(r, 0.5, Direction::Right), [10.0, 20.0, 3.0, 40.0]);
        assert_eq!(filled(r, 0.5, Direction::Left), [13.0, 20.0, 3.0, 40.0]);
    }
    #[test]
    fn text_and_tooltips_fill_in() {
        assert_eq!(fill_text("Progress {0}/{1}", &[3, 100]), "Progress 3/100");
        assert_eq!(fill_text("{x} {9}", &[1]), "{x} 0");
        assert_eq!(
            tooltip_lines("Energy: {value} / {max}\n{percent}%", 25, 100),
            ["Energy: 25 / 100", "25%"]
        );
    }
    fn screen(json: &str) -> MenuScreen {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_press_hits_the_first_button_under_it() {
        let s = screen(
            r#"{"slots":[],"widgets":[
                {"type":"button","x":10,"y":10,"w":20,"h":10,"id":1},
                {"type":"button","x":10,"y":10,"w":20,"h":10,"id":2},
                {"type":"button","x":40,"y":10,"w":20,"h":10,"id":3,"alt_id":4}]}"#,
        );
        let mut l = Locals::default();
        assert_eq!(press(&s, &[], &mut l, [15.0, 12.0], false), Some(Press::Send(1)));
        assert_eq!(press(&s, &[], &mut l, [40.0, 10.0], false), Some(Press::Send(3)));
        // A right press sends the button's alt id; a button with none does nothing for it.
        assert_eq!(press(&s, &[], &mut l, [40.0, 10.0], true), Some(Press::Send(4)));
        assert_eq!(press(&s, &[], &mut l, [15.0, 12.0], true), None);
        assert_eq!(press(&s, &[], &mut l, [30.0, 12.0], false), None);
        assert_eq!(press(&s, &[], &mut l, [15.0, 20.0], false), None);
    }

    const WINDOWED: &str = r#"{"slots":[],"widgets":[
        {"type":"button","x":0,"y":0,"w":20,"h":20,"local":{"open":"config"}},
        {"type":"button","x":30,"y":0,"w":20,"h":20,"id":9}],
      "windows":[{"id":"config","x":10,"y":30,"width":100,"height":60,"widgets":[
        {"type":"button","x":5,"y":5,"w":10,"h":10,"local":{"set":{"var":"tab","value":1}}},
        {"type":"show","when":{"local":{"var":"tab","eq":1}},"widgets":[
            {"type":"button","x":20,"y":5,"w":10,"h":10,"id":7,"alt_id":8}]},
        {"type":"button","x":80,"y":5,"w":10,"h":10,"local":"close"}]}]}"#;

    #[test]
    fn a_button_opens_a_window_and_a_press_inside_it_stays_there() {
        let s = screen(WINDOWED);
        let mut l = Locals::default();
        assert_eq!(press(&s, &[], &mut l, [5.0, 5.0], false), Some(Press::Local));
        assert_eq!(l.window.as_deref(), Some("config"));
        // A press on the window that hits none of its buttons is the window's.
        assert_eq!(press(&s, &[], &mut l, [60.0, 60.0], false), Some(Press::Local));
        // A press outside it still reaches the screen's buttons.
        assert_eq!(press(&s, &[], &mut l, [35.0, 5.0], false), Some(Press::Send(9)));
        // The same button closes it, as a tab does; so does the window's own close button.
        assert_eq!(press(&s, &[], &mut l, [5.0, 5.0], false), Some(Press::Local));
        assert_eq!(l.window, None);
        press(&s, &[], &mut l, [5.0, 5.0], false);
        press(&s, &[], &mut l, [10.0 + 85.0, 30.0 + 8.0], false);
        assert_eq!(l.window, None);
    }

    #[test]
    fn a_button_tooltip_reads_the_data_slots() {
        let s = screen(r#"{"width":176,"height":166,"slots":[],"widgets":[{"type":"button","x":0,"y":0,"w":10,"h":10,"id":1,"tooltip":"Rate: {0} FE\nMax {1}"}]}"#);
        let lines = hovered(&s, &[42, 7], &Locals::default(), [3.0, 3.0]);
        assert_eq!(lines, Some(vec!["Rate: 42 FE".to_owned(), "Max 7".to_owned()]));
    }

    #[test]
    fn a_text_can_show_a_number_kept_in_two_slots() {
        // 70000 = 0x11170: the low 16 bits then the high ones.
        assert_eq!(fill_text("{0+1} / {2}", &[0x1170, 1, 5]), "70000 / 5");
        assert_eq!(fill_text("{9+1}", &[0, 0]), "0");
    }

    #[test]
    fn a_gauge_names_its_chemical_in_the_tooltip() {
        let s = screen(
            r##"{"width":176,"height":166,"slots":[],"widgets":[{"type":"gauge","x":0,"y":0,"w":16,"h":58,"value":{"data":0},"max":{"const":1000},"kind":{"data":1},"types":[{"texture":"m:a","tint":"#B30505","label":"Redstone"},{"texture":"m:b","tint":"#6CEDD8","label":"Diamond"}],"tooltip":"{label}: {value} mB","empty":"Empty"}]}"##,
        );
        let at = [3.0, 3.0];
        assert_eq!(hovered(&s, &[250, 2], &Locals::default(), at), Some(vec!["Diamond: 250 mB".to_owned()]));
        assert_eq!(hovered(&s, &[0, 0], &Locals::default(), at), Some(vec!["Empty".to_owned()]));
        assert_eq!(hovered(&s, &[250, 2], &Locals::default(), [30.0, 3.0]), None);
    }

    #[test]
    fn a_neighbour_side_follows_the_way_the_machine_faces() {
        assert_eq!(neighbor_step("north", 0), [0, 0, -1]);
        assert_eq!(neighbor_step("north", 1), [1, 0, 0]);
        assert_eq!(neighbor_step("north", 2), [-1, 0, 0]);
        assert_eq!(neighbor_step("north", 3), [0, 0, 1]);
        assert_eq!(neighbor_step("east", 0), [1, 0, 0]);
        assert_eq!(neighbor_step("south", 3), [0, 0, -1]);
        assert_eq!(neighbor_step("west", 4), [0, 1, 0]);
        assert_eq!(neighbor_step("west", 5), [0, -1, 0]);
    }

    #[test]
    fn a_dragged_window_takes_its_presses_where_it_is_now() {
        let s = screen(WINDOWED);
        let mut l = Locals { window: Some("config".into()), offset: [20.0, 5.0], ..Locals::default() };
        // The window's button at its 5, 5 is now at the screen's 10 + 20 + 5, 30 + 5 + 5.
        assert_eq!(press(&s, &[], &mut l, [35.0, 40.0], false), Some(Press::Local));
        assert_eq!(l.var("tab"), 1);
        // Where it was is no longer the window's.
        assert_eq!(press(&s, &[], &mut l, [15.0, 35.0], false), None);
        let w = open_window(&s, &l).unwrap();
        assert_eq!(window_origin(w, &l), [30.0, 35.0]);
    }

    #[test]
    fn a_window_tab_outside_the_window_is_pressed_too() {
        let s = screen(
            r#"{"slots":[],"widgets":[{"type":"button","x":0,"y":0,"w":10,"h":10,"id":5}],
              "windows":[{"id":"w","x":50,"y":10,"width":40,"height":40,"widgets":[
                {"type":"button","x":-26,"y":2,"w":26,"h":26,"local":{"set":{"var":"type","value":1}},"tooltip":"Energy"}]}]}"#,
        );
        let mut l = Locals { window: Some("w".into()), ..Locals::default() };
        // The tab is at the window's -26, 2: the screen's 24, 12, outside the window, yet it is pressed.
        assert_eq!(press(&s, &[], &mut l, [30.0, 20.0], false), Some(Press::Local));
        assert_eq!(l.var("type"), 1);
        assert_eq!(hovered(&s, &[], &l, [30.0, 20.0]), Some(vec!["Energy".to_owned()]));
        // The screen's own buttons still work outside the window.
        assert_eq!(press(&s, &[], &mut l, [5.0, 5.0], false), Some(Press::Send(5)));
    }

    #[test]
    fn a_window_button_sets_a_variable_that_shows_a_group() {
        let s = screen(WINDOWED);
        let mut l = Locals { window: Some("config".into()), ..Locals::default() };
        let at = |x: f32, y: f32| [10.0 + x, 30.0 + y];
        // The group is hidden until the variable is set, so its button is not there.
        assert_eq!(press(&s, &[], &mut l, at(25.0, 8.0), false), Some(Press::Local));
        assert_eq!(press(&s, &[], &mut l, at(7.0, 8.0), false), Some(Press::Local));
        assert_eq!(l.var("tab"), 1);
        assert_eq!(press(&s, &[], &mut l, at(25.0, 8.0), false), Some(Press::Send(7)));
        assert_eq!(press(&s, &[], &mut l, at(25.0, 8.0), true), Some(Press::Send(8)));
    }

    #[test]
    fn nibbles_conditions_and_swatches_read_the_data() {
        let data = [0x4321, 0];
        assert_eq!(read(&Value::Nibble([0, 0]), &data), 1);
        assert_eq!(read(&Value::Nibble([0, 3]), &data), 4);
        let s = screen(
            r##"{"slots":[],"widgets":[
                {"type":"show","when":{"data":{"value":{"nibble":[0,1]},"eq":2}},"widgets":[
                    {"type":"swatch","x":0,"y":0,"w":10,"h":10,"value":{"nibble":[0,2]},
                     "colors":["#808080","#AA0000","#0000AA"],"labels":["None","Input","Output"],"tooltip":"Side: {label}"}]}]}"##,
        );
        let l = Locals::default();
        assert_eq!(showing(&s.widgets, &data, &l).len(), 1);
        assert_eq!(showing(&s.widgets, &[0x4311, 0], &l).len(), 0);
        // nibble 2 of 0x4321 is 3: past the labels, the last one stands.
        assert_eq!(hovered(&s, &data, &l, [5.0, 5.0]), Some(vec!["Side: Output".to_owned()]));
        assert_eq!(hovered(&s, &data, &l, [50.0, 5.0]), None);
    }
}

/// The readings a graph widget keeps on the client: the last `samples` of each series, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphHistory {
    pub samples: usize,
    pub series: Vec<std::collections::VecDeque<i64>>,
}

impl GraphHistory {
    pub fn new(series: usize, samples: usize) -> Self {
        Self { samples: samples.max(2), series: vec![std::collections::VecDeque::new(); series] }
    }
    /// One reading of every series (a tick); the oldest goes once `samples` are kept.
    pub fn push(&mut self, values: &[i64]) {
        for (kept, v) in self.series.iter_mut().zip(values) {
            kept.push_back(*v);
            while kept.len() > self.samples {
                kept.pop_front();
            }
        }
    }
    /// The vertical scale: the least and the greatest reading kept, the top raised so the span is at least `min_scale`.
    pub fn scale(&self, min_scale: i64) -> (i64, i64) {
        let all = self.series.iter().flatten();
        let lo = all.clone().copied().min().unwrap_or(0);
        let hi = all.copied().max().unwrap_or(0);
        (lo, hi.max(lo + min_scale.max(1)))
    }
    /// Each series as points in `rect` (`x, y, w, h`): the newest reading at the right edge, one step of `w / (samples - 1)` back
    /// for each older one; `lo` at the bottom, `hi` at the top.
    pub fn points(&self, rect: [f32; 4], min_scale: i64) -> Vec<Vec<[f32; 2]>> {
        let (lo, hi) = self.scale(min_scale);
        let [x, y, w, h] = rect;
        let step = w / (self.samples - 1) as f32;
        let span = (hi - lo) as f32;
        self.series
            .iter()
            .map(|kept| {
                let n = kept.len();
                kept.iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let px = x + w - (n - 1 - i) as f32 * step;
                        let py = y + h - (*v - lo) as f32 / span * h;
                        [px, py]
                    })
                    .collect()
            })
            .collect()
    }
}

/// The channel a radial menu travels on, host to client (`players.open-radial-menu`).
pub const RADIAL_CHANNEL: &str = "horizon:radial";

/// A radial menu as the channel carries it: the mod's id, the current entry (four bytes), then each entry's id, label and icon.
pub fn encode_radial(mod_id: &str, entries: &[(String, String, String)], current: u32) -> Vec<u8> {
    fn put(out: &mut Vec<u8>, s: &str) {
        let b = &s.as_bytes()[..s.len().min(usize::from(u16::MAX))];
        out.extend((b.len() as u16).to_le_bytes());
        out.extend(b);
    }
    let mut out = Vec::new();
    put(&mut out, mod_id);
    out.extend(current.to_le_bytes());
    out.push(entries.len().min(255) as u8);
    for (id, label, icon) in entries.iter().take(255) {
        put(&mut out, id);
        put(&mut out, label);
        put(&mut out, icon);
    }
    out
}

/// The mod id, entries and current entry of [`encode_radial`].
pub fn decode_radial(data: &[u8]) -> Option<(String, Vec<(String, String, String)>, u32)> {
    let mut at = 0;
    let text = |at: &mut usize| -> Option<String> {
        let n = usize::from(u16::from_le_bytes([*data.get(*at)?, *data.get(*at + 1)?]));
        let s = String::from_utf8(data.get(*at + 2..*at + 2 + n)?.to_vec()).ok()?;
        *at += 2 + n;
        Some(s)
    };
    let mod_id = text(&mut at)?;
    let c = data.get(at..at + 4)?;
    let current = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
    let n = usize::from(*data.get(at + 4)?);
    at += 5;
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        entries.push((text(&mut at)?, text(&mut at)?, text(&mut at)?));
    }
    Some((mod_id, entries, current))
}

/// Which of `n` entries spread around `centre` (the first at the top, then clockwise) the point `at` names: none within `dead_zone`
/// of the centre (the middle closes the menu without a choice).
pub fn radial_pick(n: usize, centre: [f32; 2], at: [f32; 2], dead_zone: f32) -> Option<usize> {
    let (dx, dy) = (at[0] - centre[0], at[1] - centre[1]);
    if n == 0 || dx.hypot(dy) < dead_zone {
        return None;
    }
    // 0 at the top, growing clockwise (the screen's y grows down).
    let angle = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU);
    let slice = std::f32::consts::TAU / n as f32;
    Some(((angle + slice / 2.0) / slice) as usize % n)
}

/// Where entry `i` of `n` sits around `centre` at `radius` (the first at the top, then clockwise).
pub fn radial_spot(i: usize, n: usize, centre: [f32; 2], radius: f32) -> [f32; 2] {
    let a = std::f32::consts::TAU * i as f32 / n.max(1) as f32;
    [centre[0] + radius * a.sin(), centre[1] - radius * a.cos()]
}

/// The channel a mod's HUD lines travel on, host to client (`players.set-hud-line`).
pub const HUD_CHANNEL: &str = "horizon:hud_line";

/// A HUD line as the channel carries it: id, icon and text (each its length in two bytes, then UTF-8), then the fraction (four bytes).
pub fn encode_hud_line(id: &str, icon: &str, text: &str, fraction: f32) -> Vec<u8> {
    let mut out = Vec::new();
    for s in [id, icon, text] {
        let b = s.as_bytes();
        out.extend((b.len().min(usize::from(u16::MAX)) as u16).to_le_bytes());
        out.extend(&b[..b.len().min(usize::from(u16::MAX))]);
    }
    out.extend(fraction.to_le_bytes());
    out
}

/// The id, icon, text and fraction of [`encode_hud_line`].
pub fn decode_hud_line(data: &[u8]) -> Option<(String, String, String, f32)> {
    let mut at = 0;
    let mut text = || -> Option<String> {
        let n = usize::from(u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]));
        let s = String::from_utf8(data.get(at + 2..at + 2 + n)?.to_vec()).ok()?;
        at += 2 + n;
        Some(s)
    };
    let (id, icon, line) = (text()?, text()?, text()?);
    let f = data.get(at..at + 4)?;
    Some((id, icon, line, f32::from_le_bytes([f[0], f[1], f[2], f[3]])))
}

/// What a confirmed text box sends the server, on `<mod id>:menu_text`: the block's position (three little-endian whole numbers),
/// the widget's `id` (one more) and the text (UTF-8, the rest of the message).
pub fn encode_menu_input(pos: [i32; 3], id: i32, text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for v in pos {
        out.extend(v.to_le_bytes());
    }
    out.extend(id.to_le_bytes());
    out.extend(text.as_bytes());
    out
}

/// The position, widget id and text of [`encode_menu_input`].
pub fn decode_menu_input(data: &[u8]) -> Option<([i32; 3], i32, String)> {
    let int = |at: usize| data.get(at..at + 4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    Some(([int(0)?, int(4)?, int(8)?], int(12)?, String::from_utf8(data.get(16..)?.to_vec()).ok()?))
}

/// The channel a block entity's menu strings travel on, host to client (`world.set-menu-text`).
pub const MENU_TEXT_CHANNEL: &str = "horizon:menu_text";

/// A block's menu strings as the channel carries them: the position (three little-endian whole numbers), the count, then each string
/// as its length (two bytes) and its UTF-8.
pub fn encode_menu_text(pos: [i32; 3], texts: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in pos {
        out.extend(v.to_le_bytes());
    }
    out.push(texts.len().min(255) as u8);
    for t in texts.iter().take(255) {
        let bytes = t.as_bytes();
        let n = bytes.len().min(usize::from(u16::MAX));
        out.extend((n as u16).to_le_bytes());
        out.extend(&bytes[..n]);
    }
    out
}

/// The position and the strings of [`encode_menu_text`]; None for bytes that are cut short or not text.
pub fn decode_menu_text(data: &[u8]) -> Option<([i32; 3], Vec<String>)> {
    let int = |at: usize| data.get(at..at + 4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let pos = [int(0)?, int(4)?, int(8)?];
    let count = usize::from(*data.get(12)?);
    let mut at = 13;
    let mut texts = Vec::with_capacity(count);
    for _ in 0..count {
        let n = usize::from(u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]));
        at += 2;
        texts.push(String::from_utf8(data.get(at..at + n)?.to_vec()).ok()?);
        at += n;
    }
    Some((pos, texts))
}

#[cfg(test)]
mod menu_text_tests {
    use super::*;

    #[test]
    fn menu_input_round_trips_and_a_short_message_is_refused() {
        let bytes = encode_menu_input([5, -64, 9], 7800, "forge:ores/iron");
        assert_eq!(decode_menu_input(&bytes), Some(([5, -64, 9], 7800, "forge:ores/iron".to_owned())));
        assert_eq!(decode_menu_input(&bytes[..15]), None);
        // An empty text is a message too (a cleared box).
        assert_eq!(decode_menu_input(&encode_menu_input([0; 3], 1, "")), Some(([0; 3], 1, String::new())));
    }

    #[test]
    fn a_press_on_a_text_field_focuses_it_and_one_beside_it_is_not_a_field_press() {
        use crate::content::{MenuScreen, Widget};
        let screen: MenuScreen = serde_json::from_str(
            r#"{"slots": [], "widgets": [
                {"type": "textfield", "x": 10, "y": 20, "w": 60, "h": 12, "id": 7800, "validator": "digit", "max_length": 7},
                {"type": "button", "x": 10, "y": 40, "w": 20, "h": 10, "id": 3}]}"#,
        )
        .unwrap();
        let mut locals = Locals::default();
        assert_eq!(press(&screen, &[], &mut locals, [12.0, 22.0], false), Some(Press::Field(7800)));
        assert_eq!(press(&screen, &[], &mut locals, [12.0, 42.0], false), Some(Press::Send(3)));
        assert_eq!(press(&screen, &[], &mut locals, [100.0, 22.0], false), None);
        let Some(Widget::TextField { max_length, validator, .. }) = crate::content::find_field(&screen, 7800) else {
            panic!("the field is found by its id");
        };
        assert_eq!(*max_length, 7);
        assert!(validator.allows('5') && !validator.allows('a'));
    }

    #[test]
    fn a_list_scrolls_by_the_wheel_and_a_click_names_the_row_under_it() {
        use crate::content::MenuScreen;
        let screen: MenuScreen = serde_json::from_str(
            r#"{"slots": [], "widgets": [
                {"type": "list", "x": 10, "y": 20, "w": 80, "h": 36, "row_height": 12, "first": 8, "count": {"data": 0}, "on_click": 7700}]}"#,
        )
        .unwrap();
        let data = [7]; // seven entries, three rows showing
        let mut locals = Locals::default();
        // The first press, on the second row (y 32..44): entry 1.
        assert_eq!(press(&screen, &data, &mut locals, [12.0, 34.0], false), Some(Press::Row { button: 7701, first: 8 }));
        // Four notches down (the wheel's down is negative) stop at 7 - 3 = 4.
        assert!(scroll_lists(&screen, &data, &mut locals, [12.0, 34.0], -9));
        assert_eq!(locals.scroll(8, 7, 3), 4);
        assert_eq!(press(&screen, &data, &mut locals, [12.0, 34.0], false), Some(Press::Row { button: 7705, first: 8 }));
        assert!(scroll_lists(&screen, &data, &mut locals, [12.0, 34.0], 1));
        assert_eq!(locals.scroll(8, 7, 3), 3);
        // Off the list the wheel is not taken; fewer entries than rows leave no row to press past them.
        assert!(!scroll_lists(&screen, &data, &mut locals, [200.0, 34.0], 1));
        assert_eq!(press(&screen, &[2], &mut Locals::default(), [12.0, 52.0], false), None);
    }

    #[test]
    fn a_graph_keeps_its_last_samples_and_scales_from_the_least_to_the_greatest() {
        let mut g = GraphHistory::new(2, 3);
        g.push(&[100, 2000]);
        g.push(&[300, 2500]);
        g.push(&[200, 2200]);
        g.push(&[400, 2100]);
        // Three kept: the first reading is gone.
        assert_eq!(g.series[0].iter().copied().collect::<Vec<_>>(), [300, 200, 400]);
        assert_eq!(g.scale(1), (200, 2500));
        // A flat history still gets the minimum span.
        let mut flat = GraphHistory::new(1, 4);
        flat.push(&[1000]);
        flat.push(&[1000]);
        assert_eq!(flat.scale(1600), (1000, 2600));
        // The newest reading is at the right edge, the scale's bottom at the bottom and its top at the top.
        let pts = g.points([10.0, 20.0, 40.0, 100.0], 1);
        assert_eq!(pts[0].last(), Some(&[50.0, 20.0 + 100.0 - (400.0 - 200.0) / 2300.0 * 100.0]));
        assert_eq!(pts[1][0], [10.0, 20.0], "2500, the greatest, is the top, two steps back from the newest");
        assert_eq!(pts[0][1], [30.0, 120.0], "200, the least, is the bottom");
    }

    #[test]
    fn a_graph_is_checked_like_the_other_widgets() {
        let ok: MenuScreen = serde_json::from_str(
            r##"{"slots": [], "widgets": [{"type": "graph", "x": 7, "y": 20, "w": 160, "h": 40, "samples": 40, "min_scale": 1600,
                "series": [{"value": {"data": 0}, "color": "#FF0000"}, {"value": {"pair": [1, 2]}, "color": "#00FFFF"}], "tooltip": "Heat {0}"}]}"##,
        )
        .unwrap();
        assert!(matches!(&ok.widgets[0], Widget::Graph { samples: 40, min_scale: 1600, series, .. } if series.len() == 2));
        assert_eq!(hovered(&ok, &[1234, 0, 0], &Locals::default(), [10.0, 30.0]), Some(vec!["Heat 1234".to_owned()]));
    }

    #[test]
    fn a_radial_menu_round_trips_and_a_point_picks_the_entry_around_it() {
        let entries = vec![
            ("vein".to_owned(), "Vein mining".to_owned(), "mekanism:module_vein".to_owned()),
            ("silk".to_owned(), "Silk touch".to_owned(), String::new()),
            ("off".to_owned(), "Off".to_owned(), String::new()),
            ("fast".to_owned(), "Fast".to_owned(), String::new()),
        ];
        let bytes = encode_radial("mekanism", &entries, 2);
        assert_eq!(decode_radial(&bytes), Some(("mekanism".to_owned(), entries, 2)));
        assert_eq!(decode_radial(&bytes[..bytes.len() - 1]), None);
        // Four entries: top, right, bottom, left; the middle is no choice.
        let c = [200.0, 120.0];
        assert_eq!(radial_pick(4, c, [200.0, 60.0], 16.0), Some(0));
        assert_eq!(radial_pick(4, c, [260.0, 125.0], 16.0), Some(1));
        assert_eq!(radial_pick(4, c, [205.0, 180.0], 16.0), Some(2));
        assert_eq!(radial_pick(4, c, [140.0, 110.0], 16.0), Some(3));
        assert_eq!(radial_pick(4, c, [205.0, 118.0], 16.0), None);
        // Just past the border between the top and the right one (45 degrees) is the right one.
        assert_eq!(radial_pick(4, c, [200.0 + 50.0, 120.0 - 49.0], 16.0), Some(1));
        let spot = radial_spot(1, 4, c, 50.0);
        assert!((spot[0] - 250.0).abs() < 1e-3 && (spot[1] - 120.0).abs() < 1e-3);
    }

    #[test]
    fn a_hud_line_round_trips_and_a_cut_one_is_refused() {
        let bytes = encode_hud_line("mekanism:energy", "mekanism:energy_unit", "Energy: 50% ✓", 0.5);
        assert_eq!(
            decode_hud_line(&bytes),
            Some(("mekanism:energy".into(), "mekanism:energy_unit".into(), "Energy: 50% ✓".into(), 0.5))
        );
        assert_eq!(decode_hud_line(&bytes[..bytes.len() - 1]), None);
    }

    #[test]
    fn validators_filter_what_is_typed() {
        use crate::content::Validator;
        assert!(Validator::Id.allows('a') && Validator::Id.allows('/') && Validator::Id.allows('#'));
        assert!(!Validator::Id.allows('A') && !Validator::Id.allows(' '));
        assert!(Validator::Any.allows('ñ') && !Validator::Any.allows('\n'));
        assert!(Validator::Digit.allows('0') && !Validator::Digit.allows('-'));
    }

    #[test]
    fn menu_text_round_trips_and_a_cut_message_is_refused() {
        let texts = vec!["forge:ores/iron".to_owned(), String::new(), "ñandú ✓".to_owned()];
        let bytes = encode_menu_text([-3, 64, 12], &texts);
        assert_eq!(decode_menu_text(&bytes), Some(([-3, 64, 12], texts)));
        assert_eq!(decode_menu_text(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode_menu_text(&[0; 5]), None);
    }
}
