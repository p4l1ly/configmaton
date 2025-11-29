use hashbrown::HashMap;
use indexmap::IndexSet; // we use IndexSet for faster worst-case iteration

use crate::blob::keyval_state::{Finals, KeyValState, LeafMeta};
use crate::blob::sediment::Sediment;
use crate::blob::vec::BlobVec;
use crate::blob::{align_up_ptr, get_behind_struct, FakeSafeIterator, UnsafeIterator};
use crate::char_runner;

#[derive(Clone)]
pub struct Runner<'a> {
    // Mapping from symbols to such current states from which a transition via the symbol exists.
    pub sparse: HashMap<&'a [u8], IndexSet<*const KeyValState<'a>>>,
}

impl<'a> Runner<'a> {
    // Initialize the state of the automaton.
    pub unsafe fn new<'b, I: IntoIterator<Item = &'b KeyValState<'a>>>(initial_states: I) -> Self
    where
        'a: 'b,
    {
        let mut result = Runner { sparse: HashMap::new() };
        for any_state_lock in initial_states {
            result.add_right_state(any_state_lock);
        }
        result
    }

    // Read a symbol, perform transitions.
    pub unsafe fn read<GetOld: FnMut(&'a [u8]), RunExt: FnMut(&'a [u8])>(
        &mut self,
        sym: &[u8],
        value: &[u8],
        mut get_old: GetOld,
        mut run_ext: RunExt,
    ) {
        let mut trans = vec![];

        // Prepare the results.
        match self.sparse.get_mut(sym) {
            None => return,
            Some(states) => {
                let old_sparse_states = std::mem::take(states);

                // First, let's remove all listeners for transitions of the old states
                for left_state in old_sparse_states.iter().cloned() {
                    let mut keyvals = (*left_state).keyvals();
                    while let Some((key, tran)) = keyvals.next() {
                        if sym == key {
                            // Register new listeners for transitions of the successors.
                            trans.push(tran);
                        } else {
                            // Remove listeners for transitions of the left_state (other than the
                            // one via `symbol` which is already removed).
                            self.sparse.get_mut(key).unwrap().swap_remove(&left_state);
                        }
                    }
                }
            }
        }

        let mut crunner = char_runner::Runner::new(
            trans.iter().flat_map(|tran| FakeSafeIterator(tran.a.iter())).copied(),
        );

        for c in value {
            crunner.read(*c);
        }

        let mut tags = crunner.get_tags().collect::<Vec<_>>();
        tags.sort_unstable();
        tags.dedup();
        let tags = tags;

        for tran in trans {
            let mut tag_i = 0;
            let target = tran.a.behind::<Finals>().evaluate(|var| {
                let var = *var;
                if tag_i == tags.len() {
                    return false;
                }
                while tags[tag_i] < var {
                    tag_i += 1;
                    if tag_i == tags.len() {
                        return false;
                    }
                }
                if var == tags[tag_i] {
                    tag_i += 1;
                    return true;
                }
                false
            });
            for right_state in target.0.a.as_ref() {
                self.add_right_state(&**right_state);
            }
            let meta: &LeafMeta = target.0.a.behind();
            let mut behind = get_behind_struct(meta);
            meta.a.each(|x| {
                get_old(x.as_ref());
                behind = x.behind();
                behind
            });
            let exts: &Sediment<'a, BlobVec<'a, u8>> = &*align_up_ptr(behind);
            exts.each(|x| {
                run_ext(x.as_ref());
                x.behind()
            });
        }
    }

    unsafe fn add_right_state(&mut self, state: &KeyValState<'a>) {
        let mut keyvals = state.keyvals();
        while let Some((key, _)) = keyvals.next() {
            self.sparse.entry(key).or_insert(IndexSet::new()).insert(state);
        }
    }
}
