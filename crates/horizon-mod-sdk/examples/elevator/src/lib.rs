//! Elevator, a Horizon server mod (`docs/mods.md`, "Server mods"): a port of
//! ElevatorMod (VsnGamer, MIT; see NOTICE). Jump on an elevator block to go
//! up to the next one above it, sneak to go down.
//!
//! The rules (which block is the target, what the server checks, what the
//! experience cost is, where the player lands) are in `logic.rs`, tested
//! without the game. This file reads the world and the players through the
//! mod API and carries out what the logic decided.
//!
//! The blocks come from the content mod `elevatorid` (`elevatorid:elevator_<colour>`).
use std::collections::{BTreeMap, HashMap};

use horizon_mod_sdk::server::{
    Args, Player, Registration, ServerMod, GameMode, player_control, players, world,
};
use horizon_mod_sdk::storage;

// The directional elevator's seat of the rules is the same file the PC mod builds from.
#[allow(dead_code)]
mod logic;

use logic::{
    BlockPos, Config, Dir, DyeColor, Elevator, Facing, Pitch, Player as Rider, Reject, World, Yaw,
};

/// The sound the Java mod plays on arrival (`elevatorid:teleport`, in the content mod).
const TELEPORT_SOUND: &str = "elevatorid:teleport";
/// The config keys `/elevator set` takes (ElevatorMod's `elevatorid-synced.toml`).
const KEYS: [&str; 9] = [
    "sameColor",
    "range",
    "activationRange",
    "precisionTarget",
    "mobSpawn",
    "resetPitchNormal",
    "resetPitchDirectional",
    "useXP",
    "XPPointsAmount",
];

/// What a player was doing the tick before.
#[derive(Clone)]
struct Seen {
    sneaking: bool,
    jumps: u32,
    on_ground: bool,
    pos: [f64; 3],
    dimension: String,
    /// Ticks to wait before this player can ride again: a ride moves them, and the move
    /// must not read as a jump or a sneak.
    cooldown: u8,
}

/// A jump rises about 0.42 a block in its first tick (more with Jump Boost); a ride, a fall
/// or a flight is outside this.
const JUMP_RISE: std::ops::RangeInclusive<f64> = 0.3..=3.0;
const RIDE_COOLDOWN: u8 = 3;

/// Whether a new mod starts telling players what it sees of their movement (`/elevator watch`).
/// Off by default; it is for tracking down a jump that is not seen.
const WATCH_DEFAULT: bool = false;

struct Elevators {
    /// Tells each player what the mod sees of their movement, when it changes.
    watch: bool,
    /// The server tick of the last call, to see whether `tick` is called every tick.
    last_tick: u64,
    config: Config,
    /// The config as `key = value` lines, kept in `storage`.
    lines: BTreeMap<String, String>,
    seen: HashMap<String, Seen>,
}

/// The world's height by dimension: (lowest block, height).
fn dimension_height(dimension: &str) -> (i32, i32) {
    match dimension {
        "minecraft:the_nether" | "minecraft:the_end" => (0, 256),
        _ => (-64, 384),
    }
}

/// The block state's name and its `key=value` properties.
fn parse_state(state: &str) -> (&str, Vec<(&str, &str)>) {
    let (name, rest) = state.split_once('[').unwrap_or((state, ""));
    let props = rest
        .trim_end_matches(']')
        .split(',')
        .filter_map(|p| p.split_once('='))
        .collect();
    (name, props)
}

fn facing_of(name: &str) -> Option<Facing> {
    Some(match name {
        "north" => Facing::North,
        "south" => Facing::South,
        "east" => Facing::East,
        "west" => Facing::West,
        _ => return None,
    })
}

/// The world as one dimension's loaded chunks show it.
struct Reader<'a> {
    dimension: &'a str,
}

impl Reader<'_> {
    fn boxes(&self, pos: BlockPos) -> Vec<world::CollisionBox> {
        world::get_collision(self.dimension, pos.x, pos.y, pos.z).unwrap_or_default()
    }
}

