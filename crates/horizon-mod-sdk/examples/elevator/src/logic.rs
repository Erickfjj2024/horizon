//! Port em Rust da lógica do ElevatorMod (VsnGamer, MIT).
//!
//! Original: <https://github.com/VsnGamer/ElevatorMod>. Veja `NOTICE.md` e `LICENSE-ElevatorMod`.
//!
//! Este crate só tem a lógica pura, sem depender do jogo: o jogo (Horizon) fornece o mundo
//! pelo trait [`World`] e aplica o resultado. A integração com o Horizon fica em outra camada.

/// Posição de bloco.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
    pub fn above(self) -> Self {
        Self { y: self.y + 1, ..self }
    }
    pub fn below(self) -> Self {
        Self { y: self.y - 1, ..self }
    }
    pub fn with_y(self, y: i32) -> Self {
        Self { y, ..self }
    }
    /// Distância de Manhattan.
    pub fn dist_manhattan(self, o: Self) -> u32 {
        self.x.abs_diff(o.x) + self.y.abs_diff(o.y) + self.z.abs_diff(o.z)
    }
}

/// As 16 cores de elevador.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DyeColor {
    White, Orange, Magenta, LightBlue, Yellow, Lime, Pink, Gray,
    LightGray, Cyan, Purple, Blue, Brown, Green, Red, Black,
}

impl DyeColor {
    /// Id do bloco do elevador de cada cor: `elevatorid:elevator_<cor>`.
    pub const BLOCK_PREFIX: &'static str = "elevatorid:elevator_";

    /// A cor de um bloco `elevatorid:elevator_<cor>`, ou `None` se não for um elevador.
    pub fn from_block_name(name: &str) -> Option<Self> {
        Some(match name.strip_prefix(Self::BLOCK_PREFIX)? {
            "white" => Self::White,
            "orange" => Self::Orange,
            "magenta" => Self::Magenta,
            "light_blue" => Self::LightBlue,
            "yellow" => Self::Yellow,
            "lime" => Self::Lime,
            "pink" => Self::Pink,
            "gray" => Self::Gray,
            "light_gray" => Self::LightGray,
            "cyan" => Self::Cyan,
            "purple" => Self::Purple,
            "blue" => Self::Blue,
            "brown" => Self::Brown,
            "green" => Self::Green,
            "red" => Self::Red,
            "black" => Self::Black,
            _ => return None,
        })
    }
}

/// Direção em que o jogador fica virado em elevadores direcionais.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    North,
    South,
    East,
    West,
}

impl Facing {
    /// Rotação horizontal (yaw) em graus, como `Direction.toYRot()`.
    pub fn to_yaw(self) -> f32 {
        match self {
            Facing::South => 0.0,
            Facing::West => 90.0,
            Facing::North => 180.0,
            Facing::East => 270.0,
        }
    }
}

/// O que o jogo diz sobre um elevador colocado no mundo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Elevator {
    pub color: DyeColor,
    pub directional: bool,
    pub facing: Facing,
    /// Altura máxima da forma de suporte do bloco (0.0 a 1.0), para posicionar o jogador em cima.
    pub support_height: f64,
}

/// O mundo, visto pelo mod.
pub trait World {
    /// O elevador em `pos`, se aquele bloco for um elevador.
    fn elevator_at(&self, pos: BlockPos) -> Option<Elevator>;
    /// O bloco em `pos` sufoca quem estiver dentro dele (bloco sólido)?
    fn is_suffocating(&self, pos: BlockPos) -> bool;
    /// `y` está dentro da altura de construção do mundo?
    fn in_build_height(&self, y: i32) -> bool;
    /// O chunk de `pos` está carregado?
    fn is_loaded(&self, pos: BlockPos) -> bool;
}

/// Configuração, com os mesmos nomes e padrões do original (`elevatorid-synced.toml`).
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub same_color: bool,
    pub range: u32,
    pub activation_range: u32,
    pub precision_target: bool,
    pub mob_spawn: bool,
    pub reset_pitch_normal: bool,
    pub reset_pitch_directional: bool,
    pub use_xp: bool,
    pub xp_points_amount: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            same_color: false,
            range: 384,
            activation_range: 6,
            precision_target: true,
            mob_spawn: false,
            reset_pitch_normal: false,
            reset_pitch_directional: true,
            use_xp: false,
            xp_points_amount: 1,
        }
    }
}

