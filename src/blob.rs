// WARNING: No endianness handling is implemented yet, as we have no use case for BigEndian.

use std::mem::{size_of, align_of};
use std::marker::PhantomData;
use std::ptr;

use twox_hash::XxHash64;

use crate::guards::Guard;
use crate::char_nfa;

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
    fn matches(&self, other: &T) -> bool;
}

pub struct EqMatch<'a, X>(pub &'a X);

impl<'a, X: Eq> Matches<X> for EqMatch<'a, X> {
    fn matches(&self, other: &X) -> bool {
        *self.0 == *other
    }
}

impl Matches<Guard> for u8 {
    fn matches(&self, other: &Guard) -> bool {
        other.contains(*self)
    }
}

pub struct AnyMatch;

impl<T> Matches<T> for AnyMatch {
    fn matches(&self, _: &T) -> bool { true }
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
    fn iter_matches<'c, 'b, X: Matches<Self::Key>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c;
}

#[repr(C)]
pub struct BlobHashMap<'a, AList> {
    mask: usize,
    _phantom: PhantomData<&'a AList>,
}

impl<'a, AList: Assocs<'a>> BlobHashMap<'a, AList> {
    unsafe fn get(&self, key: &AList::Key) -> Option<&AList::Val>
        where AList::Key: Eq + MyHash
    {
        let ix = key.my_hash() & self.mask;
        let alist_ptr = *get_behind_struct::<_, *const AList>(self).add(ix);
        if alist_ptr.is_null() {
            return None;
        }
        let alist = &*alist_ptr;
        alist.iter_matches(&EqMatch(key)).next().map(|(_, val)| val)
    }
}

impl<'a, AList> BlobHashMap<'a, AList> {
    pub unsafe fn deserialize
    <
        F: Fn(BuildCursor<AList>) -> BuildCursor<AList>,
        After,
    >
    (cur: BuildCursor<Self>, f: F) -> BuildCursor<After> {
        let mut arr_cur = cur.behind::<*const AList>(1);
        let hashmap_cap = (*cur.get_mut()).mask + 1;
        let mut alist_cur = arr_cur.behind::<AList>(hashmap_cap);
        for _ in 0..(*cur.get_mut()).mask + 1 {
            let arr_ptr = arr_cur.get_mut();
            if !(*arr_ptr).is_null() {
                Shifter(cur.buf).shift(&mut *arr_ptr);
                alist_cur = f(alist_cur);
            }
            arr_cur.inc();
        }
        alist_cur.behind(0)
    }
}

impl<'a, AList: Build> Build for BlobHashMap<'a, AList> {
    type Origin = Vec<AList::Origin>;
}

pub trait IsEmpty {
    fn is_empty(&self) -> bool;
}

impl<X> IsEmpty for Vec<X> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<'a, AList: Build> BlobHashMap<'a, AList> where AList::Origin: IsEmpty {
    pub fn reserve<R, F: Fn(&AList::Origin, &mut Reserve) -> R>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, f: F) -> (usize, Vec<R>) {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        sz.add::<*const AList>(origin.len());
        let mut results = Vec::with_capacity(origin.len());
        for alist in origin.iter() {
            if !alist.is_empty() {
                results.push(f(alist, sz));
            }
        }
        (my_addr, results)
    }

    pub unsafe fn serialize
    <
        F: FnMut(&AList::Origin, BuildCursor<AList>) -> BuildCursor<AList>,
        After,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut f: F) -> BuildCursor<After>
    {
        (*cur.get_mut()).mask = origin.len() - 1;
        let mut arr_cur = cur.behind::<*const AList>(1);
        let mut alist_cur = arr_cur.behind::<AList>(origin.len());
        for alist_origin in origin.iter() {
            if alist_origin.is_empty() {
                *arr_cur.get_mut() = ptr::null();
            } else {
                *arr_cur.get_mut() = alist_cur.cur as *const AList;
                alist_cur = f(alist_origin, alist_cur);
            }
            arr_cur.inc()
        }
        alist_cur.behind(0)
    }
}

