use std::{collections::{HashMap, HashSet}, hash::Hash};

use crate::lock::{LockSelector, Lock};

pub struct State<Symbol: Eq, Out, S: LockSelector> {
    // This is very like in the traditional finite automata. Each state has a set of transitions
    // via symbols to other states. If multiple states are parallel (let's say nondeterministic)
    // successors of a state via the same symbol, they are stored in a single vector.
    pub transitions: Box<[(Symbol, Box<[S::Lock<State<Symbol, Out, S>>]>)]>,

    // In traditional finite automata, this would be a boolean value (final / nonfinal state).
    // Here, we will combine and nest multiple "traditional" automata into one, and the higher
    // level reasoning needs to know, which automaton of the original ones is currently in a final
    // nonfinal state. The markers for the original automata that are in a final configuration when
    // we are in this state are stored here.
    pub outputs: Box<[Out]>,
}

pub struct Automaton<Symbol: Eq, Out: Hash + Eq, S: LockSelector> {
    // Union of outputs of the current states.
    pub outputs: HashMap<Out, usize>,

    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub listeners: HashMap<Symbol, HashSet<S::Lock<State<Symbol, Out, S>>>>,
}

impl<Symbol: Hash + Eq + Copy, Out: Hash + Eq + Copy, S: LockSelector>
Automaton<Symbol, Out, S>
where S::Lock<State<Symbol, Out, S>>: Hash + Eq,
{
    // Initialize the state of the automaton.
    pub fn new<I>(initial_states: I) -> Self
    where
        I: IntoIterator<Item = S::Lock<State<Symbol, Out, S>>>,
    {
        let mut outputs = HashMap::new();
        let mut listeners = HashMap::new();
        for state_lock in initial_states.into_iter() {
            let state = state_lock.borrow();
            for (symbol, _) in state.transitions.iter() {
                listeners
                    .entry(*symbol)
                    .or_insert_with(HashSet::new)
                    .insert(state_lock.clone());
            }
            for output in state.outputs.iter() {
                *outputs.entry(*output).or_insert(0) += 1;
            }
        }
        Automaton { outputs, listeners }
    }

    // Read a symbol, perform transitions, and return new and removed outputs.
    pub fn read(&mut self, symbol: Symbol) -> (Vec<Out>, Vec<Out>) {
        let mut old_states: HashSet<S::Lock<State<Symbol, Out, S>>>;
        let mut new_outputs = Vec::new();
        let mut removed_outputs = Vec::new();

        let new_states = if let Some(states) = self.listeners.get_mut(&symbol) {
            if states.is_empty() {
                return (new_outputs, removed_outputs);
            }

            old_states = HashSet::new();
            std::mem::swap(&mut old_states, states);
            states
        } else {
            return (new_outputs, removed_outputs);
        };

        // First, let's detect, which old_states will not be removed. We remove them from
        // old_states and reinsert them to the new_states.
        for left_state_lock in old_states.iter() {
            let left_state = left_state_lock.borrow();
            for (sym, right_states) in left_state.transitions.iter() {
                let sym = *sym;
                if sym == symbol {
                    for right_state_lock in right_states.iter() {
                        if old_states.contains(right_state_lock) {
                            new_states.insert(right_state_lock.clone());
                            continue;
                        }
                    }
                }
            }
        }
        for state_lock in new_states.iter() {
            old_states.remove(state_lock);
        }

        // Then, let's remove all listeners for transitions of the remaining old_states, register
        // new listeners for transitions of their successors (successors via `symbol`), from which
        // we also update the outputs.
        for left_state_lock in old_states.iter() {
            let left_state = left_state_lock.borrow();
            for (sym, right_states) in left_state.transitions.iter() {
                let sym = *sym;
                if sym == symbol {
                    // The listener for the transition via `symbol` has already been removed. Let's
                    // register new ones for the transitions of the new right states. Also, the
                    // outputs of the new right states are added to the outputs of the automaton.
                    for right_state_lock in right_states.iter() {
                        let right_state = right_state_lock.borrow();

                        // This is initialized to `true` because if there are no transitions from
                        // the state, we never escape it, therefore we don't mind incrementing its
                        // outputs each time... It's impossible in the current data structure to
                        // detect presence of the state that has no transitions, but as we've just
                        // explained, we don't mind it. If we'd mind it, we could add a transition
                        // via a special symbol which never gets read to those terminal states.
                        let mut right_state_is_new = true;

                        // Add the transitions, detect if right_state is new.
                        for (right_sym, _) in right_state.transitions.iter() {
                            right_state_is_new = self.listeners
                                .entry(*right_sym)
                                .or_insert_with(HashSet::new)
                                .insert(right_state_lock.clone());
                            if !right_state_is_new {
                                // all other transitions are certainly already registered too.
                                break;
                            }
                        }

                        // Update the automaton outputs with outputs of new right states.
                        if right_state_is_new {
                            for output in right_state.outputs.iter() {
                                let entry = self.outputs.entry(*output).or_insert(0);
                                if *entry == 0 {
                                    *entry = 1;
                                    new_outputs.push(*output);
                                } else {
                                    *entry += 1;
                                }
                            }
                        }
                    }
                } else {
                    // Remove listeners for transitions of the left_state (other than the one via
                    // `symbol` which is already removed).
                    self.listeners.get_mut(&sym).unwrap().remove(left_state_lock);
                }
            }
        }

        // Finally, subtract outputs of old_states from the automaton outputs. This must be done at
        // the end, otherwise `new_outputs` could contain something that is not really new.
        for left_state_lock in old_states.iter() {
            let left_state = left_state_lock.borrow();
            for out in left_state.outputs.iter() {
                if let Some(count) = self.outputs.get_mut(out) {
                    *count -= 1;
                    if *count == 0 {
                        self.outputs.remove(out);
                        removed_outputs.push(*out);
                    }
                }
            }
        }

        return (new_outputs, removed_outputs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{RcRefCellSelector, RcRefCell};

    fn sorted2<T: Ord>(mut x: (Vec<T>, Vec<T>)) -> (Vec<T>, Vec<T>) {
        x.0.sort();
        x.1.sort();
        x
    }

    #[test]
    fn it_works() {
        let state1: RcRefCell<State<_, _, RcRefCellSelector>> = RcRefCell::new(State {
            transitions: Box::new([(1, Box::new([]))]),
            outputs: Box::new([1]),
        });
        let state2: RcRefCell<State<_, _, RcRefCellSelector>> = RcRefCell::new(State {
            transitions: Box::new([(2, Box::new([]))]),
            outputs: Box::new([2]),
        });
        let state3: RcRefCell<State<_, _, RcRefCellSelector>> = RcRefCell::new(State {
            transitions: Box::new([(3, Box::new([state1.clone()]))]),
            outputs: Box::new([3]),
        });
        let state4: RcRefCell<State<_, _, RcRefCellSelector>> = RcRefCell::new(State {
            transitions: Box::new([(2, Box::new([state1.clone()]))]),
            outputs: Box::new([4]),
        });

        state1.borrow_mut().transitions[0].1 = Box::new([state2.clone()]);
        state2.borrow_mut().transitions[0].1 = Box::new([state3.clone()]);
        state3.borrow_mut().transitions[0].1 = Box::new([state1.clone(), state4.clone()]);

        let mut automaton = Automaton::<u8, u8, RcRefCellSelector>::new(vec![state1]);
        let no_out: Vec<u8> = Vec::new();
        let no_change: (Vec<u8>, Vec<u8>) = (no_out.clone(), no_out.clone());

        assert_eq!(automaton.outputs.keys().cloned().collect::<Vec<_>>(), vec![1]);
        assert_eq!(automaton.read(1), (vec![2], vec![1]));
        assert_eq!(automaton.read(2), (vec![3], vec![2]));
        assert_eq!(automaton.read(2), no_change);
        assert_eq!(automaton.read(1), no_change);
        assert_eq!(sorted2(automaton.read(3)), (vec![1, 4], vec![3]));
        assert_eq!(automaton.read(1), (vec![2], vec![1]));
        assert_eq!(sorted2(automaton.read(2)), (vec![1, 3], vec![2, 4]));
        assert_eq!(automaton.read(3), (vec![4], vec![3]));
        assert_eq!(automaton.read(2), (no_out.clone(), vec![4]));
        assert_eq!(automaton.read(1), (vec![2], vec![1]));
    }
}
