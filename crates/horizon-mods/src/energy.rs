//! Energy moving between neighbouring block entities (API block 3). Pure: the
//! game and the test harness build the nodes and write the energies back.
use crate::FlowSide;

/// Where face `i` looks: down, up, north, south, west, east.
pub const FACES: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];

/// One block that stores energy.
#[derive(Clone, Debug, PartialEq)]
pub struct EnergyNode {
    pub pos: [i32; 3],
    pub energy: u64,
    pub capacity: u64,
    pub max_receive: u64,
    pub max_extract: u64,
    pub faces: [FlowSide; 6],
}

fn gives(s: FlowSide) -> bool {
    matches!(s, FlowSide::Output | FlowSide::Both)
}
fn takes(s: FlowSide) -> bool {
    matches!(s, FlowSide::Input | FlowSide::Both)
}

/// One tick of transfers. Blocks are visited in position order, and each
/// block's faces in the order of [`FACES`], so a result never depends on
/// the order of `nodes`. A block gives at most `max_extract` and takes at
/// most `max_receive` in the whole tick.
pub fn transfer(nodes: &mut [EnergyNode]) {
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| nodes[i].pos);
    let at: std::collections::HashMap<[i32; 3], usize> =
        nodes.iter().enumerate().map(|(i, n)| (n.pos, i)).collect();
    let mut given = vec![0u64; nodes.len()];
    let mut taken = vec![0u64; nodes.len()];
    for &s in &order {
        for (face, d) in FACES.iter().enumerate() {
            if !gives(nodes[s].faces[face]) {
                continue;
            }
            let to = [
                nodes[s].pos[0] + d[0],
                nodes[s].pos[1] + d[1],
                nodes[s].pos[2] + d[2],
            ];
            let Some(&t) = at.get(&to) else { continue };
            if !takes(nodes[t].faces[face ^ 1]) {
                continue;
            }
            let room = nodes[t].capacity.saturating_sub(nodes[t].energy);
            let amount = nodes[s]
                .energy
                .min(nodes[s].max_extract.saturating_sub(given[s]))
                .min(nodes[t].max_receive.saturating_sub(taken[t]))
                .min(room);
            if amount == 0 {
                continue;
            }
            nodes[s].energy -= amount;
            nodes[t].energy += amount;
            given[s] += amount;
            taken[t] += amount;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FlowSide::{Both, Input, None as No, Output};

    fn node(pos: [i32; 3], energy: u64, faces: [FlowSide; 6]) -> EnergyNode {
        EnergyNode {
            pos,
            energy,
            capacity: 100,
            max_receive: 30,
            max_extract: 30,
            faces,
        }
    }
    #[test]
    fn moves_at_most_the_rate_and_what_fits() {
        let mut n = vec![
            node([0, 0, 0], 100, [Output; 6]),
            node([1, 0, 0], 90, [Input; 6]),
        ];
        transfer(&mut n);
        assert_eq!((n[0].energy, n[1].energy), (90, 100));
        let mut n = vec![
            node([0, 0, 0], 100, [Output; 6]),
            node([1, 0, 0], 0, [Input; 6]),
        ];
        transfer(&mut n);
        assert_eq!((n[0].energy, n[1].energy), (70, 30));
    }
    #[test]
    fn faces_must_match() {
        // East of the first block is its face 5; the second must take through face 4 (west).
        let mut west_closed = [Input; 6];
        west_closed[4] = No;
        let mut n = vec![
            node([0, 0, 0], 50, [Output; 6]),
            node([1, 0, 0], 0, west_closed),
        ];
        transfer(&mut n);
        assert_eq!(n[1].energy, 0);
    }
    #[test]
    fn a_chain_passes_energy_along_and_order_does_not_matter() {
        let mk = || {
            vec![
                node([0, 0, 0], 100, [Output; 6]),
                node([1, 0, 0], 0, [Both; 6]),
                node([2, 0, 0], 0, [Input; 6]),
            ]
        };
        let mut a = mk();
        transfer(&mut a);
        let mut b = mk();
        b.reverse();
        transfer(&mut b);
        b.reverse();
        assert_eq!(a, b);
        // The middle block gave nothing yet when it was visited before the first? Position order: 0 gives 30 to 1, then 1 gives 30 to 2.
        assert_eq!((a[0].energy, a[1].energy, a[2].energy), (70, 0, 30));
    }
}