#[repr(C)]
pub struct List<'a, X> {
    next: *const List<'a, X>,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> UnsafeIterator for *const List<'a, X> {
    type Item = &'a X;
    unsafe fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.is_null() {
                return None;
            }
            let item = *self;
            *self = (*item).next;
            return Some(&*get_behind_struct::<_, X>(item));
        }
    }
}

impl<'a, X> List<'a, X> {
    pub unsafe fn deserialize
    <F: Fn(BuildCursor<X>) -> BuildCursor<Self>, After>
    (mut cur: BuildCursor<Self>, f: F) -> BuildCursor<After>
    {
        loop {
            let alist = &mut *cur.get_mut();
            cur = f(cur.behind(1));
            if alist.next.is_null() { return cur.behind(0); }
            Shifter(cur.buf).shift(&mut alist.next);
        }
    }
}

impl<'a, X: Build> Build for List<'a, X> {
    type Origin = Vec<X::Origin>;
}

impl<'a, X: Build> List<'a, X> {
    pub fn reserve<R, F: Fn(&X::Origin, &mut Reserve) -> R>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, f: F) -> (usize, Vec<R>)
    {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        let mut results = Vec::with_capacity(origin.len());
        for x in origin.iter() { sz.add::<Self>(1); results.push(f(x, sz)); }
        sz.add::<Self>(0);
        (my_addr, results)
    }

    pub unsafe fn serialize
    <
        After,
        F: FnMut(&X::Origin, BuildCursor<X>) -> BuildCursor<Self>,
    >
    (origin: &<Self as Build>::Origin, mut cur: BuildCursor<Self>, mut f: F) -> BuildCursor<After>
    {
        for (i, x) in origin.iter().enumerate() {
            if i == origin.len() - 1 {
                (*cur.get_mut()).next = ptr::null();
                cur = f(x, cur.behind(1));
            } else {
                let next = &mut (*cur.get_mut()).next;
                cur = f(x, cur.behind(1));
                *next = cur.cur as *const Self;
            }
        }
        cur.behind(0)
    }
}

#[repr(C)]
pub struct AssocList<'a, KV>(List<'a, KV>);

impl<'a, KV> AssocList<'a, KV> {
    pub unsafe fn deserialize
    <
        F: Fn(BuildCursor<KV>) -> BuildCursor<List<'a, KV>>,
        After,
    >
    (cur: BuildCursor<Self>, f: F) -> BuildCursor<After> {
        <List<'a, KV>>::deserialize(cur.behind(0), f)
    }
}

impl<'a, KV: Build> Build for AssocList<'a, KV> {
    type Origin = Vec<KV::Origin>;
}

impl<'a, KV: Build> AssocList<'a, KV> {
    pub fn reserve<R, F: Fn(&KV::Origin, &mut Reserve) -> R>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, f: F) -> (usize, Vec<R>)
    { <List<'a, KV>>::reserve(origin, sz, f) }

    pub unsafe fn serialize
    <
        After,
        F: FnMut(&KV::Origin, BuildCursor<KV>) -> BuildCursor<List<'a, KV>>,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, f: F) -> BuildCursor<After>
    {
        <List<'a, KV>>::serialize(origin, cur.behind(0), f)
    }
}

pub trait Assoc<'a> {
    type Key: 'a;
    type Val: 'a;

    unsafe fn key(&self) -> &'a Self::Key;
    unsafe fn val(&self) -> &'a Self::Val;
}

#[repr(C)]
pub struct HomoKeyAssoc<'a, K, V> {
    key: K,
    _phantom: PhantomData<&'a V>
}

impl<'a, K, V> HomoKeyAssoc<'a, K, V> {
    pub unsafe fn deserialize
    <
        After,
        FK: Fn(&mut K),
        FV: Fn(BuildCursor<V>) -> BuildCursor<After>,
    >
    (cur: BuildCursor<Self>, fk: FK, fv: FV) -> BuildCursor<After>
    {
        fk(&mut (*cur.get_mut()).key);
        fv(cur.behind(1))
    }
}

