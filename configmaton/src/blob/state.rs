use std::mem::ManuallyDrop;

use super::{
    arrmap::ArrMap,
    hashmap::BlobHashMap,
    vec::{BlobVec, BlobVecIter},
    vecmap::{VecMap, VecMapIter},
    Assocs as _, Build, BuildCursor, Reserve, Shifter, UnsafeIterator,
};
use crate::guards::Guard;

type U8States<'a> = BlobVec<'a, *const U8State<'a>>;
type U8AList<'a> = VecMap<'a, u8, U8States<'a>>;
type U8ExplicitTrans<'a> = BlobHashMap<'a, U8AList<'a>>;
type U8Tags<'a> = BlobVec<'a, usize>;
type U8PatternTrans<'a> = VecMap<'a, Guard, U8States<'a>>;
type U8ArrMap<'a> = ArrMap<'a, 256, U8States<'a>>;

impl Build for *const U8State<'_> {
    type Origin = usize;
}

#[repr(C)]
pub struct U8SparseState<'a> {
    is_dense: bool,
    tags: *const U8Tags<'a>,
    explicit_trans: *const U8ExplicitTrans<'a>,
    pattern_trans: U8PatternTrans<'a>,
}

#[repr(C)]
pub struct U8DenseState<'a> {
    is_dense: bool,
    tags: *const U8Tags<'a>,
    trans: U8ArrMap<'a>,
}

#[repr(C)]
pub union U8State<'a> {
    sparse: ManuallyDrop<U8SparseState<'a>>,
    dense: ManuallyDrop<U8DenseState<'a>>,
}

impl<'a> Build for U8State<'a> {
    type Origin = U8StatePrepared;
}

impl<'a> U8State<'a> {
    pub unsafe fn iter_matches<'c, 'b>(&'c self, key: &'b u8) -> U8StateIterator<'a, 'b>
    where
        'a: 'b + 'c,
    {
        if self.sparse.is_dense {
            U8StateIterator::Dense(self.dense.trans.get(*key as usize).iter())
        } else {
            let sparse = &self.sparse;
            U8StateIterator::Sparse(U8SparseStateIterator {
                pattern_iter: sparse.pattern_trans.iter_matches(key),
                states_iter: None,
                explicit_trans: sparse.explicit_trans,
            })
        }
    }

    pub unsafe fn get_tags(&self) -> &[usize] {
        if self.sparse.tags.is_null() {
            &[]
        } else {
            (*self.sparse.tags).as_ref()
        }
    }

    pub unsafe fn deserialize<B>(state_cur: BuildCursor<U8State>) -> BuildCursor<B> {
        let shifter = Shifter(state_cur.buf);
        let state = &mut *state_cur.get_mut();
        let f_is_dense_cur = state_cur.transmute::<bool>();
        let f_tags_cur = f_is_dense_cur.behind::<*const U8Tags>(1);
        let shiftq = |q: &mut *const U8State| shifter.shift(q);

        if state.sparse.is_dense {
            let dense = &mut state.dense;
            let f_trans_cur = f_tags_cur.behind::<U8ArrMap>(1);
            let tags_cur: BuildCursor<u8> =
                U8ArrMap::deserialize(f_trans_cur, |qs_cur| U8States::deserialize(qs_cur, shiftq));

            if dense.tags.is_null() {
                tags_cur.align()
            } else {
                shifter.shift(&mut dense.tags);
                U8Tags::deserialize(tags_cur.align(), |_| ())
            }
        } else {
            let sparse = &mut state.sparse;
            shifter.shift(&mut sparse.explicit_trans);

            let f_explicit_trans_cur = f_tags_cur.behind::<*const U8ExplicitTrans>(1);
            let f_pattern_trans_cur = f_explicit_trans_cur.behind::<U8PatternTrans>(1);
            let exp_cur = U8PatternTrans::deserialize(
                f_pattern_trans_cur,
                |_| (),
                |qs_cur| U8States::deserialize(qs_cur, shiftq),
            );

            let tags_cur: BuildCursor<u8> = U8ExplicitTrans::deserialize(exp_cur, |alist_cur| {
                U8AList::deserialize(
                    alist_cur,
                    |_| (),
                    |qs_cur| U8States::deserialize(qs_cur, shiftq),
                )
            });

            if sparse.tags.is_null() {
                tags_cur.align()
            } else {
                shifter.shift(&mut sparse.tags);
                U8Tags::deserialize(tags_cur.align(), |_| ())
            }
        }
    }

    pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
        sz.add::<U8State>(0);
        let result = sz.0;
        sz.add::<bool>(1);
        sz.add::<*const U8Tags>(1);
        match origin {
            U8StatePrepared::Sparse(sparse) => {
                sz.add::<*const U8ExplicitTrans>(1);
                U8PatternTrans::reserve(&sparse.pattern_trans, sz, |qs, sz| {
                    U8States::reserve(qs, sz);
                });
                U8ExplicitTrans::reserve(&sparse.explicit_trans, sz, |alist, sz| {
                    U8AList::reserve(alist, sz, |qs, sz| {
                        U8States::reserve(qs, sz);
                    });
                });
                if !sparse.tags.is_empty() {
                    U8Tags::reserve(&sparse.tags, sz);
                }
            }
            U8StatePrepared::Dense(dense) => {
                U8ArrMap::reserve(&dense.trans, sz, |qs, sz| {
                    U8States::reserve(qs, sz);
                });
                if !dense.tags.is_empty() {
                    U8Tags::reserve(&dense.tags, sz);
                }
            }
        }

        result
    }

    pub unsafe fn serialize<After>(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        qptrs: &Vec<usize>,
    ) -> BuildCursor<After> {
        let state = &mut *cur.get_mut();
        let f_is_dense_cur = cur.transmute::<bool>();
        let f_tags_cur = f_is_dense_cur.behind::<*const U8Tags>(1);
        let setq = |q: &usize, qref: &mut *const U8State| {
            *qref = qptrs[*q] as *const U8State;
        };

        match origin {
            U8StatePrepared::Sparse(sparse_origin) => {
                let sparse = &mut state.sparse;
                sparse.is_dense = false;
                let f_explicit_trans_cur = f_tags_cur.behind::<*const U8ExplicitTrans>(1);
                let f_pattern_trans_cur = f_explicit_trans_cur.behind::<U8PatternTrans>(1);
                let exp_cur = U8PatternTrans::serialize(
                    &sparse_origin.pattern_trans,
                    f_pattern_trans_cur,
                    |guard, guardref| {
                        *guardref = *guard;
                    },
                    |qs, qs_cur| U8States::serialize(qs, qs_cur, setq),
                );
                sparse.explicit_trans = exp_cur.cur as *const U8ExplicitTrans;
                let tags_cur: BuildCursor<u8> = U8ExplicitTrans::serialize(
                    &sparse_origin.explicit_trans,
                    exp_cur,
                    |alist, alist_cur| {
                        U8AList::serialize(
                            alist,
                            alist_cur,
                            |c, c_cur| {
                                *c_cur = *c;
                            },
                            |qs, qs_cur| U8States::serialize(qs, qs_cur, setq),
                        )
                    },
                );
                if sparse_origin.tags.is_empty() {
                    sparse.tags = std::ptr::null();
                    tags_cur.align()
                } else {
                    let tags_cur = tags_cur.align();
                    sparse.tags = tags_cur.cur as *const U8Tags;
                    U8Tags::serialize(&sparse_origin.tags, tags_cur, |t, tref| {
                        *tref = *t;
                    })
                }
            }
            U8StatePrepared::Dense(dense_origin) => {
                let dense = &mut state.dense;
                dense.is_dense = true;
                let f_trans_cur = f_tags_cur.behind::<U8ArrMap>(1);
                let tags_cur: BuildCursor<u8> =
                    U8ArrMap::serialize(&dense_origin.trans, f_trans_cur, |qs, qs_cur| {
                        U8States::serialize(qs, qs_cur, setq)
                    });
                if dense_origin.tags.is_empty() {
                    dense.tags = std::ptr::null();
                    tags_cur.align()
                } else {
                    let tags_cur = tags_cur.align();
                    dense.tags = tags_cur.cur as *const U8Tags;
                    U8Tags::serialize(&dense_origin.tags, tags_cur, |t, tref| {
                        *tref = *t;
                    })
                }
            }
        }
    }
}

pub struct U8SparseStateIterator<'a, 'b> {
    states_iter: Option<BlobVecIter<'a, *const U8State<'a>>>,
    pattern_iter: VecMapIter<'a, 'b, u8, Guard, U8States<'a>>,
    explicit_trans: *const U8ExplicitTrans<'a>,
}

pub type U8DenseStateIterator<'a> = BlobVecIter<'a, *const U8State<'a>>;

