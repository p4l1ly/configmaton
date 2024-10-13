use std::{collections::{hash_map::Entry, hash_set, HashMap, HashSet}, hash::Hash, iter::Zip, mem::MaybeUninit, vec};
use std::ptr::addr_of_mut;

use crate::lock::{LockSelector, Lock};
use crate::config_parser::guards::Guard;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Explicit {
    OldVar(String),
    NewVar(String),
    EndVar,
    Char(u8),
}

type StateLock<S> = <S as LockSelector>::Lock<State<S>>;
type TransLock<S> = <S as LockSelector>::Lock<Trans<S>>;
pub type Pattern = Guard;

pub struct Trans<S: LockSelector> {
    pub states: Box<[StateLock<S>]>,
}

pub struct State<S: LockSelector> {
    // This is very like in the traditional finite automata. Each state has a set of transitions
    // via symbols to other states. If multiple states are parallel (let's say nondeterministic)
    // successors of a state via the same symbol, they are stored in a single vector.
    pub explicit_transitions: Box<[(Explicit, TransLock<S>)]>,
    pub pattern_transitions: Box<[(Pattern, TransLock<S>)]>,
}

pub struct ExplicitListeners<S: LockSelector> {
    old_vars: HashMap<String, HashSet<StateLock<S>>>,
    new_vars: HashMap<String, HashSet<StateLock<S>>>,
    end_var: HashSet<StateLock<S>>,
    chars: [HashSet<StateLock<S>>; 256],
}

impl<S: LockSelector> ExplicitListeners<S>
where StateLock<S>: Hash + Eq + std::fmt::Debug,
{
    pub fn new() -> Self {
        ExplicitListeners {
            old_vars: HashMap::new(),
            new_vars: HashMap::new(),
            end_var: HashSet::new(),
            chars: unsafe {
                let mut chars = MaybeUninit::uninit();
                let pchars: *mut [HashSet<StateLock<S>>; 256] = chars.as_mut_ptr();
                for i in 0..256 {
                    addr_of_mut!((*pchars)[i]).write(HashSet::new());
                }
                chars.assume_init()
            },
        }
    }

    pub fn add(&mut self, sym: Explicit, state: StateLock<S>) {
        match sym {
            Explicit::OldVar(s) => {
                self.old_vars.entry(s).or_insert_with(HashSet::new).insert(state);
            },
            Explicit::NewVar(s) => {
                self.new_vars.entry(s).or_insert_with(HashSet::new).insert(state);
            },
            Explicit::EndVar => {
                self.end_var.insert(state);
            },
            Explicit::Char(c) => {
                self.chars[c as usize].insert(state);
            },
        }
    }

    pub fn get_mut(&mut self, sym: &Explicit) -> &mut HashSet<StateLock<S>> {
        match sym {
            Explicit::OldVar(s) => {
                self.old_vars.entry(s.clone()).or_insert_with(HashSet::new)
            },
            Explicit::NewVar(s) => {
                self.new_vars.entry(s.clone()).or_insert_with(HashSet::new)
            }
            Explicit::EndVar => &mut self.end_var,
            Explicit::Char(c) => &mut self.chars[*c as usize],
        }
    }
}

impl<S: LockSelector> std::fmt::Debug for ExplicitListeners<S>
where StateLock<S>: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExplicitListeners")
            .field("old_vars", &self.old_vars)
            .field("new_vars", &self.new_vars)
            .field("end_var", &self.end_var)
            .field("chars", &self.chars)
            .finish()
    }
}

pub struct Listeners<S: LockSelector> {
    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub explicit_listeners: ExplicitListeners<S>,
    pub pattern_listeners: HashMap<Pattern, HashSet<StateLock<S>>>,
}

