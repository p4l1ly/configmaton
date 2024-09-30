use std::{collections::{HashMap, HashSet, hash_map::Entry}, hash::Hash};

use crate::lock::{LockSelector, Lock};
use serde_json::Number;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Explicit {
    Key,
    Value(String),
    End,
    Char(char),
    Num(Number),
    Bool(bool),
    Null,
    ListF,
    DictF,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Pattern {
    Not(Explicit),
    Any,
    CharRange(char, char),
    NumCondition(NumCondition),
}

type StateLock<S> = <S as LockSelector>::Lock<State<S>>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum NumCondition {
    Less(Number),
    LessEq(Number),
    Eq(Number),
    Greater(Number),
    GreaterEq(Number),
    NotEq(Number),
    In(Number, Number),
    NotIn(Number, Number),
}

impl NumCondition {
    pub fn is_satisfied_by(&self, num: Number) -> bool {
        match self {
            NumCondition::Less(ref n) => compare_numbers(&num, n, |a, b| a < b),
            NumCondition::LessEq(ref n) => compare_numbers(&num, n, |a, b| a <= b),
            NumCondition::Eq(ref n) => compare_numbers(&num, n, |a, b| a == b),
            NumCondition::Greater(ref n) => compare_numbers(&num, n, |a, b| a > b),
            NumCondition::GreaterEq(ref n) => compare_numbers(&num, n, |a, b| a >= b),
            NumCondition::NotEq(ref n) => compare_numbers(&num, n, |a, b| a != b),
            NumCondition::In(ref low, ref high) => {
                compare_numbers(&num, low, |a, b| a >= b)
                    && compare_numbers(&num, high, |a, b| a <= b)
            },
            NumCondition::NotIn(ref low, ref high) => {
                !(compare_numbers(&num, low, |a, b| a >= b)
                    && compare_numbers(&num, high, |a, b| a <= b))
            }
        }
    }
}

fn compare_numbers(num1: &Number, num2: &Number, cmp: impl Fn(f64, f64) -> bool) -> bool {
    let n1 = num1.as_f64().expect("First number should be convertible to f64");
    let n2 = num2.as_f64().expect("Second number should be convertible to f64");
    cmp(n1, n2)
}

pub struct State<S: LockSelector> {
    // This is very like in the traditional finite automata. Each state has a set of transitions
    // via symbols to other states. If multiple states are parallel (let's say nondeterministic)
    // successors of a state via the same symbol, they are stored in a single vector.
    pub explicit_transitions: Box<[(Explicit, Box<[StateLock<S>]>)]>,
    pub pattern_transitions: Box<[(Pattern, Box<[StateLock<S>]>)]>,
    pub marker: bool,  // WARNING: read must be locked because of this field.
}

pub struct Listeners<S: LockSelector> {
    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub explicit_listeners: HashMap<Explicit, HashSet<StateLock<S>>>,
    pub pattern_listeners: HashMap<Pattern, HashSet<StateLock<S>>>,
}

impl<S: LockSelector>
Listeners<S>
where StateLock<S>: Hash + Eq + std::fmt::Debug,
{
    // Initialize the state of the automaton.
    pub fn new<I>(initial_states: I) -> Self
    where
        I: IntoIterator<Item = StateLock<S>>,
    {
        let mut explicit_listeners = HashMap::new();
        let mut pattern_listeners = HashMap::new();
        for state_lock in initial_states.into_iter() {
            let state = state_lock.borrow();
            for (symbol, _) in state.explicit_transitions.iter() {
                explicit_listeners
                    .entry(symbol.clone())
                    .or_insert_with(HashSet::new)
                    .insert(state_lock.clone());
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
    pub fn read(&mut self, explicit: Explicit, patterns: HashSet<Pattern>)
        -> (HashSet<StateLock<S>>, Vec<HashSet<StateLock<S>>>)
    {
        dbg!(&explicit, &patterns, &self.explicit_listeners, &self.pattern_listeners);

        // Prepare the results.
        let mut explicit_old_states: HashSet<StateLock<S>> = HashSet::new();
        let mut pattern_old_statess: Vec<HashSet<StateLock<S>>> = Vec::with_capacity(patterns.len());
        let mut any_pattern = false;

        if let Some(states) = self.explicit_listeners.get_mut(&explicit) {
            if !states.is_empty() {
                std::mem::swap(&mut explicit_old_states, states);
            }
        }

        for pattern in patterns.iter() {
            if let Some(states) = self.pattern_listeners.get_mut(pattern) {
                if states.is_empty() {
                    pattern_old_statess.push(HashSet::new());
                    continue;
                }

                any_pattern = true;
                pattern_old_statess.push(std::mem::take(states));
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

        // The following should be locked, globally for the structure of states.

        // First, let's detect, which old_states will not be removed. We remove them from
        // old_states and reinsert them to the new_states.
        for left_state_lock in all_old_states.iter() {
            let mut left_state = left_state_lock.borrow_mut();
            let mut self_transition = false;

            for (sym, right_states) in left_state.explicit_transitions.iter() {
                if explicit == *sym {
                    for right_state_lock in right_states.iter() {
                        if right_state_lock == left_state_lock {
                            self_transition = true;
                            continue;
                        }
                        if all_old_states.contains(right_state_lock) {
                            right_state_lock.borrow_mut().marker = true;
                            continue;
                        }
                    }
                }
            }

            for (pattern, right_states) in left_state.pattern_transitions.iter() {
                if patterns.contains(pattern) {
                    for right_state_lock in right_states.iter() {
                        if right_state_lock == left_state_lock {
                            self_transition = true;
                            continue;
                        }
                        if all_old_states.contains(right_state_lock) {
                            right_state_lock.borrow_mut().marker = true;
                            continue;
                        }
                    }
                }
            }

            if self_transition {
                left_state.marker = true;
            }
        }

        // Then, let's remove all listeners for transitions of the remaining old_states, register
        // new listeners for transitions of their successors (successors via `symbol`).
        for left_state_lock in all_old_states.iter() {
            let left_state = left_state_lock.borrow();
            for (sym, right_states) in left_state.explicit_transitions.iter() {
                if explicit == *sym {
                    // The listener for the transition via `symbol` has already been removed. Let's
                    // register new ones for the transitions of the new right states.
                    'outer: for right_state_lock in right_states.iter() {
                        let right_state = right_state_lock.borrow();

                        for (right_sym, _) in right_state.explicit_transitions.iter() {
                            if !self.explicit_listeners
                                .entry(right_sym.clone())
                                .or_insert_with(HashSet::new)
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
                } else {
                    // Remove listeners for transitions of the left_state (other than the one via
                    // `symbol` which is already removed).
                    self.explicit_listeners.get_mut(&sym).unwrap().remove(left_state_lock);
                }
            }

            if patterns.is_empty() {
                continue;
            }

            for (pattern, right_states) in left_state.pattern_transitions.iter() {
                if patterns.contains(pattern) {
                    // The listener for the transition via `symbol` has been already removed. Let's
                    // register new ones for the transitions of the new right states.
                    'outer: for right_state_lock in right_states.iter() {
                        let right_state = right_state_lock.borrow();

                        for (right_sym, _) in right_state.explicit_transitions.iter() {
                            if !self.explicit_listeners
                                .entry(right_sym.clone())
                                .or_insert_with(HashSet::new)
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
                } else {
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
                        Entry::Vacant(_) => {
                            panic!("Removing unregistered pattern listener.");
                        }
                    }
                }
            }
        }

        for left_state_lock in all_old_states.iter() {
            left_state_lock.borrow_mut().marker = false;
        }

        (explicit_old_states, pattern_old_statess)
    }

    pub fn get_active_satisfied_patterns(&self, explicit: Explicit) -> HashSet<Pattern> {
        get_satisfied_patterns(explicit, self.pattern_listeners.keys().cloned())
    }
}

pub fn get_satisfied_patterns<I: Iterator<Item=Pattern>>(explicit: Explicit, patterns: I)
    -> HashSet<Pattern>
{
    let mut result = HashSet::new();
    for pattern in patterns {
        match pattern.clone() {
            Pattern::Not(sym) => {
                if explicit != sym {
                    result.insert(pattern.clone());
                }
            },
            Pattern::Any => {
                result.insert(pattern.clone());
            },
            Pattern::CharRange(from, to) => {
                if let Explicit::Char(c) = explicit {
                    if c >= from && c <= to {
                        result.insert(pattern.clone());
                    }
                }
            },
            Pattern::NumCondition(num_cond) => {
                if let Explicit::Num(num) = explicit.clone() {
                    if num_cond.is_satisfied_by(num) {
                        result.insert(pattern.clone());
                    }
                }
            },
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{RcRefCellSelector, RcRefCell};

    fn new_state() -> RcRefCell<State<RcRefCellSelector>> {
        let result = RcRefCell::new(State {
            explicit_transitions: Box::new([]),
            pattern_transitions: Box::new([]),
            marker: false,
        });
        dbg!(&result);
        result
    }

    fn set_explicit(
        state: &RcRefCell<State<RcRefCellSelector>>,
        transitions: Vec<(Explicit, Vec<RcRefCell<State<RcRefCellSelector>>>)>,
    ) {
        state.borrow_mut().explicit_transitions =
            transitions.into_iter().map(|(sym, states)|
                (sym, states.into_boxed_slice())).collect::<Vec<_>>().into_boxed_slice();
    }

    fn set_pattern(
        state: &RcRefCell<State<RcRefCellSelector>>,
        transitions: Vec<(Pattern, Vec<RcRefCell<State<RcRefCellSelector>>>)>,
    ) {
        state.borrow_mut().pattern_transitions =
            transitions.into_iter().map(|(sym, states)|
                (sym, states.into_boxed_slice())).collect::<Vec<_>>().into_boxed_slice();
    }

    #[test]
    fn explicit_works() {
        let qs = vec![new_state(), new_state(), new_state(), new_state()];

        let my_set_explicit = |state_ix: usize, transitions: Vec<(char, Vec<usize>)>| {
            set_explicit(&qs[state_ix], transitions.into_iter().map(|(sym, states)|
                (
                    Explicit::Char(sym),
                    states.into_iter().map(
                        |i| qs[i].clone()).collect::<Vec<_>>()
                )
            ).collect::<Vec<_>>());
        };

        my_set_explicit(0, vec![('a', vec![1])]);
        my_set_explicit(1, vec![('b', vec![2])]);
        my_set_explicit(2, vec![('c', vec![0, 3])]);
        my_set_explicit(3, vec![('b', vec![0])]);

        let mut automaton = Listeners::<RcRefCellSelector>::new(vec![qs[0].clone()]);
        let mut read_and_check_predecessors = |sym: char, expected: Vec<usize>| {
            let pre = automaton.read(Explicit::Char(sym), HashSet::new());
            let expected2 = (
                HashSet::from_iter(expected.into_iter().map(|i| qs[i].clone())),
                vec![],
            );
            assert_eq!(pre, expected2);
        };

        // 0--- 0--a-->1
        read_and_check_predecessors('a', vec![0]);
        // -1-- 1--b-->2
        read_and_check_predecessors('b', vec![1]);
        // --2-
        read_and_check_predecessors('b', vec![]);
        // --2-
        read_and_check_predecessors('a', vec![]);
        // --2- 2--c-->0 2--c-->3
        read_and_check_predecessors('c', vec![2]);
        // 0--3 0--a-->1
        read_and_check_predecessors('a', vec![0]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors('b', vec![1, 3]);
        // 0-2- 2--c-->3
        read_and_check_predecessors('c', vec![2]);
        // 0--3 3--b-->0
        read_and_check_predecessors('b', vec![3]);
        // 0--- 0--a-->1
        read_and_check_predecessors('a', vec![0]);
        // -1-- 1--b-->2
        read_and_check_predecessors('b', vec![1]);
    }

    #[test]
    fn pattern_works() {
        let qs = vec![new_state(), new_state(), new_state(), new_state()];

        let my_set_explicit = |state_ix: usize, transitions: Vec<(char, Vec<usize>)>| {
            set_explicit(&qs[state_ix], transitions.into_iter().map(|(sym, states)|
                (
                    Explicit::Char(sym),
                    states.into_iter().map(
                        |i| qs[i].clone()).collect::<Vec<_>>()
                )
            ).collect::<Vec<_>>());
        };

        let pats = [
            Pattern::Any,
            Pattern::Not(Explicit::Char('b')),
        ];
        let any = 0;
        let nb = 1;

        let my_set_pattern = |state_ix: usize, transitions: Vec<(usize, Vec<usize>)>| {
            set_pattern(&qs[state_ix], transitions.into_iter().map(|(sym, states)|
                (
                    pats[sym].clone(),
                    states.into_iter().map(
                        |i| qs[i].clone()).collect::<Vec<_>>()
                )
            ).collect::<Vec<_>>());
        };

        my_set_explicit(0, vec![('a', vec![1])]);
        my_set_explicit(1, vec![('b', vec![2])]);
        my_set_explicit(2, vec![('c', vec![0, 3])]);
        my_set_explicit(3, vec![('b', vec![0])]);
        my_set_pattern(0, vec![(any, vec![3])]);
        my_set_pattern(3, vec![(nb, vec![3])]);

        let mut automaton = Listeners::<RcRefCellSelector>::new(vec![qs[0].clone()]);
        let mut read_and_check_predecessors =
            |
                sym: char,
                patterns: Vec<usize>,
                expected: Vec<usize>,
                expected_patterns: Vec<Vec<usize>>,
            | {
            let patterns2 = HashSet::from_iter(patterns.iter().map(|i| pats[*i].clone()));
            let pre = automaton.read(Explicit::Char(sym), patterns2.clone());
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
        read_and_check_predecessors('a', vec![any], vec![0], vec![vec![0]]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors('b', vec![], vec![1, 3], vec![]);
        // 0-2- 0-any->3
        read_and_check_predecessors('b', vec![any], vec![], vec![vec![0]]);
        // --23 3--nb->0
        read_and_check_predecessors('a', vec![nb], vec![], vec![vec![3]]);
        // -123 2--c-->0 2--c-->3 3--nb->3
        read_and_check_predecessors('c', vec![nb], vec![2], vec![vec![3]]);
        // 01-3 0--c-->1 0-any->3 3--nb->3
        read_and_check_predecessors('a', vec![any, nb], vec![0], vec![vec![0], vec![3]]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors('b', vec![], vec![1, 3], vec![]);
        // 0-2- 0-any->3 2--c-->3
        read_and_check_predecessors('c', vec![any], vec![2], vec![vec![0]]);
        // 0--3 3--b-->0
        read_and_check_predecessors('b', vec![], vec![3], vec![]);
        // 0--- 0--a-->1 0-any->3 3--nb->3
        read_and_check_predecessors('a', vec![any], vec![0], vec![vec![0]]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_predecessors('b', vec![], vec![1, 3], vec![]);
    }

    #[test]
    fn get_satisfied_patterns_works() {
        let pats = [
            Pattern::Any,
            Pattern::Not(Explicit::Char('b')),
            Pattern::CharRange('b', 'd'),
            Pattern::NumCondition(NumCondition::NotIn(Number::from(1), Number::from(4))),
        ];
        let any = 0;
        let nb = 1;
        let range = 2;
        let num = 3;

        let check_char = |sym: Explicit, patterns: Vec<usize>, expected: Vec<usize>| {
            let patterns2 = patterns.iter().map(|i| pats[*i].clone()).collect::<HashSet<_>>();
            let result = get_satisfied_patterns(sym, patterns2.into_iter());
            let expected2 = HashSet::from_iter(expected.into_iter().map(|i| pats[i].clone()));
            assert_eq!(result, expected2);
        };

        check_char(Explicit::Char('a'), vec![any], vec![any]);
        check_char(Explicit::Char('b'), vec![any, nb], vec![any]);
        check_char(Explicit::Char('a'), vec![any, nb], vec![any, nb]);
        check_char(Explicit::Char('a'), vec![nb], vec![nb]);
        check_char(Explicit::Char('b'), vec![nb, range, num], vec![range]);
        check_char(Explicit::Num(Number::from(5)), vec![any, nb, range, num], vec![any, nb, num]);
        check_char(Explicit::Num(Number::from(3)), vec![range, num], vec![]);
        check_char(Explicit::Num(Number::from(4)), vec![range, num], vec![]);
        check_char(Explicit::Num(Number::from(0)), vec![range, num], vec![num]);
        check_char(Explicit::Num(Number::from(1)), vec![range, num], vec![]);
    }
}
