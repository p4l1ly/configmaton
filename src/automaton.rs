use hashbrown::{HashMap, HashSet, hash_map::Entry};
use indexmap::IndexSet;  // we use IndexSet for faster worst-case iteration
use smallvec::SmallVec;
use std::{hash::Hash, mem::MaybeUninit};
use std::ptr::addr_of_mut;

use crate::borrow_lock::Lock;

type StateLock<'a, TT> = Lock<'a, State<'a, TT>>;

pub struct Tran<'a, TT> {
    pub right_states: SmallVec<[StateLock<'a, TT>; 1]>,
    pub tran_trigger: TT,
}

pub trait TranListener<TT> {
    fn trigger(&mut self, tran_trigger: &TT);
}

pub struct State<'a, TT> {
    // This is very like in the traditional finite automata. Each state has a set of transitions
    // via symbols to other states. If multiple states are parallel (let's say nondeterministic)
    // successors of a state via the same symbol, they are stored in a single vector.
    pub explicit_trans: Box<[(String, Tran<'a, TT>)]>,
}

pub struct Listeners<'a, TT> (
    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub HashMap<String, IndexSet<StateLock<'a, TT>>>,
);

impl<'a, TT> Listeners<'a, TT>
{
    // Initialize the state of the automaton.
    pub fn new<I: IntoIterator<Item = StateLock<'a, TT>>>(initial_states: I) -> Self
    {
        let mut result = Listeners(HashMap::new());
        for any_state_lock in initial_states { result.add_right_state(any_state_lock); }
        result
    }

    fn get_mut(&mut self, sym: String) -> &mut IndexSet<StateLock<'a, TT>> {
        self.0.entry(sym).or_insert_with(IndexSet::new)
    }

    // Read a symbol, perform transitions.
    pub fn read<TL: TranListener<TT>>(&mut self, sym: String, tl: &mut TL) {
        // Prepare the results.
        let old_states = std::mem::take(self.get_mut(sym.clone()));

        // First, let's remove all listeners for transitions of the old states
        for left_state_lock in old_states.iter() {
            let left_state = left_state_lock.0;
            for (gsym, _) in left_state.explicit_trans.iter() {
                if sym != *gsym {
                    // Remove listeners for transitions of the left_state (other than the one via
                    // `symbol` which is already removed).
                    self.get_mut(gsym.clone()).swap_remove(left_state_lock);
                }
            }
        }

        // Then, let's register new listeners for transitions of the successors.
        let mut follow_tran = |slf: &mut Self, tran: &Tran<'a, TT>| {
            for right_state in tran.right_states.iter() {
                slf.add_right_state(right_state.clone());
            }
            tl.trigger(&tran.tran_trigger);
        };

        for left_state_lock in old_states.iter() {
            let left_state = left_state_lock.0;
            for (gsym, tran) in left_state.explicit_trans.iter() {
                if sym == *gsym { follow_tran(self, tran); }
            }
        }
    }

    fn add_right_state(&mut self, state_lock: StateLock<'a, TT>) {
        let state = state_lock.0;
        for (gsym, _) in state.explicit_trans.iter() {
            if !self.get_mut(gsym.clone()).insert(state_lock.clone())
                { return; }
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
        trans: Vec<(Guard, TestTran<'a>)>,
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
}
