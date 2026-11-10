//! Fluids and chemicals moving between neighbouring block entities' tanks
//! (API block 4). Pure, like [`crate::energy`]: the game and the test harness
//! build the nodes and write the tanks back.
use crate::FlowSide;
use crate::energy::FACES;

/// One tank, with what the block's mod declared and what it holds now.
#[derive(Clone, Debug, PartialEq)]
pub struct TankState {
    pub kind: String,
    pub capacity: u64,
    pub max_rate: u64,
    pub faces: [FlowSide; 6],
    /// Empty when the tank is empty.
    pub substance: String,
    pub amount: u64,
    /// The only substances it takes in from outside (`world.set-tank-filter`); empty: any of its kind.
    pub accepts: Vec<String>,
}

impl TankState {
    /// Whether it takes `substance` in now: it is empty or holds the same, and the filter lets it.
    pub fn takes_in(&self, substance: &str) -> bool {
        (self.amount == 0 || self.substance == substance) && (self.accepts.is_empty() || self.accepts.iter().any(|a| a == substance))
    }
}

/// One block with tanks.
#[derive(Clone, Debug, PartialEq)]
pub struct TankNode {
    pub pos: [i32; 3],
    pub tanks: Vec<TankState>,
}

fn gives(s: FlowSide) -> bool {
    matches!(s, FlowSide::Output | FlowSide::Both)
}
fn takes(s: FlowSide) -> bool {
    matches!(s, FlowSide::Input | FlowSide::Both)
}

/// One tick of transfers, in position order, each block's tanks in order and
/// their faces in the order of [`FACES`]; a tank gives to the first tank of
/// the neighbour that accepts, and gives and takes at most its `max_rate` a
/// tick in all faces together.
pub fn transfer(nodes: &mut [TankNode]) {
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| nodes[i].pos);
    let at: std::collections::HashMap<[i32; 3], usize> =
        nodes.iter().enumerate().map(|(i, n)| (n.pos, i)).collect();
    let mut given: Vec<Vec<u64>> = nodes.iter().map(|n| vec![0; n.tanks.len()]).collect();
    let mut taken = given.clone();
    for &s in &order {
        for ti in 0..nodes[s].tanks.len() {
            for (face, d) in FACES.iter().enumerate() {
                if !gives(nodes[s].tanks[ti].faces[face]) || nodes[s].tanks[ti].amount == 0 {
                    continue;
                }
                let to = [
                    nodes[s].pos[0] + d[0],
                    nodes[s].pos[1] + d[1],
                    nodes[s].pos[2] + d[2],
                ];
                let Some(&t) = at.get(&to) else { continue };
                for di in 0..nodes[t].tanks.len() {
                    let (a, b) = if s == t {
                        continue;
                    } else if s < t {
                        let (l, r) = nodes.split_at_mut(t);
                        (&mut l[s].tanks[ti], &mut r[0].tanks[di])
                    } else {
                        let (l, r) = nodes.split_at_mut(s);
                        (&mut r[0].tanks[ti], &mut l[t].tanks[di])
                    };
                    if a.kind != b.kind || !takes(b.faces[face ^ 1]) || !b.takes_in(&a.substance) {
                        continue;
                    }
                    let amount = a
                        .amount
                        .min(a.max_rate.saturating_sub(given[s][ti]))
                        .min(b.max_rate.saturating_sub(taken[t][di]))
                        .min(b.capacity.saturating_sub(b.amount));
                    if amount == 0 {
                        continue;
                    }
                    if b.amount == 0 {
                        b.substance.clone_from(&a.substance);
                    }
                    a.amount -= amount;
                    b.amount += amount;
                    if a.amount == 0 {
                        a.substance.clear();
                    }
                    given[s][ti] += amount;
                    taken[t][di] += amount;
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FlowSide::{Input, Output};

    fn tank(kind: &str, substance: &str, amount: u64, side: FlowSide) -> TankState {
        TankState {
            kind: kind.into(),
            capacity: 100,
            max_rate: 30,
            faces: [side; 6],
            substance: substance.into(),
            amount,
            accepts: Vec::new(),
        }
    }
    #[test]
    fn a_filtered_tank_takes_only_what_it_lists() {
        let mut oxygen = tank("gas", "mymod:oxygen", 50, Output);
        oxygen.accepts = Vec::new();
        let mut picky = tank("gas", "", 0, Input);
        picky.accepts = vec!["mymod:osmium".into()];
        let mut nodes = vec![node(0, oxygen.clone()), node(1, picky.clone())];
        transfer(&mut nodes);
        assert_eq!(nodes[1].tanks[0].amount, 0, "oxygen is not on its list");
        let mut osmium = oxygen;
        osmium.substance = "mymod:osmium".into();
        let mut nodes = vec![node(0, osmium), node(1, picky)];
        transfer(&mut nodes);
        assert!(nodes[1].tanks[0].amount > 0);
    }

    fn node(x: i32, t: TankState) -> TankNode {
        TankNode {
            pos: [x, 0, 0],
            tanks: vec![t],
        }
    }
    #[test]
    fn moves_the_substance_at_the_rate() {
        let mut n = vec![
            node(0, tank("fluid", "minecraft:water", 100, Output)),
            node(1, tank("fluid", "", 0, Input)),
        ];
        transfer(&mut n);
        assert_eq!(n[0].tanks[0].amount, 70);
        assert_eq!(n[1].tanks[0].amount, 30);
        assert_eq!(n[1].tanks[0].substance, "minecraft:water");
    }
    #[test]
    fn another_substance_or_kind_does_not_mix() {
        let mut n = vec![
            node(0, tank("fluid", "minecraft:water", 50, Output)),
            node(1, tank("fluid", "minecraft:lava", 10, Input)),
        ];
        transfer(&mut n);
        assert_eq!((n[0].tanks[0].amount, n[1].tanks[0].amount), (50, 10));
        let mut n = vec![
            node(0, tank("gas", "mek:hydrogen", 50, Output)),
            node(1, tank("fluid", "", 0, Input)),
        ];
        transfer(&mut n);
        assert_eq!(n[1].tanks[0].amount, 0);
    }
    #[test]
    fn an_emptied_tank_forgets_its_substance() {
        let mut n = vec![
            node(0, tank("fluid", "minecraft:water", 20, Output)),
            node(1, tank("fluid", "", 0, Input)),
        ];
        transfer(&mut n);
        assert_eq!(n[0].tanks[0].substance, "");
        assert_eq!(n[1].tanks[0].amount, 20);
    }
    #[test]
    fn the_second_tank_takes_what_the_first_cannot() {
        let mut full = tank("fluid", "minecraft:lava", 100, Input);
        full.capacity = 100;
        let mut n = vec![
            node(0, tank("fluid", "minecraft:water", 50, Output)),
            TankNode {
                pos: [1, 0, 0],
                tanks: vec![full, tank("fluid", "", 0, Input)],
            },
        ];
        transfer(&mut n);
        assert_eq!(n[1].tanks[1].amount, 30);
        assert_eq!(n[1].tanks[1].substance, "minecraft:water");
    }
}