impl World for Reader<'_> {
    fn elevator_at(&self, pos: BlockPos) -> Option<Elevator> {
        let state = world::get_block(self.dimension, pos.x, pos.y, pos.z)?;
        let (name, props) = parse_state(&state);
        let color = DyeColor::from_block_name(name)?;
        let prop = |key: &str| props.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        let top = self.boxes(pos).iter().map(|b| b.max_y).fold(0.0_f64, f64::max);
        Some(Elevator {
            color,
            directional: prop("directional") == Some("true"),
            facing: prop("facing").and_then(facing_of).unwrap_or(Facing::North),
            support_height: if top > 0.0 { top } else { 1.0 },
        })
    }

    fn is_suffocating(&self, pos: BlockPos) -> bool {
        self.boxes(pos).iter().any(|b| {
            b.max_x - b.min_x >= 0.999 && b.max_y - b.min_y >= 0.999 && b.max_z - b.min_z >= 0.999
        })
    }

    fn in_build_height(&self, y: i32) -> bool {
        let (min, height) = dimension_height(self.dimension);
        y >= min && y < min + height
    }

    fn is_loaded(&self, pos: BlockPos) -> bool {
        world::get_block(self.dimension, pos.x, pos.y, pos.z).is_some()
    }
}

fn render(lines: &BTreeMap<String, String>) -> String {
    let mut text = String::from("[General]\n");
    for (k, v) in lines {
        text.push_str(&format!("{k} = {v}\n"));
    }
    text
}

impl Elevators {
    /// `/elevator set <key> <value>`: only when you are alone on the server.
    fn set(&mut self, key: &str, value: &str) -> Result<String, String> {
        if !KEYS.contains(&key) {
            return Err(format!("Unknown setting {key}. Settings: {}", KEYS.join(", ")));
        }
        let boolean = matches!(
            key,
            "sameColor" | "precisionTarget" | "mobSpawn" | "resetPitchNormal" | "resetPitchDirectional"
                | "useXP"
        );
        let ok = if boolean {
            matches!(value, "true" | "false")
        } else {
            value.parse::<u32>().is_ok()
        };
        if !ok {
            let want = if boolean { "true or false" } else { "a whole number" };
            return Err(format!("{key} takes {want}"));
        }
        self.lines.insert(key.to_owned(), value.to_owned());
        let text = render(&self.lines);
        self.config = Config::from_toml(&text);
        storage::set("config", text.as_bytes())?;
        Ok(format!("{key} = {value}"))
    }

    fn show(&self) -> String {
        let c = &self.config;
        format!(
            "sameColor={} range={} activationRange={} precisionTarget={} resetPitchNormal={} \
             resetPitchDirectional={} useXP={} XPPointsAmount={}",
            c.same_color,
            c.range,
            c.activation_range,
            c.precision_target,
            c.reset_pitch_normal,
            c.reset_pitch_directional,
            c.use_xp,
            c.xp_points_amount
        )
    }

