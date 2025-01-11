use std::{ops::{Deref, DerefMut}, sync::{RwLock, RwLockReadGuard, RwLockWriteGuard}};

use hashbrown::HashMap;
use crate::holder::Holder;

pub struct Onion<'a, L: Locker, Child> {
    parent: Option<*const Self>,
    children: Holder<Child>,
    data: L::Lock<HashMap<&'a [u8], &'a [u8]>>,
}

pub trait LockerSuper {
    type Guard<'a, X: 'a>: Deref<Target = X>;
    type GuardMut<'a, X: 'a>: DerefMut<Target = X>;
}

pub trait Locker: LockerSuper {
    type Lock<T>;

    fn new<T>(x: T) -> Self::Lock<T>;
    fn read<'a, T>(lock: &'a Self::Lock<T>) -> Self::Guard<'a, T>;
    fn write<'a, T>(lock: &'a mut Self::Lock<T>) -> Self::GuardMut<'a, T>;
}

pub struct ThreadUnsafeLocker;
impl LockerSuper for ThreadUnsafeLocker {
    type Guard<'a, X: 'a> = &'a X;
    type GuardMut<'a, X: 'a> = &'a mut X;
}
impl Locker for ThreadUnsafeLocker {
    type Lock<T> = T;

    fn new<T>(x: T) -> Self::Lock<T> { x }
    fn read<'a, T>(lock: &'a Self::Lock<T>) -> Self::Guard<'a, T> { lock }
    fn write<'a, T>(lock: &'a mut Self::Lock<T>) -> Self::GuardMut<'a, T> { lock }
}

pub struct ThreadSafeLocker;
impl LockerSuper for ThreadSafeLocker {
    type Guard<'a, X: 'a> = RwLockReadGuard<'a, X>;
    type GuardMut<'a, X: 'a> = RwLockWriteGuard<'a, X>;
}
impl Locker for ThreadSafeLocker {
    type Lock<T> = RwLock<T>;

    fn new<T>(x: T) -> Self::Lock<T> { RwLock::new(x) }
    fn read<'a, T>(lock: &'a Self::Lock<T>) -> Self::Guard<'a, T> { lock.read().unwrap() }
    fn write<'a, T>(lock: &'a mut Self::Lock<T>) -> Self::GuardMut<'a, T> { lock.write().unwrap() }
}

impl<'a, L: Locker, Child> Onion<'a, L, Child>
{
    pub fn new() -> Self {
        Onion {
            parent: None,
            children: Holder::new(),
            data: L::new(HashMap::new()),
        }
    }

    // Unfortunately, I did not find a way to express that the parent outlives child but both
    // remain mutable.
    pub fn make_child<NewChild: FnOnce(Self) -> Child>
        (&mut self, new_child: NewChild) -> *mut Child
    {
        self.children.add(new_child(Onion {
            parent: Some(self),
            children: Holder::new(),
            data: L::new(HashMap::new()),
        }))
    }

    pub fn get(&self, key: &[u8]) -> Option<&'a [u8]> {
        if let Some(value) = L::read(&self.data).get(key) {
            return Some(value);
        }

        let mut parent = self.parent?;
        loop {
            let parent_onion = unsafe { &*parent };
            if let Some(value) = L::read(&parent_onion.data).get(key) {
                return Some(value);
            }
            parent = parent_onion.parent?;
        }
    }

    pub fn set(&mut self, key: &'a [u8], value: &'a [u8]) {
        L::write(&mut self.data).insert(key, value);
    }

    pub fn iter_children(&mut self) -> impl Iterator<Item = *mut Child> {
        self.children.iter_mut()
    }

    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct JustOnion<'a>(Onion<'a, ThreadUnsafeLocker, Self>);

    #[test]
    fn onion_works() {
        let mut onion1 = JustOnion(Onion::new());
        onion1.0.set(b"a", b"1");
        onion1.0.set(b"b", b"2");
        onion1.0.set(b"a", b"3");
        assert_eq!(onion1.0.get(b"a"), Some(b"3".as_ref()));
        assert_eq!(onion1.0.get(b"b"), Some(b"2".as_ref()));
        assert_eq!(onion1.0.get(b"c"), None);

        let onion2 = unsafe { &mut *onion1.0.make_child(|onion| JustOnion(onion)) };
        let onion3 = unsafe { &mut *onion1.0.make_child(|onion| JustOnion(onion)) };
        onion2.0.set(b"b", b"4");
        onion2.0.set(b"c", b"5");
        onion3.0.set(b"b", b"6");

        assert_eq!(onion1.0.get(b"a"), Some(b"3".as_ref()));
        assert_eq!(onion1.0.get(b"b"), Some(b"2".as_ref()));
        assert_eq!(onion1.0.get(b"c"), None);
        assert_eq!(onion3.0.get(b"d"), None);

        assert_eq!(onion2.0.get(b"a"), Some(b"3".as_ref()));
        assert_eq!(onion2.0.get(b"b"), Some(b"4".as_ref()));
        assert_eq!(onion2.0.get(b"c"), Some(b"5".as_ref()));
        assert_eq!(onion2.0.get(b"d"), None);

        assert_eq!(onion3.0.get(b"a"), Some(b"3".as_ref()));
        assert_eq!(onion3.0.get(b"b"), Some(b"6".as_ref()));
        assert_eq!(onion3.0.get(b"c"), None);
        assert_eq!(onion3.0.get(b"d"), None);

        onion1.0.set(b"a", b"7");
        onion1.0.set(b"b", b"8");

        assert_eq!(onion1.0.get(b"a"), Some(b"7".as_ref()));
        assert_eq!(onion1.0.get(b"b"), Some(b"8".as_ref()));
        assert_eq!(onion1.0.get(b"c"), None);
        assert_eq!(onion3.0.get(b"d"), None);

        assert_eq!(onion2.0.get(b"a"), Some(b"7".as_ref()));
        assert_eq!(onion2.0.get(b"b"), Some(b"4".as_ref()));
        assert_eq!(onion2.0.get(b"c"), Some(b"5".as_ref()));
        assert_eq!(onion2.0.get(b"d"), None);

        assert_eq!(onion3.0.get(b"a"), Some(b"7".as_ref()));
        assert_eq!(onion3.0.get(b"b"), Some(b"6".as_ref()));
        assert_eq!(onion3.0.get(b"c"), None);
        assert_eq!(onion3.0.get(b"d"), None);
    }
}
