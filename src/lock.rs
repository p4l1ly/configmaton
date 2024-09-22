use std::cell::{RefCell, Ref, RefMut};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard};

pub trait LockSuper {
    type Guard<'a, X: 'a>: Deref<Target = X>;
    type GuardMut<'a, X: 'a>: DerefMut<Target = X>;
}

pub trait Lock<T>: LockSuper + Clone + From<T> {
    fn borrow(&self) -> Self::Guard<'_, T>;
    fn borrow_mut(&self) -> Self::GuardMut<'_, T>;
}

pub trait LockSelector {
    type Lock<T>: Lock<T>;
}

pub struct RcRefCell<T>(std::rc::Rc<RefCell<T>>);

pub struct RcRefCellSelector {}
impl LockSelector for RcRefCellSelector {
    type Lock<T> = RcRefCell<T>;
}

impl<T> Clone for RcRefCell<T> {
    fn clone(&self) -> Self {
        RcRefCell(self.0.clone())
    }
}

impl<T> From<T> for RcRefCell<T> {
    fn from(x: T) -> Self {
        RcRefCell(std::rc::Rc::new(RefCell::new(x)))
    }
}

impl<T> LockSuper for RcRefCell<T> {
    type Guard<'a, X: 'a> = Ref<'a, X>;
    type GuardMut<'a, X: 'a> = RefMut<'a, X>;
}

impl<T> Lock<T> for RcRefCell<T> {
    fn borrow(&self) -> Ref<'_, T> {
        self.0.borrow()
    }

    fn borrow_mut(&self) -> RefMut<'_, T> {
        self.0.borrow_mut()
    }
}

pub struct ArcMutex<T>(Arc<Mutex<T>>);

pub struct ArcMutexSelector {}
impl LockSelector for ArcMutexSelector {
    type Lock<T> = ArcMutex<T>;
}

impl<T> From<T> for ArcMutex<T> {
    fn from(x: T) -> Self {
        ArcMutex(Arc::new(Mutex::new(x)))
    }
}

impl<T> Clone for ArcMutex<T> {
    fn clone(&self) -> Self {
        ArcMutex(self.0.clone())
    }
}

impl<T> LockSuper for ArcMutex<T> {
    type Guard<'a, X: 'a> = MutexGuard<'a, X>;
    type GuardMut<'a, X: 'a> = MutexGuard<'a, X>;
}

impl<T> Lock<T> for ArcMutex<T> {
    fn borrow(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }

    fn borrow_mut(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}


// This is unsafe and we sould allow it only with special compilation flag which marks caller
// methods as unsafe.
pub struct RawPtr<T>(*mut T);

pub struct RawPtrSelector {}
impl LockSelector for RawPtrSelector {
    type Lock<T> = RawPtr<T>;
}

impl<T> Clone for RawPtr<T> {
    fn clone(&self) -> Self {
        RawPtr(self.0)
    }
}

impl<T> From<T> for RawPtr<T> {
    fn from(x: T) -> Self {
        RawPtr(Box::into_raw(Box::new(x)))
    }
}

impl<T> LockSuper for RawPtr<T> {
    type Guard<'a, X: 'a> = &'a X;
    type GuardMut<'a, X: 'a> = &'a mut X;
}

impl<T> Lock<T> for RawPtr<T> {
    fn borrow(&self) -> &T {
        unsafe { &*self.0 }
    }

    fn borrow_mut(&self) -> &mut T {
        unsafe { &mut *self.0 }
    }
}