    /// `ElevatorHandler.tryTeleport` and `TeleportPacket.handle` in one place:
    /// finds the target, checks it as the server does, charges and moves the player.
    fn ride(&self, p: &Player, motion: &player_control::Motion, dir: Dir) -> bool {
        if p.health <= 0.0 || p.game_mode == GameMode::Spectator {
            return false;
        }
        let cfg = &self.config;
        let here = BlockPos::new(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let reader = Reader { dimension: &p.dimension };
        let Some(from) = logic::origin_elevator(&reader, here, cfg) else {
            return false;
        };
        let Some(to) = logic::find_target(&reader, from, dir, cfg) else {
            return false;
        };
        let rider = Rider {
            alive: true,
            creative: p.game_mode == GameMode::Creative,
            block_pos: here,
            xp_progress_points: logic::xp_progress_points(motion.xp_level, motion.xp_progress),
            xp_level: u32::try_from(motion.xp_level).unwrap_or(0),
        };
        if logic::validate(&reader, &rider, from, to, cfg).is_err() {
            return false;
        }
        let Some(target) = reader.elevator_at(to) else {
            return false;
        };
        // The experience check and charge: only with `useXP` on, and not in creative.
        match logic::xp_cost(&rider, cfg) {
            Ok(0) => {}
            Ok(cost) => {
                if player_control::take_xp(&p.id, cost).is_err() {
                    return false;
                }
            }
            Err(Reject::MissingXp) => {
                players::tell(&p.id, "You do not have enough XP to use elevators");
                return false;
            }
            Err(_) => return false,
        }
        let dest = logic::destination(to, &target, p.x, p.z, cfg);
        // A thin block lying on the elevator (a carpet) would hold the player's feet inside its
        // collision, where the game cannot stand or crouch them (it crawls them, and a sneak is
        // not seen). They land on top of it instead: a sixteenth of a block higher.
        let thin = reader
            .boxes(to.above())
            .iter()
            .filter(|b| b.min_y < 0.01 && b.max_y < 0.999)
            .map(|b| b.max_y)
            .fold(0.0_f64, f64::max);
        let land_y = dest.y.max(f64::from(to.y) + target.support_height + thin);
        let yaw = match dest.yaw {
            Yaw::Set(v) => Some(v),
            Yaw::Keep => None,
        };
        let pitch = (dest.pitch == Pitch::Reset).then_some(0.0);
        if player_control::teleport(&p.id, &p.dimension, dest.x, land_y, dest.z, yaw, pitch).is_ok() {
            player_control::play_sound(
                &p.dimension,
                TELEPORT_SOUND,
                f64::from(to.x) + 0.5,
                f64::from(to.y) + 0.5,
                f64::from(to.z) + 0.5,
                1.0,
                1.0,
            );
            return true;
        }
        false
    }

    /// Whether the player stood on an elevator in `seen` (their feet on its top).
    fn stood_on_elevator(seen: &Seen) -> bool {
        let reader = Reader { dimension: &seen.dimension };
        let below = BlockPos::new(
            seen.pos[0].floor() as i32,
            (seen.pos[1] - 0.001).floor() as i32,
            seen.pos[2].floor() as i32,
        );
        reader.elevator_at(below).is_some()
    }

    /// `/elevator debug`: what the mod saw of the player who typed it on its last tick. (A typed
    /// command is not given the players' motion, so this reads what `tick` kept.)
    fn debug(&self, p: &Player) -> String {
        // The id a typed command carries is not always the one `players::online` lists (it
        // differed for the owner of a single-player world), so find the player by name.
        let listed = players::online().into_iter().find(|q| q.name == p.name);
        let p = listed.as_ref().unwrap_or(p);
        let Some(seen) = self.seen.get(&p.id) else {
            return "The mod has not seen you move yet: wait a second and try again".into();
        };
        let reader = Reader { dimension: &p.dimension };
        let here = BlockPos::new(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let origin = logic::origin_elevator(&reader, here, &self.config);
        let at = |d| origin.and_then(|o| logic::find_target(&reader, o, d, &self.config));
        let show = |b: Option<BlockPos>| b.map_or("none".to_string(), |b| format!("{} {} {}", b.x, b.y, b.z));
        format!(
            "sneaking={} onGround={} jumps={} y={:.3} origin={} up={} down={} cooldown={}",
            seen.sneaking,
            seen.on_ground,
            seen.jumps,
            p.y,
            show(origin),
            show(at(Dir::Up)),
            show(at(Dir::Down)),
            seen.cooldown
        )
    }
}

impl ServerMod for Elevators {
    fn init() -> Result<(Self, Registration), String> {
        let text = storage::get("config")
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        let lines = text
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect();
        Ok((
            Elevators {
                watch: WATCH_DEFAULT,
                last_tick: 0,
                config: Config::from_toml(&text),
                lines,
                seen: HashMap::new(),
            },
            Registration::new().on_tick().command("elevator"),
        ))
    }

    fn tick(&mut self, tick: u64) {
        let gap = tick.saturating_sub(self.last_tick);
        self.last_tick = tick;
        let online = players::online();
        self.seen.retain(|id, _| online.iter().any(|p| p.id == *id));
        for p in &online {
            let Some(m) = player_control::get_motion(&p.id) else {
                continue;
            };
            let mut now = Seen {
                sneaking: m.sneaking,
                jumps: m.jumps,
                on_ground: m.on_ground,
                pos: [p.x, p.y, p.z],
                dimension: p.dimension.clone(),
                cooldown: 0,
            };
            let Some(prev) = self.seen.get(&p.id).cloned() else {
                self.seen.insert(p.id.clone(), now);
                continue;
            };
            now.cooldown = prev.cooldown.saturating_sub(1);
            let rise = p.y - prev.pos[1];
            if self.watch
                && (rise.abs() > 0.05
                    || m.on_ground != prev.on_ground
                    || m.jumps != prev.jumps
                    || m.sneaking != prev.sneaking)
            {
                players::tell(
                    &p.id,
                    &format!(
                        "tick {tick} (+{gap}): y={:.3} rise={rise:+.3} onGround={} jumps={} sneaking={} onElevator={}",
                        p.y,
                        m.on_ground,
                        m.jumps,
                        m.sneaking,
                        Self::stood_on_elevator(&prev)
                    ),
                );
            }
            // A jump is seen three ways, any of which is enough: the game's jump count went up,
            // the player left the ground going up, or they stood on an elevator and rose by a
            // jump's height. (The count alone missed the owner of a world on an integrated
            // server.) The two read from the player's movement wait a few ticks after a ride,
            // whose own move must not read as a jump.
            let from_movement = prev.cooldown == 0
                && ((prev.on_ground && !m.on_ground && rise > 0.1 && rise <= *JUMP_RISE.end())
                    || (prev.dimension == p.dimension
                        && JUMP_RISE.contains(&rise)
                        && Self::stood_on_elevator(&prev)));
            let jumped = m.jumps > prev.jumps || from_movement;
            let mut rode = false;
            // As in ElevatorHandler: the sneak key first, then the jump key.
            if m.sneaking && !prev.sneaking {
                rode |= self.ride(p, &m, Dir::Down);
            }
            if jumped {
                let went = self.ride(p, &m, Dir::Up);
                if self.watch {
                    let why = if m.jumps > prev.jumps {
                        "the jump count"
                    } else {
                        "the movement"
                    };
                    players::tell(
                        &p.id,
                        &format!("jump seen by {why}: {}", if went { "went up" } else { "no elevator to go up to" }),
                    );
                }
                rode |= went;
            }
            if rode {
                now.cooldown = RIDE_COOLDOWN;
            }
            self.seen.insert(p.id.clone(), now);
        }
    }

    fn command(&mut self, player: &Player, _name: &str, args: &Args) -> Result<Option<String>, String> {
        let mut words = args.text().split_whitespace();
        match (words.next(), words.next(), words.next()) {
            (None | Some("show"), _, _) => Ok(Some(self.show())),
            (Some("debug"), _, _) => Ok(Some(self.debug(player))),
            (Some("watch"), Some(mode), _) if matches!(mode, "on" | "off") => {
                self.watch = mode == "on";
                Ok(Some(format!("watch {mode}")))
            }
            (Some("set"), Some(key), Some(value)) => {
                if players::online().len() > 1 {
                    return Err("Settings can only be changed when you are alone on the server".into());
                }
                self.set(key, value).map(Some)
            }
            _ => Err("Use /elevator show, /elevator debug, /elevator watch on|off, or /elevator set <setting> <value>".into()),
        }
    }
}

horizon_mod_sdk::export_server_mod!(Elevators);
