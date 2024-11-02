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

#[repr(C)]
struct BlobHashMap<'a, AList> {
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
        alist.iter_matches(&EqMatch(key)).next()
    }
}

impl<'a, AList> BlobHashMap<'a, AList> {
    pub unsafe fn deserialize
    <
        F: Fn(BuildCursor<AList>) -> BuildCursor<AList>,
        After,
    >
    (cur: BuildCursor<Self>, each: F) -> BuildCursor<After> {
        let mut arr_cur = cur.behind::<*const AList>(1);
        let hashmap_cap = (*cur.get_mut()).mask + 1;
        let mut alist_cur = arr_cur.behind::<AList>(hashmap_cap);
        for _ in 0..(*cur.get_mut()).mask + 1 {
            let arr_ptr = arr_cur.get_mut();
            if !(*arr_ptr).is_null() {
                Shifter(cur.buf).shift(&mut *arr_ptr);
                alist_cur = each(alist_cur);
            }
            arr_cur.inc();
        }
        alist_cur.behind(0)
    }
}

pub trait AssocsSuper<'a> {
    type Key: 'a;
    type Val: 'a;
    type I<'b, X: 'b + Matches<Self::Key>>: UnsafeIterator<Item = &'a Self::Val> where 'a: 'b;
}

trait Assocs<'a>: AssocsSuper<'a> {
    unsafe fn iter_matches<'c, 'b, X: Matches<Self::Key>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c;
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
    (mut cur: BuildCursor<Self>, each: F) -> BuildCursor<After>
    {
        loop {
            let alist = &mut *cur.get_mut();
            cur = each(cur.behind(1));
            if alist.next.is_null() { return cur.behind(0); }
            Shifter(cur.buf).shift(&mut alist.next);
        }
    }
}

#[repr(C)]
pub struct AssocList<'a, KV>(List<'a, KV>);

impl<'a, KV> AssocList<'a, KV> {
    pub unsafe fn deserialize
    <
        F: Fn(BuildCursor<KV>) -> BuildCursor<Self>,
        After,
    >
    (cur: BuildCursor<Self>, each: F) -> BuildCursor<After> {
        <List<'a, KV>>::deserialize(cur.behind(0), |item_cur| {
            each(item_cur).behind(0)
        })
    }
}

pub trait Assoc<'a> {
    type Key: 'a;
    type Val: 'a;

    unsafe fn key(&self) -> &'a Self::Key;
    unsafe fn val(&self) -> &'a Self::Val;
}

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
    (cur: BuildCursor<Self>, each_k: FK, each_v: FV) -> BuildCursor<After>
    {
        each_k(&mut (*cur.get_mut()).key);
        each_v(cur.behind(1))
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
    type Item = &'a KV::Val;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        while let Some(key_val) = self.cur.next() {
            if self.x.matches(&key_val.key()) { return Some(key_val.val()); }
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
    unsafe fn iter_matches<'c, 'b, X: Matches<KV::Key>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c
    { AssocListIter { x: key, cur: &self.0 } }
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
    pub unsafe fn iter(&self) -> BlobVecIter<'a, X> {
        let cur = get_behind_struct::<_, X>(self);
        BlobVecIter {
            cur,
            end: cur.add(self.len),
            _phantom: PhantomData,
        }
    }

    pub unsafe fn get(&self, ix: usize) -> &X {
        assert!(ix < self.len);
        &*get_behind_struct::<_, X>(self).add(ix)
    }

    pub unsafe fn deserialize<F: Fn(&mut X), After>
        (cur: BuildCursor<Self>, each: F) -> BuildCursor<After>
    {
        let mut xcur = cur.behind(1);
        for _ in 0..(*cur.get_mut()).len { each(&mut *xcur.get_mut()); xcur.inc(); }
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
struct AssocVecmap<'a, K, V> {
    keys: BlobVec<'a, (K, *const V)>,
    _phantom: PhantomData<&'a V>,
}

impl<'a, K, V> AssocVecmap<'a, K, V> {
    pub unsafe fn deserialize<
        After,
        FK: Fn(&mut K),
        FV: Fn(BuildCursor<V>) -> BuildCursor<V>,
    >
    (cur: BuildCursor<Self>, each_k: FK, each_v: FV) -> BuildCursor<After>
    {
        let kcur = cur.behind::<BlobVec<'a, (K, *const V)>>(0);
        let len = (*kcur.get_mut()).len;
        let mut vcur = BlobVec::deserialize(kcur, |kv| { each_k(&mut kv.0); });
        for _ in 0..len { vcur = each_v(vcur); }
        vcur.behind(0)
    }
}

struct AssocVecmapIter<'a, 'b, X, K, V> {
    x: &'b X,
    vec_iter: BlobVecIter<'a, (K, *const V)>,
    _phantom: PhantomData<&'a K>,
}

impl<'a, 'b, X: Matches<K>, K, V: 'b> UnsafeIterator for AssocVecmapIter<'a, 'b, X, K, V> {
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

impl<'a, K: 'a, V: 'a> AssocsSuper<'a> for AssocVecmap<'a, K, V> {
    type Key = K;
    type Val = V;
    type I<'b, X: 'b + Matches<K>> = AssocVecmapIter<'a, 'b, X, K, V> where 'a: 'b;
}