impl<'a, K: Build, V: Build> Build for HomoKeyAssoc<'a, K, V> {
    type Origin = (K::Origin, V::Origin);
}

impl<'a, K: Build, V: Build> HomoKeyAssoc<'a, K, V> {
    pub fn reserve<RV, FV: Fn(&V::Origin, &mut Reserve) -> RV>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, fv: FV) -> (usize, RV)
    {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        let rv = fv(&origin.1, sz);
        (my_addr, rv)
    }

    pub unsafe fn serialize
    <
        After,
        FK: FnMut(&K::Origin, &mut K),
        FV: FnMut(&V::Origin, BuildCursor<V>) -> BuildCursor<After>,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut fk: FK, mut fv: FV)
    -> BuildCursor<After>
    {
        fk(&origin.0, &mut (*cur.get_mut()).key);
        fv(&origin.1, cur.behind(1))
    }
}

impl<'a, K: 'a, V: 'a> Assoc<'a> for HomoKeyAssoc<'a, K, V> {
    type Key = K;
    type Val = V;

    unsafe fn key(&self) -> &'a K { &*(&self.key as *const _) }
    unsafe fn val(&self) -> &'a V { &*get_behind_struct(self) }
}

pub struct AssocListIter<'a, 'b, X, KV> {
    x: &'b X,
    cur: *const List<'a, KV>,
}

impl<'a, 'b, KV: 'b + Assoc<'a>, X: Matches<KV::Key>> UnsafeIterator
for AssocListIter<'a, 'b, X, KV>
{
    type Item = (&'a KV::Key, &'a KV::Val);

    unsafe fn next(&mut self) -> Option<Self::Item> {
        while let Some(key_val) = self.cur.next() {
            let key = key_val.key();
            if self.x.matches(key) { return Some((key, key_val.val())); }
        }
        None
    }
}

impl<'a, KV: Assoc<'a>> AssocsSuper<'a> for AssocList<'a, KV> {
    type Key = KV::Key;
    type Val = KV::Val;
    type I<'b, X: 'b + Matches<KV::Key>> = AssocListIter<'a, 'b, X, KV> where 'a: 'b;
}

impl<'a, KV: Assoc<'a>> Assocs<'a> for AssocList<'a, KV> {
    fn iter_matches<'c, 'b, X: Matches<KV::Key>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c
    { AssocListIter { x: key, cur: &self.0 } }
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

impl<'a, X: Build> Build for BlobVec<'a, X> {
    type Origin = Vec<X::Origin>;
}

#[repr(C)]
pub struct BlobVec<'a, X> {
    len: usize,
    _phantom: PhantomData<&'a X>,
}

pub struct BlobVecIter<'a, X> {
    cur: *const X,
    end: *const X,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> BlobVec<'a, X> {
    pub fn iter(&self) -> BlobVecIter<'a, X> {
        let cur = unsafe { get_behind_struct::<_, X>(self) };
        BlobVecIter {
            cur,
            end: unsafe { cur.add(self.len) },
            _phantom: PhantomData,
        }
    }

    pub unsafe fn get(&self, ix: usize) -> &X {
        assert!(ix < self.len);
        &*get_behind_struct::<_, X>(self).add(ix)
    }

    pub unsafe fn as_ref(&self) -> &[X] {
        std::slice::from_raw_parts(get_behind_struct::<_, X>(self), self.len)
    }

    pub unsafe fn deserialize<F: Fn(&mut X), After>
    (cur: BuildCursor<Self>, f: F) -> BuildCursor<After>
    {
        let mut xcur = cur.behind(1);
        for _ in 0..(*cur.get_mut()).len { f(&mut *xcur.get_mut()); xcur.inc(); }
        xcur.behind(0)
    }
}

impl<'a, X: Build> BlobVec<'a, X> {
    pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        sz.add::<X>(origin.len());
        my_addr
    }

    pub fn elem_addr(my_addr: usize, ix: usize) -> usize {
        align_up(my_addr + size_of::<Self>(), align_of::<X>()) + size_of::<X>() * ix
    }

    pub unsafe fn serialize<F: FnMut(&X::Origin, &mut X), After>
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut f: F) -> BuildCursor<After>
    {
        (*cur.get_mut()).len = origin.len();
        let mut xcur = cur.behind(1);
        for x in origin.iter() { f(x, &mut *xcur.get_mut()); xcur.inc(); }
        xcur.behind(0)
    }
}

