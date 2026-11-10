//! Data components of an item stack (`ItemStack::components` is their SNBT):
//! reads and writes the top-level entries of that compound without touching
//! the values you leave alone, so a mod can keep `mymod:energy` on a stack
//! next to whatever else the game or another mod put there.
//!
//! ```
//! use horizon_mod_sdk::components::Components;
//! let mut c = Components::parse(r#"{"minecraft:damage":3,"mymod:energy":1500L}"#).unwrap();
//! assert_eq!(c.get_long("mymod:energy"), Some(1500));
//! c.set_long("mymod:energy", 1400);
//! assert_eq!(c.get_long("minecraft:damage"), Some(3));
//! assert_eq!(c.to_snbt(), r#"{"minecraft:damage":3,"mymod:energy":1400L}"#);
//! ```

/// The top-level entries of a components compound, each value as raw SNBT.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Components {
    entries: Vec<(String, String)>,
}

impl Components {
    /// No components.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads `{key:value,...}`; an empty text is no components. `None` when
    /// the text is not a compound.
    pub fn parse(snbt: &str) -> Option<Self> {
        let s = snbt.trim();
        if s.is_empty() {
            return Some(Self::new());
        }
        let b = s.as_bytes();
        if b.first() != Some(&b'{') || b.last() != Some(&b'}') {
            return None;
        }
        let inner = &s[1..s.len() - 1];
        let mut entries = Vec::new();
        for part in split_top_level(inner, ',')? {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let colon = find_key_end(part)?;
            let key = unquote(part[..colon].trim());
            let value = part[colon + 1..].trim().to_owned();
            entries.push((key, value));
        }
        Some(Self { entries })
    }

    /// The compound as SNBT, keys quoted, in the order set.
    pub fn to_snbt(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = self
            .entries
            .iter()
            .map(|(k, v)| format!("\"{}\":{v}", k.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();
        format!("{{{}}}", parts.join(","))
    }

    /// The raw SNBT of a component.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
    pub fn get_long(&self, key: &str) -> Option<i64> {
        let v = self.get(key)?;
        let v = v.trim_end_matches(|c: char| matches!(c, 'L' | 'l' | 'b' | 'B' | 's' | 'S'));
        v.parse().ok()
    }
    pub fn get_double(&self, key: &str) -> Option<f64> {
        let v = self.get(key)?;
        let v = v.trim_end_matches(|c: char| matches!(c, 'd' | 'D' | 'f' | 'F'));
        v.parse().ok()
    }
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            "true" | "1b" => Some(true),
            "false" | "0b" => Some(false),
            _ => None,
        }
    }
    pub fn get_string(&self, key: &str) -> Option<String> {
        let v = self.get(key)?;
        (v.starts_with('"') || v.starts_with('\'')).then(|| unquote(v))
    }

    /// Sets a component to raw SNBT (`"{side:\"left\"}"`).
    pub fn set(&mut self, key: &str, snbt: &str) {
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = snbt.to_owned(),
            None => self.entries.push((key.to_owned(), snbt.to_owned())),
        }
    }
    pub fn set_long(&mut self, key: &str, v: i64) {
        self.set(key, &format!("{v}L"));
    }
    pub fn set_double(&mut self, key: &str, v: f64) {
        self.set(key, &format!("{v:?}d"));
    }
    pub fn set_bool(&mut self, key: &str, v: bool) {
        self.set(key, if v { "1b" } else { "0b" });
    }
    pub fn set_string(&mut self, key: &str, v: &str) {
        self.set(key, &format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"")));
    }
    /// Removes a component; true when it was there.
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(k, _)| k != key);
        before != self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Splits at `sep` outside strings and brackets; `None` for unbalanced text.
fn split_top_level(s: &str, sep: char) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
        if depth < 0 {
            return None;
        }
    }
    if depth != 0 || quote.is_some() {
        return None;
    }
    out.push(&s[start..]);
    Some(out)
}

/// The index of the `:` that ends the key of an entry.
fn find_key_end(entry: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in entry.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == ':' {
            return Some(i);
        }
    }
    None
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    let quoted = s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')));
    if !quoted {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut escaped = false;
    for c in s[1..s.len() - 1].chars() {
        if escaped {
            out.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_without_touching_the_rest() {
        let mut c = Components::parse(
            r#"{"minecraft:damage":3, "mymod:energy":1500L, "mymod:config":{side:"left,right",on:1b,list:[1,2]}}"#,
        )
        .unwrap();
        assert_eq!(c.get_long("mymod:energy"), Some(1500));
        assert_eq!(c.get_long("minecraft:damage"), Some(3));
        assert_eq!(c.get("mymod:config"), Some(r#"{side:"left,right",on:1b,list:[1,2]}"#));
        c.set_long("mymod:energy", 7);
        c.set_bool("mymod:lit", true);
        c.set_string("mymod:owner", "Al\"ex");
        let again = Components::parse(&c.to_snbt()).unwrap();
        assert_eq!(again, c);
        assert_eq!(again.get_long("mymod:energy"), Some(7));
        assert_eq!(again.get_bool("mymod:lit"), Some(true));
        assert_eq!(again.get_string("mymod:owner").as_deref(), Some("Al\"ex"));
        assert_eq!(again.get("mymod:config"), c.get("mymod:config"));
    }

    #[test]
    fn empty_and_wrong_text() {
        assert_eq!(Components::parse("").unwrap(), Components::new());
        assert_eq!(Components::new().to_snbt(), "");
        assert!(Components::parse("[1]").is_none());
        assert!(Components::parse("{a:{}").is_none());
    }

    #[test]
    fn removes() {
        let mut c = Components::parse("{a:1,b:2}").unwrap();
        assert!(c.remove("a"));
        assert!(!c.remove("a"));
        assert_eq!(c.to_snbt(), r#"{"b":2}"#);
    }
}
