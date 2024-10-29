use std::{fmt::Debug, hash::Hash};

pub struct Lock<'a, T> (pub &'a T);

impl<'a, T> Clone for Lock<'a, T> {
    fn clone(&self) -> Self {
        Lock(self.0)
    }
}

impl<'a, T> Hash for Lock<'a, T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(self.0 as *const T, state);
    }
}

impl<'a, T> PartialEq for Lock<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0 as *const T, other.0 as *const T)
    }
}

impl<'a, T> Eq for Lock<'a, T> { }

impl<'a, T> Debug for Lock<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lock({:?})", self.0 as *const T)
    }
}