impl<'a, X> UnsafeIterator for BlobVecIter<'a, X> {
    type Item = &'a X;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        if self.cur == self.end {
            return None;
        }
        let ret = self.cur;
        self.cur = self.cur.add(1);
        Some(&*ret)
    }
}

#[repr(C)]
pub struct VecMapItem<K, V> {
    key: K,
    val: *const V,
}

impl<K: Build, V: Build> Build for VecMapItem<K, V> {
    type Origin = (K::Origin, V::Origin);
}

type VecMapVec<'a, K, V> = BlobVec<'a, VecMapItem<K, V>>;

#[repr(C)]
pub struct VecMap<'a, K, V> {
    keys: VecMapVec<'a, K, V>,
    _phantom: PhantomData<&'a V>,
}

impl<'a, K: Build, V: Build> Build for VecMap<'a, K, V> {
    type Origin = Vec<(K::Origin, V::Origin)>;
}

impl<'a, K: Build, V: Build> VecMap<'a, K, V> {
    pub fn reserve<RV, FV: Fn(&V::Origin, &mut Reserve) -> RV>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, fv: FV) -> (usize, Vec<RV>)
    {
        let my_addr = <VecMapVec<'a, K, V>>::reserve(origin, sz);
        let mut vaddrs = Vec::with_capacity(origin.len());
        for (_, v) in origin.iter() { vaddrs.push(fv(v, sz)); }
        (my_addr, vaddrs)
    }

    pub fn key_addr(my_addr: usize, ix: usize) -> usize {
        <VecMapVec<'a, K, V>>::elem_addr(my_addr, ix)
    }

    pub unsafe fn serialize
    <
        After,
        FK: FnMut(&K::Origin, &mut K),
        FV: FnMut(&V::Origin, BuildCursor<V>) -> BuildCursor<V>,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut fk: FK, mut fv: FV)
    -> BuildCursor<After>
    {
        let kcur = cur.behind::<VecMapVec<'a, K, V>>(0);
        let item_cur = kcur.behind::<VecMapItem<K, V>>(1);
        let mut vcur = item_cur.behind::<V>(origin.len());
        <VecMapVec<'a, K, V>>::serialize::<_, V>(origin, kcur, |kv, bk| {
            fk(&kv.0, &mut bk.key);
            bk.val = vcur.cur as *const V;
            vcur = fv(&kv.1, vcur.clone());
        });
        vcur.behind(0)
    }
}

impl<'a, K, V> VecMap<'a, K, V> {
    pub unsafe fn deserialize<
        After,
        FK: Fn(&mut K),
        FV: Fn(BuildCursor<V>) -> BuildCursor<V>,
    >
    (cur: BuildCursor<Self>, fk: FK, fv: FV) -> BuildCursor<After>
    {
        let kcur = cur.behind::<VecMapVec<'a, K, V>>(0);
        let len = (*kcur.get_mut()).len;
        let shifter = Shifter(cur.buf);
        let mut vcur = BlobVec::deserialize(kcur, |kv| {
            fk(&mut kv.key);
            shifter.shift(&mut kv.val);
        });
        for _ in 0..len { vcur = fv(vcur); }
        vcur.behind(0)
    }
}

pub struct VecMapIter<'a, 'b, X, K, V> {
    x: &'b X,
    vec_iter: BlobVecIter<'a, VecMapItem<K, V>>,
    _phantom: PhantomData<&'a K>,
}

impl<'a, 'b, X: Matches<K>, K, V: 'b> UnsafeIterator for VecMapIter<'a, 'b, X, K, V> {
    type Item = (&'a K, &'a V);

    unsafe fn next(&mut self) -> Option<Self::Item> {
        while let Some(VecMapItem{ key, val }) = self.vec_iter.next() {
            if self.x.matches(key) {
                return Some((&key, &**val));
            }
        }
        None
    }
}

