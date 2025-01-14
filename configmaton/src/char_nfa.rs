use std::collections::VecDeque;

use hashbrown::{HashMap, hash_map::Entry};

use super::guards;
use super::guards::{Guard, Monoid};
use super::char_enfa::{Cfg, Nfa as Enfa, OrderedIxs};


pub struct State {
    pub transitions: Vec<(Guard, usize)>,
    pub tags: OrderedIxs,
    pub is_deterministic: bool,
}

pub struct Nfa {
    pub states: Vec<State>,
    pub configurations_to_states: HashMap<OrderedIxs, (usize, usize)>,
    pub visited_states: HashMap<usize, usize>,
}

impl Nfa {
    pub fn new() -> Self {
        Nfa {
            states: vec![],
            configurations_to_states: HashMap::new(),
            visited_states: HashMap::new(),
        }
    }

    pub fn add_nfa(&mut self, enfa: Enfa, tag: usize) {
        let mut reachable_configurations: HashMap<Cfg, usize> = HashMap::new();
        let mut frontier: Vec<(OrderedIxs, usize)> = vec![];

        let q = enfa.expand_config(vec![0]);
        let qix = self.states.len();
        reachable_configurations.insert(q.clone(), qix);
        let Cfg(enfa_config, is_final) = q;

        self.states.push(
            State {
                transitions: vec![],
                tags: OrderedIxs(if is_final { vec![tag] } else { vec![] }),
                is_deterministic: false,
            },
        );

        frontier.push((enfa_config, qix));

        while let Some((enfa_config, state_ix)) = frontier.pop() {
            let mut transitions = vec![];
            for enfa_state_ix in enfa_config.0 {
                for t in enfa.states[enfa_state_ix].transitions.iter() {
                    transitions.push(*t);
                }
            }

            let mut suc_to_guard: HashMap<usize, Guard> = HashMap::new();
            for (range, suc) in transitions {
                suc_to_guard.entry(suc).or_insert(Guard::empty()).add_range(range);
            }

            let mut cfgsuc_to_guard: HashMap<Cfg, Guard> = HashMap::new();
            for (suc, guard) in suc_to_guard {
                let cfgsuc = enfa.expand_config(vec![suc]);
                cfgsuc_to_guard.entry(cfgsuc).or_insert(Guard::empty()).union_update(&guard);
            }

            // 4. the DFA state transitions to the newly-created or reused states of the expanded
            //   configurations
            // 5. put the newly-created ones to the frontier, together with their state index.

            for (cfgsuc, guard) in cfgsuc_to_guard {
                let new_state_ix = *reachable_configurations.entry(cfgsuc.clone()).or_insert_with(|| {
                    let is_final = cfgsuc.1;
                    let new_state_ix = self.states.len();
                    self.states.push(State {
                        transitions: vec![],
                        tags: OrderedIxs(if is_final { vec![tag] } else { vec![] }),
                        is_deterministic: false,
                    });
                    frontier.push((cfgsuc.0, new_state_ix));
                    new_state_ix
                });
                self.states[state_ix].transitions.push((guard, new_state_ix));
            }
        }
    }

    fn continue_to_state(
        suc_ix: usize,
        visited_states: &mut HashMap<usize, usize>,
        states_len: usize,
        frontier: &mut VecDeque<usize>,
        stop_size: usize,
    ) {
        match visited_states.entry(suc_ix) {
            Entry::Vacant(entry) => {
                if states_len < stop_size { frontier.push_back(suc_ix); }
                entry.insert(stop_size);
            },
            Entry::Occupied(entry) => {
                if *entry.get() < stop_size && states_len < stop_size
                    { frontier.push_back(suc_ix); }
            }
        }
    }