pub enum U8StateIterator<'a, 'b> {
    Sparse(U8SparseStateIterator<'a, 'b>),
    Dense(U8DenseStateIterator<'a>),
}

impl<'a, 'b> UnsafeIterator for U8SparseStateIterator<'a, 'b>
where
    'a: 'b,
{
    type Item = *const U8State<'a>;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        if let Some(states_iter) = self.states_iter.as_mut() {
            if let Some(state) = states_iter.next() {
                return Some(&**state);
            }
        }
        loop {
            if let Some((_, states)) = self.pattern_iter.next() {
                let mut states_iter = states.iter();
                if let Some(state) = states_iter.next() {
                    self.states_iter = Some(states_iter);
                    return Some(*state);
                }
            } else {
                if self.explicit_trans.is_null() {
                    return None;
                } else {
                    let explicit_trans = &*self.explicit_trans;
                    self.explicit_trans = std::ptr::null();
                    if let Some(states) = explicit_trans.get(self.pattern_iter.x) {
                        let mut states_iter = states.iter();
                        if let Some(state) = states_iter.next() {
                            self.states_iter = Some(states_iter);
                            return Some(*state);
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct U8DenseStatePrepared {
    tags: Vec<usize>,
    trans: [Vec<usize>; 256],
}

#[derive(Debug)]
pub struct U8SparseStatePrepared {
    tags: Vec<usize>,
    pattern_trans: Vec<(Guard, Vec<usize>)>,
    explicit_trans: Vec<Vec<(u8, Vec<usize>)>>, // has size of 2**hashmap_cap
}

#[derive(Debug)]
pub enum U8StatePrepared {
    Sparse(U8SparseStatePrepared),
    Dense(U8DenseStatePrepared),
}

pub mod build {
    use std::array;

    use super::*;
    use crate::char_nfa;
    use hashbrown::HashMap;

    pub trait U8BuildConfig {
        fn guard_size_keep(&self) -> u32;
        fn hashmap_cap_power_fn(&self, len: usize) -> usize;
        fn dense_guard_count(&self) -> usize;
    }

    impl U8StatePrepared {
        pub fn prepare<Cfg: U8BuildConfig>(old: &char_nfa::State, cfg: &Cfg) -> Self {
            if old.transitions.len() < cfg.dense_guard_count() {
                let mut pattern_trans0 = HashMap::<Guard, Vec<usize>>::new();
                let mut explicitized_guard_trans = Vec::<(Guard, usize)>::new();
                for (guard, target) in old.transitions.iter().copied() {
                    if guard.size() >= cfg.guard_size_keep() {
                        pattern_trans0.entry(guard).or_insert(Vec::new()).push(target);
                    } else {
                        explicitized_guard_trans.push((guard, target));
                    }
                }
                let mut explicit_trans0 = HashMap::<u8, Vec<usize>>::new();
                let mut c = 0;
                loop {
                    for (guard, target) in explicitized_guard_trans.iter() {
                        if guard.contains(c) {
                            explicit_trans0.entry(c).or_insert(Vec::new()).push(*target);
                        }
                    }
                    if c == 255 {
                        break;
                    }
                    c += 1;
                }
                let hashmap_cap_power = cfg.hashmap_cap_power_fn(explicit_trans0.len());
                let hashmap_cap = 1 << hashmap_cap_power;
                let hashmap_mask = hashmap_cap - 1;
                let mut hashmap_alists = Vec::<Vec<(u8, Vec<usize>)>>::with_capacity(hashmap_cap);
                for _ in 0..hashmap_cap {
                    hashmap_alists.push(Vec::new())
                }
                for (c, targets) in explicit_trans0 {
                    hashmap_alists[c as usize & hashmap_mask].push((c, targets));
                }

                Self::Sparse(U8SparseStatePrepared {
                    tags: old.tags.0.clone(),
                    pattern_trans: pattern_trans0.into_iter().collect(),
                    explicit_trans: hashmap_alists,
                })
            } else {
                let mut trans = array::from_fn(|_| Vec::new());
                let mut c = 0;
                loop {
                    for (guard, target) in old.transitions.iter() {
                        if guard.contains(c) {
                            trans[c as usize].push(*target);
                        }
                    }
                    if c == 255 {
                        break;
                    }
                    c += 1;
                }
                Self::Dense(U8DenseStatePrepared { tags: old.tags.0.clone(), trans })
            }
        }
    }
}
