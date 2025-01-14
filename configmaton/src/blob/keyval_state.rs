use super::{bdd::{Bdd, BddOrigin}, list::List, sediment::Sediment, state::U8State, tupellum::Tupellum, vec::BlobVec, Build, BuildCursor, Reserve, Shifter, UnsafeIterator};

pub struct LeafOrigin {
    pub states: Vec<usize>,
    pub get_olds: Vec<Vec<u8>>,
    pub exts: Vec<Vec<u8>>,
}

pub struct TranOrigin {
    pub key: Vec<u8>,
    pub dfa_inits: Vec<usize>,
    pub bdd: BddOrigin<usize, LeafOrigin>,
}

pub struct StateOrigin {
    pub transitions: Vec<TranOrigin>,
}

pub type Bytes<'a> = BlobVec<'a, u8>;
pub type LeafMeta<'a> = Tupellum<'a, Sediment<'a, Bytes<'a>>, Sediment<'a, Bytes<'a>>>;
pub type Leaf0<'a> = Tupellum<'a, BlobVec<'a, *const KeyValState<'a>>, LeafMeta<'a>>;
pub struct Leaf<'a>(pub Leaf0<'a>);
pub type Finals<'a> = Bdd<'a, usize, Leaf<'a>>;
pub type InitsAndFinals<'a> = Tupellum<'a, BlobVec<'a, *const U8State<'a>>, Finals<'a>>;
pub type Tran0<'a> = Tupellum<'a, Bytes<'a>, InitsAndFinals<'a>>;
pub struct Tran<'a>(Tran0<'a>);
pub type KeyValStateSparse<'a> = List<'a, Tran<'a>>;

#[repr(C)]
pub struct KeyValState<'a> {
    pub sparse: KeyValStateSparse<'a>,
}

pub struct SparseIterator<'a>(*const KeyValStateSparse<'a>);

impl<'a> UnsafeIterator for SparseIterator<'a> {
    type Item = (&'a [u8], &'a InitsAndFinals<'a>);

    unsafe fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|tupellum| {
            (tupellum.0.a.as_ref(), tupellum.0.a.behind())
        })
    }
}

impl Build for *const KeyValState<'_> { type Origin = usize; }
impl<'a> Build for Leaf<'a> { type Origin = LeafOrigin; }
impl<'a> Build for Tran<'a> { type Origin = TranOrigin; }
impl<'a> Build for KeyValState<'a> { type Origin = StateOrigin; }

