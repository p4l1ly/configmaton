use hashbrown::HashMap;
use indexmap::IndexSet;  // we use IndexSet for faster worst-case iteration
use smallvec::SmallVec;

use crate::borrow_lock::Lock;
use crate::char_runner;

pub enum AnyStateLock<'a, TT> {
    Sparse(&'a SparseState<'a, TT>),
    Dense(&'a DenseState<'a, TT>),
}

pub struct Target<'a, TT> {
    pub right_states: SmallVec<[AnyStateLock<'a, TT>; 1]>,
    pub tran_trigger: TT,
}

pub trait TranListener<TT> {
    fn trigger(&mut self, tran_trigger: &TT);
}

pub enum Bdd<'a, TT> {
    Leaf(Target<'a, TT>),
    Node {
        tag: usize,
        positive: &'a Bdd<'a, TT>,
        negative: &'a Bdd<'a, TT>,
    }
}

impl<'a, TT> Bdd<'a, TT> {
    pub fn read(&self, tags: Vec<usize>) -> &Target<'a, TT> {
        let mut bdd = self;
        'outer: for present_tag in tags.iter() {
            loop {
                match bdd {
                    Bdd::Leaf(_) => { break 'outer; },
                    Bdd::Node { tag, positive, negative } => {
                        if tag == present_tag {
                            bdd = &positive;
                            break;
                        } else if tag < present_tag {
                            bdd = &negative;
                        } else {
                            break;
                        }
                    },
                }
            }
        }

        loop {
            match bdd {
                Bdd::Leaf(target) => {
                    return target;
                },
                Bdd::Node { tag: _, positive: _, negative } => { bdd = &negative; },
            }
        }
    }
}

pub struct Tran<'a, TT> {
    pub char_inits: SmallVec<[char_runner::AnyStateLock<'a>; 1]>,
    pub bdd: Bdd<'a, TT>,  // The BDD must be ordered.
}

pub struct SparseState<'a, TT> {
    // This is very like in the traditional finite automata. Each state has a set of transitions
    // via symbols to other states. If multiple states are parallel (let's say nondeterministic)
    // successors of a state via the same symbol, they are stored in a single vector.
    pub trans: Box<[(&'a str, Tran<'a, TT>)]>,
}

pub struct DenseState<'a, TT> {
    pub trans: HashMap<&'a str, Tran<'a, TT>>,
}

pub struct Runner<'a, TT> {
    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub sparse: HashMap<&'a str, IndexSet<Lock<'a, SparseState<'a, TT>>>>,
    pub dense: IndexSet<Lock<'a, DenseState<'a, TT>>>,
}

impl<'a, TT> Runner<'a, TT>
{
    // Initialize the state of the automaton.
    pub fn new<'b, I: IntoIterator<Item = &'b AnyStateLock<'a, TT>>>(initial_states: I) -> Self
        where 'a: 'b
    {
        let mut result = Runner{ sparse: HashMap::new(), dense: IndexSet::new() };
        for any_state_lock in initial_states { result.add_right_state(any_state_lock); }
        result
    }

    // Read a symbol, perform transitions.
    pub fn read<TL: TranListener<TT>>(&mut self, sym: &str, value: &str, tl: &mut TL) {
        let old_dense_states = std::mem::take(&mut self.dense);
        let mut trans = vec![];

        // Prepare the results.
        match self.sparse.get_mut(sym) {
            None => return,
            Some(states) => {
                let old_sparse_states = std::mem::take(states);

                // First, let's remove all listeners for transitions of the old states
                for left_state_lock in old_sparse_states.iter() {
                    let left_state = left_state_lock.0;
                    for (key, _tran) in left_state.trans.iter() {
                        if sym != *key {
                            // Remove listeners for transitions of the left_state (other than the
                            // one via `symbol` which is already removed).
                            self.sparse.get_mut(key).unwrap().swap_remove(left_state_lock);
                        }
                    }
                }

                // Then, let's register new listeners for transitions of the successors.
                for left_state_lock in old_sparse_states.iter() {
                    let left_state = left_state_lock.0;
                    for (key, tran) in left_state.trans.iter() {
                        if sym == *key {
                            trans.push(tran);
                        }
                    }
                }
            },
        }

        for left_state_lock in old_dense_states.iter() {
            let left_state = left_state_lock.0;
            if let Some(tran) = left_state.trans.get(sym) {
                trans.push(tran);
            } else {
                self.dense.insert(left_state_lock.clone());
            }
        }

        let mut crunner =
            char_runner::Runner::new(trans.iter().flat_map(|tran| tran.char_inits.iter()));

        for c in value.chars() {
            crunner.read(c as u8);
        }

        let mut tags = crunner.get_tags();
        tags.sort_unstable();
        tags.dedup();

        for tran in trans {
            let target = tran.bdd.read(tags);
            for right_state_lock in target.right_states.iter() {
                self.add_right_state(right_state_lock);
            }
            tl.trigger(&target.tran_trigger);
            break;
        }
    }

    fn add_right_state(&mut self, state_lock: &AnyStateLock<'a, TT>) {
        match state_lock {
            AnyStateLock::Sparse(state) => {
                for (gsym, _) in state.trans.iter() {
                    self.sparse.entry(gsym).or_insert(IndexSet::new()).insert(Lock(state));
                }
            },
            AnyStateLock::Dense(state) => {
                self.dense.insert(Lock(state));
            },
        }
    }
}
