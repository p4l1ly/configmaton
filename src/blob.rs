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

struct EqMatch<'a, X>(&'a X);

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

pub trait UnsafeIterator {
    type Item;
    unsafe fn next(&mut self) -> Option<Self::Item>;
}

fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}

unsafe fn get_behind_struct<A, B>(a: *const A) -> *const B {
    align_up((a as *const u8).add(size_of::<A>()) as usize, align_of::<B>()) as *const B
}

struct Reserve(usize);

impl Reserve {
    fn add<T>(&mut self, n: usize) {
        self.0 = align_up(self.0, align_of::<T>()) + size_of::<T>() * n;
    }
}

#[derive(Copy)]
struct BuildIx<A>{
    ix: usize,
    buf: *mut u8,
    _phantom: PhantomData<A>,
}

impl<A> BuildIx<A> {
    fn inc(&mut self) {
        self.ix += size_of::<A>();
    }

    fn behind<B>(&self, n: usize) -> BuildIx<B> {
        BuildIx {
            ix: align_up(self.ix + size_of::<A>() * n, align_of::<B>()),
            buf: self.buf,
            _phantom: PhantomData
        }
    }

    unsafe fn get_mut(&self) -> *mut A {
        self.buf.add(self.ix) as *mut A
    }

}

impl<A> Clone for BuildIx<A> {
    fn clone(&self) -> Self {
        Self { ix: self.ix, buf: self.buf, _phantom: PhantomData }
    }
}

struct Shifter(*mut u8);
impl Shifter {
    unsafe fn shift<T>(&self, x: &mut *const T) {
        *x = self.0.add(*x as *const u8 as usize) as *const T
    }
}

#[repr(C)]
struct BlobHashMap<'a, AList> {
    mask: usize,
    _phantom: PhantomData<&'a AList>,
}

impl<'a, AList: AssocList<'a>> BlobHashMap<'a, AList> {
    unsafe fn get(&self, key: &AList::Key) -> Option<&AList::Val>
        where AList::Key: Eq + MyHash
    {
        let ix = key.my_hash() & self.mask;
        let alist_ptr = *get_behind_struct::<_, *const AList>(self).add(ix);
        if alist_ptr.is_null() {
            return None;
        }
        let alist = &*alist_ptr;
        alist.iter_matches(&EqMatch(key)).next()
    }
}

pub trait AssocListSuper<'a> {
    type Key: 'a;
    type Val: 'a;
    type I<'b, X: 'b + Matches<Self::Key>>: UnsafeIterator<Item = &'a Self::Val> where 'a: 'b;
}

trait AssocList<'a>: AssocListSuper<'a> {
    unsafe fn iter_matches<'c, 'b, X: Matches<Self::Key>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c;
}

#[repr(C)]
struct HomoKeyAlternAssocList<'a, K, V> {
    key: K,
    next: *const HomoKeyAlternAssocList<'a, K, V>,
    _phantom: PhantomData<&'a V>,
}

struct HomoKeyAlternAssocListIter<'a, 'b, X, K, V> {
    x: &'b X,
    cur: *const HomoKeyAlternAssocList<'a, K, V>,
    _phantom: PhantomData<&'a K>,
}

impl<'a, 'b, X: Matches<K>, K, V: 'b> UnsafeIterator for HomoKeyAlternAssocListIter<'a, 'b, X, K, V> {
    type Item = &'a V;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        loop {
            let cur_ref = &*self.cur;
            if self.x.matches(&cur_ref.key) {
                return Some(&*get_behind_struct::<_, V>(self.cur));
            }
            if cur_ref.next.is_null() {
                return None;
            }
            self.cur = cur_ref.next;
        }
    }
}

impl<'a, K: 'a, V: 'a> AssocListSuper<'a> for HomoKeyAlternAssocList<'a, K, V> {
    type Key = K;
    type Val = V;
    type I<'b, X: 'b + Matches<K>> = HomoKeyAlternAssocListIter<'a, 'b, X, K, V> where 'a: 'b;
}

