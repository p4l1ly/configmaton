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

    // Read a symbol, perform transitions.
    pub fn read(&mut self, explicit: Explicit, patterns: HashSet<Pattern>)
        -> (HashSet<StateLock<S>>, Vec<HashSet<StateLock<S>>>)
    {
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
            let left_state = left_state_lock.borrow();
            for (sym, right_states) in left_state.explicit_transitions.iter() {
                if explicit == *sym {
                    for right_state_lock in right_states.iter() {
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
                        if all_old_states.contains(right_state_lock) {
                            right_state_lock.borrow_mut().marker = true;
                            continue;
                        }
                    }
                }
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
                    for right_state_lock in right_states.iter() {
                        let right_state = right_state_lock.borrow();

                        for (right_sym, _) in right_state.explicit_transitions.iter() {
                            let right_state_is_new = self.explicit_listeners
                                .entry(right_sym.clone())
                                .or_insert_with(HashSet::new)
                                .insert(right_state_lock.clone());
                            if !right_state_is_new {
                                // all other transitions are certainly already registered too.
                                break;
                            }
                        }

                        for (right_sym, _) in right_state.pattern_transitions.iter() {
                            let right_state_is_new = self.pattern_listeners
                                .entry(right_sym.clone())
                                .or_insert_with(HashSet::new)
                                .insert(right_state_lock.clone());
                            if !right_state_is_new {
                                // all other transitions are certainly already registered too.
                                break;
                            }
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
                    for right_state_lock in right_states.iter() {
                        let right_state = right_state_lock.borrow();

                        for (right_sym, _) in right_state.explicit_transitions.iter() {
                            let right_state_is_new = self.explicit_listeners
                                .entry(right_sym.clone())
                                .or_insert_with(HashSet::new)
                                .insert(right_state_lock.clone());
                            if !right_state_is_new {
                                // all other transitions are certainly already registered too.
                                break;
                            }
                        }

                        for (right_sym, _) in right_state.pattern_transitions.iter() {
                            let right_state_is_new = self.pattern_listeners
                                .entry(right_sym.clone())
                                .or_insert_with(HashSet::new)
                                .insert(right_state_lock.clone());
                            if !right_state_is_new {
                                // all other transitions are certainly already registered too.
                                break;
                            }
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


}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{RcRefCellSelector, RcRefCell};

    fn new_state() -> RcRefCell<State<RcRefCellSelector>> {
        RcRefCell::new(State {
            explicit_transitions: Box::new([]),
            pattern_transitions: Box::new([]),
            marker: false,
        })
    }

    fn set_explicit(
        state: &RcRefCell<State<RcRefCellSelector>>,
        transitions: Vec<(Explicit, Vec<RcRefCell<State<RcRefCellSelector>>>)>,
    ) {
        state.borrow_mut().explicit_transitions =
            transitions.into_iter().map(|(sym, states)|
                (sym, states.into_boxed_slice())).collect::<Vec<_>>().into_boxed_slice();
    }

    #[test]
    fn explicit_works() {
        let qs = vec![new_state(), new_state(), new_state(), new_state()];

        let to_listeners = |state_ixs: Vec<usize>| {
            (HashSet::from_iter(state_ixs.into_iter().map(|i| qs[i].clone())), vec![])
        };

        let my_set_explicit = |state_ix: usize, transitions: Vec<(char, Vec<usize>)>| {
            set_explicit(&qs[state_ix], transitions.into_iter().map(|(sym, states)|
                (
                    Explicit::Char(sym),
                    states.into_iter().map(
                        |i| qs[i].clone()).collect::<Vec<_>>()
                )
            ).collect::<Vec<_>>());
        };

        my_set_explicit(0, vec![('1', vec![1])]);
        my_set_explicit(1, vec![('2', vec![2])]);
        my_set_explicit(2, vec![('3', vec![0, 3])]);
        my_set_explicit(3, vec![('2', vec![0])]);

        let mut automaton = Listeners::<RcRefCellSelector>::new(vec![qs[0].clone()]);
        let mut my_read = |sym: char| {
            automaton.read(Explicit::Char(sym), HashSet::new())
        };
        let mut read_and_check_predecessors = |sym: char, expected: Vec<usize>| {
            assert_eq!(my_read(sym), to_listeners(expected));
        };

        read_and_check_predecessors('1', vec![0]);
        read_and_check_predecessors('2', vec![1]);
        read_and_check_predecessors('2', vec![]);
        read_and_check_predecessors('1', vec![]);
        read_and_check_predecessors('3', vec![2]);
        read_and_check_predecessors('1', vec![0]);
        read_and_check_predecessors('2', vec![1, 3]);
        read_and_check_predecessors('3', vec![2]);
        read_and_check_predecessors('2', vec![3]);
        read_and_check_predecessors('1', vec![0]);
        read_and_check_predecessors('2', vec![1]);
    }
}
