//! Transmitter networks (API block 9): cables and pipes that join the blocks
//! beside them into one energy or substance network. A network is the
//! transmitters of one medium that touch through faces both let connect; the
//! blocks beside it that give (their face toward it outputs) feed it, those
//! that take (input) are fed, at most the network's throughput a tick, shared
//! evenly. Pure, like [`crate::energy`] and [`crate::flow`]: the game and the
//! test harness build the nodes and write the results back.
use crate::{
    energy::{EnergyNode, FACES},
    flow::{TankNode, TankState},
};
use crate::FlowSide;
use std::collections::HashMap;

/// What a transmitter carries.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Medium {
    Energy,
    /// Tanks of this kind (`fluid`, `gas`...).
    Tank(String),
    /// Items between containers.
    Items,
}

/// One cable or pipe block.
#[derive(Clone, Debug, PartialEq)]
pub struct Transmitter {
    pub pos: [i32; 3],
    pub medium: Medium,
    /// What its network moves a tick at most; a network takes the least of its blocks'.
    pub throughput: u64,
    /// The faces it connects through.
    pub connect: [bool; 6],
}

fn gives(s: FlowSide) -> bool {
    matches!(s, FlowSide::Output | FlowSide::Both)
}
fn takes(s: FlowSide) -> bool {
    matches!(s, FlowSide::Input | FlowSide::Both)
}
fn step(pos: [i32; 3], face: usize) -> [i32; 3] {
    let d = FACES[face];
    [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]]
}

/// The networks: for each, the indices of its transmitters (in `all`) in
/// position order, and the least throughput among them. Networks come in the
/// order of their first transmitter's position.
pub fn networks(all: &[Transmitter]) -> Vec<(Vec<usize>, u64)> {
    let mut order: Vec<usize> = (0..all.len()).collect();
    order.sort_by_key(|&i| all[i].pos);
    let at: HashMap<[i32; 3], usize> = all.iter().enumerate().map(|(i, t)| (t.pos, i)).collect();
    let mut seen = vec![false; all.len()];
    let mut out = Vec::new();
    for &start in &order {
        if seen[start] {
            continue;
        }
        let mut members = vec![start];
        seen[start] = true;
        let mut next = 0;
        while next < members.len() {
            let t = members[next];
            next += 1;
            for face in 0..6 {
                if !all[t].connect[face] {
                    continue;
                }
                let Some(&o) = at.get(&step(all[t].pos, face)) else {
                    continue;
                };
                if !seen[o] && all[o].medium == all[t].medium && all[o].connect[face ^ 1] {
                    seen[o] = true;
                    members.push(o);
                }
            }
        }
        members.sort_by_key(|&i| all[i].pos);
        let throughput = members.iter().map(|&i| all[i].throughput).min().unwrap_or(0);
        out.push((members, throughput));
    }
    out
}

