use std::{collections::{hash_map::Entry, hash_set, HashMap, HashSet}, hash::Hash, iter::Zip, mem::MaybeUninit, vec};
use std::ptr::addr_of_mut;

use crate::lock::{LockSelector, Lock};
use crate::config_parser::guards::Guard;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Explicit {
    // TODO Microopt templating: For deterministic automata there is no OldVar
    OldVar(String),
    NewVar(String),
    EndVar,
    Char(u8),
}

type StateLock<S, TL> = <S as LockSelector>::Lock<State<S, TL>>;
pub type Pattern = Guard;

// There will be always at least one successor and mostly only one. Therefore we store the first
// successor specially, to avoid indirection.
pub struct Succ<S: LockSelector, TL>(StateLock<S, TL>, Box<[StateLock<S, TL>]>);
impl<S: LockSelector, TL> Clone for Succ<S, TL> {
    fn clone(&self) -> Self {
        Succ(self.0.clone(), self.1.clone())
    }
}

pub trait TransactionListener {
    fn run(&self);
}

pub struct State<S: LockSelector, TL: Sized> {
    // This is very like in the traditional finite automata. Each state has a set of transitions
    // via symbols to other states. If multiple states are parallel (let's say nondeterministic)
    // successors of a state via the same symbol, they are stored in a single vector.
    pub explicit_transitions: Box<[(Explicit, Succ<S, TL>, TL)]>,
    pub pattern_transitions: Box<[(Pattern, Succ<S, TL>, TL)]>,
}

pub struct ExplicitListeners<S: LockSelector, TL> {
    old_vars: HashMap<String, HashSet<StateLock<S, TL>>>,
    new_vars: HashMap<String, HashSet<StateLock<S, TL>>>,
    end_var: HashSet<StateLock<S, TL>>,
    chars: [HashSet<StateLock<S, TL>>; 256],
}