impl Config {
    /// Limita os valores aos intervalos do original: range 3..=4064, activationRange 1..=64, XP >= 1.
    pub fn clamped(mut self) -> Self {
        self.range = self.range.clamp(3, 4064);
        self.activation_range = self.activation_range.clamp(1, 64);
        self.xp_points_amount = self.xp_points_amount.max(1);
        self
    }
}

impl Config {
    /// Lê o arquivo `config/elevatorid-synced.toml` do ElevatorMod: linhas `chave = valor` (as
    /// seções e os comentários `#` são ignorados). Chaves ausentes ou inválidas ficam no padrão e
    /// os valores são limitados aos intervalos do original.
    pub fn from_toml(text: &str) -> Self {
        let mut cfg = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.split('#').next().unwrap_or("").trim());
            let flag = || value.parse::<bool>().ok();
            let number = || value.parse::<u32>().ok();
            match key {
                "sameColor" => cfg.same_color = flag().unwrap_or(cfg.same_color),
                "range" => cfg.range = number().unwrap_or(cfg.range),
                "activationRange" => cfg.activation_range = number().unwrap_or(cfg.activation_range),
                "precisionTarget" => cfg.precision_target = flag().unwrap_or(cfg.precision_target),
                "mobSpawn" => cfg.mob_spawn = flag().unwrap_or(cfg.mob_spawn),
                "resetPitchNormal" => cfg.reset_pitch_normal = flag().unwrap_or(cfg.reset_pitch_normal),
                "resetPitchDirectional" => {
                    cfg.reset_pitch_directional = flag().unwrap_or(cfg.reset_pitch_directional)
                }
                "useXP" => cfg.use_xp = flag().unwrap_or(cfg.use_xp),
                "XPPointsAmount" => cfg.xp_points_amount = number().unwrap_or(cfg.xp_points_amount),
                _ => {}
            }
        }
        cfg.clamped()
    }
}

/// XP necessário para passar do nível `level` para o próximo (`Player.getXpNeededForNextLevel`).
pub fn xp_needed(level: i32) -> i32 {
    if level >= 30 {
        112 + (level - 30) * 9
    } else if level >= 15 {
        37 + (level - 15) * 5
    } else {
        7 + level * 2
    }
}

/// `Math.round(float)` do Java: arredonda para cima nos meios (`floor(x + 0.5)`).
pub fn java_round(x: f32) -> i32 {
    (x + 0.5).floor() as i32
}

/// Pontos de XP já ganhos no nível atual, como o original calcula
/// (`Math.round(experienceProgress * getXpNeededForNextLevel())`).
pub fn xp_progress_points(level: i32, progress: f32) -> i64 {
    i64::from(java_round(progress * xp_needed(level) as f32))
}

/// Canal da mensagem do cliente para o servidor (equivale ao `elevatorid:teleport_request`).
pub const CHANNEL: &str = "elevator:teleport_request";

/// Codifica o pedido de teleporte: seis inteiros de 32 bits (`from` e `to`), little-endian.
pub fn encode_request(from: BlockPos, to: BlockPos) -> [u8; 24] {
    let mut out = [0u8; 24];
    for (i, v) in [from.x, from.y, from.z, to.x, to.y, to.z].into_iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

/// Lê um pedido de teleporte; `None` se o tamanho não for o certo.
pub fn decode_request(data: &[u8]) -> Option<(BlockPos, BlockPos)> {
    if data.len() != 24 {
        return None;
    }
    let n = |i: usize| i32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap());
    Some((BlockPos::new(n(0), n(1), n(2)), BlockPos::new(n(3), n(4), n(5))))
}

/// Sentido do movimento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

impl Dir {
    fn step(self) -> i32 {
        match self {
            Dir::Up => 1,
            Dir::Down => -1,
        }
    }
}

/// O elevador está num lugar válido? Precisa de um bloco não sólido acima.
pub fn is_valid_pos(world: &dyn World, pos: BlockPos) -> bool {
    !world.is_suffocating(pos.above())
}

