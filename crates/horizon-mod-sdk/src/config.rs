//! A mod's settings in a small text file (`key = value` lines, `#` comments)
//! kept with capability `storage`, with typed reads that fall back to a
//! default and stay inside a range, so a hand-edited file can't break the
//! mod. (Mekanism's TOML configs map onto this: one file per config, a key
//! per entry.)
//!
//! ```
//! use horizon_mod_sdk::config::Config;
//! let c = Config::parse("# tuning\nsmelt_ticks = 400\nloud = true\nname = \"Alex\"\n");
//! assert_eq!(c.get_i64("smelt_ticks", 200, 1, 10_000), 400);
//! assert_eq!(c.get_i64("missing", 200, 1, 10_000), 200);
//! assert_eq!(c.get_i64("smelt_ticks", 200, 1, 100), 100);
//! assert!(c.get_bool("loud", false));
//! assert_eq!(c.get_str("name", "x"), "Alex");
//! ```

/// The entries of one config file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    entries: Vec<(String, String)>,
}

impl Config {
    /// Reads `key = value` lines; blank lines and `#` comments are skipped, a
    /// string may be quoted, and a later line wins.
    pub fn parse(text: &str) -> Self {
        let mut c = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(v);
            c.set(k.trim(), v);
        }
        c
    }
    /// The file's text, one line an entry.
    pub fn to_text(&self) -> String {
        self.entries.iter().map(|(k, v)| format!("{k} = {v}\n")).collect()
    }
    pub fn set(&mut self, key: &str, value: &str) {
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = value.to_owned(),
            None => self.entries.push((key.to_owned(), value.to_owned())),
        }
    }
    fn raw(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    /// A whole number, `default` when missing or not one, cut to `min..=max`.
    pub fn get_i64(&self, key: &str, default: i64, min: i64, max: i64) -> i64 {
        self.raw(key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
            .clamp(min, max)
    }
    /// A number, `default` when missing, not a number or not finite.
    pub fn get_f64(&self, key: &str, default: f64, min: f64, max: f64) -> f64 {
        self.raw(key)
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(default)
            .clamp(min, max)
    }
    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        match self.raw(key) {
            Some("true") => true,
            Some("false") => false,
            _ => default,
        }
    }
    pub fn get_str<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.raw(key).unwrap_or(default)
    }
    /// The file `name` of this mod's storage; empty when there is none.
    pub fn load(name: &str) -> Self {
        crate::storage::get(name)
            .map(|b| Self::parse(&String::from_utf8_lossy(&b)))
            .unwrap_or_default()
    }
    /// Writes the file `name` of this mod's storage.
    pub fn save(&self, name: &str) {
        let _ = crate::storage::set(name, self.to_text().as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_reads_default_and_clamp() {
        let c = Config::parse("a = 5\nb = nope\nc = 2.5\nd = inf\ne = true\n# x = 1\nf = \"hi there\"\na = 7\n");
        assert_eq!(c.get_i64("a", 0, 0, 10), 7);
        assert_eq!(c.get_i64("b", 3, 0, 10), 3);
        assert_eq!(c.get_f64("c", 0.0, 0.0, 1.0), 1.0);
        assert_eq!(c.get_f64("d", 0.5, 0.0, 1.0), 0.5);
        assert!(c.get_bool("e", false));
        assert!(!c.get_bool("missing", false));
        assert_eq!(c.get_str("f", ""), "hi there");
        assert_eq!(c.get_i64("x", 9, 0, 10), 9);
    }
    #[test]
    fn text_round_trips() {
        let mut c = Config::default();
        c.set("rate", "30");
        c.set("on", "true");
        c.set("rate", "40");
        assert_eq!(c.to_text(), "rate = 40\non = true\n");
        assert_eq!(Config::parse(&c.to_text()), c);
    }
}
