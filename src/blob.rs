use std::mem::{size_of, align_of};

use twox_hash::XxHash64;

use crate::guards::Guard;

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

#[repr(C)]
struct HashMap<'a, AList> {
    capacity: usize,
    _phantom: std::marker::PhantomData<&'a AList>,
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

unsafe fn get_behind_struct<A, B>(ptr: *const A) -> *const B {
    align_up((ptr as *const u8).add(size_of::<A>()) as usize, align_of::<B>()) as *const B
}

trait GetBehind {
    unsafe fn get_behind<B>(&self) -> *const B;
}

impl<'a, AList: AssocList<'a>> HashMap<'a, AList> {
    unsafe fn get(&self, key: &AList::Key) -> Option<&AList::Val>
        where AList::Key: Eq + MyHash
    {
        let ix = key.my_hash() & ((1 << self.capacity) - 1);
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
    _phantom: std::marker::PhantomData<&'a V>,
}

struct HomoKeyAlternAssocListIter<'a, 'b, X, K, V> {
    x: &'b X,
    cur: *const HomoKeyAlternAssocList<'a, K, V>,
    _phantom: std::marker::PhantomData<&'a K>,
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
            _phantom: std::marker::PhantomData,
        }
    }
}

#[repr(C)]
struct HomoVec<'a, X> {
    len: usize,
    _phantom: std::marker::PhantomData<&'a X>,
}

impl<'a, X> GetBehind for HomoVec<'a, X> {
    unsafe fn get_behind<B>(&self) -> *const B {
        if self.len == 0 { return get_behind_struct::<_, B>(self); }
        get_behind_struct::<_, B>(get_behind_struct::<_, X>(self).add(self.len - 1))
    }
}

struct HomoVecIter<'a, X> {
    cur: *const X,
    end: *const X,
    _phantom: std::marker::PhantomData<&'a X>,
}

impl<'a, X> HomoVec<'a, X> {
    unsafe fn iter(&self) -> HomoVecIter<'a, X> {
        let cur = get_behind_struct::<_, X>(self);
        HomoVecIter {
            cur,
            end: cur.add(self.len),
            _phantom: std::marker::PhantomData,
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
    _phantom: std::marker::PhantomData<&'a V>,
}

impl<'a, K, V: GetBehind> GetBehind for HomoKeyAssocList<'a, K, V> {
    unsafe fn get_behind<B>(&self) -> *const B {
        if self.keys.len == 0 { return get_behind_struct::<_, B>(&self.keys); }
        (*self.keys.get(self.keys.len - 1).1).get_behind::<B>()
    }
}

struct HomoKeyAssocListIter<'a, 'b, X, K, V> {
    x: &'b X,
    vec_iter: HomoVecIter<'a, (K, *const V)>,
    _phantom: std::marker::PhantomData<&'a K>,
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
            _phantom: std::marker::PhantomData,
        }
    }
}

#[repr(C)]
pub struct U8State<'a> {
    tags_ptr: *const usize,
    tags_len: usize,
    pattern_trans: HomoKeyAssocList<'a, Guard, HomoVec<'a, *const U8State<'a>>>,
}

impl<'a> U8State<'a> {
    pub unsafe fn iter_matches<'c, 'b>(&'c self, key: &'b u8) -> U8StateIterator<'a, 'b>
        where 'a: 'b + 'c
    {
        U8StateIterator {
            pattern_iter: self.pattern_trans.iter_matches(key),
            states_iter: None,
            explicits: self.pattern_trans.get_behind(),
        }
    }
}

pub struct U8StateIterator<'a, 'b> {
    pattern_iter: HomoKeyAssocListIter<'a, 'b, u8, Guard, HomoVec<'a, *const U8State<'a>>>,
    states_iter: Option<HomoVecIter<'a, *const U8State<'a>>>,
    explicits: *const HashMap<'a, HomoKeyAlternAssocList<'a, u8, HomoVec<'a, *const U8State<'a>>>>
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
                if !self.explicits.is_null() {
                    let explicits = &*self.explicits;
                    self.explicits = std::ptr::null();
                    if let Some(states) = explicits.get(self.pattern_iter.x) {
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