impl<'a, K: 'a, V: 'a> AssocList<'a> for HomoKeyAlternAssocList<'a, K, V> {
    unsafe fn iter_matches<'c, 'b, X: Matches<K>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c
    {
        HomoKeyAlternAssocListIter {
            x: key,
            cur: self,
            _phantom: PhantomData,
        }
    }
}

#[repr(C)]
struct HomoVec<'a, X> {
    len: usize,
    _phantom: PhantomData<&'a X>,
}

struct HomoVecIter<'a, X> {
    cur: *const X,
    end: *const X,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> HomoVec<'a, X> {
    unsafe fn iter(&self) -> HomoVecIter<'a, X> {
        let cur = get_behind_struct::<_, X>(self);
        HomoVecIter {
            cur,
            end: cur.add(self.len),
            _phantom: PhantomData,
        }
    }

    unsafe fn get(&self, ix: usize) -> &X {
        assert!(ix < self.len);
        &*get_behind_struct::<_, X>(self).add(ix)
    }
}

impl<'a, X> UnsafeIterator for HomoVecIter<'a, X> {
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
struct HomoKeyAssocList<'a, K, V> {
    keys: HomoVec<'a, (K, *const V)>,
    _phantom: PhantomData<&'a V>,
}

struct HomoKeyAssocListIter<'a, 'b, X, K, V> {
    x: &'b X,
    vec_iter: HomoVecIter<'a, (K, *const V)>,
    _phantom: PhantomData<&'a K>,
}

impl<'a, 'b, X: Matches<K>, K, V: 'b> UnsafeIterator for HomoKeyAssocListIter<'a, 'b, X, K, V> {
    type Item = &'a V;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        while let Some((key, val)) = self.vec_iter.next() {
            if self.x.matches(key) {
                return Some(&**val);
            }
        }
        None
    }
}

impl<'a, K: 'a, V: 'a> AssocListSuper<'a> for HomoKeyAssocList<'a, K, V> {
    type Key = K;
    type Val = V;
    type I<'b, X: 'b + Matches<K>> = HomoKeyAssocListIter<'a, 'b, X, K, V> where 'a: 'b;
}

impl<'a, K: 'a, V: 'a> AssocList<'a> for HomoKeyAssocList<'a, K, V> {
    unsafe fn iter_matches<'c, 'b, X: Matches<K>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c
    {
        HomoKeyAssocListIter {
            x: key,
            vec_iter: self.keys.iter(),
            _phantom: PhantomData,
        }
    }
}

type U8States<'a> = HomoVec<'a, *const U8State<'a>>;
type U8AList<'a> = HomoKeyAlternAssocList<'a, u8, U8States<'a>>;
type U8ExplicitTrans<'a> = BlobHashMap<'a, U8AList<'a>>;
type U8Tags<'a> = HomoVec<'a, usize>;

#[repr(C)]
pub struct U8State<'a> {
    tags: *const U8Tags<'a>,
    explicit_trans: *const U8ExplicitTrans<'a>,
    pattern_trans: HomoKeyAssocList<'a, Guard, HomoVec<'a, *const U8State<'a>>>,
}

impl<'a> U8State<'a> {
    pub unsafe fn iter_matches<'c, 'b>(&'c self, key: &'b u8) -> U8StateIterator<'a, 'b>
        where 'a: 'b + 'c
    {
        U8StateIterator {
            pattern_iter: self.pattern_trans.iter_matches(key),
            states_iter: None,
            explicit_trans: self.explicit_trans,
        }
    }

}

pub struct U8StateIterator<'a, 'b> {
    pattern_iter: HomoKeyAssocListIter<'a, 'b, u8, Guard, HomoVec<'a, *const U8State<'a>>>,
    states_iter: Option<HomoVecIter<'a, *const U8State<'a>>>,
    explicit_trans: *const U8ExplicitTrans<'a>,
}

