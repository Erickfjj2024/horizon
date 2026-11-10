//! What a declared block entity renderer works out without drawing (API block
//! 11): the part of a tank box a fluid fills, which sprite and tint a substance
//! has, how far a spinning box has turned, and how the server's tanks and
//! energy are quantised so the clients are told only when a drawing changes.
use crate::content::parse_color;
use std::collections::BTreeMap;

/// The box (`from`, `to` in texels) a tank of `fraction` (0 to 1) fills,
/// rising from the bottom of `from`..`to`; none for an empty one.
pub fn fluid_box(from: [f32; 3], to: [f32; 3], fraction: f32) -> Option<([f32; 3], [f32; 3])> {
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return None;
    }
    let mut top = to;
    top[1] = from[1] + (to[1] - from[1]) * fraction;
    Some((from, top))
}

/// The sprite of `substance`: the renderer's own, else vanilla's for water
/// and lava, else the default.
pub fn fluid_sprite<'a>(
    sprites: &'a BTreeMap<String, String>,
    default: Option<&'a String>,
    substance: &str,
) -> Option<&'a str> {
    if let Some(s) = sprites.get(substance) {
        return Some(s);
    }
    match substance {
        "minecraft:water" => Some("block/water_still"),
        "minecraft:lava" => Some("block/lava_still"),
        _ => default.map(String::as_str),
    }
}

/// The 0xRRGGBB tint of `substance`: the renderer's own, else water's blue,
/// else none (white).
pub fn fluid_tint(tints: &BTreeMap<String, String>, substance: &str) -> u32 {
    match tints.get(substance).and_then(|c| parse_color(c)) {
        Some(c) => c & 0x00FF_FFFF,
        None if substance == "minecraft:water" => 0x3F76E4,
        None => 0xFFFFFF,
    }
}

/// How far a part has turned, in degrees, at `game_time` and `partial` tick.
pub fn spin_degrees(game_time: i64, partial: f32, degrees_per_tick: f32) -> f32 {
    ((game_time % 360_000) as f32 + partial) * degrees_per_tick % 360.0
}

/// The angle of a turn at time `time` (ticks and the partial tick): `offset` degrees
/// plus `degrees_per_tick` a tick, kept within a turn.
pub fn turn_degrees(time: f32, degrees_per_tick: f32, offset: f32) -> f32 {
    (offset + time * degrees_per_tick) % 360.0
}

/// How far a bounce has lifted a part at `time`: `amplitude` times the sine of
/// `degrees_per_tick` degrees a tick.
pub fn bounce_offset(time: f32, amplitude: f32, degrees_per_tick: f32) -> f32 {
    amplitude * (time * degrees_per_tick).to_radians().sin()
}

/// A value in sixteenths (0 to 16), for telling clients of a change only when
/// what they draw does.
pub fn sixteenths(value: u64, max: u64) -> u8 {
    if max == 0 {
        return 0;
    }
    // Rounded up from empty, so a little still shows.
    let v = (u128::from(value.min(max)) * 16 / u128::from(max)) as u8;
    if value > 0 && v == 0 { 1 } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fluid_rises_from_the_bottom() {
        let (from, to) = ([2.0, 1.0, 2.0], [14.0, 15.0, 14.0]);
        assert_eq!(fluid_box(from, to, 0.0), None);
        assert_eq!(fluid_box(from, to, 0.5), Some((from, [14.0, 8.0, 14.0])));
        assert_eq!(fluid_box(from, to, 2.0), Some((from, to)));
    }
    #[test]
    fn substances_have_sprites_and_tints() {
        let mut sprites = BTreeMap::new();
        sprites.insert("mymod:hydrogen".to_owned(), "mymod:block/hydrogen".to_owned());
        let default = "mymod:block/gas".to_owned();
        assert_eq!(fluid_sprite(&sprites, Some(&default), "mymod:hydrogen"), Some("mymod:block/hydrogen"));
        assert_eq!(fluid_sprite(&sprites, Some(&default), "minecraft:lava"), Some("block/lava_still"));
        assert_eq!(fluid_sprite(&sprites, Some(&default), "mymod:other"), Some("mymod:block/gas"));
        assert_eq!(fluid_sprite(&sprites, None, "mymod:other"), None);
        let mut tints = BTreeMap::new();
        tints.insert("mymod:hydrogen".to_owned(), "#80FF8000".to_owned());
        assert_eq!(fluid_tint(&tints, "mymod:hydrogen"), 0xFF8000);
        assert_eq!(fluid_tint(&tints, "minecraft:water"), 0x3F76E4);
        assert_eq!(fluid_tint(&tints, "minecraft:lava"), 0xFFFFFF);
    }
    #[test]
    fn turns_and_bounces() {
        assert_eq!(turn_degrees(10.0, 4.0, 36.0), 76.0);
        assert!(turn_degrees(1.0e6, 4.0, 36.0) < 360.0);
        assert!(bounce_offset(0.0, 0.5, 3.0).abs() < 1e-6);
        // A quarter of the sine's period at 3 degrees a tick is 30 ticks.
        assert!((bounce_offset(30.0, 0.5, 3.0) - 0.5).abs() < 1e-5);
        assert!((bounce_offset(90.0, 0.5, 3.0) + 0.5).abs() < 1e-5);
    }
    #[test]
    fn spin_and_quantising() {
        assert_eq!(spin_degrees(10, 0.5, 4.0), 42.0);
        assert!(spin_degrees(1_000_000_000, 0.0, 7.0) < 360.0);
        assert_eq!(sixteenths(0, 100), 0);
        assert_eq!(sixteenths(1, 100), 1);
        assert_eq!(sixteenths(50, 100), 8);
        assert_eq!(sixteenths(100, 100), 16);
        assert_eq!(sixteenths(500, 100), 16);
        assert_eq!(sixteenths(5, 0), 0);
    }
}