impl<'a, K: 'a, V: 'a> Assocs<'a> for AssocVecmap<'a, K, V> {
    unsafe fn iter_matches<'c, 'b, X: Matches<K>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c
    {
        AssocVecmapIter {
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
type U8PatternTrans<'a> = AssocVecmap<'a, Guard, U8States<'a>>;

#[repr(C)]
pub struct U8State<'a> {
    tags: *const U8Tags<'a>,
    explicit_trans: *const U8ExplicitTrans<'a>,
    pattern_trans: U8PatternTrans<'a>,
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
}

pub struct U8StateIterator<'a, 'b> {
    pattern_iter: AssocVecmapIter<'a, 'b, u8, Guard, U8States<'a>>,
    states_iter: Option<BlobVecIter<'a, *const U8State<'a>>>,
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

pub struct U8StatePrepared {
    tags: Vec<usize>,
    pattern_trans: Vec<(Guard, Vec<usize>)>,
    explicit_trans: Vec<Vec<(u8, Vec<usize>)>>,  // has size of 2**hashmap_cap
}


// impl U8StatePrepared {
//     pub fn reserve(&self, sz: &mut Reserve) -> usize {
//         sz.add::<U8State>(0);
//         let result = sz.0;
//         sz.add::<U8State>(1);
//         sz.add::<U8PatternItem>(self.pattern_trans.len());
//         for (_, targets) in self.pattern_trans.iter() {
//             sz.add::<U8States>(1);
//             sz.add::<*const U8State>(targets.len());
//         }
//         sz.add::<U8ExplicitTrans>(1);
//         sz.add::<*const U8AList>(self.explicit_trans.len());
//         for alist in self.explicit_trans.iter() {
//             for (_, targets) in alist.iter() {
//                 sz.add::<U8AList>(1);
//                 sz.add::<U8AItem>(1);
//                 sz.add::<U8States>(1);
//                 sz.add::<*const U8State>(targets.len());
//             }
//         }
//         result
//     }
// 
//     pub unsafe fn serialize(&self, buf: *mut u8, ix: usize, ptrs: &Vec<usize>) {
//         let state_cur = BuildCursor::<U8State> { cur: ptrs[ix], buf, _phantom: PhantomData };
//         assert!(state_cur.cur == align_up(state_cur.cur, align_of::<U8State>()));
// 
//         let exp_cur = {
//             let mut item_cur = state_cur.behind::<U8PatternItem>(1);
//             let mut qs_cur = item_cur.behind::<U8States>(self.pattern_trans.len());
//             for (guard, qs) in self.pattern_trans.iter() {
//                 *item_cur.get_mut() = (*guard, qs_cur.cur as *mut U8States);
//                 *qs_cur.get_mut() = BlobVec { len: qs.len(), _phantom: PhantomData };
//                 let mut qcur = qs_cur.behind::<*const U8State>(1);
//                 for &q in qs {
//                     *qcur.get_mut() = ptrs[q] as *const U8State;
//                     qcur.inc();
//                 }
//                 qs_cur = qcur.behind(0);
//                 item_cur.inc();
//             }
//             qs_cur.behind::<U8ExplicitTrans>(0)
//         };
//         *exp_cur.get_mut() = BlobHashMap
//             { mask: self.explicit_trans.len() - 1, _phantom: PhantomData };
//         let tags_cur = {
//             let mut arr_cur = exp_cur.behind::<*const U8AList>(1);
//             let mut alist_cur = arr_cur.behind::<U8AList>(self.explicit_trans.len());
//             for arritem_trans in self.explicit_trans.iter() {
//                 if arritem_trans.is_empty() {
//                     *arr_cur.get_mut() = ptr::null();
//                 } else {
//                     *arr_cur.get_mut() = alist_cur.cur as *const U8AList;
// 
//                     for (i, (guard, qs)) in arritem_trans.iter().enumerate() {
//                         let gqs_cur = alist_cur.behind::<U8AItem>(1);
//                         (*gqs_cur.get_mut()).0 = *guard;
//                         let qs_cur = gqs_cur.behind::<U8States>(1);
//                         *qs_cur.get_mut() = BlobVec { len: qs.len(), _phantom: PhantomData };
// 
//                         let mut qcur = qs_cur.behind::<*const U8State>(1);
//                         for &q in qs {
//                             *qcur.get_mut() = ptrs[q] as *const U8State;
//                             qcur.inc();
//                         }
// 
//                         let alist_ptr = alist_cur.get_mut();
//                         alist_cur = qcur.behind(0);
//                         let next =
//                             if i == arritem_trans.len() - 1 { ptr::null() }
//                             else { alist_cur.cur as *const U8List };
//                         *alist_ptr = U8AList { list: U8List { next, _phantom: PhantomData } };
//                     }
//                 }
//                 arr_cur.inc()
//             }
//             alist_cur
//         };
// 
//         let tag_ptr =
//             if self.tags.is_empty() { ptr::null() }
//             else {
//                 let tags_cur = tags_cur.behind::<U8Tags>(0);
//                 *tags_cur.get_mut() = BlobVec { len: self.tags.len(), _phantom: PhantomData };
//                 let mut tag_cur = tags_cur.behind::<usize>(1);
//                 for &tag in self.tags.iter() {
//                     *tag_cur.get_mut() = tag;
//                     tag_cur.inc();
//                 }
//                 tags_cur.cur as *const U8Tags
//             };
// 
//         *state_cur.get_mut() = U8State {
//             tags: tag_ptr,
//             explicit_trans: exp_cur.cur as *const U8ExplicitTrans,
//             pattern_trans: AssocVecmap {
//                 keys: BlobVec { len: self.pattern_trans.len(), _phantom: PhantomData },
//                 _phantom: PhantomData,
//             }
//         }
//     }
// }


mod build {
    use hashbrown::HashMap;
    use super::*;

    pub trait U8BuildConfig {
        fn guard_size_keep(&self) -> u32;
        fn hashmap_cap_fn(&self, len: usize) -> usize;
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