impl<'a> KeyValState<'a> {
    pub fn keyvals(&self) -> SparseIterator<'a> {
        SparseIterator(&self.sparse)
    }

    pub unsafe fn deserialize<B>(state_cur: BuildCursor<KeyValState>) -> BuildCursor<B> {
        let shifter = Shifter(state_cur.buf);
        let state = &mut *state_cur.get_mut();
        let sparse_cur = state_cur.goto(&mut state.sparse);
        KeyValStateSparse::deserialize(sparse_cur,
            |keyval_cur| Tran0::deserialize(keyval_cur.transmute(),
                |key_cur| {
                    Bytes::deserialize(key_cur, |_| ())
                },
                |iaf_cur| InitsAndFinals::deserialize(iaf_cur,
                    |inits_cur| BlobVec::<*const U8State>::deserialize(inits_cur,
                        |initq| shifter.shift(initq),
                    ),
                    |finals_cur| Finals::deserialize(finals_cur,
                        |leaf_cur| Leaf0::deserialize(leaf_cur.transmute(),
                            |post_cur| BlobVec::<*const KeyValState>::deserialize(post_cur,
                                |postq| shifter.shift(postq),
                            ),
                            |meta_cur| LeafMeta::deserialize(meta_cur,
                                |getolds_cur| Sediment::<Bytes>::deserialize(getolds_cur,
                                    |getold_cur| Bytes::deserialize(getold_cur, |_| ())
                                ),
                                |exts_cur| Sediment::<Bytes>::deserialize(exts_cur,
                                    |ext_cur| Bytes::deserialize(ext_cur, |_| ())
                                ),
                            )
                        ),
                        |_| (),
                    )
                )
            )
        )
    }

    pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
        sz.add::<KeyValState>(0);
        let result = sz.0;
        KeyValStateSparse::reserve(&origin.transitions, sz,
            |tran, sz| {
                Tran0::reserve(&(&tran.key, &(&tran.dfa_inits, &tran.bdd)), sz,
                    |key, sz| { Bytes::reserve(key, sz); },
                    |iaf, sz| {
                        InitsAndFinals::reserve(iaf, sz,
                            |inits, sz| { BlobVec::<*const U8State>::reserve(inits, sz); },
                            |finals, sz| {
                                Finals::reserve(finals, sz,
                                    |leaf, sz| {
                                        Leaf0::reserve(
                                            &(&leaf.states, &(&leaf.get_olds, &leaf.exts)), sz,
                                            |postq, sz| {
                                                BlobVec::<*const KeyValState>::reserve(postq, sz);
                                            },
                                            |meta, sz| {
                                                LeafMeta::reserve(meta, sz,
                                                    |getolds, sz| {
                                                        Sediment::<Bytes>::reserve(getolds, sz,
                                                            |getold, sz|
                                                                { Bytes::reserve(getold, sz); }
                                                        );
                                                    },
                                                    |exts, sz| {
                                                        Sediment::<Bytes>::reserve(exts, sz,
                                                            |ext, sz| { Bytes::reserve(ext, sz); }
                                                        );
                                                    }
                                                );
                                            }
                                        );
                                    }
                                );
                            }
                        );
                    }
                );
            }
        );
        result
    }

    pub unsafe fn serialize<After>(
        origin: &<Self as Build>::Origin,
        state_cur: BuildCursor<KeyValState>,
        u8qptrs: &Vec<usize>,
        kvqptrs: &Vec<usize>,
    ) -> BuildCursor<After>
    {
        let state = &mut *state_cur.get_mut();
        let sparse_cur = state_cur.goto(&mut state.sparse);
        KeyValStateSparse::serialize(&origin.transitions, sparse_cur,
            |tran, tran_cur| Tran0::serialize(
                &(&tran.key, &(&tran.dfa_inits, &tran.bdd)),
                tran_cur.transmute(),
                |key, key_cur| Bytes::serialize(key, key_cur, |x, y| *y = *x),
                |iaf, iaf_cur| InitsAndFinals::serialize(iaf, iaf_cur,
                    |inits, inits_cur| BlobVec::<*const U8State>::serialize(
                        inits, inits_cur, |x, y| *y = u8qptrs[*x] as *const U8State
                    ),
                    |finals, finals_cur| Finals::serialize(finals, finals_cur,
                        |leaf, leaf_cur| Leaf0::serialize(
                            &(&leaf.states, &(&leaf.get_olds, &leaf.exts)), leaf_cur.transmute(),
                            |postq, post_cur| BlobVec::<*const KeyValState>::serialize(
                                postq, post_cur, |x, y| *y = kvqptrs[*x] as *const KeyValState,
                            ),
                            |meta, meta_cur| LeafMeta::serialize(meta, meta_cur,
                                |getolds, getolds_cur| Sediment::<Bytes>::serialize(
                                    getolds, getolds_cur,
                                    |getold, getold_cur| Bytes::serialize(
                                        getold, getold_cur, |x, y| *y = *x)
                                ),
                                |exts, exts_cur| Sediment::<Bytes>::serialize(exts, exts_cur,
                                    |ext, ext_cur| Bytes::serialize(ext, ext_cur, |x, y| *y = *x)
                                ),
                            )
                        ),
                        |x, y| *y = *x,
                    )
                )
            )
        )
    }
}


#[cfg(test)]
mod tests {
    use crate::blob::{align_up_ptr, get_behind_struct};

    use super::*;