impl<'a, 'b> UnsafeIterator for U8StateIterator<'a, 'b> where 'a: 'b {
    type Item = &'a U8State<'a>;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        if let Some(states_iter) = self.states_iter.as_mut() {
            if let Some(state) = states_iter.next() {
                return Some(&**state);
            }
        }
        loop {
            if let Some(states) = self.pattern_iter.next() {
                let mut states_iter = states.iter();
                if let Some(state) = states_iter.next() {
                    self.states_iter = Some(states_iter);
                    return Some(&**state);
                }
            } else {
                if !self.explicit_trans.is_null() {
                    let explicit_trans = &*self.explicit_trans;
                    self.explicit_trans = std::ptr::null();
                    if let Some(states) = explicit_trans.get(self.pattern_iter.x) {
                        let mut states_iter = states.iter();
                        if let Some(state) = states_iter.next() {
                            self.states_iter = Some(states_iter);
                            return Some(&**state);
                        } else { return None; }
                    } else { return None; }
                }
            }
        }
    }
}

struct U8StatePrepared {
    tags: Vec<usize>,
    pattern_trans: Vec<(Guard, Vec<usize>)>,
    explicit_trans: Vec<Vec<(u8, Vec<usize>)>>,  // has size of 2**hashmap_cap
}


type U8PatternItem<'a> = (Guard, *const U8States<'a>);
impl U8StatePrepared {
    fn reserve(&self, sz: &mut Reserve) -> usize {
        sz.add::<U8State>(0);
        let result = sz.0;
        sz.add::<U8State>(1);
        sz.add::<U8PatternItem>(self.pattern_trans.len());
        for (_, targets) in self.pattern_trans.iter() {
            sz.add::<U8States>(1);
            sz.add::<*const U8State>(targets.len());
        }
        sz.add::<U8ExplicitTrans>(1);
        sz.add::<*const U8AList>(self.explicit_trans.len());
        for alist in self.explicit_trans.iter() {
            for (_, targets) in alist.iter() {
                sz.add::<U8AList>(1);
                sz.add::<U8States>(1);
                sz.add::<*const U8State>(targets.len());
            }
        }
        result
    }

    unsafe fn deserialize(state_ix: &mut BuildIx<U8State>) {
        let shifter = Shifter(state_ix.buf);
        let state = &mut *state_ix.get_mut();
        if !state.tags.is_null() { shifter.shift(&mut state.tags); }
        shifter.shift(&mut state.explicit_trans);

        let mut item_ix = state_ix.behind::<U8PatternItem>(1);
        let pattern_trans_len = state.pattern_trans.keys.len;
        let mut qs_ix = item_ix.behind::<U8States>(pattern_trans_len);
        for _ in 0..pattern_trans_len {
            let item = &mut *item_ix.get_mut();
            shifter.shift(&mut item.1);
            let mut qix = qs_ix.behind::<*const U8State>(1);
            for _ in 0..(*qs_ix.get_mut()).len {
                shifter.shift(&mut *qix.get_mut());
                qix.inc();
            }
            qs_ix = qix.behind(0);
            item_ix.inc();
        }

        let exp_ix = qs_ix.behind::<U8ExplicitTrans>(0);
        let mut arr_ix = exp_ix.behind::<*const U8AList>(1);
        let hashmap_cap = (*exp_ix.get_mut()).mask + 1;
        let mut alist_ix = arr_ix.behind::<U8AList>(hashmap_cap);
        for _ in 0..hashmap_cap {
            let arr_ptr = arr_ix.get_mut();
            if !(*arr_ptr).is_null() {
                shifter.shift(&mut *arr_ptr);
                loop {
                    let alist = &mut *alist_ix.get_mut();
                    let qs_ix = alist_ix.behind::<U8States>(1);
                    let mut qix = qs_ix.behind::<*const U8State>(1);
                    for _ in 0..(*qs_ix.get_mut()).len {
                        shifter.shift(&mut *qix.get_mut());
                        qix.inc();
                    }
                    alist_ix = qix.behind(0);
                    if alist.next.is_null() { break; }
                    shifter.shift(&mut alist.next);
                }
            }
            arr_ix.inc()
        }
    }

