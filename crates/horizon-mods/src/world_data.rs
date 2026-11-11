//! The data mods keep in the world (FORK-36, capability `world`'s `world-data`): byte values under `<mod id>:<path>` keys, saved with
//! the world in [`FILE`] (what Mekanism keeps in `SavedData`: frequencies, block owners, security). The rules of a key and the limits
//! are here, with the file's format; the runtime holds each mod's values during calls and the game reads and writes the file.
use std::collections::BTreeMap;

/// Where the values are, inside the world's folder.
pub const FILE: &str = "data/horizon/mod_world_data.bin";
/// The most bytes a value holds.
pub const MAX_VALUE: usize = 256 * 1024;
/// The most values a mod keeps.
pub const MAX_VALUES: usize = 64;
/// The most bytes of a key's path (after `<mod id>:`).
pub const MAX_PATH: usize = 128;

const MAGIC: &[u8; 4] = b"HWD1";

/// Whether `key` is one of mod `mod_id`'s: `<mod id>:<path>`, the path 1 to [`MAX_PATH`] bytes of `a-z 0-9 _ . / -`, with no empty,
/// `.` or `..` segment.
pub fn key_ok(mod_id: &str, key: &str) -> bool {
    let Some(path) = key.strip_prefix(mod_id).and_then(|k| k.strip_prefix(':')) else {
        return false;
    };
    (1..=MAX_PATH).contains(&path.len())
        && path.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
        && path.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

/// Sets `key` of mod `mod_id` to `value` in `values` (that mod's): an error for a key that is not the mod's, a value over
/// [`MAX_VALUE`], or a new key past [`MAX_VALUES`].
pub fn put(values: &mut BTreeMap<String, Vec<u8>>, mod_id: &str, key: &str, value: Vec<u8>) -> Result<(), String> {
    if !key_ok(mod_id, key) {
        return Err(format!("{key:?} is not {mod_id}:<path> (a-z 0-9 _ . / -, 1 to {MAX_PATH} bytes)"));
    }
    if value.len() > MAX_VALUE {
        return Err(format!("a value holds at most {} KiB", MAX_VALUE / 1024));
    }
    if !values.contains_key(key) && values.len() >= MAX_VALUES {
        return Err(format!("a mod keeps at most {MAX_VALUES} values"));
    }
    values.insert(key.to_owned(), value);
    Ok(())
}

/// The keys of `values` that start with `prefix`, in order.
pub fn keys(values: &BTreeMap<String, Vec<u8>>, prefix: &str) -> Vec<String> {
    values.keys().filter(|k| k.starts_with(prefix)).cloned().collect()
}

/// The values of mod `mod_id` among every mod's.
pub fn of_mod(all: &BTreeMap<String, Vec<u8>>, mod_id: &str) -> BTreeMap<String, Vec<u8>> {
    let prefix = format!("{mod_id}:");
    all.iter().filter(|(k, _)| k.starts_with(&prefix)).map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Puts mod `mod_id`'s values back among every mod's, in place of what it had.
pub fn replace_mod(all: &mut BTreeMap<String, Vec<u8>>, mod_id: &str, values: &BTreeMap<String, Vec<u8>>) {
    let prefix = format!("{mod_id}:");
    all.retain(|k, _| !k.starts_with(&prefix));
    all.extend(values.iter().map(|(k, v)| (k.clone(), v.clone())));
}

/// The file of `all`: `HWD1`, the count, then each key (2-byte length, UTF-8) and value (4-byte length, bytes), little-endian.
pub fn encode(all: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend((all.len() as u32).to_le_bytes());
    for (k, v) in all {
        out.extend((k.len() as u16).to_le_bytes());
        out.extend(k.as_bytes());
        out.extend((v.len() as u32).to_le_bytes());
        out.extend(v);
    }
    out
}

/// The values a file holds; none for a file that is not one (a damaged file is not half read).
pub fn decode(bytes: &[u8]) -> Option<BTreeMap<String, Vec<u8>>> {
    let mut at = 0usize;
    let mut take = |n: usize| {
        let s = bytes.get(at..at + n)?;
        at += n;
        Some(s)
    };
    if take(4)? != MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let mut all = BTreeMap::new();
    for _ in 0..count {
        let klen = u16::from_le_bytes(take(2)?.try_into().ok()?) as usize;
        let key = String::from_utf8(take(klen)?.to_vec()).ok()?;
        let vlen = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        let value = take(vlen)?.to_vec();
        all.insert(key, value);
    }
    (at == bytes.len()).then_some(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_the_mods_own_with_a_plain_path() {
        assert!(key_ok("mekanism", "mekanism:frequencies/teleporter"));
        assert!(key_ok("mekanism", "mekanism:owners"));
        for bad in ["other:owners", "mekanism:", "mekanism", "mekanism:Owners", "mekanism:a//b", "mekanism:../x", "mekanism:a b", "mekanismx:a"] {
            assert!(!key_ok("mekanism", bad), "{bad}");
        }
        assert!(key_ok("m", &format!("m:{}", "a".repeat(128))));
        assert!(!key_ok("m", &format!("m:{}", "a".repeat(129))));
    }

    #[test]
    fn values_have_a_size_and_a_count_limit() {
        let mut v = BTreeMap::new();
        assert!(put(&mut v, "m", "m:big", vec![0; MAX_VALUE]).is_ok());
        assert!(put(&mut v, "m", "m:bigger", vec![0; MAX_VALUE + 1]).unwrap_err().contains("256 KiB"));
        assert!(put(&mut v, "m", "x:y", vec![]).is_err());
        for i in 1..MAX_VALUES {
            put(&mut v, "m", &format!("m:k{i}"), vec![i as u8]).unwrap();
        }
        assert_eq!(v.len(), 64);
        assert!(put(&mut v, "m", "m:one_more", vec![]).unwrap_err().contains("64 values"));
        // Changing a key it has is not a new one.
        assert!(put(&mut v, "m", "m:k1", vec![9]).is_ok());
        assert_eq!(keys(&v, "m:k1"), ["m:k1", "m:k10", "m:k11", "m:k12", "m:k13", "m:k14", "m:k15", "m:k16", "m:k17", "m:k18", "m:k19"]);
    }

    #[test]
    fn the_file_keeps_every_mods_values_and_a_mod_replaces_only_its_own() {
        let mut all = BTreeMap::new();
        all.insert("a:x".to_owned(), vec![1, 2]);
        all.insert("b:y".to_owned(), vec![]);
        all.insert("b:z".to_owned(), vec![3; 300]);
        let bytes = encode(&all);
        assert_eq!(decode(&bytes), Some(all.clone()));
        assert_eq!(decode(&bytes[..bytes.len() - 1]), None, "a cut file");
        assert_eq!(decode(b"nope"), None);
        let mut b = of_mod(&all, "b");
        assert_eq!(b.len(), 2);
        b.remove("b:y");
        b.insert("b:w".into(), vec![7]);
        replace_mod(&mut all, "b", &b);
        assert_eq!(all.keys().collect::<Vec<_>>(), ["a:x", "b:w", "b:z"]);
    }
}
