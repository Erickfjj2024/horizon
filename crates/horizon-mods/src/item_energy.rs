//! Energy in items any mod can read and move (round 4, item 5): NeoForge's `IEnergyStorage` item capability (Forge Energy, FE). An
//! item declares how much it holds and how fast in its content (`"energy": {"capacity", "max_insert", "max_extract"}`); what it holds
//! is the component `horizon:energy` (a long) of the stack, so a stack carries it wherever it goes. Pure: the runtime answers
//! `recipe-types.item-energy`, `item-energy-insert` and `item-energy-extract` from here.
use serde::Deserialize;

/// The component that holds the energy, in FE.
pub const COMPONENT: &str = "horizon:energy";

/// How much an item holds and how much a call may move into or out of it (`EnergyStorage(capacity, maxReceive, maxExtract)`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnergySpec {
    pub capacity: u64,
    #[serde(default = "unlimited")]
    pub max_insert: u64,
    #[serde(default = "unlimited")]
    pub max_extract: u64,
}

fn unlimited() -> u64 {
    u64::MAX
}

/// The items that hold energy, every mod's, by id: the game (and a test script) adds them from the content it loads.
static TABLE: std::sync::RwLock<Option<std::collections::HashMap<String, EnergySpec>>> = std::sync::RwLock::new(None);

/// Adds the items that hold energy (from the mods' content).
pub fn register(items: impl IntoIterator<Item = (String, EnergySpec)>) {
    if let Ok(mut t) = TABLE.write() {
        t.get_or_insert_with(Default::default).extend(items);
    }
}

/// How much item `item` holds and how fast, if it holds energy.
pub fn spec(item: &str) -> Option<EnergySpec> {
    TABLE.read().ok()?.as_ref()?.get(item).copied()
}

/// The energy the components hold (0 when they have none or it does not read).
pub fn stored(components: &str) -> u64 {
    let Some(at) = find_key(components) else { return 0 };
    let rest = components[at..].trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

/// Where the value of the component starts, after its key and colon.
fn find_key(components: &str) -> Option<usize> {
    for key in [format!("\"{COMPONENT}\""), format!("'{COMPONENT}'"), COMPONENT.to_owned()] {
        let mut from = 0;
        while let Some(i) = components[from..].find(&key) {
            let start = from + i;
            let end = start + key.len();
            // A whole key: after `{` or `,`, and followed by `:`.
            let before = components[..start].trim_end().chars().last();
            let after = components[end..].trim_start();
            if matches!(before, Some('{') | Some(',')) && after.starts_with(':') {
                return Some(components.len() - after.len() + 1);
            }
            from = end;
        }
    }
    None
}

/// The components with the energy set to `energy` (the component added when missing; `{}` made when empty).
pub fn with_stored(components: &str, energy: u64) -> String {
    let value = format!("{energy}L");
    if let Some(at) = find_key(components) {
        let rest = &components[at..];
        let lead = rest.len() - rest.trim_start().len();
        let len = rest.trim_start().chars().take_while(|c| c.is_ascii_digit() || matches!(c, 'L' | 'l' | '-')).count();
        let mut out = components.to_owned();
        out.replace_range(at + lead..at + lead + len, &value);
        return out;
    }
    let body = components.trim();
    let inner = body.strip_prefix('{').and_then(|b| b.strip_suffix('}')).unwrap_or("").trim();
    if inner.is_empty() { format!("{{\"{COMPONENT}\":{value}}}") } else { format!("{{{inner},\"{COMPONENT}\":{value}}}") }
}

/// `IEnergyStorage.receiveEnergy`: up to `amount` goes in, as much as the room and `max_insert` allow. The energy taken and the
/// components after (unchanged when `simulate`).
pub fn insert(spec: &EnergySpec, components: &str, amount: u64, simulate: bool) -> (u64, String) {
    let have = stored(components).min(spec.capacity);
    let taken = amount.min(spec.max_insert).min(spec.capacity - have);
    let after = if simulate || taken == 0 { components.to_owned() } else { with_stored(components, have + taken) };
    (taken, after)
}

/// `IEnergyStorage.extractEnergy`: up to `amount` comes out, as much as it holds and `max_extract` allow.
pub fn extract(spec: &EnergySpec, components: &str, amount: u64, simulate: bool) -> (u64, String) {
    let have = stored(components).min(spec.capacity);
    let given = amount.min(spec.max_extract).min(have);
    let after = if simulate || given == 0 { components.to_owned() } else { with_stored(components, have - given) };
    (given, after)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BATTERY: EnergySpec = EnergySpec { capacity: 1000, max_insert: 100, max_extract: 50 };

    #[test]
    fn the_energy_is_read_and_written_in_the_components() {
        assert_eq!(stored(""), 0);
        assert_eq!(stored("{\"horizon:energy\":250L}"), 250);
        assert_eq!(stored("{\"minecraft:damage\":3, 'horizon:energy': 7L}"), 7);
        // A key that only contains the name is not it.
        assert_eq!(stored("{\"mymod:horizon:energy_old\":5L}"), 0);
        assert_eq!(with_stored("", 40), "{\"horizon:energy\":40L}");
        assert_eq!(with_stored("{\"minecraft:damage\":3}", 40), "{\"minecraft:damage\":3,\"horizon:energy\":40L}");
        assert_eq!(with_stored("{\"horizon:energy\":250L,\"minecraft:damage\":3}", 9), "{\"horizon:energy\":9L,\"minecraft:damage\":3}");
    }

    #[test]
    fn charging_and_draining_keep_to_the_room_the_charge_and_the_rates() {
        // 100 at most a call goes in; the room limits it near full.
        assert_eq!(insert(&BATTERY, "", 500, false), (100, "{\"horizon:energy\":100L}".to_owned()));
        assert_eq!(insert(&BATTERY, "{\"horizon:energy\":950L}", 500, false).0, 50);
        assert_eq!(insert(&BATTERY, "{\"horizon:energy\":1000L}", 500, false), (0, "{\"horizon:energy\":1000L}".to_owned()));
        // A simulation answers without changing it.
        assert_eq!(insert(&BATTERY, "", 30, true), (30, String::new()));
        // 50 at most a call comes out, and no more than it holds.
        assert_eq!(extract(&BATTERY, "{\"horizon:energy\":120L}", 500, false), (50, "{\"horizon:energy\":70L}".to_owned()));
        assert_eq!(extract(&BATTERY, "{\"horizon:energy\":20L}", 500, false).0, 20);
        assert_eq!(extract(&BATTERY, "", 500, false), (0, String::new()));
    }
}