    unsafe fn serialize(&self, buf: *mut u8, ix: usize, ptrs: &Vec<usize>) {
        let state_ix = BuildIx::<U8State> { ix: ptrs[ix], buf, _phantom: PhantomData };
        assert!(state_ix.ix == align_up(state_ix.ix, align_of::<U8State>()));

        let mut item_ix = state_ix.behind::<U8PatternItem>(1);
        let mut qs_ix = item_ix.behind::<U8States>(self.pattern_trans.len());
        for (guard, qs) in self.pattern_trans.iter() {
            *item_ix.get_mut() = (*guard, qs_ix.ix as *mut U8States);
            *qs_ix.get_mut() = HomoVec { len: qs.len(), _phantom: PhantomData };
            let mut qix = qs_ix.behind::<*const U8State>(1);
            for &q in qs {
                *qix.get_mut() = ptrs[q] as *const U8State;
                qix.inc();
            }
            qs_ix = qix.behind(0);
            item_ix.inc();
        }

        let exp_ix = qs_ix.behind::<U8ExplicitTrans>(0);
        *exp_ix.get_mut() = BlobHashMap
            { mask: self.explicit_trans.len() - 1, _phantom: PhantomData };
        let mut arr_ix = exp_ix.behind::<*const U8AList>(1);
        let mut alist_ix = arr_ix.behind::<U8AList>(self.explicit_trans.len());
        for arritem_trans in self.explicit_trans.iter() {
            if arritem_trans.is_empty() {
                *arr_ix.get_mut() = ptr::null();
            } else {
                *arr_ix.get_mut() = alist_ix.ix as *const U8AList;

                for (i, (guard, qs)) in arritem_trans.iter().enumerate() {
                    let qs_ix = alist_ix.behind::<U8States>(1);
                    *qs_ix.get_mut() = HomoVec { len: qs.len(), _phantom: PhantomData };

                    let mut qix = qs_ix.behind::<*const U8State>(1);
                    for &q in qs {
                        *qix.get_mut() = ptrs[q] as *const U8State;
                        qix.inc();
                    }

                    let alist_ptr = alist_ix.get_mut();
                    alist_ix = qix.behind(0);
                    let next =
                        if i == arritem_trans.len() - 1 { ptr::null() }
                        else { alist_ix.ix as *const U8AList };
                    *alist_ptr = U8AList { key: *guard, next, _phantom: PhantomData };
                }
            }
            arr_ix.inc()
        }

        let tag_ptr =
            if self.tags.is_empty() { ptr::null() }
            else {
                let tags_ix = alist_ix.behind::<U8Tags>(0);
                *tags_ix.get_mut() = HomoVec { len: self.tags.len(), _phantom: PhantomData };
                let mut tag_ix = tags_ix.behind::<usize>(1);
                for &tag in self.tags.iter() {
                    *tag_ix.get_mut() = tag;
                    tag_ix.inc();
                }
                tags_ix.ix as *const U8Tags
            };

        *state_ix.get_mut() = U8State {
            tags: tag_ptr,
            explicit_trans: exp_ix.ix as *const U8ExplicitTrans,
            pattern_trans: HomoKeyAssocList {
                keys: HomoVec { len: self.pattern_trans.len(), _phantom: PhantomData },
                _phantom: PhantomData,
            }
        }
    }
}


mod build {
    use hashbrown::HashMap;
    use super::*;

    pub trait U8BuildConfig {
        fn guard_size_keep(&self) -> u32;
        fn hashmap_cap_fn(&self, len: usize) -> usize;
    }

    impl U8StatePrepared {
        fn prepare<Cfg: U8BuildConfig>(old: &char_nfa::State, cfg: &Cfg) -> Self {
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
            let hashmap_cap_power = cfg.hashmap_cap_fn(explicit_trans0.len());
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