/// The positions of the blocks beside a network that are not its transmitters,
/// each with the face of the *network side* it touches (the neighbour's own
/// face toward it is the opposite one), in position order, once per face.
fn attached(all: &[Transmitter], members: &[usize]) -> Vec<([i32; 3], usize)> {
    let mine: std::collections::HashSet<[i32; 3]> = members.iter().map(|&i| all[i].pos).collect();
    let mut out = Vec::new();
    for &i in members {
        for face in 0..6 {
            if !all[i].connect[face] {
                continue;
            }
            let p = step(all[i].pos, face);
            if !mine.contains(&p) {
                out.push((p, face));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The item networks: for each, the blocks beside it with the face of the network side they touch, and its throughput (items a tick).
pub fn item_networks(all: &[Transmitter]) -> Vec<(Vec<([i32; 3], usize)>, u64)> {
    networks(all)
        .into_iter()
        .filter(|(members, _)| all[members[0]].medium == Medium::Items)
        .map(|(members, throughput)| (attached(all, &members), throughput))
        .collect()
}

/// Shares `total` among `caps` as evenly as their limits allow (water
/// filling): the remainder of an uneven split goes to the first ones.
pub fn share(total: u64, caps: &[u64]) -> Vec<u64> {
    let mut got = vec![0u64; caps.len()];
    let mut left = total;
    loop {
        let open: Vec<usize> = (0..caps.len()).filter(|&i| got[i] < caps[i]).collect();
        if left == 0 || open.is_empty() {
            break;
        }
        let each = left / open.len() as u64;
        let extra = (left % open.len() as u64) as usize;
        let mut moved = 0;
        for (k, &i) in open.iter().enumerate() {
            let want = each + u64::from(k < extra);
            let give = want.min(caps[i] - got[i]);
            got[i] += give;
            moved += give;
        }
        if moved == 0 {
            break;
        }
        left -= moved;
    }
    got
}

/// One tick of energy networks. `given`/`taken` are the budgets the
/// neighbour transfers already used this tick, by node index.
pub fn transfer_energy(all: &[Transmitter], nodes: &mut [EnergyNode]) {
    let at: HashMap<[i32; 3], usize> = nodes.iter().enumerate().map(|(i, n)| (n.pos, i)).collect();
    let mut given = vec![0u64; nodes.len()];
    let mut taken = vec![0u64; nodes.len()];
    for (members, throughput) in networks(all) {
        if all[members[0]].medium != Medium::Energy {
            continue;
        }
        // The nodes beside it, and which of their faces touch it.
        let mut gives_to: Vec<usize> = Vec::new();
        let mut takes_from: Vec<usize> = Vec::new();
        for (p, face) in attached(all, &members) {
            let Some(&n) = at.get(&p) else { continue };
            let side = nodes[n].faces[face ^ 1];
            if gives(side) && !gives_to.contains(&n) {
                gives_to.push(n);
            }
            if takes(side) && !takes_from.contains(&n) {
                takes_from.push(n);
            }
        }
        // gives_to: sources; takes_from: sinks (the names say where the energy goes).
        let sources = gives_to;
        let sinks = takes_from;
        let avail: Vec<u64> = sources
            .iter()
            .map(|&n| nodes[n].energy.min(nodes[n].max_extract.saturating_sub(given[n])))
            .collect();
        // Pure sinks are filled before nodes that also give.
        let pure: Vec<usize> = sinks.iter().copied().filter(|n| !sources.contains(n)).collect();
        let both: Vec<usize> = sinks.iter().copied().filter(|n| sources.contains(n)).collect();
        let room = |n: usize, taken: &[u64], nodes: &[EnergyNode]| {
            nodes[n]
                .capacity
                .saturating_sub(nodes[n].energy)
                .min(nodes[n].max_receive.saturating_sub(taken[n]))
        };
        let mut budget = throughput.min(avail.iter().sum());
        // Energy moves out of sources in order, into one group of sinks after another.
        let mut drawn = vec![0u64; sources.len()];
        for group in [pure, both] {
            if budget == 0 {
                break;
            }
            let caps: Vec<u64> = group.iter().map(|&n| room(n, &taken, nodes)).collect();
            let want = budget.min(caps.iter().sum());
            let parts = share(want, &caps);
            for (&n, part) in group.iter().zip(parts) {
                if part == 0 {
                    continue;
                }
                // Take it from the sources in order, never from the sink itself.
                let mut need = part;
                for (k, &s) in sources.iter().enumerate() {
                    if s == n || need == 0 {
                        continue;
                    }
                    let take = need.min(avail[k] - drawn[k]);
                    drawn[k] += take;
                    need -= take;
                }
                let moved = part - need;
                nodes[n].energy += moved;
                taken[n] += moved;
                budget -= moved;
            }
        }
        for (k, &s) in sources.iter().enumerate() {
            nodes[s].energy -= drawn[k];
            given[s] += drawn[k];
        }
    }
}

/// One tick of substance networks (the tanks of nodes beside a network whose
/// face toward it gives or takes, of the network's kind).
pub fn transfer_tanks(all: &[Transmitter], nodes: &mut [TankNode]) {
    let at: HashMap<[i32; 3], usize> = nodes.iter().enumerate().map(|(i, n)| (n.pos, i)).collect();
    for (members, throughput) in networks(all) {
        let Medium::Tank(kind) = &all[members[0]].medium else {
            continue;
        };
        let mut left = throughput;
        // Sources in order: a tank that gives toward the network.
        let mut refs: Vec<(usize, usize, usize)> = Vec::new(); // node, tank, face
        for (p, face) in attached(all, &members) {
            let Some(&n) = at.get(&p) else { continue };
            for (ti, t) in nodes[n].tanks.iter().enumerate() {
                if &t.kind == kind && !refs.iter().any(|r| r.0 == n && r.1 == ti) {
                    refs.push((n, ti, face));
                }
            }
        }
        let tank = |nodes: &[TankNode], r: (usize, usize, usize)| -> TankState {
            nodes[r.0].tanks[r.1].clone()
        };
        let sources: Vec<(usize, usize, usize)> = refs
            .iter()
            .copied()
            .filter(|&r| {
                let t = tank(nodes, r);
                gives(t.faces[r.2 ^ 1]) && t.amount > 0
            })
            .collect();
        for src in sources {
            if left == 0 {
                break;
            }
            let s = tank(nodes, src);
            if s.amount == 0 {
                continue;
            }
            let send = s.amount.min(s.max_rate).min(left);
            // Sinks that take this substance, never the source's own tank.
            let sinks: Vec<(usize, usize, usize)> = refs
                .iter()
                .copied()
                .filter(|&r| (r.0, r.1) != (src.0, src.1))
                .filter(|&r| {
                    let t = tank(nodes, r);
                    takes(t.faces[r.2 ^ 1]) && t.takes_in(&s.substance)
                })
                .collect();
            let caps: Vec<u64> = sinks
                .iter()
                .map(|&r| {
                    let t = tank(nodes, r);
                    t.capacity.saturating_sub(t.amount).min(t.max_rate)
                })
                .collect();
            let parts = share(send.min(caps.iter().sum()), &caps);
            let mut total = 0;
            for (&r, part) in sinks.iter().zip(parts) {
                if part == 0 {
                    continue;
                }
                let t = &mut nodes[r.0].tanks[r.1];
                if t.amount == 0 {
                    t.substance.clone_from(&s.substance);
                }
                t.amount += part;
                total += part;
            }
            let t = &mut nodes[src.0].tanks[src.1];
            t.amount -= total;
            if t.amount == 0 {
                t.substance.clear();
            }
            left -= total;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FlowSide::{Input, Output};

    fn cable(x: i32, medium: Medium, throughput: u64) -> Transmitter {
        Transmitter {
            pos: [x, 0, 0],
            medium,
            throughput,
            connect: [true; 6],
        }
    }
    fn energy(x: i32, e: u64, side: FlowSide) -> EnergyNode {
        EnergyNode {
            pos: [x, 0, 0],
            energy: e,
            capacity: 1000,
            max_receive: 1000,
            max_extract: 1000,
            faces: [side; 6],
        }
    }

    #[test]
    fn cables_that_touch_make_one_network_and_the_least_throughput_rules() {
        let all = vec![
            cable(1, Medium::Energy, 100),
            cable(2, Medium::Energy, 40),
            cable(3, Medium::Energy, 100),
            cable(5, Medium::Energy, 7),
            cable(6, Medium::Tank("fluid".into()), 9),
        ];
        let n = networks(&all);
        assert_eq!(n, vec![(vec![0, 1, 2], 40), (vec![3], 7), (vec![4], 9)]);
    }
    #[test]
    fn a_face_that_does_not_connect_splits_a_network() {
        let mut a = cable(1, Medium::Energy, 10);
        a.connect[5] = false;
        let all = vec![a, cable(2, Medium::Energy, 10)];
        assert_eq!(networks(&all).len(), 2);
    }
    #[test]
    fn sharing_is_even_and_limited() {
        assert_eq!(share(10, &[100, 100, 100]), [4, 3, 3]);
        assert_eq!(share(10, &[2, 100, 100]), [2, 4, 4]);
        assert_eq!(share(10, &[1, 1]), [1, 1]);
        assert_eq!(share(0, &[5]), [0]);
    }
    #[test]
    fn energy_goes_from_generators_to_machines_through_a_cable_line() {
        // gen(0) - cable 1..4 - machine(5) and machine(6) beside it? Machines sit at x=5 and (above) .
        let all: Vec<_> = (1..=4).map(|x| cable(x, Medium::Energy, 50)).collect();
        let mut nodes = vec![
            energy(0, 500, Output),
            energy(5, 0, Input),
        ];
        transfer_energy(&all, &mut nodes);
        assert_eq!((nodes[0].energy, nodes[1].energy), (450, 50));
    }
    #[test]
    fn energy_is_shared_evenly_between_machines() {
        let all = vec![cable(1, Medium::Energy, 90)];
        let mut up = cable(1, Medium::Energy, 90);
        up.pos = [1, 0, 0];
        let mut nodes = vec![
            energy(0, 500, Output),
            energy(2, 0, Input),
            EnergyNode {
                pos: [1, 1, 0],
                ..energy(0, 0, Input)
            },
        ];
        transfer_energy(&all, &mut nodes);
        assert_eq!(nodes[0].energy, 410);
        assert_eq!((nodes[1].energy, nodes[2].energy), (45, 45));
    }
    #[test]
    fn a_network_is_limited_by_what_sinks_have_room_for() {
        let all = vec![cable(1, Medium::Energy, 1000)];
        let mut nodes = vec![energy(0, 500, Output), energy(2, 990, Input)];
        transfer_energy(&all, &mut nodes);
        assert_eq!((nodes[0].energy, nodes[1].energy), (490, 1000));
    }
    #[test]
    fn tanks_pass_one_substance_along_a_pipe() {
        let t = |side, substance: &str, amount| TankState {
            kind: "fluid".into(),
            capacity: 1000,
            max_rate: 1000,
            faces: [side; 6],
            substance: substance.into(),
            amount,
            accepts: Vec::new(),
        };
        let all = vec![cable(1, Medium::Tank("fluid".into()), 30)];
        let mut nodes = vec![
            TankNode { pos: [0, 0, 0], tanks: vec![t(Output, "minecraft:water", 100)] },
            TankNode { pos: [2, 0, 0], tanks: vec![t(Input, "", 0)] },
            TankNode { pos: [1, 1, 0], tanks: vec![t(Input, "minecraft:lava", 5)] },
        ];
        transfer_tanks(&all, &mut nodes);
        // Lava does not take water; the water goes where it is accepted, 30 at most.
        assert_eq!(nodes[0].tanks[0].amount, 70);
        assert_eq!(nodes[1].tanks[0].amount, 30);
        assert_eq!(nodes[1].tanks[0].substance, "minecraft:water");
        assert_eq!(nodes[2].tanks[0].amount, 5);
    }

    #[test]
    fn an_item_network_lists_the_blocks_beside_it() {
        let items = |x| cable(x, Medium::Items, 4);
        let all = [items(0), items(1), cable(5, Medium::Energy, 10)];
        let nets = item_networks(&all);
        assert_eq!(nets.len(), 1);
        let (ends, throughput) = &nets[0];
        assert_eq!(*throughput, 4);
        // Beside the two cables, but not the cables themselves.
        assert!(ends.iter().all(|(p, _)| p[0] == -1 || p[0] == 2 || p[1] != 0 || p[2] != 0));
        assert!(ends.iter().any(|(p, f)| *p == [-1, 0, 0] && *f == 2 || *p == [2, 0, 0]));
    }
}