/// Elevador de origem: o jogador (parte de baixo) está num elevador ou até `activation_range - 1`
/// blocos acima dele. Devolve `None` se não houver, ou se o primeiro achado for inválido.
pub fn origin_elevator(world: &dyn World, player: BlockPos, cfg: &Config) -> Option<BlockPos> {
    let mut pos = player;
    for _ in 0..cfg.activation_range {
        if world.elevator_at(pos).is_some() {
            return if is_valid_pos(world, pos) { Some(pos) } else { None };
        }
        pos = pos.below();
    }
    None
}

/// Busca (lado do cliente) o elevador de destino na mesma coluna, no sentido pedido.
pub fn find_target(world: &dyn World, from: BlockPos, dir: Dir, cfg: &Config) -> Option<BlockPos> {
    let from_elevator = world.elevator_at(from)?;
    let mut to = from;
    loop {
        to = to.with_y(to.y + dir.step());
        if !world.in_build_height(to.y) || to.y.abs_diff(from.y) > cfg.range {
            return None;
        }
        if let Some(target) = world.elevator_at(to) {
            if is_valid_pos(world, to)
                && (!cfg.same_color || from_elevator.color == target.color)
            {
                return Some(to);
            }
        }
    }
}

/// Estado do jogador que o servidor precisa para validar um teleporte.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub alive: bool,
    pub creative: bool,
    pub block_pos: BlockPos,
    /// Pontos de XP já ganhos no nível atual: `round(progresso * xp_para_o_próximo_nível)`.
    pub xp_progress_points: i64,
    pub xp_level: u32,
}

/// Motivo de recusa de um pedido de teleporte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    PlayerDead,
    NotLoaded,
    TooFarFromOrigin,
    NotSameColumn,
    SameHeight,
    NotElevators,
    InvalidDestination,
    ColorMismatch,
    /// Falta XP; o jogo mostra a mensagem `elevatorid.message.missing_xp`.
    MissingXp,
}

/// Valida o pedido como o servidor do original faz (`isBadTeleportPacket`).
pub fn validate(
    world: &dyn World,
    player: &Player,
    from: BlockPos,
    to: BlockPos,
    cfg: &Config,
) -> Result<(), Reject> {
    if !player.alive {
        return Err(Reject::PlayerDead);
    }
    if !world.is_loaded(from) || !world.is_loaded(to) {
        return Err(Reject::NotLoaded);
    }
    if player.block_pos.dist_manhattan(from) > cfg.activation_range {
        return Err(Reject::TooFarFromOrigin);
    }
    if from.x != to.x || from.z != to.z {
        return Err(Reject::NotSameColumn);
    }
    if from.y == to.y {
        return Err(Reject::SameHeight);
    }
    let (Some(a), Some(b)) = (world.elevator_at(from), world.elevator_at(to)) else {
        return Err(Reject::NotElevators);
    };
    if !is_valid_pos(world, to) {
        return Err(Reject::InvalidDestination);
    }
    if cfg.same_color && a.color != b.color {
        return Err(Reject::ColorMismatch);
    }
    Ok(())
}

/// Quanto de XP cobrar. `Ok(0)` se nada é cobrado; `Err(MissingXp)` se não há XP suficiente.
pub fn xp_cost(player: &Player, cfg: &Config) -> Result<u32, Reject> {
    if !cfg.use_xp || player.creative {
        return Ok(0);
    }
    let cost = cfg.xp_points_amount;
    if player.xp_progress_points - i64::from(cost) >= 0 || player.xp_level > 0 {
        Ok(cost)
    } else {
        Err(Reject::MissingXp)
    }
}

/// Como mexer na rotação horizontal do jogador.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Yaw {
    /// Mantém o yaw atual do jogador.
    Keep,
    /// Define o yaw.
    Set(f32),
}

/// Como mexer na inclinação (pitch) do jogador.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pitch {
    Keep,
    Reset,
}

/// Onde colocar o jogador.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Destination {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: Yaw,
    pub pitch: Pitch,
    /// Depois do teleporte a velocidade é multiplicada por (1, 0, 1): zera a vertical.
    pub zero_vertical_velocity: bool,
}

