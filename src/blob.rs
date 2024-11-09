// WARNING: No endianness handling is implemented yet, as we have no use case for BigEndian.

use std::mem::{align_of, size_of};
use std::marker::PhantomData;

use twox_hash::XxHash64;

use crate::guards::Guard;
use vec::BlobVec;

pub mod hashmap;
pub mod list;
pub mod assoc_list;
pub mod homo_key_assoc;
pub mod vec;
pub mod sediment;
pub mod vecmap;
pub mod listmap;
pub mod arrmap;
pub mod state;

pub trait MyHash {
    fn my_hash(&self) -> usize;
}

impl MyHash for u8 {
    fn my_hash(&self) -> usize {
        *self as usize
    }
}

impl MyHash for &[u8] {
    fn my_hash(&self) -> usize {
        XxHash64::oneshot(1234, self) as usize
    }
}

pub trait Matches<T> {
    unsafe fn matches(&self, other: &T) -> bool;
}

pub struct EqMatch<'a, X>(pub &'a X);

impl<'a, X: Eq> Matches<X> for EqMatch<'a, X> {
    unsafe fn matches(&self, other: &X) -> bool {
        *self.0 == *other
    }
}

impl Matches<Guard> for u8 {
    unsafe fn matches(&self, other: &Guard) -> bool {
        other.contains(*self)
    }
}

impl<'a, 'b> Matches<BlobVec<'a, u8>> for &'b [u8] {
    unsafe fn matches(&self, other: &BlobVec<'a, u8>) -> bool {
        *self == other.as_ref()
    }
}

pub struct AnyMatch;

impl<T> Matches<T> for AnyMatch {
    unsafe fn matches(&self, _: &T) -> bool { true }
}

pub trait UnsafeIterator {
    type Item;
    unsafe fn next(&mut self) -> Option<Self::Item>;
}

fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}

pub fn align_up_ptr<A>(a: *mut u8) -> *mut u8 {
    align_up(a as usize, align_of::<A>()) as *mut u8
}

unsafe fn get_behind_struct<A, B>(a: *const A) -> *const B {
    align_up((a as *const u8).add(size_of::<A>()) as usize, align_of::<B>()) as *const B
}

pub struct Reserve(pub usize);

impl Reserve {
    pub fn add<T>(&mut self, n: usize) {
        self.0 = align_up(self.0, align_of::<T>()) + size_of::<T>() * n;
    }
}

#[derive(Copy)]
pub struct BuildCursor<A>{
    pub cur: usize,
    pub buf: *mut u8,
    _phantom: PhantomData<A>,
}

impl<A> BuildCursor<A> {
    pub fn new(buf: *mut u8) -> Self {
        Self { cur: 0, buf, _phantom: PhantomData }
    }

    pub fn inc(&mut self) {
        self.cur += size_of::<A>();
    }

    pub fn behind<B>(&self, n: usize) -> BuildCursor<B> {
        BuildCursor {
            cur: align_up(self.cur + size_of::<A>() * n, align_of::<B>()),
            buf: self.buf,
            _phantom: PhantomData
        }
    }

    pub unsafe fn get_mut(&self) -> *mut A {
        self.buf.add(self.cur) as *mut A
    }
}

impl<A> Clone for BuildCursor<A> {
    fn clone(&self) -> Self {
        Self { cur: self.cur, buf: self.buf, _phantom: PhantomData }
    }
}

pub struct Shifter(pub *const u8);
impl Shifter {
    pub unsafe fn shift<T>(&self, x: &mut *const T) {
        *x = self.0.add(*x as *const u8 as usize) as *const T
    }
}

pub trait AssocsSuper<'a> {
    type Key: 'a;
    type Val: 'a;
    type I<'b, X: 'b + Matches<Self::Key>>: UnsafeIterator<Item = (&'a Self::Key, &'a Self::Val)>
        where 'a: 'b;
}

pub trait Assocs<'a>: AssocsSuper<'a> {
    unsafe fn iter_matches<'c, 'b, X: Matches<Self::Key>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c;
}

pub trait IsEmpty {
    fn is_empty(&self) -> bool;
}

impl<X> IsEmpty for Vec<X> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

