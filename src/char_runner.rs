use hashbrown::HashMap;
use smallvec::SmallVec;

use crate::guards::Guard;
use crate::borrow_lock::Lock;

type SparseStateLock<'a> = Lock<'a, SparseState<AnyStateLock<'a>>>;
type DenseStateLock<'a> = Lock<'a, DenseState<AnyStateLock<'a>>>;

#[derive(Debug)]
pub enum AnyStateLock<'a> {
    Sparse(SparseStateLock<'a>),
    Dense(DenseStateLock<'a>),
}

impl<'a> Clone for AnyStateLock<'a> {
    fn clone(&self) -> Self {
        match self {
            AnyStateLock::Sparse(lock) => AnyStateLock::Sparse(lock.clone()),
            AnyStateLock::Dense(lock) => AnyStateLock::Dense(lock.clone()),
        }
    }
}

pub struct SparseState<T> {
    pub explicit_trans: HashMap<u8, SmallVec<[T; 1]>>,
    pub pattern_trans: Box<[(Guard, SmallVec<[T; 1]>)]>,
    pub tags: Vec<usize>,
}

impl<T> SparseState<T> {
    pub fn map<T2, F: FnMut(T) -> T2 + Copy>(self, f: F) -> SparseState<T2> {
        SparseState {
            explicit_trans: self.explicit_trans.into_iter().map(|(k, v)|
                (k, SmallVec::from_iter(v.into_iter().map(f)))
            ).collect(),
            pattern_trans: self.pattern_trans.into_vec().into_iter().map(|(k, v)|
                (k, SmallVec::from_iter(v.into_iter().map(f)))
            ).collect::<Vec<_>>().into_boxed_slice(),
            tags: self.tags,
        }
    }
}

pub struct DenseState<T> {
    pub trans: [SmallVec<[T; 1]>; 256],
    pub tags: Vec<usize>,
}

impl<T> DenseState<T> {
    pub fn map<T2, F: FnMut(T) -> T2 + Copy>(self, f: F) -> DenseState<T2> {
        DenseState{
            trans: self.trans.map(|v| SmallVec::from_iter(v.into_iter().map(f))),
            tags: self.tags,
        }
    }
}

pub struct Listeners<'a> {
    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub sparse_states: Vec<SparseStateLock<'a>>,
    pub dense_states: Vec<DenseStateLock<'a>>,
}

impl<'a> Listeners<'a>
{
    // Initialize the state of the automaton.
    pub fn new<I: IntoIterator<Item = AnyStateLock<'a>>>(initial_states: I) -> Self
    {
        let mut result = Listeners {
            sparse_states: Vec::new(),
            dense_states: Vec::new(),
        };
        for any_state_lock in initial_states.into_iter() {
            result.add_right_state(&any_state_lock);
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
            dbg!(&state.explicit_trans);
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
        dbg!(&self.sparse_states);
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
            AnyStateLock::Sparse(state_lock) => self.sparse_states.push(state_lock.clone()),
            AnyStateLock::Dense(state_lock) => self.dense_states.push(state_lock.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use hashbrown::HashSet;

    use super::*;

    type State<'a> = SparseState<AnyStateLock<'a>>;

    fn new_state<'a>(tag: usize) -> State<'a> {
        let result = SparseState {
            explicit_trans: HashMap::new(),
            pattern_trans: Box::new([]),
            tags: vec![tag],
        };
        result
    }

    unsafe fn lock<'a, T>(x: *const T) -> Lock<'a, T> {
        Lock(&*x)
    }

    fn set_explicit<'a>(
        state: &mut State<'a>,
        c: u8,
        right: SparseStateLock<'a>,
    ) {
        let right = AnyStateLock::Sparse(right);
        state.explicit_trans.entry(c).or_insert_with(SmallVec::new).push(right);
    }

    fn set_pattern<'a>(
        state: &mut State<'a>,
        guard: Guard,
        right: SparseStateLock<'a>,
    ) {
        let right = AnyStateLock::Sparse(right);
        let mut trans = state.pattern_trans.to_vec();
        trans.push((guard, SmallVec::from_elem(right, 1)));
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
                let right = lock(&qs[right]);
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
                let right = lock(&qs[right]);
                set_pattern(&mut qs[left], pats[pat].clone(), right);
            }
        }

        let mut automaton = Listeners::new(vec![AnyStateLock::Sparse(unsafe{lock(&qs[0])})]);
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