impl<'a, K: 'a, V: 'a> AssocsSuper<'a> for VecMap<'a, K, V> {
    type Key = K;
    type Val = V;
    type I<'b, X: 'b + Matches<K>> = VecMapIter<'a, 'b, X, K, V> where 'a: 'b;
}

impl<'a, K: 'a, V: 'a> Assocs<'a> for VecMap<'a, K, V> {
    fn iter_matches<'c, 'b, X: Matches<K>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c
    {
        VecMapIter {
            x: key,
            vec_iter: self.keys.iter(),
            _phantom: PhantomData,
        }
    }
}

type U8States<'a> = BlobVec<'a, *const U8State<'a>>;
type U8AItem<'a> = HomoKeyAssoc<'a, u8, U8States<'a>>;
type U8AList<'a> = AssocList<'a, U8AItem<'a>>;
type U8ExplicitTrans<'a> = BlobHashMap<'a, U8AList<'a>>;
type U8Tags<'a> = BlobVec<'a, usize>;
type U8PatternTrans<'a> = VecMap<'a, Guard, U8States<'a>>;

impl Build for *const U8State<'_> {
    type Origin = usize;
}

impl Build for usize {
    type Origin = usize;
}

impl Build for () {
    type Origin = ();
}

#[repr(C)]
pub struct U8State<'a> {
    tags: *const U8Tags<'a>,
    explicit_trans: *const U8ExplicitTrans<'a>,
    pattern_trans: U8PatternTrans<'a>,
}

impl<'a> Build for U8State<'a> {
    type Origin = U8StatePrepared;
}

impl<'a> U8State<'a> {
    pub fn iter_matches<'c, 'b>(&'c self, key: &'b u8) -> U8StateIterator<'a, 'b>
        where 'a: 'b + 'c
    {
        U8StateIterator {
            pattern_iter: self.pattern_trans.iter_matches(key),
            states_iter: None,
            explicit_trans: self.explicit_trans,
        }
    }

    pub unsafe fn get_tags(&self) -> &[usize] {
        if self.tags.is_null() { &[] }
        else { (*self.tags).as_ref() }
    }

    pub unsafe fn deserialize<B>(state_cur: BuildCursor<U8State>) -> BuildCursor<B> {
        let shifter = Shifter(state_cur.buf);
        let shiftq = |q: &mut *const U8State| shifter.shift(q);
        let shiftqs1 = |qs_cur| U8States::deserialize(qs_cur, shiftq);
        let shiftqs2 = |qs_cur| U8States::deserialize(qs_cur, shiftq);
        let state = &mut *state_cur.get_mut();
        shifter.shift(&mut state.explicit_trans);

        let f_tags_cur = state_cur.behind::<*const U8Tags>(0);
        let f_explicit_trans_cur = f_tags_cur.behind::<*const U8ExplicitTrans>(1);
        let f_pattern_trans_cur = f_explicit_trans_cur.behind::<U8PatternTrans>(1);
        let exp_cur = U8PatternTrans::deserialize(f_pattern_trans_cur, |_| (), shiftqs1);

        let tags_cur: BuildCursor<u8> = U8ExplicitTrans::deserialize(exp_cur, |alist_cur|
            U8AList::deserialize(alist_cur, |kv_cur|
                U8AItem::deserialize(kv_cur, |_| (), shiftqs2)
            )
        );

        if state.tags.is_null() { tags_cur.behind(0) }
        else {
            shifter.shift(&mut state.tags);
            U8Tags::deserialize(tags_cur.behind(0), |_| ())
        }
    }

    fn resqs(qs: &Vec<usize>, sz: &mut Reserve) {
        U8States::reserve(qs, sz);
    }

    pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
        sz.add::<U8State>(0);
        let result = sz.0;
        sz.add::<*const U8Tags>(1);
        sz.add::<*const U8ExplicitTrans>(1);
        U8PatternTrans::reserve(&origin.pattern_trans, sz, Self::resqs);
        U8ExplicitTrans::reserve(&origin.explicit_trans, sz, |alist, sz| {
            U8AList::reserve(alist, sz, |kv, sz| {
                U8AItem::reserve(kv, sz, Self::resqs);
            });
        });
        if !origin.tags.is_empty() {
            U8Tags::reserve(&origin.tags, sz);
        }
        result
    }

    pub unsafe fn serialize<After>
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, qptrs: &Vec<usize>)
    -> BuildCursor<After>
    {
        let state = &mut *cur.get_mut();
        let setq = |q: &usize, qref: &mut *const U8State| { *qref = qptrs[*q] as *const U8State; };
        let f_tags_cur = cur.behind::<*const U8Tags>(0);
        let f_explicit_trans_cur = f_tags_cur.behind::<*const U8ExplicitTrans>(1);
        let f_pattern_trans_cur = f_explicit_trans_cur.behind::<U8PatternTrans>(1);
        let exp_cur = U8PatternTrans::serialize(
            &origin.pattern_trans, f_pattern_trans_cur,
            |guard, guardref| { *guardref = *guard; },
            |qs, qs_cur| { U8States::serialize(qs, qs_cur, setq) }
        );
        state.explicit_trans = exp_cur.cur as *const U8ExplicitTrans;
        let tags_cur: BuildCursor<u8> = U8ExplicitTrans::serialize(
            &origin.explicit_trans, exp_cur, |alist, alist_cur| {
                U8AList::serialize(alist, alist_cur, |kv, kv_cur| {
                    U8AItem::serialize(kv, kv_cur, |c, c_cur| { *c_cur = *c; },
                        |qs, qs_cur| { U8States::serialize(qs, qs_cur, setq) }
                    )
                })
            }
        );
        if origin.tags.is_empty() { tags_cur.behind(0) }
        else {
            let tags_cur = tags_cur.behind(0);
            state.tags = tags_cur.cur as *const U8Tags;
            U8Tags::serialize(&origin.tags, tags_cur, |t, tref| { *tref = *t; })
        }
    }
}

pub struct U8StateIterator<'a, 'b> {
    pattern_iter: VecMapIter<'a, 'b, u8, Guard, U8States<'a>>,
    states_iter: Option<BlobVecIter<'a, *const U8State<'a>>>,
    explicit_trans: *const U8ExplicitTrans<'a>,
}

impl<'a, 'b> UnsafeIterator for U8StateIterator<'a, 'b> where 'a: 'b {
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
                if self.explicit_trans.is_null() { return None; }
                else {
                    let explicit_trans = &*self.explicit_trans;
                    self.explicit_trans = std::ptr::null();
                    if let Some(states) = explicit_trans.get(self.pattern_iter.x) {
                        let mut states_iter = states.iter();
                        if let Some(state) = states_iter.next() {
                            self.states_iter = Some(states_iter);
                            return Some(*state);
                        } else { return None; }
                    } else { return None; }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct U8StatePrepared {
    tags: Vec<usize>,
    pattern_trans: Vec<(Guard, Vec<usize>)>,
    explicit_trans: Vec<Vec<(u8, Vec<usize>)>>,  // has size of 2**hashmap_cap
}


mod build {
    use hashbrown::HashMap;
    use super::*;

    pub trait U8BuildConfig {
        fn guard_size_keep(&self) -> u32;
        fn hashmap_cap_power_fn(&self, len: usize) -> usize;
    }

    impl U8StatePrepared {
        pub fn prepare<Cfg: U8BuildConfig>(old: &char_nfa::State, cfg: &Cfg) -> Self {
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
                if c == 255 { break; }
                c += 1;
            }
            let hashmap_cap_power = cfg.hashmap_cap_power_fn(explicit_trans0.len());
            let hashmap_cap = 1 << hashmap_cap_power;
            let hashmap_mask = hashmap_cap - 1;
            let mut hashmap_alists = Vec::<Vec<(u8, Vec<usize>)>>::with_capacity(hashmap_cap);
            for _ in 0..hashmap_cap { hashmap_alists.push(Vec::new()) }
            for (c, targets) in explicit_trans0 {
                hashmap_alists[c as usize & hashmap_mask].push((c, targets));
            }

            Self {
                tags: old.tags.0.clone(),
                pattern_trans: pattern_trans0.into_iter().collect(),
                explicit_trans: hashmap_alists
            }
        }
    }
}


#[cfg(test)]
pub mod tests {
    use crate::char_enfa::OrderedIxs;