    fn continue_to_cfg(
        &mut self,
        cfg: &OrderedIxs,
        frontier: &mut VecDeque<usize>,
        stop_size: usize,
    ) -> usize
    {
        if cfg.0.len() == 1 {
            let suc_ix = cfg.0[0];
            Self::continue_to_state(
                suc_ix, &mut self.visited_states, self.states.len(), frontier, stop_size);
            suc_ix
        } else {
            let (suc_ix, stop_size0) = *self.configurations_to_states.entry(cfg.clone())
                .or_insert_with(|| {
                let mut tags = OrderedIxs(vec![]);
                let mut transitions = vec![];
                for suc_ix in cfg.0.iter() {
                    let state = &self.states[*suc_ix];
                    tags.append(&state.tags);
                    transitions.extend(&state.transitions);
                }
                let suc_ix = self.states.len();
                self.states.push(State { transitions, tags, is_deterministic: false });
                if self.states.len() < stop_size { frontier.push_back(suc_ix); }
                (suc_ix, stop_size)
            });
            if stop_size0 < stop_size && self.states.len() < stop_size
                { frontier.push_back(suc_ix); }
            suc_ix
        }
    }

    pub fn determinize(&mut self, init_states: OrderedIxs, stop_size: usize) -> usize {
        let mut frontier: VecDeque<usize> = VecDeque::new();

        let new_init = self.continue_to_cfg(&init_states, &mut frontier, stop_size);

        while let Some(pre_ix) = frontier.pop_front() {
            let states_len = self.states.len();
            let pre = &mut self.states[pre_ix];

            // It is asctually safe because self is mutated in "continue_to_cfg" only if cfg is
            // not singleton. In this case, it is always singleton.
            if pre.is_deterministic {
                // Only continue the determinization with its successors.
                for (_guard, suc) in pre.transitions.iter() {
                    Self::continue_to_state(
                        *suc, &mut self.visited_states, states_len, &mut frontier, stop_size);
                }
                continue
            }
            pre.is_deterministic = true;

            // // efficient mintermization:
            // // 1. join ranges that lead to the same state and states with the same ranges,
            // //    cyclically.
            // // 2. mintermize
            // // 3. join ranges that lead to the same state and states with the same ranges,
            // //    cyclically.
            // // 4. the DFA state transitions to the newly-created or reused states of the expanded
            // //    configurations
            // // 5. put the newly-created ones to the frontier, together with their state index.

            // Let's go:

            let mut suc_to_guard: HashMap<usize, Guard> = HashMap::new();
            for (range, suc) in pre.transitions.iter() {
                suc_to_guard.entry(*suc).or_insert(Guard::empty()).union_update(range);
            }

            let mut cfgsuc_to_guard: HashMap<OrderedIxs, Guard> = HashMap::new();
            for (suc, guard) in suc_to_guard {
                cfgsuc_to_guard.entry(OrderedIxs(vec![suc]))
                    .or_insert(Guard::empty()).union_update(&guard);
            }

            let mut len_before = cfgsuc_to_guard.len();
            let mut guard_to_cfgsuc: HashMap<Guard, OrderedIxs> = HashMap::new();
            loop {
                for (cfgsuc, guard) in cfgsuc_to_guard.drain() {
                    guard_to_cfgsuc.entry(guard).or_insert(Monoid::empty()).append(&cfgsuc);
                }

                for (guard, cfgsuc) in guard_to_cfgsuc.drain() {
                    cfgsuc_to_guard.entry(cfgsuc).or_insert(Guard::empty()).union_update(&guard);
                }

                if cfgsuc_to_guard.len() == len_before { break; }
                len_before = cfgsuc_to_guard.len();
            }

            // 2. mintermize
            guard_to_cfgsuc = guards::Guard::mintermize(cfgsuc_to_guard.drain());

            // 3. join ranges that lead to the same state and states with the same ranges,
            //   cyclically.

            len_before = guard_to_cfgsuc.len();
            loop {
                for (guard, cfgsuc) in guard_to_cfgsuc.drain() {
                    cfgsuc_to_guard.entry(cfgsuc).or_insert(Guard::empty()).union_update(&guard);
                }

                for (cfgsuc, guard) in cfgsuc_to_guard.drain() {
                    guard_to_cfgsuc.entry(guard).or_insert(Monoid::empty()).append(&cfgsuc);
                }

                if guard_to_cfgsuc.len() == len_before { break; }
                len_before = guard_to_cfgsuc.len();
            }

            // 4. the DFA state transitions to the newly-created or reused states of the expanded
            //   configurations
            // 5. put the newly-created ones to the frontier, together with their state index.

            pre.transitions.clear();
            for (guard, cfgsuc) in guard_to_cfgsuc {
                let suc_ix = self.continue_to_cfg(&cfgsuc, &mut frontier, stop_size);
                self.states[pre_ix].transitions.push((guard, suc_ix));
            }
        }
        new_init
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ast::parse_regex;

    #[test]
    fn dfa_works() {
        let enfa = Enfa::from_ast(parse_regex("a([bA-D]|[cB-C])*d"));
        let mut dfa = Nfa::new();
        dfa.add_nfa(enfa, 0);
        dfa.determinize(OrderedIxs(vec![0]), 1000);
        let qfinal = OrderedIxs(vec![0]);
        let qnonfinal = OrderedIxs(vec![]);

        assert_eq!(dfa.states.len(), 4);
        assert_eq!(dfa.states[0].tags, qnonfinal);
        for q in dfa.states.iter_mut() {
            q.transitions.sort();
        }

        assert_eq!(dfa.states[0].transitions.len(), 2);
        assert_eq!(dfa.states[0].transitions[0].0,
            Guard::from_ranges(vec![(b'a', b'a')]));
        assert_eq!(dfa.states[0].transitions[1].0,
            Guard::from_ranges(vec![(0, b'a' - 1), (b'a' + 1, 255)]));

        let qsink = dfa.states[0].transitions[1].1;
        let q2 = dfa.states[0].transitions[0].1;

        assert_eq!(dfa.states[qsink].tags, qnonfinal);
        assert_eq!(dfa.states[qsink].transitions, vec![(Guard::full(), qsink)]);

        assert_eq!(dfa.states[q2].tags, qnonfinal);
        assert_eq!(dfa.states[q2].transitions.len(), 3);
        assert_eq!(dfa.states[q2].transitions[0],
            (Guard::from_ranges(vec![(b'A', b'D'), (b'b', b'c')]), q2));
        assert_eq!(dfa.states[q2].transitions[1].0,
            Guard::from_ranges(vec![(b'd', b'd')]));
        assert_eq!(dfa.states[q2].transitions[2],
            (Guard::from_ranges(vec![(0, b'A' - 1), (b'D' + 1, b'b' - 1), (b'd' + 1, 255)]),
                qsink));

        let qf = dfa.states[q2].transitions[1].1;
        assert_eq!(dfa.states[qf].tags, qfinal);
        assert_eq!(dfa.states[qf].transitions, vec![(Guard::full(), qsink)]);
    }

    #[test]
    fn emptyword_dfa_works1() {
        let enfa = Enfa::from_ast(parse_regex(""));
        let mut nfa = Nfa::new();
        nfa.add_nfa(enfa, 0);
        nfa.determinize(OrderedIxs(vec![0]), 1000);

        assert_eq!(nfa.states.len(), 2);
        assert_eq!(nfa.states[0].tags, OrderedIxs(vec![0]));
        assert_eq!(nfa.states[0].transitions, vec![(Guard::full(), 1)]);

        assert_eq!(nfa.states[1].tags, OrderedIxs(vec![]));
        assert_eq!(nfa.states[1].transitions, vec![(Guard::full(), 1)]);
    }

    #[test]
    fn emptyword_nfa_works2() {
        let enfa = Enfa::from_ast(parse_regex(""));
        let mut nfa = Nfa::new();
        nfa.add_nfa(enfa, 0);

        assert_eq!(nfa.states.len(), 1);
        assert_eq!(nfa.states[0].tags, OrderedIxs(vec![0]));
        assert_eq!(nfa.states[0].transitions, vec![]);
    }
}
