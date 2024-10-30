use hashbrown::HashMap;
use indexmap::IndexSet;
use smallvec::SmallVec;

use crate::guards::Guard;
use crate::borrow_lock::Lock;

pub enum AnyStateLock<'a> {
    Sparse(&'a SparseState<'a>),
    Dense(&'a DenseState<'a>),
}

pub struct SparseState<'a> {
    pub explicit_trans: HashMap<u8, SmallVec<[AnyStateLock<'a>; 1]>>,
    pub pattern_trans: Box<[(Guard, SmallVec<[AnyStateLock<'a>; 1]>)]>,
    pub tags: Vec<usize>,
}

pub struct DenseState<'a> {
    pub trans: [SmallVec<[AnyStateLock<'a>; 1]>; 256],
    pub tags: Vec<usize>,
}

pub struct Runner<'a> {
    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub sparse_states: IndexSet<Lock<'a, SparseState<'a>>>,
    pub dense_states: IndexSet<Lock<'a, DenseState<'a>>>,
}

impl<'a> Runner<'a>
{
    // Initialize the state of the automaton.
    pub fn new<'b, I: IntoIterator<Item = &'b AnyStateLock<'a>>>(initial_states: I) -> Self
        where 'a: 'b
    {
        let mut result = Runner {
            sparse_states: IndexSet::new(),
            dense_states: IndexSet::new(),
        };
        for any_state_lock in initial_states.into_iter() {
            result.add_right_state(any_state_lock);
        }
        result
    }

    // Read a symbol, perform transitions.
    pub fn read(&mut self, symbol: u8) {
        dbg!(&self.sparse_states);
        let sparse_states = std::mem::take(&mut self.sparse_states);
        let dense_states = std::mem::take(&mut self.dense_states);

        // Finally, let's handle the self-handling states.
        for state_lock in sparse_states.into_iter() {
            let state = state_lock.0;
            state.explicit_trans.get(&symbol).map(|rights|
                for right in rights.iter() { self.add_right_state(right) }
            );
            for (pattern, rights) in state.pattern_trans.iter() {
                if pattern.contains(symbol) {
                    for right in rights.iter() { self.add_right_state(right) }
                }
            }
        }

        for state_lock in dense_states.into_iter() {
            let rights = &state_lock.0.trans[symbol as usize];
            for right in rights.iter() { self.add_right_state(right) }
        }
    }

    pub fn get_tags(&self) -> Vec<usize> {
        let mut result = Vec::new();
        for state_lock in self.sparse_states.iter() {
            result.extend(state_lock.0.tags.iter().cloned());
        }
        for state_lock in self.dense_states.iter() {
            result.extend(state_lock.0.tags.iter().cloned());
        }
        result
    }

    fn add_right_state(&mut self, any_state_lock: &AnyStateLock<'a>) {
        match any_state_lock {
            AnyStateLock::Sparse(state) => self.sparse_states.insert(Lock(state)),
            AnyStateLock::Dense(state) => self.dense_states.insert(Lock(state)),
        };
    }
}

#[cfg(test)]
mod tests {
    use hashbrown::HashSet;

    use super::*;

    fn new_state<'a>(tag: usize) -> SparseState<'a> {
        let result = SparseState {
            explicit_trans: HashMap::new(),
            pattern_trans: Box::new([]),
            tags: vec![tag],
        };
        result
    }

    fn set_explicit<'a>(
        state: &mut SparseState<'a>,
        c: u8,
        right: &'a SparseState<'a>,
    ) {
        let right = AnyStateLock::Sparse(right);
        state.explicit_trans.entry(c).or_insert_with(SmallVec::new).push(right);
    }

    fn set_pattern<'a>(
        state: &mut SparseState<'a>,
        guard: Guard,
        right: &'a SparseState<'a>,
    ) {
        let right = AnyStateLock::Sparse(right);
        let mut trans = std::mem::take(&mut state.pattern_trans).into_vec();
        trans.push((guard, SmallVec::from_buf([right])));
        state.pattern_trans = trans.into_boxed_slice();
    }

    #[test]
    fn explicit_works() {
        let (a, b, c) = (0, 1, 2);

        let mut qs = vec![new_state(0), new_state(1), new_state(2), new_state(3)];
        unsafe {
            for (left, c, right) in [
                (0, a, 1),
                (0, b, 0),
                (0, c, 0),

                (1, b, 2),

                (2, a, 2),
                (2, b, 2),
                (2, c, 0),
                (2, c, 3),

                (3, a, 3),
                (3, b, 0),
                (3, c, 3),
            ] {
                let right = &*(&qs[right] as *const _);
                set_explicit(&mut qs[left], c, right);
            }
        }

        let pats = [
            Guard::from_ranges(vec![(0, 0), (2, 255)]),
        ];
        let nb = 0;

        unsafe {
            for (left, pat, right) in [
                (1, nb, 1),
            ] {
                let right = &*(&qs[right] as *const _);
                set_pattern(&mut qs[left], pats[pat].clone(), right);
            }
        }

        let mut automaton = Runner::new(vec![&AnyStateLock::Sparse(unsafe{&*(&qs[0] as *const _)})]);
        let mut read_and_check_trans = |sym: u8, expected: Vec<usize>| {
            automaton.read(sym);
            let real = automaton.get_tags().into_iter().collect::<HashSet<_>>();
            let expected = expected.into_iter().collect::<HashSet<_>>();
            assert_eq!(real, expected);
        };

        // 0--- 0--a-->1
        read_and_check_trans(a, vec![1]);
        // -1-- 1--b-->2
        read_and_check_trans(b, vec![2]);
        // --2-
        read_and_check_trans(b, vec![2]);
        // --2-
        read_and_check_trans(a, vec![2]);
        // --2- 2--c-->0 2--c-->3
        read_and_check_trans(c, vec![0, 3]);
        // 0--3 0--a-->1
        read_and_check_trans(a, vec![1, 3]);
        // -1-3 1--b-->2 3--b-->0
        read_and_check_trans(b, vec![2, 0]);
        // 0-2- 2--c-->3
        read_and_check_trans(c, vec![0, 3]);
        // 0--3 3--b-->0
        read_and_check_trans(b, vec![0]);
        // 0--- 0--a-->1
        read_and_check_trans(a, vec![1]);
        // -1-- 1--b-->2
        read_and_check_trans(b, vec![2]);
    }
}
