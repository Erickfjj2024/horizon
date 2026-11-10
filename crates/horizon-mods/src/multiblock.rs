//! Multiblock structures (API block 10): the blocks of one group that touch
//! through faces form a structure candidate, and a mod decides whether it is a
//! turbine, a tank or nothing. Pure, like the other tables: the game and the
//! test harness collect the members and call the mod when the structures change.
use std::collections::{BTreeMap, HashMap, HashSet};

/// One block of a group, with its state (`mymod:casing[facing=north]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub pos: [i32; 3],
    pub state: String,
}

/// Blocks of a group that touch: their members in position order, and the
/// least and greatest corner of the box around them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Structure {
    pub members: Vec<Member>,
    pub min: [i32; 3],
    pub max: [i32; 3],
}

const FACES: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];

/// The structures of `members` (one group in one dimension), ordered by their
/// least corner.
pub fn structures(members: &[Member]) -> Vec<Structure> {
    let by_pos: HashMap<[i32; 3], &Member> = members.iter().map(|m| (m.pos, m)).collect();
    let mut positions: Vec<[i32; 3]> = by_pos.keys().copied().collect();
    positions.sort();
    let mut seen: HashSet<[i32; 3]> = HashSet::new();
    let mut out = Vec::new();
    for start in positions {
        if !seen.insert(start) {
            continue;
        }
        let mut found = vec![start];
        let mut next = 0;
        while next < found.len() {
            let p = found[next];
            next += 1;
            for d in FACES {
                let q = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                if by_pos.contains_key(&q) && seen.insert(q) {
                    found.push(q);
                }
            }
        }
        found.sort();
        let mut min = found[0];
        let mut max = found[0];
        for p in &found {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        out.push(Structure {
            members: found.iter().map(|p| by_pos[p].clone()).collect(),
            min,
            max,
        });
    }
    out.sort_by_key(|s| s.min);
    out
}

/// What the game remembers of the last structures it told a mod about, by
/// (dimension, group), to tell it again only when they change.
#[derive(Debug, Default)]
pub struct Seen(BTreeMap<(String, String), Vec<Structure>>);

impl Seen {
    /// Whether `now` differs from what was told for this dimension and group
    /// (the first call with none counts as unchanged); remembers `now`.
    pub fn changed(&mut self, dimension: &str, group: &str, now: &[Structure]) -> bool {
        let key = (dimension.to_owned(), group.to_owned());
        let was = self.0.get(&key).map_or(&[][..], Vec::as_slice);
        let changed = was != now;
        if changed {
            if now.is_empty() {
                self.0.remove(&key);
            } else {
                self.0.insert(key, now.to_vec());
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(x: i32, y: i32, z: i32) -> Member {
        Member {
            pos: [x, y, z],
            state: "mymod:casing".into(),
        }
    }
    /// A hollow 3x3x3 shell, as a turbine's casing would be.
    fn shell() -> Vec<Member> {
        let mut v = Vec::new();
        for x in 0..3 {
            for y in 0..3 {
                for z in 0..3 {
                    if !(x == 1 && y == 1 && z == 1) {
                        v.push(m(x, y, z));
                    }
                }
            }
        }
        v
    }
    #[test]
    fn a_hollow_cuboid_is_one_structure_with_its_box() {
        let s = structures(&shell());
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].min, s[0].max, s[0].members.len()), ([0, 0, 0], [2, 2, 2], 26));
    }
    #[test]
    fn blocks_that_do_not_touch_are_apart_and_diagonals_do_not_join() {
        let s = structures(&[m(0, 0, 0), m(1, 0, 0), m(5, 0, 0), m(6, 1, 0), m(2, 1, 1)]);
        let boxes: Vec<_> = s.iter().map(|s| (s.min, s.members.len())).collect();
        assert_eq!(boxes, [([0, 0, 0], 2), ([2, 1, 1], 1), ([5, 0, 0], 1), ([6, 1, 0], 1)]);
    }
    #[test]
    fn taking_a_block_out_splits_and_seen_tells_once() {
        let mut seen = Seen::default();
        let mut all = vec![m(0, 0, 0), m(1, 0, 0), m(2, 0, 0)];
        let now = structures(&all);
        assert!(seen.changed("d", "g", &now));
        assert!(!seen.changed("d", "g", &now));
        all.remove(1);
        let now = structures(&all);
        assert_eq!(now.len(), 2);
        assert!(seen.changed("d", "g", &now));
        // Another dimension or group is its own.
        assert!(seen.changed("e", "g", &now));
        // Everything gone is a change, once.
        assert!(seen.changed("d", "g", &[]));
        assert!(!seen.changed("d", "g", &[]));
    }
    #[test]
    fn a_changed_state_is_a_change() {
        let mut seen = Seen::default();
        let mut a = vec![m(0, 0, 0)];
        assert!(seen.changed("d", "g", &structures(&a)));
        a[0].state = "mymod:valve".into();
        assert!(seen.changed("d", "g", &structures(&a)));
    }
}