    use super::*;
    use super::build::*;

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
        let mut iter = blobvec.iter();
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

        let mut iter = vecmap.iter_matches(&EqMatch(&3));
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&3, b"hello".as_ref()));
        assert_eq!(unsafe { iter.next() }.is_none(), true);

        let mut iter = vecmap.iter_matches(&AnyMatch);
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&1, b"foo".as_ref()));
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&3, b"hello".as_ref()));
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&5, b"".as_ref()));
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
    }

    pub unsafe fn create_states<'a>(buf: &'a mut Vec<u8>, qs: Vec<char_nfa::State>)
        -> Vec<&'a U8State<'a>>
    {
        let states = qs.iter().map(|q|
            ((), U8StatePrepared::prepare(&q, &TestU8BuildConfig))).collect();
        let mut sz = Reserve(0);
        let (list_addr, addrs) = VecMap::<(), U8State>::reserve(&states, &mut sz, |state, sz| {
            U8State::reserve(state, sz)
        });
        assert_eq!(list_addr, 0);
        buf.resize(sz.0 + size_of::<usize>(), 0);
        let buf = align_up_ptr::<u128>(buf.as_mut_ptr());
        let mut cur = BuildCursor::new(buf);
        cur = unsafe { VecMap::<(), U8State>::serialize(&states, cur, |_, _| (),
            |state, state_cur| { U8State::serialize(state, state_cur, &addrs) }
        )};
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let mut cur = BuildCursor::new(buf);
        cur = unsafe { VecMap::<(), U8State>::deserialize(cur, |_| (),
            |state_cur| U8State::deserialize(state_cur)) };
        assert_eq!(cur.cur, cur.cur);  // suppress unused_assign warning
        let vecmap = unsafe { &*(buf as *const VecMap<(), U8State>) };
        let mut iter = vecmap.iter_matches(&AnyMatch);
        let result = (0..qs.len()).map(|_| iter.next().unwrap().1).collect::<Vec<_>>();
        assert!(unsafe { iter.next() }.is_none());
        result
    }

    #[test]
    fn test_states() {
        let states = vec![
            char_nfa::State {
                tags: OrderedIxs(vec![]),
                transitions: vec![
                    (Guard::from_range((b'a', b'a')), 0),
                    (Guard::from_range((b'a', b'a')), 1),
                    (Guard::from_range((b'c', b'z')), 1),
                ],
                is_deterministic: false,
            },
            char_nfa::State {
                tags: OrderedIxs(vec![1, 2]),
                transitions: vec![(Guard::from_range((b'b', b'b')), 0)],
                is_deterministic: false,
            },
        ];
        let mut buf = vec![];
        let states = unsafe { create_states(&mut buf, states) };
        let state0 = states[0];
        let state1 = states[1];

        let mut iter = state0.iter_matches(&b'b');
        assert!(unsafe { iter.next() }.is_none());

        let mut iter = state0.iter_matches(&b'a');
        let mut succs = vec![
            unsafe { iter.next() }.unwrap(),
            unsafe { iter.next() }.unwrap(),
        ];
        assert!(unsafe { iter.next() }.is_none());
        succs.sort();
        assert_eq!(succs, [state0 as *const U8State, state1]);

        let mut iter = state0.iter_matches(&b'p');
        let succs = vec![unsafe { iter.next() }.unwrap()];
        assert!(unsafe { iter.next() }.is_none());
        assert_eq!(succs, vec![state1 as *const U8State]);

        let no_tags: &[usize] = &[];
        assert_eq!(unsafe { state0.get_tags() }, no_tags);
        assert_eq!(unsafe { state1.get_tags() }, &[1usize, 2]);
    }
}