pub trait Assoc<'a> {
    type Key: 'a;
    type Val: 'a;

    unsafe fn key(&self) -> &'a Self::Key;
    unsafe fn val(&self) -> &'a Self::Val;
}

pub trait Build {
    type Origin;
}

impl Build for u8 {
    type Origin = u8;
}

impl Build for Guard {
    type Origin = Guard;
}

impl Build for usize {
    type Origin = usize;
}

impl Build for () {
    type Origin = ();
}


#[cfg(test)]
pub mod tests {
    use crate::char_enfa::OrderedIxs;

    use super::*;
    use super::{
        hashmap::*, assoc_list::*, state::{*, build::*}, vecmap::*, listmap::*, homo_key_assoc::*,
        sediment::*,
    };
    use crate::char_nfa;

    #[test]
    pub fn test_blobvec() {
        let origin = vec![1usize, 3, 5];
        let mut sz = Reserve(0);
        let my_addr = BlobVec::<usize>::reserve(&origin, &mut sz);
        assert_eq!(my_addr, 0);
        assert_eq!(sz.0, 4 * size_of::<usize>());
        let mut buf = vec![0u8; sz.0];
        let mut cur = BuildCursor::new(buf.as_mut_ptr());
        cur = unsafe { BlobVec::<usize>::serialize(&origin, cur, |x, xcur| { *xcur = *x; }) };
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let mut cur = BuildCursor::new(buf.as_mut_ptr());
        cur = unsafe { BlobVec::<usize>::deserialize(cur, |_| ()) };
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let blobvec = unsafe { &*(buf.as_ptr() as *const BlobVec<usize>) };
        assert_eq!(blobvec.len, 3);
        assert_eq!(unsafe { blobvec.get(0) }, &1);
        assert_eq!(unsafe { blobvec.get(1) }, &3);
        assert_eq!(unsafe { blobvec.get(2) }, &5);
        let mut iter = unsafe { blobvec.iter() };
        assert_eq!(unsafe { iter.next() }, Some(&1));
        assert_eq!(unsafe { iter.next() }, Some(&3));
        assert_eq!(unsafe { iter.next() }, Some(&5));
        assert_eq!(unsafe { iter.next() }, None);
        assert_eq!(unsafe{ blobvec.as_ref() }, &[1, 3, 5]);
    }