impl<S: LockSelector, TL> ExplicitListeners<S, TL>
where StateLock<S, TL>: Hash + Eq + std::fmt::Debug,
{
    pub fn new() -> Self {
        ExplicitListeners {
            old_vars: HashMap::new(),
            new_vars: HashMap::new(),
            end_var: HashSet::new(),
            chars: unsafe {
                let mut chars = MaybeUninit::uninit();
                let pchars: *mut [HashSet<StateLock<S, TL>>; 256] = chars.as_mut_ptr();
                for i in 0..256 {
                    addr_of_mut!((*pchars)[i]).write(HashSet::new());
                }
                chars.assume_init()
            },
        }
    }

    pub fn add(&mut self, sym: Explicit, state: StateLock<S, TL>) {
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

    pub fn get_mut(&mut self, sym: &Explicit) -> &mut HashSet<StateLock<S, TL>> {
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

impl<S: LockSelector, TL> std::fmt::Debug for ExplicitListeners<S, TL>
where StateLock<S, TL>: std::fmt::Debug,
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

pub struct Listeners<S: LockSelector, TL> {
    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub explicit_listeners: ExplicitListeners<S, TL>,
    pub pattern_listeners: HashMap<Pattern, HashSet<StateLock<S, TL>>>,
}

impl<S: LockSelector, TL: TransactionListener> Listeners<S, TL>
where StateLock<S, TL>: Hash + Eq + std::fmt::Debug,
{
    // Initialize the state of the automaton.
    pub fn new<I: IntoIterator<Item = StateLock<S, TL>>>(initial_states: I) -> Self
    {
        let mut explicit_listeners = ExplicitListeners::new();
        let mut pattern_listeners = HashMap::new();
        for state_lock in initial_states.into_iter() {
            let state = state_lock.borrow();
            for (symbol, _, _) in state.explicit_transitions.iter() {
                explicit_listeners.add(symbol.clone(), state_lock.clone());
            }

            for (pattern, _, _) in state.pattern_transitions.iter() {
                pattern_listeners
                    .entry(pattern.clone())
                    .or_insert_with(HashSet::new)
                    .insert(state_lock.clone());
            }
        }
        Listeners { explicit_listeners, pattern_listeners }
    }

    // Read a symbol, perform transitions.
    pub fn read(&mut self, explicit: Explicit) {
        dbg!(&explicit, &self.explicit_listeners, &self.pattern_listeners);

        // Prepare the results.
        let mut all_old_states = std::mem::take(self.explicit_listeners.get_mut(&explicit));
        let mut any_pattern = false;

        if let Explicit::Char(c) = explicit {
            self.pattern_listeners.retain(|pattern, states|
                if pattern.contains(c) {
                    any_pattern = true;
                    all_old_states.extend(states.drain());
                    false
                } else { true }
            );
        }

        dbg!(&all_old_states);

        // First, let's remove all listeners for transitions of the old states
        for left_state_lock in all_old_states.iter() {
            let left_state = left_state_lock.borrow();
            for (sym, _, _) in left_state.explicit_transitions.iter() {
                if explicit != *sym {
                    // Remove listeners for transitions of the left_state (other than the one via
                    // `symbol` which is already removed).
                    self.explicit_listeners.get_mut(&sym).remove(left_state_lock);
                }
            }

            for (pattern, _, _) in left_state.pattern_transitions.iter() {
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
        for left_state_lock in all_old_states.iter() {
            let left_state = left_state_lock.borrow();
            for (sym, right_states, tl) in left_state.explicit_transitions.iter() {
                if explicit == *sym {
                    self.add_right_states(right_states);
                    tl.run();
                }
            }

            if let Explicit::Char(c) = explicit {
                if any_pattern {
                    for (pattern, right_states, tl) in left_state.pattern_transitions.iter() {
                        if pattern.contains(c) {
                            self.add_right_states(right_states);
                            tl.run();
                        }
                    }
                }
            }
        }
    }

    fn add_right_states(&mut self, right_states: &Succ<S, TL>) {
        self.add_right_state(&right_states.0);
        for right_state_lock in right_states.1.iter() {
            self.add_right_state(right_state_lock);
        }
    }

    fn add_right_state(&mut self, right_state_lock: &StateLock<S, TL>) {
        let right_state = right_state_lock.borrow();

        for (right_sym, _, _) in right_state.explicit_transitions.iter() {
            if !self.explicit_listeners.get_mut(&right_sym).insert(right_state_lock.clone())
                { return; }
        }

        for (right_sym, _, _) in right_state.pattern_transitions.iter() {
            if !self.pattern_listeners
                .entry(right_sym.clone())
                .or_insert_with(HashSet::new)
                .insert(right_state_lock.clone())
                { return; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{RcRefCellSelector, RcRefCell};

    struct RcTransListener {
        id: u8,
        ids: RcRefCell<HashSet<u8>>,
    }

    impl TransactionListener for RcTransListener {
        fn run(&self) {
            self.ids.borrow_mut().insert(self.id);
        }
    }

    type RcState = RcRefCell<State<RcRefCellSelector, RcTransListener>>;
    type RcSucc = Succ<RcRefCellSelector, RcTransListener>;

    fn new_state() -> RcState {
        let result = RcRefCell::new(State {
            explicit_transitions: Box::new([]),
            pattern_transitions: Box::new([]),
        });
        dbg!(&result);
        result
    }

    fn new_trans(mut states: Vec<RcState>) -> RcSucc {
        Succ(states.pop().unwrap(), states.into_boxed_slice())
    }

    fn set_explicit(
        state: &RcState,
        transitions: Vec<(Explicit, RcSucc, RcTransListener)>,
    ) {
        state.borrow_mut().explicit_transitions = transitions.into_boxed_slice();
    }

    fn set_pattern(
        state: &RcState,
        transitions: Vec<(Pattern, RcSucc, RcTransListener)>,
    ) {
        state.borrow_mut().pattern_transitions = transitions.into_boxed_slice();
    }

    #[test]
    fn explicit_works() {
        let qs = vec![new_state(), new_state(), new_state(), new_state()];
        let ts = vec![vec![1], vec![2], vec![0, 3], vec![0]].into_iter().map(|states|
            new_trans(states.into_iter().map(|i| qs[i].clone()).collect::<Vec<_>>())
        ).collect::<Vec<_>>();
        let (t1, t2, t03, t0) = (0, 1, 2, 3);
        let (a, b, c) = (0, 1, 2);

        let trans_ids: RcRefCell<HashSet<u8>> = RcRefCell::new(HashSet::new());

        let my_set_explicit = |state_ix: usize, transitions: Vec<(u8, usize, u8)>| {
            set_explicit(&qs[state_ix], transitions.into_iter().map(|(sym, trans_ix, trans_id)|
                (
                    Explicit::Char(sym),
                    ts[trans_ix].clone(),
                    RcTransListener{ id: trans_id, ids: trans_ids.clone() }
                )
            ).collect::<Vec<_>>());
        };

        let a01 = 0;
        let b12 = 1;
        let c203 = 2;
        let b30 = 3;

        my_set_explicit(0, vec![(a, t1, a01)]);
        my_set_explicit(1, vec![(b, t2, b12)]);
        my_set_explicit(2, vec![(c, t03, c203)]);
        my_set_explicit(3, vec![(b, t0, b30)]);

        let mut automaton = Listeners::<RcRefCellSelector, RcTransListener>::new(vec![qs[0].clone()]);
        let mut read_and_check_transitions = |sym: u8, expected: Vec<u8>| {
            trans_ids.borrow_mut().clear();
            automaton.read(Explicit::Char(sym));
            let expected = expected.into_iter().collect::<HashSet<_>>();
            assert_eq!(*trans_ids.borrow(), expected);
        };

        // 0--- 0--a-->1
        read_and_check_transitions(a, vec![a01]);
        // -1-- 1--b-->2
        read_and_check_transitions(b, vec![b12]);
        // --2-
        read_and_check_transitions(b, vec![]);
        // --2-
        read_and_check_transitions(a, vec![]);
        // --2- 2--c-->0 2--c-->3
        read_and_check_transitions(c, vec![c203]);
        // 0--3 0--a-->1
        read_and_check_transitions(a, vec![a01]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_transitions(b, vec![b12, b30]);
        // 0-2- 2--c-->3
        read_and_check_transitions(c, vec![c203]);
        // 0--3 3--b-->0
        read_and_check_transitions(b, vec![b30]);
        // 0--- 0--a-->1
        read_and_check_transitions(a, vec![a01]);
        // -1-- 1--b-->2
        read_and_check_transitions(b, vec![b12]);
    }

    #[test]
    fn pattern_works() {
        let qs = vec![new_state(), new_state(), new_state(), new_state()];
        let ts = vec![vec![1], vec![2], vec![0, 3], vec![0], vec![3]].into_iter().map(|states|
            new_trans(states.into_iter().map(|i| qs[i].clone()).collect::<Vec<_>>())
        ).collect::<Vec<_>>();
        let (t1, t2, t03, t0, t3) = (0, 1, 2, 3, 4);
        let (a, b, c) = (0, 1, 2);

        let trans_ids: RcRefCell<HashSet<u8>> = RcRefCell::new(HashSet::new());

        let my_set_explicit = |state_ix: usize, transitions: Vec<(u8, usize, u8)>| {
            set_explicit(&qs[state_ix], transitions.into_iter().map(|(sym, trans_ix, trans_id)|
                (
                    Explicit::Char(sym),
                    ts[trans_ix].clone(),
                    RcTransListener{ id: trans_id, ids: trans_ids.clone() }
                )
            ).collect::<Vec<_>>());
        };

        let pats = [
            Guard::from_ranges(vec![(0, 255)]),
            Guard::from_ranges(vec![(0, 0), (2, 255)]),
        ];
        let (any, nb) = (0, 1);

        let my_set_pattern = |state_ix: usize, transitions: Vec<(usize, usize, u8)>| {
            set_pattern(&qs[state_ix], transitions.into_iter().map(|(sym, trans_ix, trans_id)|
                (
                    pats[sym].clone(),
                    ts[trans_ix].clone(),
                    RcTransListener{ id: trans_id, ids: trans_ids.clone() }
                )
            ).collect::<Vec<_>>());
        };

        let a01 = 0;
        let b12 = 1;
        let c203 = 2;
        let b30 = 3;
        let any03 = 4;
        let nb33 = 4;

        my_set_explicit(0, vec![(a, t1, a01)]);
        my_set_explicit(1, vec![(b, t2, b12)]);
        my_set_explicit(2, vec![(c, t03, c203)]);
        my_set_explicit(3, vec![(b, t0, b30)]);
        my_set_pattern(0, vec![(any, t3, any03)]);
        my_set_pattern(3, vec![(nb, t3, nb33)]);

        let mut automaton = Listeners::<RcRefCellSelector, RcTransListener>::new(vec![qs[0].clone()]);
        let mut read_and_check_transitions = |sym: u8, expected: Vec<u8>| {
            trans_ids.borrow_mut().clear();
            automaton.read(Explicit::Char(sym));
            let expected = expected.into_iter().collect::<HashSet<_>>();
            assert_eq!(*trans_ids.borrow(), expected);
        };

        // 0--- 0--a-->1 0-any->3
        read_and_check_transitions(a, vec![a01, any03]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_transitions(b, vec![b12, b30]);
        // 0-2- 0-any->3
        read_and_check_transitions(b, vec![any03]);
        // --23 3--nb->3
        read_and_check_transitions(a, vec![nb33]);
        // -123 2--c-->0 2--c-->3 3--nb->3
        read_and_check_transitions(c, vec![c203, nb33]);
        // 01-3 0--a-->1 0-any->3 3--nb->3
        read_and_check_transitions(a, vec![a01, any03, nb33]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_transitions(b, vec![b12, b30]);
        // 0-2- 0-any->3 2--c-->3
        read_and_check_transitions(c, vec![any03, c203]);
        // 0--3 0-any->3 3--b-->0
        read_and_check_transitions(b, vec![any03, b30]);
        // 0--3 0--a-->1 0-any->3 3--nb->3
        read_and_check_transitions(a, vec![a01, any03, nb33]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_transitions(b, vec![b12, b30]);
    }
}