    #[test]
    fn test_keyval_state() {
        let state_origins = vec![
            StateOrigin {
                transitions: vec![
                    TranOrigin {
                        key: b"key1".to_vec(),
                        dfa_inits: vec![0, 2],
                        bdd: BddOrigin::NodeBothOwned {
                            var: 3,
                            pos: Box::new(
                                BddOrigin::Leaf(
                                    LeafOrigin {
                                        states: vec![0],
                                        get_olds: vec![b"get1a".to_vec(), b"get1b".to_vec()],
                                        exts: vec![],
                                    }
                                )
                            ),
                            neg: Box::new(
                                BddOrigin::Leaf(
                                    LeafOrigin {
                                        states: vec![],
                                        get_olds: vec![],
                                        exts: vec![b"ext1a".to_vec()],
                                    }
                                )
                            ),
                        },
                    },
                ]
            },
        ];
        let mut buf = vec![];
        let mut sz = Reserve(0);
        let mut addrs = Vec::<usize>::new();
        let list_addr = Sediment::<KeyValState>::reserve(&state_origins, &mut sz, |state, sz| {
            addrs.push(KeyValState::reserve(state, sz));
        });
        assert_eq!(list_addr, 0);
        buf.resize(sz.0, 0u8);
        let buf = buf.as_mut_ptr();
        let mut cur = BuildCursor::new(buf);
        cur = unsafe { Sediment::<KeyValState>::serialize(&state_origins, cur,
            |state, state_cur| {
                KeyValState::serialize(state, state_cur, &vec![256, 1024, 4096], &addrs)
            }
        )};
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let mut cur = BuildCursor::new(buf);
        cur = unsafe { Sediment::<KeyValState>::deserialize(cur,
            |state_cur| KeyValState::deserialize(state_cur)) };
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let q0 = unsafe { &*(buf.add(addrs[0]) as *const KeyValState) };

        let mut keyvals = q0.keyvals();
        let (key, tran) = unsafe { keyvals.next() }.unwrap();
        assert!(unsafe { keyvals.next() }.is_none());
        assert_eq!(key, b"key1");
        assert_eq!(
            unsafe { tran.a.as_ref() }.iter().copied()
                .map(|x| x as usize - buf as usize).collect::<Vec<_>>(),
            vec![256, 4096],
        );
        let bdd: &Finals = unsafe { tran.a.behind() };

        let leaf = unsafe { bdd.evaluate(|var| match *var { 3 => true, _ => unreachable!() }) };
        assert_eq!(unsafe { leaf.0.a.as_ref() }, [q0 as *const _]);
        let meta: &LeafMeta = unsafe { leaf.0.a.behind() };
        let mut getolds = vec![];
        let mut behind = unsafe { get_behind_struct(meta) };
        unsafe { meta.a.each(|x| {
            getolds.push(x.as_ref());
            behind = x.behind();
            behind
        })};
        assert_eq!(getolds, vec![b"get1a", b"get1b"]);
        let mut exts_vec = vec![];
        let exts: &Sediment<BlobVec<u8>> = unsafe { &*align_up_ptr(behind) };
        unsafe { exts.each(|x| {
            exts_vec.push(x.as_ref());
            x.behind()
        })};
        assert!(exts_vec.is_empty());

        let leaf = unsafe { bdd.evaluate(|var| match *var { 3 => false, _ => unreachable!() }) };
        assert!(unsafe { leaf.0.a.as_ref() }.is_empty());
        let meta: &LeafMeta = unsafe { leaf.0.a.behind() };
        let mut getolds = vec![];
        let mut behind = unsafe { get_behind_struct(meta) };
        unsafe { meta.a.each(|x| {
            getolds.push(x.as_ref());
            behind = x.behind();
            behind
        })};
        assert!(getolds.is_empty());
        let mut exts_vec = vec![];
        let exts: &Sediment<BlobVec<u8>> = unsafe { &*align_up_ptr(behind) };
        unsafe { exts.each(|x| {
            exts_vec.push(x.as_ref());
            x.behind()
        })};
        assert_eq!(exts_vec, vec![b"ext1a"]);
    }
}