/// Calcula o destino. `player_x`/`player_z` valem quando `precision_target` está desligado.
pub fn destination(
    to: BlockPos,
    target: &Elevator,
    player_x: f64,
    player_z: f64,
    cfg: &Config,
) -> Destination {
    let (x, z) = if cfg.precision_target {
        (f64::from(to.x) + 0.5, f64::from(to.z) + 0.5)
    } else {
        (player_x, player_z)
    };
    let y = f64::from(to.y) + target.support_height.max(0.0);
    let reset_pitch = if target.directional {
        cfg.reset_pitch_directional
    } else {
        cfg.reset_pitch_normal
    };
    Destination {
        x,
        y,
        z,
        yaw: if target.directional { Yaw::Set(target.facing.to_yaw()) } else { Yaw::Keep },
        pitch: if reset_pitch { Pitch::Reset } else { Pitch::Keep },
        zero_vertical_velocity: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        elevators: HashMap<BlockPos, Elevator>,
        solid: Vec<BlockPos>,
        unloaded: Vec<BlockPos>,
    }

    fn elev(color: DyeColor) -> Elevator {
        Elevator { color, directional: false, facing: Facing::South, support_height: 1.0 }
    }

    impl Fake {
        fn put(&mut self, p: BlockPos, c: DyeColor) {
            self.elevators.insert(p, elev(c));
        }
    }

    impl World for Fake {
        fn elevator_at(&self, p: BlockPos) -> Option<Elevator> {
            self.elevators.get(&p).copied()
        }
        fn is_suffocating(&self, p: BlockPos) -> bool {
            self.solid.contains(&p)
        }
        fn in_build_height(&self, y: i32) -> bool {
            (-64..320).contains(&y)
        }
        fn is_loaded(&self, p: BlockPos) -> bool {
            !self.unloaded.contains(&p)
        }
    }

    fn player(at: BlockPos) -> Player {
        Player { alive: true, creative: false, block_pos: at, xp_progress_points: 0, xp_level: 0 }
    }

    const A: BlockPos = BlockPos::new(0, 64, 0);
    const B: BlockPos = BlockPos::new(0, 70, 0);

    #[test]
    fn defaults_match_original() {
        let c = Config::default();
        assert_eq!((c.range, c.activation_range, c.xp_points_amount), (384, 6, 1));
        assert!(!c.same_color && !c.use_xp && !c.mob_spawn);
        assert!(c.precision_target && c.reset_pitch_directional && !c.reset_pitch_normal);
    }

    #[test]
    fn config_is_clamped() {
        let c = Config { range: 1, activation_range: 999, xp_points_amount: 0, ..Config::default() }.clamped();
        assert_eq!((c.range, c.activation_range, c.xp_points_amount), (3, 64, 1));
        assert_eq!(Config { range: 9999, ..Config::default() }.clamped().range, 4064);
    }

    #[test]
    fn jump_finds_elevator_above_and_sneak_finds_below() {
        let mut w = Fake::default();
        w.put(A, DyeColor::White);
        w.put(B, DyeColor::Red);
        let c = Config::default();
        assert_eq!(find_target(&w, A, Dir::Up, &c), Some(B));
        assert_eq!(find_target(&w, B, Dir::Down, &c), Some(A));
        assert_eq!(find_target(&w, B, Dir::Up, &c), None);
    }

    #[test]
    fn skips_elevator_with_solid_block_above() {
        let mut w = Fake::default();
        w.put(A, DyeColor::White);
        w.put(A.with_y(66), DyeColor::White);
        w.solid.push(A.with_y(67));
        w.put(B, DyeColor::White);
        assert_eq!(find_target(&w, A, Dir::Up, &Config::default()), Some(B));
    }

    #[test]
    fn same_color_skips_other_colors() {
        let mut w = Fake::default();
        w.put(A, DyeColor::White);
        w.put(A.with_y(66), DyeColor::Red);
        w.put(B, DyeColor::White);
        let c = Config { same_color: true, ..Config::default() };
        assert_eq!(find_target(&w, A, Dir::Up, &c), Some(B));
        assert_eq!(find_target(&w, A, Dir::Up, &Config::default()), Some(A.with_y(66)));
    }

    #[test]
    fn range_limits_the_search() {
        let mut w = Fake::default();
        w.put(A, DyeColor::White);
        w.put(B, DyeColor::White);
        let c = Config { range: 5, ..Config::default() };
        assert_eq!(find_target(&w, A, Dir::Up, &c), None);
        let c = Config { range: 6, ..Config::default() };
        assert_eq!(find_target(&w, A, Dir::Up, &c), Some(B));
    }

    #[test]
    fn search_stops_at_build_height() {
        let mut w = Fake::default();
        let top = BlockPos::new(0, 319, 0);
        w.put(top, DyeColor::White);
        assert_eq!(find_target(&w, top, Dir::Up, &Config::default()), None);
    }

    #[test]
    fn origin_is_found_up_to_activation_range_below() {
        let mut w = Fake::default();
        w.put(A, DyeColor::White);
        let c = Config::default();
        assert_eq!(origin_elevator(&w, A, &c), Some(A));
        assert_eq!(origin_elevator(&w, A.with_y(69), &c), Some(A)); // 5 acima, dentro do alcance 6
        assert_eq!(origin_elevator(&w, A.with_y(70), &c), None); // 6 acima, fora
    }

    #[test]
    fn origin_is_none_when_first_elevator_is_blocked() {
        let mut w = Fake::default();
        w.put(A, DyeColor::White);
        w.solid.push(A.above());
        assert_eq!(origin_elevator(&w, A, &Config::default()), None);
    }

    #[test]
    fn valid_request_passes() {
        let mut w = Fake::default();
        w.put(A, DyeColor::White);
        w.put(B, DyeColor::Blue);
        assert_eq!(validate(&w, &player(A), A, B, &Config::default()), Ok(()));
    }

    #[test]
    fn rejects_bad_requests() {
        let mut w = Fake::default();
        w.put(A, DyeColor::White);
        w.put(B, DyeColor::Blue);
        let c = Config::default();
        let dead = Player { alive: false, ..player(A) };
        assert_eq!(validate(&w, &dead, A, B, &c), Err(Reject::PlayerDead));
        assert_eq!(validate(&w, &player(A.with_y(80)), A, B, &c), Err(Reject::TooFarFromOrigin));
        assert_eq!(validate(&w, &player(A), A, BlockPos::new(1, 70, 0), &c), Err(Reject::NotSameColumn));
        assert_eq!(validate(&w, &player(A), A, A, &c), Err(Reject::SameHeight));
        assert_eq!(validate(&w, &player(A), A, B.with_y(75), &c), Err(Reject::NotElevators));
        let cs = Config { same_color: true, ..c.clone() };
        assert_eq!(validate(&w, &player(A), A, B, &cs), Err(Reject::ColorMismatch));
        w.solid.push(B.above());
        assert_eq!(validate(&w, &player(A), A, B, &c), Err(Reject::InvalidDestination));
        w.solid.clear();
        w.unloaded.push(B);
        assert_eq!(validate(&w, &player(A), A, B, &c), Err(Reject::NotLoaded));
    }

    #[test]
    fn xp_rules() {
        let off = Config::default();
        let on = Config { use_xp: true, xp_points_amount: 3, ..Config::default() };
        let p = player(A);
        assert_eq!(xp_cost(&p, &off), Ok(0));
        assert_eq!(xp_cost(&p, &on), Err(Reject::MissingXp));
        assert_eq!(xp_cost(&Player { xp_progress_points: 3, ..p }, &on), Ok(3));
        assert_eq!(xp_cost(&Player { xp_progress_points: 2, ..p }, &on), Err(Reject::MissingXp));
        assert_eq!(xp_cost(&Player { xp_level: 1, ..p }, &on), Ok(3));
        assert_eq!(xp_cost(&Player { creative: true, ..p }, &on), Ok(0));
    }

    #[test]
    fn destination_centers_and_stands_on_block() {
        let t = elev(DyeColor::White);
        let d = destination(B, &t, 0.9, 0.1, &Config::default());
        assert_eq!((d.x, d.y, d.z), (0.5, 71.0, 0.5));
        assert_eq!((d.yaw, d.pitch), (Yaw::Keep, Pitch::Keep));
        assert!(d.zero_vertical_velocity);
    }

    #[test]
    fn destination_keeps_player_xz_without_precision_target() {
        let c = Config { precision_target: false, ..Config::default() };
        let d = destination(B, &elev(DyeColor::White), 0.9, 0.1, &c);
        assert_eq!((d.x, d.z), (0.9, 0.1));
    }

    #[test]
    fn directional_sets_yaw_and_resets_pitch() {
        let t = Elevator { directional: true, facing: Facing::West, ..elev(DyeColor::White) };
        let d = destination(B, &t, 0.0, 0.0, &Config::default());
        assert_eq!((d.yaw, d.pitch), (Yaw::Set(90.0), Pitch::Reset));
        let c = Config { reset_pitch_directional: false, ..Config::default() };
        assert_eq!(destination(B, &t, 0.0, 0.0, &c).pitch, Pitch::Keep);
    }

    #[test]
    fn config_reads_the_original_toml() {
        let toml = "[General]\n#Should elevators have the same color\nsameColor = true\nrange = 100\n\
                    activationRange = 2\nuseXP = true # custo\nXPPointsAmount = 5\nprecisionTarget = false\n\
                    resetPitchNormal = true\nbogus = 1\n";
        let c = Config::from_toml(toml);
        assert!(c.same_color && c.use_xp && c.reset_pitch_normal && !c.precision_target);
        assert_eq!((c.range, c.activation_range, c.xp_points_amount), (100, 2, 5));
        assert!(c.reset_pitch_directional); // ausente: fica no padrão
    }

    #[test]
    fn config_toml_values_are_clamped_and_bad_values_ignored() {
        let c = Config::from_toml("range = 1\nactivationRange = 500\nXPPointsAmount = 0\nsameColor = maybe\n");
        assert_eq!((c.range, c.activation_range, c.xp_points_amount), (3, 64, 1));
        assert!(!c.same_color);
        assert_eq!(Config::from_toml(""), Config::default());
    }

    #[test]
    fn xp_needed_matches_minecraft() {
        assert_eq!(xp_needed(0), 7);
        assert_eq!(xp_needed(14), 35);
        assert_eq!(xp_needed(15), 37);
        assert_eq!(xp_needed(29), 107);
        assert_eq!(xp_needed(30), 112);
        assert_eq!(xp_needed(40), 202);
    }

    #[test]
    fn java_round_rounds_halves_up() {
        assert_eq!(java_round(3.5), 4);
        assert_eq!(java_round(2.5), 3);
        assert_eq!(java_round(2.4), 2);
        assert_eq!(java_round(-0.5), 0);
        assert_eq!(java_round(-1.5), -1);
    }

    #[test]
    fn xp_progress_points_follow_the_original_formula() {
        assert_eq!(xp_progress_points(0, 0.0), 0);
        assert_eq!(xp_progress_points(0, 0.5), 4); // 0.5 * 7 = 3.5, Math.round dá 4
        assert_eq!(xp_progress_points(10, 0.5), 14); // 0.5 * 27 = 13.5
        assert_eq!(xp_progress_points(20, 0.25), 16); // 0.25 * 62 = 15.5
    }

    #[test]
    fn requests_round_trip() {
        let (a, b) = (BlockPos::new(-5, 64, 300), BlockPos::new(-5, -60, 300));
        assert_eq!(decode_request(&encode_request(a, b)), Some((a, b)));
        assert_eq!(decode_request(&[0; 23]), None);
        assert_eq!(decode_request(&[0; 25]), None);
    }

    #[test]
    fn block_names_map_to_colors() {
        assert_eq!(DyeColor::from_block_name("elevatorid:elevator_white"), Some(DyeColor::White));
        assert_eq!(DyeColor::from_block_name("elevatorid:elevator_light_blue"), Some(DyeColor::LightBlue));
        assert_eq!(DyeColor::from_block_name("elevatorid:elevator_light_gray"), Some(DyeColor::LightGray));
        assert_eq!(DyeColor::from_block_name("elevatorid:elevator_rainbow"), None);
        assert_eq!(DyeColor::from_block_name("minecraft:stone"), None);
    }

    #[test]
    fn facing_yaws_match_minecraft() {
        assert_eq!(Facing::South.to_yaw(), 0.0);
        assert_eq!(Facing::West.to_yaw(), 90.0);
        assert_eq!(Facing::North.to_yaw(), 180.0);
        assert_eq!(Facing::East.to_yaw(), 270.0);
    }
}
