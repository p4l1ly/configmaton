use indexmap::IndexSet;

use crate::blob::{state::{U8State, U8StateIterator}, UnsafeIterator};

pub struct Runner<'a> {
    pub states: IndexSet<*const U8State<'a>>,
}

impl<'a> Runner<'a>
{
    // Initialize the state of the automaton.
    pub fn new<I: IntoIterator<Item = *const U8State<'a>>>(initial_states: I) -> Self {
        Runner { states: initial_states.into_iter().collect() }
    }

    // Read a symbol, perform transitions.
    pub unsafe fn read(&mut self, symbol: u8) {
        let states = std::mem::take(&mut self.states);

        // Finally, let's handle the self-handling states.
        for state in states.into_iter() {
            let state = &*state;
            match state.iter_matches(&symbol) {
                U8StateIterator::Sparse(mut iter) => {
                    while let Some(right) = iter.next() {
                        self.states.insert(right);
                    }
                },
                U8StateIterator::Dense(mut iter) => {
                    while let Some(right) = iter.next() {
                        self.states.insert(*right);
                    }
                },
            }
        }
    }

    pub unsafe fn get_tags<'b>(&'b self) -> impl Iterator<Item = usize> + 'b {
        self.states.iter().flat_map(|state| (&**state).get_tags().iter().cloned())
    }
}

#[cfg(test)]
mod tests {
    use hashbrown::HashSet;

    use crate::{blob::tests::create_states, char_enfa::OrderedIxs, char_nfa, guards::Guard};

    use super::*;

    fn new_state(
        tag: usize,
        transitions: Vec<(u8, u8, usize)>,
    ) -> char_nfa::State {
        char_nfa::State {
            tags: OrderedIxs(vec![tag]),
            transitions: transitions.into_iter().map(|(a, b, q)|
                (Guard::from_range((a, b)), q)
            ).collect(),
            is_deterministic: false,
        }
    }

    #[test]
    fn explicit_works() {
        let (a, b, c) = (0, 1, 2);

        let qs = vec![
            new_state(0, vec![(a, a, 1), (b, c, 0)]),
            new_state(1, vec![(a, a, 1), (b, b, 2), (c, 255, 1)]),
            new_state(2, vec![(a, b, 2), (c, c, 0), (c, c, 3)]),
            new_state(3, vec![(a, a, 3), (b, b, 0), (c, c, 3)]),
        ];
        let mut buf = vec![];
        let qs = unsafe { create_states(&mut buf, qs) };

        let mut automaton = Runner::new([qs[0] as *const _]);
        let mut read_and_check_trans = |sym: u8, expected: Vec<usize>| {
            unsafe { automaton.read(sym) };
            let real = unsafe { automaton.get_tags() }.into_iter().collect::<HashSet<_>>();
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
