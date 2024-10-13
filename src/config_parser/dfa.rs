use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use serde_json::map::VacantEntry;

use super::guards;
use super::guards::{Guard, Monoid};
use super::nfa::{Nfa, Cfg};


pub struct DfaState {
    transitions: Vec<(Guard, usize)>,
    is_final: bool,
}

pub struct Dfa {
    states: Vec<DfaState>,
}

impl Dfa {
    pub fn from_nfa(nfa: Nfa) -> Self {
        let mut reachable_configurations: HashMap<Cfg, usize> = HashMap::new();
        let mut frontier: Vec<(Vec<usize>, usize)> = vec![];

        let q = nfa.expand_config(vec![0]);
        reachable_configurations.insert(q.clone(), 0);
        let Cfg(nfa_config, is_final) = q;

        let mut automaton = Self {
            states: vec![DfaState { transitions: vec![], is_final }],
        };

        frontier.push((nfa_config, 0));

        while let Some((nfa_config, state_ix)) = frontier.pop() {
            let mut transitions = vec![];
            for nfa_state_ix in nfa_config {
                for t in nfa.states[nfa_state_ix].transitions.iter() {
                    transitions.push(*t);
                }
            }

            // efficient mintermization:
            // 1. join ranges that lead to the same state and states with the same ranges,
            //    cyclically.
            // 2. mintermize
            // 3. join ranges that lead to the same state and states with the same ranges,
            //    cyclically.
            // 4. the DFA state transitions to the newly-created or reused states of the expanded
            //    configurations
            // 5. put the newly-created ones to the frontier, together with their state index.

            // Let's go:

            // 1. join ranges that lead to the same state and states with the same ranges,
            //    cyclically.
            let mut suc_to_guard: HashMap<usize, Guard> = HashMap::new();
            for (range, suc) in transitions {
                match suc_to_guard.entry(suc) {
                    Entry::Vacant(entry) => { entry.insert(vec![range]); },
                    Entry::Occupied(mut entry) => {
                        let guard = entry.get_mut();
                        *guard = guards::add_range(std::mem::take(guard), range);
                    },
                }
            }

            let mut cfgsuc_to_guard: HashMap<Cfg, Guard> = HashMap::new();
            for (suc, guard) in suc_to_guard {
                let cfgsuc = nfa.expand_config(vec![suc]);
                let old = cfgsuc_to_guard.entry(cfgsuc).or_insert(Guard::new());
                *old = guards::union(old, &guard);
            }

            let mut len_before = cfgsuc_to_guard.len();
            let mut guard_to_cfgsuc: HashMap<Guard, Cfg> = HashMap::new();
            loop {
                for (cfgsuc, guard) in cfgsuc_to_guard.drain() {
                    guard_to_cfgsuc.entry(guard).or_insert(Monoid::empty()).append(cfgsuc);
                }

                for (guard, cfgsuc) in guard_to_cfgsuc.drain() {
                    let old = cfgsuc_to_guard.entry(cfgsuc).or_insert(Guard::new());
                    *old = guards::union(old, &guard);
                }

                if cfgsuc_to_guard.len() == len_before {
                    break;
                }
                len_before = cfgsuc_to_guard.len();
            }

            // 2. mintermize
            guard_to_cfgsuc = guards::mintermize(cfgsuc_to_guard.drain());

            // 3. join ranges that lead to the same state and states with the same ranges,
            //   cyclically.

            len_before = guard_to_cfgsuc.len();
            loop {
                for (guard, cfgsuc) in guard_to_cfgsuc.drain() {
                    let old = cfgsuc_to_guard.entry(cfgsuc).or_insert(Guard::new());
                    *old = guards::union(old, &guard);
                }

                for (cfgsuc, guard) in cfgsuc_to_guard.drain() {
                    guard_to_cfgsuc.entry(guard).or_insert(Monoid::empty()).append(cfgsuc);
                }

                if guard_to_cfgsuc.len() == len_before {
                    break;
                }
                len_before = guard_to_cfgsuc.len();
            }

            // 4. the DFA state transitions to the newly-created or reused states of the expanded
            //   configurations
            // 5. put the newly-created ones to the frontier, together with their state index.

            for (guard, cfgsuc) in guard_to_cfgsuc {
                let new_state_ix = *reachable_configurations.entry(cfgsuc.clone()).or_insert_with(|| {
                    let is_final = cfgsuc.1;
                    let new_state_ix = automaton.states.len();
                    automaton.states.push(DfaState { transitions: vec![], is_final });
                    frontier.push((cfgsuc.0, new_state_ix));
                    new_state_ix
                });
                automaton.states[state_ix].transitions.push((guard, new_state_ix));
            }
        }

        automaton
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::nfa::Nfa;
    use super::super::ast::parse_regex;

    #[test]
    fn dfa_works() {
        let nfa = Nfa::from_ast(parse_regex("a([bA-D]|[cB-C])*d"));
        let mut dfa = Dfa::from_nfa(nfa);

        assert_eq!(dfa.states.len(), 4);
        assert_eq!(dfa.states[0].is_final, false);
        for q in dfa.states.iter_mut() {
            q.transitions.sort();
        }

        assert_eq!(dfa.states[0].transitions.len(), 2);
        assert_eq!(dfa.states[0].transitions[0].0, vec![(0, b'a' - 1), (b'a' + 1, 255)]);
        assert_eq!(dfa.states[0].transitions[1].0, vec![(b'a', b'a')]);

        let qsink = dfa.states[0].transitions[0].1;
        let q2 = dfa.states[0].transitions[1].1;

        assert_eq!(dfa.states[qsink].is_final, false);
        assert_eq!(dfa.states[qsink].transitions, vec![(vec![(0, 255)], qsink)]);

        assert_eq!(dfa.states[q2].is_final, false);
        assert_eq!(dfa.states[q2].transitions.len(), 3);
        assert_eq!(dfa.states[q2].transitions[1], (vec![(b'A', b'D'), (b'b', b'c')], q2));
        assert_eq!(dfa.states[q2].transitions[0],
            (vec![(0, b'A' - 1), (b'D' + 1, b'b' - 1), (b'd' + 1, 255)], qsink));
        assert_eq!(dfa.states[q2].transitions[2].0, vec![(b'd', b'd')]);

        let qf = dfa.states[q2].transitions[2].1;
        assert_eq!(dfa.states[qf].is_final, true);
        assert_eq!(dfa.states[qf].transitions, vec![(vec![(0, 255)], qsink)]);
    }

    #[test]
    fn epsilon_dfa_works() {
        let nfa = Nfa::from_ast(parse_regex(""));
        let dfa = Dfa::from_nfa(nfa);

        assert_eq!(dfa.states.len(), 2);
        assert_eq!(dfa.states[0].is_final, true);
        assert_eq!(dfa.states[0].transitions, vec![(vec![(0, 255)], 1)]);

        assert_eq!(dfa.states[1].is_final, false);
        assert_eq!(dfa.states[1].transitions, vec![(vec![(0, 255)], 1)]);
    }
}
