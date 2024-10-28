use hashbrown::{HashMap, HashSet, hash_map::Entry};
use indexmap::IndexSet;  // we use IndexSet for faster worst-case iteration
use std::{hash::Hash, mem::MaybeUninit};
use std::ptr::addr_of_mut;

use crate::guards::Guard;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Explicit {
    Var(String),
    EndVar,
    Char(u8),
}

pub type Pattern = Guard;

pub struct Lock<'a, T> (pub &'a T);

impl<'a, T> Clone for Lock<'a, T> {
    fn clone(&self) -> Self {
        Lock(self.0)
    }
}

impl<'a, T> Hash for Lock<'a, T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(self.0 as *const T, state);
    }
}

impl<'a, T> PartialEq for Lock<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0 as *const T, other.0 as *const T)
    }
}

impl<'a, T> Eq for Lock<'a, T> { }

type StateLock<'a, TT> = Lock<'a, State<Tran<'a, TT>>>;
type SelfHandlingSparseStateLock<'a, TT> = Lock<'a, SelfHandlingSparseState<Tran<'a, TT>>>;
type SelfHandlingDenseStateLock<'a, TT> = Lock<'a, SelfHandlingDenseState<Tran<'a, TT>>>;
type TranBodyLock<'a, TT> = Lock<'a, TranBody<'a, TT>>;

// There will be always at least one successor and mostly only one. Therefore we store the first
// successor specially, to avoid indirection.
pub struct Succ<'a, TT>(pub AnyStateLock<'a, TT>, pub Box<[AnyStateLock<'a, TT>]>);

impl<'a, TT> Clone for Succ<'a, TT> {
    fn clone(&self) -> Self {
        Succ(self.0.clone(), self.1.clone())
    }
}

pub enum AnyStateLock<'a, TT> {
    Normal(StateLock<'a, TT>),
    Sparse(SelfHandlingSparseStateLock<'a, TT>),
    Dense(SelfHandlingDenseStateLock<'a, TT>),
    None,
}

impl<'a, TT> Clone for AnyStateLock<'a, TT> {
    fn clone(&self) -> Self {
        match self {
            AnyStateLock::Normal(lock) => AnyStateLock::Normal(lock.clone()),
            AnyStateLock::Sparse(lock) => AnyStateLock::Sparse(lock.clone()),
            AnyStateLock::Dense(lock) => AnyStateLock::Dense(lock.clone()),
            AnyStateLock::None => AnyStateLock::None,
        }
    }
}

pub struct TranBody<'a, TT> {
    pub right_states: Succ<'a, TT>,
    pub tran_trigger: TT,
}

pub enum Tran<'a, TT> {
    Owned(TranBody<'a, TT>),
    Shared(TranBodyLock<'a, TT>),
}

pub trait TranListener<TT> {
    fn trigger(&mut self, tran_trigger: &TT);
}

pub struct SelfHandlingSparseState<T> {
    pub explicit_trans: HashMap<Explicit, T>,
    pub pattern_trans: Box<[(Pattern, T)]>,
}

impl<T> SelfHandlingSparseState<T> {
    pub fn map<T2, F: FnMut(T) -> T2>(self, mut f: F) -> SelfHandlingSparseState<T2> {
        SelfHandlingSparseState {
            explicit_trans: self.explicit_trans.into_iter().map(|(k, v)| (k, f(v))).collect(),
            pattern_trans: self.pattern_trans.into_vec().into_iter()
                .map(|(k, v)| (k, f(v))).collect::<Vec<_>>().into_boxed_slice(),
        }
    }
}

pub struct SelfHandlingDenseState<T>(pub [Option<T>; 257]);

impl<T> SelfHandlingDenseState<T> {
    pub fn map<T2, F: FnMut(T) -> T2 + Copy>(self, f: F) -> SelfHandlingDenseState<T2> {
        SelfHandlingDenseState(self.0.map(|x| x.map(f)))
    }
}

pub struct State<T> {
    // This is very like in the traditional finite automata. Each state has a set of transitions
    // via symbols to other states. If multiple states are parallel (let's say nondeterministic)
    // successors of a state via the same symbol, they are stored in a single vector.
    pub explicit_trans: Box<[(Explicit, T)]>,
    pub pattern_trans: Box<[(Pattern, T)]>,
}

impl<T> State<T> {
    pub fn map<T2, F: FnMut(T) -> T2>(self, mut f: F) -> State<T2> {
        State {
            explicit_trans:
                self.explicit_trans.into_vec().into_iter().map(|(k, v)| (k, f(v))).collect(),
            pattern_trans:
                self.pattern_trans.into_vec().into_iter().map(|(k, v)| (k, f(v))).collect(),
        }
    }
}

pub struct ExplicitListeners<'a, TT> {
    vars: HashMap<String, IndexSet<StateLock<'a, TT>>>,
    end_var: IndexSet<StateLock<'a, TT>>,
    chars: [IndexSet<StateLock<'a, TT>>; 256],
}

impl<'a, TT> ExplicitListeners<'a, TT>
{
    pub fn new() -> Self {
        ExplicitListeners {
            vars: HashMap::new(),
            end_var: IndexSet::new(),
            chars: unsafe {
                let mut chars = MaybeUninit::uninit();
                let pchars: *mut [IndexSet<StateLock<'a, TT>>; 256] = chars.as_mut_ptr();
                for i in 0..256 {
                    addr_of_mut!((*pchars)[i]).write(IndexSet::new());
                }
                chars.assume_init()
            },
        }
    }

    pub fn add(&mut self, sym: Explicit, state: StateLock<'a, TT>) {
        match sym {
            Explicit::Var(s) => {
                self.vars.entry(s).or_insert_with(IndexSet::new).insert(state);
            },
            Explicit::EndVar => {
                self.end_var.insert(state);
            },
            Explicit::Char(c) => {
                self.chars[c as usize].insert(state);
            },
        }
    }

    pub fn get_mut(&mut self, sym: &Explicit) -> &mut IndexSet<StateLock<'a, TT>> {
        match sym {
            Explicit::Var(s) => {
                self.vars.entry(s.clone()).or_insert_with(IndexSet::new)
            }
            Explicit::EndVar => &mut self.end_var,
            Explicit::Char(c) => &mut self.chars[*c as usize],
        }
    }
}

impl<'a, TT> std::fmt::Debug for ExplicitListeners<'a, TT>
where StateLock<'a, TT>: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExplicitListeners")
            .field("vars", &self.vars)
            .field("end_var", &self.end_var)
            .field("chars", &self.chars)
            .finish()
    }
}

pub struct Listeners<'a, TT> {
    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub explicit_listeners: ExplicitListeners<'a, TT>,
    pub pattern_listeners: HashMap<Pattern, IndexSet<StateLock<'a, TT>>>,
    pub self_handling_sparse_states: Vec<SelfHandlingSparseStateLock<'a, TT>>,
    pub self_handling_dense_states: Vec<SelfHandlingDenseStateLock<'a, TT>>,
}

