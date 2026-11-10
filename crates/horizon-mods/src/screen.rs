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

impl Locals {
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
    fn menu_text_round_trips_and_a_cut_message_is_refused() {
        let texts = vec!["forge:ores/iron".to_owned(), String::new(), "ñandú ✓".to_owned()];
        let bytes = encode_menu_text([-3, 64, 12], &texts);
        assert_eq!(decode_menu_text(&bytes), Some(([-3, 64, 12], texts)));
        assert_eq!(decode_menu_text(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode_menu_text(&[0; 5]), None);
    }
}
