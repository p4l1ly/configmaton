use std::{ops::{Deref, DerefMut}, sync::{RwLock, RwLockReadGuard, RwLockWriteGuard}};

use hashbrown::HashMap;
use crate::holder::Holder;

pub struct Onion<'a, L: Locker> {
    parent: Option<*const Self>,
    children: Holder<Self>,
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

impl<'a, L: Locker> Onion<'a, L>
{
    pub fn new() -> Self {
        Onion {
            parent: None,
            children: Holder::new(),
            data: L::new(HashMap::new()),
        }
    }

    pub fn make_child(&mut self) -> &mut Self {
        self.children.add(Onion {
            parent: Some(self),
            children: Holder::new(),
            data: L::new(HashMap::new()),
        })
    }

    pub fn get_rec(&self, key: &[u8]) -> Option<&'a [u8]> {
        if let Some(value) = L::read(&self.data).get(key) {
            return Some(value);
        }
        unsafe { &*(self.parent?) }.get_rec(key)
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
}