impl<'a, TT> Listeners<'a, TT>
{
    // Initialize the state of the automaton.
    pub fn new<I: IntoIterator<Item = AnyStateLock<'a, TT>>>(initial_states: I) -> Self
    {
        let mut result = Listeners {
            explicit_listeners: ExplicitListeners::new(),
            pattern_listeners: HashMap::new(),
            self_handling_sparse_states: Vec::new(),
            self_handling_dense_states: Vec::new(),
        };
        for any_state_lock in initial_states.into_iter() {
            result.add_right_state(&any_state_lock);
        }
        result
    }

    // Read a symbol, perform transitions.
    pub fn read<TL: TranListener<TT>>(&mut self, explicit: Explicit, tl: &mut TL) {
        // Prepare the results.
        let mut all_old_states = std::mem::take(self.explicit_listeners.get_mut(&explicit));
        let mut any_pattern = false;

        if let Explicit::Char(c) = explicit {
            self.pattern_listeners.retain(|pattern, states|
                if pattern.contains(c) {
                    any_pattern = true;
                    all_old_states.extend(states.drain(..));
                    false
                } else { true }
            );
        }

        let self_handling_sparse_states = std::mem::take(&mut self.self_handling_sparse_states);
        let self_handling_dense_states = std::mem::take(&mut self.self_handling_dense_states);

        // First, let's remove all listeners for transitions of the old states
        for left_state_lock in all_old_states.iter() {
            let left_state = left_state_lock.0;
            for (sym, _) in left_state.explicit_trans.iter() {
                if explicit != *sym {
                    // Remove listeners for transitions of the left_state (other than the one via
                    // `symbol` which is already removed).
                    self.explicit_listeners.get_mut(&sym).swap_remove(left_state_lock);
                }
            }

            for (pattern, _) in left_state.pattern_trans.iter() {
                // Remove listeners for transitions of the left_state (other than the one via
                // `pattern` which is already removed). This is different from the explicit
                // part because we want to remove the `pattern` key from `pattern_listeners`,
                // because `pattern_listeners` are iterated over in `get_satisfied_patterns`.

                match self.pattern_listeners.entry(pattern.clone()) {
                    Entry::Occupied(mut entry) => {
                        let is_empty = {
                            let x = entry.get_mut();
                            x.swap_remove(left_state_lock);
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
        let mut visited_trans = HashSet::new();

        let mut follow_tran_body = |slf: &mut Self, body: &TranBody<'a, TT>| {
            slf.add_right_state(&body.right_states.0);
            for right_state in body.right_states.1.iter() {
                slf.add_right_state(right_state);
            }
            tl.trigger(&body.tran_trigger);
        };

        let mut follow_tran = |slf: &mut Self, tran: &Tran<'a, TT>| {
            match tran {
                Tran::Owned(body) => follow_tran_body(slf, body),
                Tran::Shared(lock) => {
                    if !visited_trans.insert(lock.clone()) { return; }
                    follow_tran_body(slf, lock.0);
                },
            }
        };

        for left_state_lock in all_old_states.iter() {
            let left_state = left_state_lock.0;
            for (sym, tran) in left_state.explicit_trans.iter() {
                if explicit == *sym { follow_tran(self, tran); }
            }

            if !any_pattern { continue; }

            if let Explicit::Char(c) = explicit {
                for (pattern, tran) in left_state.pattern_trans.iter() {
                    if pattern.contains(c) { follow_tran(self, tran); }
                }
            }
        }

        // Finally, let's handle the self-handling states.
        for state_lock in self_handling_sparse_states.into_iter() {
            let state = state_lock.0;
            let mut transitioned = false;
            state.explicit_trans.get(&explicit).map(|tran| {
                transitioned = true;
                follow_tran(self, tran);
            });
            if let Explicit::Char(c) = explicit {
                for (pattern, tran) in state.pattern_trans.iter() {
                    if pattern.contains(c) {
                        transitioned = true;
                        follow_tran(self, tran);
                    }
                }
            }
            if !transitioned {
                self.self_handling_sparse_states.push(state_lock.clone());
            }
        }

        for state_lock in self_handling_dense_states.into_iter() {
            let state = state_lock.0;
            let mtran = match &explicit {
                Explicit::Char(c) => &state.0[*c as usize],
                Explicit::EndVar => &state.0[256],
                _ => panic!("Var-Chars alternation violation"),
            };
            match mtran {
                Some(x) => follow_tran(self, x),
                None => self.self_handling_dense_states.push(state_lock.clone()),
            }
        }
    }

    fn add_right_state(&mut self, any_state_lock: &AnyStateLock<'a, TT>) {
        match any_state_lock {
            AnyStateLock::Normal(state_lock) => {
                let state = state_lock.0;
                for (symbol, _) in state.explicit_trans.iter() {
                    if !self.explicit_listeners.get_mut(&symbol).insert(state_lock.clone())
                        { return; }
                }

                for (pattern, _) in state.pattern_trans.iter() {
                    if !self.pattern_listeners
                        .entry(pattern.clone())
                        .or_insert_with(IndexSet::new)
                        .insert(state_lock.clone())
                        { return; }
                }
            },
            AnyStateLock::Sparse(state_lock) => {
                self.self_handling_sparse_states.push(state_lock.clone());
            },
            AnyStateLock::Dense(state_lock) => {
                self.self_handling_dense_states.push(state_lock.clone());
            },
            AnyStateLock::None => { },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTranListener (HashSet<u8>);

    impl TranListener<u8> for TestTranListener {
        fn trigger(&mut self, tran_trigger: &u8) {
            self.0.insert(*tran_trigger);
        }
    }

    type TestTran<'a> = Tran<'a, u8>;
    type TestState<'a> = Lock<'a, State<TestTran<'a>>>;

    fn new_state<'a>() -> State<TestTran<'a>> {
        let result = State {
            explicit_trans: Box::new([]),
            pattern_trans: Box::new([]),
        };
        result
    }

    fn new_tran(states: Vec<TestState>, tt: u8) -> TestTran {
        let mut states = states.into_iter().map(AnyStateLock::Normal);
        Tran::Owned(TranBody {
            right_states: Succ(
                states.next().unwrap(),
                states.collect::<Vec<_>>().into_boxed_slice(),
            ),
            tran_trigger: tt,
        })
    }

    unsafe fn lock<'a, T>(x: *const T) -> Lock<'a, T> {
        Lock(&*x)
    }

    fn set_explicit<'a>(
        state: &mut State<TestTran<'a>>,
        trans: Vec<(Explicit, TestTran<'a>)>,
    ) {
        state.explicit_trans = trans.into_boxed_slice();
    }

    fn set_pattern<'a>(
        state: &mut State<TestTran<'a>>,
        trans: Vec<(Pattern, TestTran<'a>)>,
    ) {
        state.pattern_trans = trans.into_boxed_slice();
    }

    #[test]
    fn explicit_works() {
        let (a, b, c) = (0, 1, 2);

        let a01 = 0;
        let b12 = 1;
        let c203 = 2;
        let b30 = 3;

        let mut qs = vec![new_state(), new_state(), new_state(), new_state()];
        unsafe {
            for (left, c, rights, tt) in [
                (0, a, vec![1], a01),
                (1, b, vec![2], b12),
                (2, c, vec![0, 3], c203),
                (3, b, vec![0], b30)
            ] {
                let rights = rights.into_iter().map(|i| lock(&qs[i])).collect::<Vec<_>>();
                let tran = new_tran(rights, tt);
                set_explicit(&mut qs[left], vec![(Explicit::Char(c), tran)]);
            }
        }

        let mut automaton = Listeners::new(vec![AnyStateLock::Normal(Lock(&qs[0]))]);
        let mut tl = TestTranListener(HashSet::new());
        let mut read_and_check_trans = |sym: u8, expected: Vec<u8>| {
            tl.0.clear();
            automaton.read(Explicit::Char(sym), &mut tl);
            let expected = expected.into_iter().collect::<HashSet<_>>();
            assert_eq!(tl.0, expected);
        };

        // 0--- 0--a-->1
        read_and_check_trans(a, vec![a01]);
        // -1-- 1--b-->2
        read_and_check_trans(b, vec![b12]);
        // --2-
        read_and_check_trans(b, vec![]);
        // --2-
        read_and_check_trans(a, vec![]);
        // --2- 2--c-->0 2--c-->3
        read_and_check_trans(c, vec![c203]);
        // 0--3 0--a-->1
        read_and_check_trans(a, vec![a01]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_trans(b, vec![b12, b30]);
        // 0-2- 2--c-->3
        read_and_check_trans(c, vec![c203]);
        // 0--3 3--b-->0
        read_and_check_trans(b, vec![b30]);
        // 0--- 0--a-->1
        read_and_check_trans(a, vec![a01]);
        // -1-- 1--b-->2
        read_and_check_trans(b, vec![b12]);
    }

    #[test]
    fn pattern_works() {
        let mut qs = vec![new_state(), new_state(), new_state(), new_state()];

        let (a, b, c) = (0, 1, 2);

        let a01 = 0;
        let b12 = 1;
        let c203 = 2;
        let b30 = 3;
        let any03 = 4;
        let nb33 = 5;

        unsafe {
            for (left, c, rights, tt) in [
                (0, a, vec![1], a01),
                (1, b, vec![2], b12),
                (2, c, vec![0, 3], c203),
                (3, b, vec![0], b30)
            ] {
                let rights = rights.into_iter().map(|i| lock(&qs[i])).collect::<Vec<_>>();
                let tran = new_tran(rights, tt);
                set_explicit(&mut qs[left], vec![(Explicit::Char(c), tran)]);
            }
        }

        let pats = [
            Guard::from_ranges(vec![(0, 255)]),
            Guard::from_ranges(vec![(0, 0), (2, 255)]),
        ];
        let (any, nb) = (0, 1);

        unsafe {
            for (left, pat, rights, tt) in [
                (0, any, vec![3], any03),
                (3, nb, vec![3], nb33),
            ] {
                let rights = rights.into_iter().map(|i| lock(&qs[i])).collect::<Vec<_>>();
                let tran = new_tran(rights, tt);
                set_pattern(&mut qs[left], vec![(pats[pat].clone(), tran)]);
            }
        }

        let mut automaton = Listeners::new(vec![AnyStateLock::Normal(Lock(&qs[0]))]);
        let mut tl = TestTranListener(HashSet::new());
        let mut read_and_check_trans = |sym: u8, expected: Vec<u8>| {
            tl.0.clear();
            automaton.read(Explicit::Char(sym), &mut tl);
            let expected = expected.into_iter().collect::<HashSet<_>>();
            assert_eq!(tl.0, expected);
        };

        // 0--- 0--a-->1 0-any->3
        read_and_check_trans(a, vec![a01, any03]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_trans(b, vec![b12, b30]);
        // 0-2- 0-any->3
        read_and_check_trans(b, vec![any03]);
        // --23 3--nb->3
        read_and_check_trans(a, vec![nb33]);
        // -123 2--c-->0 2--c-->3 3--nb->3
        read_and_check_trans(c, vec![c203, nb33]);
        // 01-3 0--a-->1 0-any->3 3--nb->3
        read_and_check_trans(a, vec![a01, any03, nb33]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_trans(b, vec![b12, b30]);
        // 0-2- 0-any->3 2--c-->3
        read_and_check_trans(c, vec![any03, c203]);
        // 0--3 0-any->3 3--b-->0
        read_and_check_trans(b, vec![any03, b30]);
        // 0--3 0--a-->1 0-any->3 3--nb->3
        read_and_check_trans(a, vec![a01, any03, nb33]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_trans(b, vec![b12, b30]);
    }
}