impl<S: LockSelector> Listeners<S>
where
    StateLock<S>: Hash + Eq + std::fmt::Debug,
    TransLock<S>: Hash + Eq + std::fmt::Debug,
{
    // Initialize the state of the automaton.
    pub fn new<I: IntoIterator<Item = StateLock<S>>>(initial_states: I) -> Self
    {
        let mut explicit_listeners = ExplicitListeners::new();
        let mut pattern_listeners = HashMap::new();
        for state_lock in initial_states.into_iter() {
            let state = state_lock.borrow();
            for (symbol, _) in state.explicit_transitions.iter() {
                explicit_listeners.add(symbol.clone(), state_lock.clone());
            }

            for (pattern, _) in state.pattern_transitions.iter() {
                pattern_listeners
                    .entry(pattern.clone())
                    .or_insert_with(HashSet::new)
                    .insert(state_lock.clone());
            }
        }
        Listeners { explicit_listeners, pattern_listeners }
    }

    // Read a symbol, perform transitions. Return the states from which transitions have been
    // triggered. mapped to the symbols/patterns which have triggered the transitions. The first
    // set contains the states from which transitions have been triggered via `explicit`. The
    // second vector contains the sets of states from which transitions have been triggered via
    // the patterns in the order of `patterns.iter()`.
    pub fn read_with_patterns(&mut self, explicit: Explicit, patterns: &HashSet<Pattern>)
        -> (HashSet<StateLock<S>>, Vec<HashSet<StateLock<S>>>)
    {
        dbg!(&explicit, &patterns, &self.explicit_listeners, &self.pattern_listeners);

        // Prepare the results.
        let mut explicit_old_states: HashSet<StateLock<S>> = HashSet::new();
        let mut pattern_old_statess: Vec<HashSet<StateLock<S>>> = Vec::with_capacity(patterns.len());
        let mut any_pattern = false;

        let states = self.explicit_listeners.get_mut(&explicit);
        if !states.is_empty() {
            std::mem::swap(&mut explicit_old_states, states);
        }

        for pattern in patterns.iter() {
            if let Some(states) = self.pattern_listeners.remove(pattern) {
                any_pattern = true;
                pattern_old_statess.push(states);
            } else {
                pattern_old_statess.push(HashSet::new());
            }
        }

        let mut all_old_states_multi: HashSet<StateLock<S>>;
        let all_old_states: &HashSet<StateLock<S>>;

        if any_pattern {
            all_old_states_multi = explicit_old_states.clone();
            for old_states in pattern_old_statess.iter() {
                all_old_states_multi.extend(old_states.iter().cloned());
            }
            all_old_states = &all_old_states_multi;
        } else {
            if explicit_old_states.is_empty() {
                return (explicit_old_states, pattern_old_statess);
            } else {
                all_old_states = &explicit_old_states;
            }
        }

        dbg!(all_old_states);

        // First, let's remove all listeners for transitions of the old states
        for left_state_lock in all_old_states.iter() {
            let left_state = left_state_lock.borrow();
            for (sym, _) in left_state.explicit_transitions.iter() {
                if explicit != *sym {
                    // Remove listeners for transitions of the left_state (other than the one via
                    // `symbol` which is already removed).
                    self.explicit_listeners.get_mut(&sym).remove(left_state_lock);
                }
            }

            for (pattern, _) in left_state.pattern_transitions.iter() {
                // Remove listeners for transitions of the left_state (other than the one via
                // `pattern` which is already removed). This is different from the explicit
                // part because we want to remove the `pattern` key from `pattern_listeners`,
                // because `pattern_listeners` are iterated over in `get_satisfied_patterns`.

                match self.pattern_listeners.entry(pattern.clone()) {
                    Entry::Occupied(mut entry) => {
                        let is_empty = {
                            let x = entry.get_mut();
                            x.remove(left_state_lock);
                            x.is_empty()
                        };
                        if is_empty {
                            entry.remove();
                        }
                    },
                    // This happens only if the pattern is in `patterns`, because we have just
                    // cleared its listeners at the beginning of this function.
                    Entry::Vacant(_) => { }
                }
            }
        }

        // Then, let's register new listeners for transitions of the successors.
        let mut trans = HashSet::new();

        for left_state_lock in all_old_states.iter() {
            let left_state = left_state_lock.borrow();
            for (sym, right_states) in left_state.explicit_transitions.iter() {
                if explicit == *sym {
                    trans.insert(right_states.clone());
                }
            }

            if patterns.is_empty() {
                continue;
            }

            for (pattern, right_states) in left_state.pattern_transitions.iter() {
                if patterns.contains(pattern) {
                    trans.insert(right_states.clone());
                }
            }
        }

        for t in trans {
            let right_states = t.borrow();
            'outer: for right_state_lock in right_states.states.iter() {
                let right_state = right_state_lock.borrow();

                for (right_sym, _) in right_state.explicit_transitions.iter() {
                    if !self.explicit_listeners
                        .get_mut(&right_sym)
                        .insert(right_state_lock.clone())
                        { continue 'outer; }
                }

                for (right_sym, _) in right_state.pattern_transitions.iter() {
                    if !self.pattern_listeners
                        .entry(right_sym.clone())
                        .or_insert_with(HashSet::new)
                        .insert(right_state_lock.clone())
                        { continue 'outer; }
                }
            }
        }

        (explicit_old_states, pattern_old_statess)
    }

    pub fn read(&mut self, explicit: Explicit)
        -> (
            HashSet<StateLock<S>>,
            Zip<hash_set::IntoIter<Pattern>, vec::IntoIter<HashSet<StateLock<S>>>>
        )
    {
        let active_patterns = self.pattern_listeners.keys().cloned();
        let patterns = get_satisfied_patterns(explicit.clone(), active_patterns);
        let (explicit_states, pattern_states) = self.read_with_patterns(explicit, &patterns);
        (explicit_states, patterns.into_iter().zip(pattern_states.into_iter()))
    }
}

pub fn get_satisfied_patterns<I: Iterator<Item=Pattern>>(explicit: Explicit, patterns: I)
    -> HashSet<Pattern>
{
    let mut result = HashSet::new();
    for guard in patterns {
        if let Explicit::Char(c) = explicit {
            if guard.contains(c) {
                result.insert(guard);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{RcRefCellSelector, RcRefCell};

    type RcState = RcRefCell<State<RcRefCellSelector>>;
    type RcTrans = RcRefCell<Trans<RcRefCellSelector>>;

    fn new_state() -> RcState {
        let result = RcRefCell::new(State {
            explicit_transitions: Box::new([]),
            pattern_transitions: Box::new([]),
        });
        dbg!(&result);
        result
    }

    fn new_trans(states: Vec<RcState>) -> RcRefCell<Trans<RcRefCellSelector>> {
        RcRefCell::new(Trans{states: states.into_boxed_slice()})
    }

    fn set_explicit(
        state: &RcState,
        transitions: Vec<(Explicit, &RcTrans)>,
    ) {
        state.borrow_mut().explicit_transitions =
            transitions.into_iter().map(|(sym, trans)| {
                (sym, trans.clone())
            }).collect::<Vec<_>>().into_boxed_slice();
    }

    fn set_pattern(
        state: &RcState,
        transitions: Vec<(Pattern, &RcTrans)>,
    ) {
        state.borrow_mut().pattern_transitions =
            transitions.into_iter().map(|(sym, trans)| {
                (sym, trans.clone())
            }).collect::<Vec<_>>().into_boxed_slice();
    }

    #[test]
    fn explicit_works() {
        let qs = vec![new_state(), new_state(), new_state(), new_state()];
        let ts = vec![vec![1], vec![2], vec![0, 3], vec![0]].into_iter().map(|states|
            new_trans(states.into_iter().map(|i| qs[i].clone()).collect::<Vec<_>>())
        ).collect::<Vec<_>>();
        let (t1, t2, t03, t0) = (0, 1, 2, 3);
        let (a, b, c) = (0, 1, 2);

        let my_set_explicit = |state_ix: usize, transitions: Vec<(u8, usize)>| {
            set_explicit(&qs[state_ix], transitions.into_iter().map(|(sym, trans_ix)|
                (Explicit::Char(sym), &ts[trans_ix])
            ).collect::<Vec<_>>());
        };

        my_set_explicit(0, vec![(a, t1)]);
        my_set_explicit(1, vec![(b, t2)]);
        my_set_explicit(2, vec![(c, t03)]);
        my_set_explicit(3, vec![(b, t0)]);

        let mut automaton = Listeners::<RcRefCellSelector>::new(vec![qs[0].clone()]);
        let mut read_and_check_predecessors = |sym: u8, expected: Vec<usize>| {
            let patterns = HashSet::new();
            let pre = automaton.read_with_patterns(Explicit::Char(sym), &patterns);
            let expected2 = (
                HashSet::from_iter(expected.into_iter().map(|i| qs[i].clone())),
                vec![],
            );
            assert_eq!(pre, expected2);
        };

        // 0--- 0--a-->1
        read_and_check_predecessors(a, vec![0]);
        // -1-- 1--b-->2
        read_and_check_predecessors(b, vec![1]);
        // --2-
        read_and_check_predecessors(b, vec![]);
        // --2-
        read_and_check_predecessors(a, vec![]);
        // --2- 2--c-->0 2--c-->3
        read_and_check_predecessors(c, vec![2]);
        // 0--3 0--a-->1
        read_and_check_predecessors(a, vec![0]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors(b, vec![1, 3]);
        // 0-2- 2--c-->3
        read_and_check_predecessors(c, vec![2]);
        // 0--3 3--b-->0
        read_and_check_predecessors(b, vec![3]);
        // 0--- 0--a-->1
        read_and_check_predecessors(a, vec![0]);
        // -1-- 1--b-->2
        read_and_check_predecessors(b, vec![1]);
    }

    #[test]
    fn pattern_works() {
        let qs = vec![new_state(), new_state(), new_state(), new_state()];
        let ts = vec![vec![1], vec![2], vec![0, 3], vec![0], vec![3]].into_iter().map(|states|
            new_trans(states.into_iter().map(|i| qs[i].clone()).collect::<Vec<_>>())
        ).collect::<Vec<_>>();
        let (t1, t2, t03, t0, t3) = (0, 1, 2, 3, 4);
        let (a, b, c) = (0, 1, 2);

        let my_set_explicit = |state_ix: usize, transitions: Vec<(u8, usize)>| {
            set_explicit(&qs[state_ix], transitions.into_iter().map(|(sym, trans_ix)|
                (Explicit::Char(sym), &ts[trans_ix])
            ).collect::<Vec<_>>());
        };

        let pats = [
            Guard::from_ranges(vec![(0, 255)]),
            Guard::from_ranges(vec![(0, 0)]),  // Singleton patterns should not appear in production.
            Guard::from_ranges(vec![(2, 255)]),
        ];
        let (any, lb, gb) = (0, 1, 2);

        let my_set_pattern = |state_ix: usize, transitions: Vec<(usize, usize)>| {
            set_pattern(&qs[state_ix], transitions.into_iter().map(|(sym, trans_ix)|
                (pats[sym].clone(), &ts[trans_ix])
            ).collect::<Vec<_>>());
        };

        my_set_explicit(0, vec![(a, t1)]);
        my_set_explicit(1, vec![(b, t2)]);
        my_set_explicit(2, vec![(c, t03)]);
        my_set_explicit(3, vec![(b, t0)]);
        my_set_pattern(0, vec![(any, t3)]);
        my_set_pattern(3, vec![(lb, t3), (gb, t3)]);

        let mut automaton = Listeners::<RcRefCellSelector>::new(vec![qs[0].clone()]);
        let mut read_and_check_predecessors =
            |
                sym: u8,
                patterns: Vec<usize>,
                expected: Vec<usize>,
                expected_patterns: Vec<Vec<usize>>,
            | {
            let patterns2 = HashSet::from_iter(patterns.iter().map(|i| pats[*i].clone()));
            let pre = automaton.read_with_patterns(Explicit::Char(sym), &patterns2);
            let expected2 = HashSet::from_iter(expected.into_iter().map(|i| qs[i].clone()));
            assert_eq!(pre.0, expected2);

            let pre_patterns: HashMap<Pattern, HashSet<RcRefCell<State<RcRefCellSelector>>>> =
                HashMap::from_iter(patterns2.into_iter().zip(pre.1.into_iter()));
            let expected_patterns2 = expected_patterns.into_iter().enumerate().collect::<Vec<_>>();
            let expected_patterns3 = expected_patterns2.into_iter().map(|(i, expected)|
                (pats[patterns[i]].clone(), expected.into_iter().map(|i| qs[i].clone()).collect::<HashSet<_>>())
            ).collect::<HashMap<_, _>>();
            assert_eq!(pre_patterns, expected_patterns3);
        };

        // 0--- 0--a-->1 0-any->3
        read_and_check_predecessors(a, vec![any], vec![0], vec![vec![0]]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors(b, vec![], vec![1, 3], vec![]);
        // 0-2- 0-any->3
        read_and_check_predecessors(b, vec![any], vec![], vec![vec![0]]);
        // --23 3--lb->0
        read_and_check_predecessors(a, vec![lb], vec![], vec![vec![3]]);
        // -123 2--c-->0 2--c-->3 3--gb->3
        read_and_check_predecessors(c, vec![gb], vec![2], vec![vec![3]]);
        // 01-3 0--a-->1 0-any->3 3--lb->3
        read_and_check_predecessors(a, vec![any, lb], vec![0], vec![vec![0], vec![3]]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors(b, vec![], vec![1, 3], vec![]);
        // 0-2- 0-any->3 2--c-->3
        read_and_check_predecessors(c, vec![any], vec![2], vec![vec![0]]);
        // 0--3 0-any->3 3--b-->0
        read_and_check_predecessors(b, vec![any], vec![3], vec![vec![0]]);
        // 0--3 0--a-->1 0-any->3 3--nb->3
        read_and_check_predecessors(a, vec![any, lb], vec![0], vec![vec![0], vec![3]]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors(b, vec![], vec![1, 3], vec![]);

        let mut automaton = Listeners::<RcRefCellSelector>::new(vec![qs[0].clone()]);
        let mut read_and_check_predecessors =
            |
                sym: u8,
                expected: Vec<usize>,
                expected_patterns: Vec<(usize, Vec<usize>)>,
            | {
            let pre = automaton.read(Explicit::Char(sym));
            let expected2 = HashSet::from_iter(expected.into_iter().map(|i| qs[i].clone()));
            assert_eq!(pre.0, expected2);

            let pre_patterns = pre.1.into_iter().collect::<HashMap<_, _>>();
            let expected_patterns3 = expected_patterns.into_iter().map(|(pix, states)|
                (pats[pix].clone(), HashSet::from_iter(states.into_iter().map(|i| qs[i].clone())))
            ).collect::<HashMap<_, _>>();
            assert_eq!(pre_patterns, expected_patterns3);
        };

        // 0--- 0--a-->1 0-any->3
        read_and_check_predecessors(a, vec![0], vec![(any, vec![0])]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors(b, vec![1, 3], vec![]);
        // 0-2- 0-any->3
        read_and_check_predecessors(b, vec![], vec![(any, vec![0])]);
        // --23 3--nb->0
        read_and_check_predecessors(a, vec![], vec![(lb, vec![3])]);
        // -123 2--c-->0 2--c-->3 3--nb->3
        read_and_check_predecessors(c, vec![2], vec![(gb, vec![3])]);
        // 01-3 0--c-->1 0-any->3 3--nb->3
        read_and_check_predecessors(a, vec![0], vec![(any, vec![0]), (lb, vec![3])]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors(b, vec![1, 3], vec![]);
        // 0-2- 0-any->3 2--c-->3
        read_and_check_predecessors(c, vec![2], vec![(any, vec![0])]);
        // 0--3 0-any->3 3--b-->0
        read_and_check_predecessors(b, vec![3], vec![(any, vec![0])]);
        // 0--3 0--a-->1 0-any->3 3--nb->3
        read_and_check_predecessors(a, vec![0], vec![(any, vec![0]), (lb, vec![3])]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors(b, vec![1, 3], vec![]);
    }

    #[test]
    fn get_satisfied_patterns_works() {
        let (a, b) = (0, 1);
        let pats = [
            Guard::from_ranges(vec![(0, 255)]),
            Guard::from_ranges(vec![(0, 0)]),
            Guard::from_ranges(vec![(2, 255)]),
            Guard::from_ranges(vec![(1, 3)]),
        ];
        let any = 0;
        let lb = 1;
        let gb = 2;
        let range = 3;

        let check_char = |sym: Explicit, patterns: Vec<usize>, expected: Vec<usize>| {
            let patterns2 = patterns.iter().map(|i| pats[*i].clone()).collect::<HashSet<_>>();
            let result = get_satisfied_patterns(sym, patterns2.into_iter());
            let expected2 = HashSet::from_iter(expected.into_iter().map(|i| pats[i].clone()));
            assert_eq!(result, expected2);
        };

        check_char(Explicit::Char(a), vec![any], vec![any]);
        check_char(Explicit::Char(b), vec![any, lb, gb], vec![any]);
        check_char(Explicit::Char(a), vec![any, lb, gb], vec![any, lb]);
        check_char(Explicit::Char(a), vec![lb, gb], vec![lb]);
        check_char(Explicit::Char(b), vec![lb, gb, range], vec![range]);
    }
}