    #[test]
    pub fn test_vecmap() {
        let origin = vec![(1, b"foo".to_vec()), (3, b"hello".to_vec()), (5, b"".to_vec())];
        let mut sz = Reserve(1);
        let addr = VecMap::<usize, BlobVec<u8>>::reserve(&origin, &mut sz, |x, sz| {
            BlobVec::<u8>::reserve(x, sz);
        });
        assert_eq!(addr.0, if align_of::<usize>() == 1 { 0 } else { align_of::<usize>() });
        let mut buf = vec![0u8; sz.0];
        let mut cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(addr.0) });
        cur = unsafe { VecMap::<usize, BlobVec<u8>>::serialize(&origin, cur,
            |x, xcur| { *xcur = *x; },
            |x, xcur| { BlobVec::<u8>::serialize(x, xcur, |y, ycur| { *ycur = *y; }) }
        )};
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let mut cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(addr.0) });
        cur = unsafe { VecMap::<usize, BlobVec<u8>>::deserialize(cur,
            |_| (),
            |xcur| BlobVec::<u8>::deserialize(xcur, |_| ())
        )};
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let vecmap = unsafe {
            &*(buf.as_ptr().add(addr.0) as *const VecMap::<usize, BlobVec<u8>>) };

        let mut iter = unsafe { vecmap.iter_matches(&EqMatch(&3)) };
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&3, b"hello".as_ref()));
        assert_eq!(unsafe { iter.next() }.is_none(), true);

        let mut iter = unsafe { vecmap.iter_matches(&AnyMatch) };
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&1, b"foo".as_ref()));
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&3, b"hello".as_ref()));
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&5, b"".as_ref()));
    }

    #[test]
    pub fn test_listmap() {
        let origin = vec![
            (b"aa".to_vec(), b"foo".to_vec()),
            (b"bb".to_vec(), b"hello".to_vec()),
            (b"aa".to_vec(), b"".to_vec()),
        ];
        let mut sz = Reserve(1);
        let addr = ListMap::<BlobVec<u8>, BlobVec<u8>>::reserve(&origin, &mut sz,
            |x, sz| { BlobVec::<u8>::reserve(x, sz); },
            |x, sz| { BlobVec::<u8>::reserve(x, sz); },
        );
        assert_eq!(addr.0, if align_of::<usize>() == 1 { 0 } else { align_of::<usize>() });
        let mut buf = vec![0u8; sz.0];
        let mut cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(addr.0) });
        cur = unsafe { ListMap::<BlobVec<u8>, BlobVec<u8>>::serialize(&origin, cur,
            |x, xcur| { BlobVec::<u8>::serialize(x, xcur, |y, ycur| { *ycur = *y; }) },
            |x, xcur| { BlobVec::<u8>::serialize(x, xcur, |y, ycur| { *ycur = *y; }) },
        )};
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let mut cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(addr.0) });
        cur = unsafe { ListMap::<BlobVec<u8>, BlobVec<u8>>::deserialize(cur,
            |xcur| BlobVec::<u8>::deserialize(xcur, |_| ()),
            |xcur| BlobVec::<u8>::deserialize(xcur, |_| ()),
        )};
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let vecmap = unsafe {
            &*(buf.as_ptr().add(addr.0) as *const ListMap::<BlobVec<u8>, BlobVec<u8>>) };

        let key = b"aa".as_ref();
        let mut iter = unsafe { vecmap.iter_matches(&key) };
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!(unsafe { (k.as_ref(), v.as_ref()) }, (b"aa".as_ref(), b"foo".as_ref()));
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!(unsafe { (k.as_ref(), v.as_ref()) }, (b"aa".as_ref(), b"".as_ref()));
        assert_eq!(unsafe { iter.next() }.is_none(), true);
    }

    #[test]
    pub fn test_blobhashmap() {
        let origin0 = vec![(1, b"foo".to_vec()), (3, b"hello".to_vec()), (5, b"".to_vec())];
        let mut origin = vec![vec![], vec![], vec![], vec![]];
        for (k, v) in origin0 {
            origin[k as usize & 3].push((k, v));
        }
        let mut sz = Reserve(0);
        let my_addr = BlobHashMap::<AssocList<HomoKeyAssoc<u8, BlobVec<u8>>>>::reserve(
            &origin, &mut sz,
            |alist, sz| {
                AssocList::<HomoKeyAssoc<u8, BlobVec<u8>>>::reserve(alist, sz, |kv, sz| {
                    HomoKeyAssoc::<u8, BlobVec<u8>>::reserve(kv, sz, |v, sz| {
                        BlobVec::<u8>::reserve(v, sz);
                    });
                });
            }
        );
        assert_eq!(my_addr.0, 0);
        let mut buf = vec![0u8; sz.0];
        let mut cur = BuildCursor::new(buf.as_mut_ptr());
        cur = unsafe { BlobHashMap::<AssocList<HomoKeyAssoc<u8, BlobVec<u8>>>>::serialize(
            &origin, cur,
            |alist, alist_cur| {
                AssocList::<HomoKeyAssoc<u8, BlobVec<u8>>>::serialize(alist, alist_cur,
                    |kv, kv_cur| {
                        HomoKeyAssoc::<u8, BlobVec<u8>>::serialize(kv, kv_cur,
                            |k, k_cur| { *k_cur = *k; },
                            |v, v_cur| {
                                BlobVec::<u8>::serialize(v, v_cur, |x, x_cur| { *x_cur = *x; })
                            }
                        )
                    }
                )
            }
        )};
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let mut cur = BuildCursor::new(buf.as_mut_ptr());
        cur = unsafe { BlobHashMap::<AssocList<HomoKeyAssoc<u8, BlobVec<u8>>>>::deserialize(cur,
            |alist_cur| AssocList::<HomoKeyAssoc<u8, BlobVec<u8>>>::deserialize(alist_cur,
                |kv_cur| HomoKeyAssoc::<u8, BlobVec<u8>>::deserialize(kv_cur, |_| (), |v_cur|
                    BlobVec::<u8>::deserialize(v_cur, |_| ())
                )
            )
        )};
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let hash = unsafe { &*(buf.as_ptr() as
            *const BlobHashMap::<AssocList<HomoKeyAssoc<u8, BlobVec<u8>>>>) };
        assert_eq!(unsafe { hash.get(&3).unwrap().as_ref() }, b"hello".as_ref());
    }

    pub struct TestU8BuildConfig;
    impl U8BuildConfig for TestU8BuildConfig {
        fn guard_size_keep(&self) -> u32 { 2 }
        fn hashmap_cap_power_fn(&self, _len: usize) -> usize { 1 }
        fn dense_guard_count(&self) -> usize { 3 }
    }

    pub unsafe fn create_states<'a>(buf: &'a mut Vec<u8>, qs: Vec<char_nfa::State>)
        -> Vec<&'a U8State<'a>>
    {
        let states = qs.iter().map(|q|
            U8StatePrepared::prepare(&q, &TestU8BuildConfig)).collect();
        let mut sz = Reserve(0);
        let (list_addr, addrs) = Sediment::<U8State>::reserve(&states, &mut sz, |state, sz| {
            U8State::reserve(state, sz)
        });
        assert_eq!(list_addr, 0);
        buf.resize(sz.0 + size_of::<usize>(), 0);
        let buf = align_up_ptr::<u128>(buf.as_mut_ptr());
        let mut cur = BuildCursor::new(buf);
        cur = unsafe { Sediment::<U8State>::serialize(&states, cur,
            |state, state_cur| { U8State::serialize(state, state_cur, &addrs) })};
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let mut cur = BuildCursor::new(buf);
        cur = unsafe { Sediment::<U8State>::deserialize(cur,
            |state_cur| U8State::deserialize(state_cur)) };
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        (0..qs.len()).map(|i| &*(buf.add(addrs[i]) as *const U8State)).collect()
    }

    pub fn expect_dense<'a, 'b>(iter: U8StateIterator<'a, 'b>) -> U8DenseStateIterator<'a> {
        match iter {
            U8StateIterator::Dense(iter) => iter,
            _ => unreachable!(),
        }
    }

    pub fn expect_sparse<'a, 'b>(iter: U8StateIterator<'a, 'b>) -> U8SparseStateIterator<'a, 'b> {
        match iter {
            U8StateIterator::Sparse(iter) => iter,
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_states() {
        let states = vec![
            char_nfa::State {
                tags: OrderedIxs(vec![]),
                transitions: vec![
                    (Guard::from_range((b'a', b'b')), 0),
                    (Guard::from_range((b'a', b'a')), 1),
                    (Guard::from_range((b'd', b'z')), 1),
                ],
                is_deterministic: false,
            },
            char_nfa::State {
                tags: OrderedIxs(vec![1, 2]),
                transitions: vec![(Guard::from_range((b'b', b'm')), 0)],
                is_deterministic: false,
            },
        ];
        let mut buf = vec![];
        let states = unsafe { create_states(&mut buf, states) };
        let state0 = states[0];
        let state1 = states[1];

        let mut iter = expect_dense(unsafe { state0.iter_matches(&b'c') });
        assert!(unsafe { iter.next() }.is_none());

        let mut iter = expect_dense(unsafe { state0.iter_matches(&b'a') });
        let mut succs = vec![
            *unsafe { iter.next() }.unwrap(),
            *unsafe { iter.next() }.unwrap(),
        ];
        assert!(unsafe { iter.next() }.is_none());
        succs.sort();
        assert_eq!(succs, [state0 as *const U8State, state1]);

        let mut iter = expect_dense(unsafe { state0.iter_matches(&b'p') });
        let succs = vec![*unsafe { iter.next() }.unwrap()];
        assert!(unsafe { iter.next() }.is_none());
        assert_eq!(succs, vec![state1 as *const U8State]);

        let mut iter = expect_sparse(unsafe { state1.iter_matches(&b'a') });
        assert!(unsafe { iter.next() }.is_none());

        let mut iter = expect_sparse(unsafe { state1.iter_matches(&b'c') });
        let succs = vec![unsafe { iter.next() }.unwrap()];
        assert!(unsafe { iter.next() }.is_none());
        assert_eq!(succs, vec![state0 as *const U8State]);

        let no_tags: &[usize] = &[];
        assert_eq!(unsafe { state0.get_tags() }, no_tags);
        assert_eq!(unsafe { state1.get_tags() }, &[1usize, 2]);
    }
}
