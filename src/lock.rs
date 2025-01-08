use std::cell::{Ref, RefMut};
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard};

use crate::holder::{Holder, TimedRefCell};

pub trait LockSuper {
    type Guard<'a, X: 'a>: Deref<Target = X>;
    type GuardMut<'a, X: 'a>: DerefMut<Target = X>;
}

pub trait LockSelector: LockSuper {
    type MaybeLock<'a, T: 'a>: Copy;
    type Lock<'a, T: 'a>: Copy;
    type Holder<'a, T: 'a>;

    fn from_maybe_lock<'a, T>(lock: &Self::MaybeLock<'a, T>) -> Option<Self::Lock<'a, T>>;
    fn to_maybe_lock<'a, T>(lock: &Self::Lock<'a, T>) -> Self::MaybeLock<'a, T>;

    fn maybe_borrow<'a, T>(lock: &Self::MaybeLock<'a, T>) -> Option<Self::Guard<'a, T>>;
    fn maybe_borrow_mut<'a, T>(lock: &Self::MaybeLock<'a, T>) -> Option<Self::GuardMut<'a, T>>;
    fn borrow<'a, T>(lock: &Self::Lock<'a, T>) -> Self::Guard<'a, T>;
    fn borrow_mut<'b, 'a, T>(lock: &'b Self::Lock<'a, T>) -> Self::GuardMut<'b, T>;

    fn new_holder<'a, T>() -> Self::Holder<'a, T>;
    fn empty<'a, T>() -> Self::MaybeLock<'a, T>;
    fn add_holder<'b, 'a, T>(holder: &'b mut Self::Holder<'a, T>, value: T) -> Self::Lock<'a, T>;
}

type RefCellLock<'a, T> = Option<&'a TimedRefCell<'a, T>>;
pub struct RefCellSelector;

impl LockSuper for RefCellSelector {
    type Guard<'a, X: 'a> = Ref<'a, X>;
    type GuardMut<'a, X: 'a> = RefMut<'a, X>;
}

impl LockSelector for RefCellSelector {
    type MaybeLock<'a, T: 'a> = RefCellLock<'a, T>;
    type Lock<'a, T: 'a> = &'a TimedRefCell<'a, T>;
    type Holder<'a, T: 'a> = Holder<'a, T>;

    fn from_maybe_lock<'a, T>(lock: &Self::MaybeLock<'a, T>) -> Option<Self::Lock<'a, T>> {
        *lock
    }

    fn to_maybe_lock<'a, T>(lock: &Self::Lock<'a, T>) -> Self::MaybeLock<'a, T> {
        Some(lock)
    }

    fn maybe_borrow<'a, T>(lock: &Self::MaybeLock<'a, T>) -> Option<Self::Guard<'a, T>> {
        lock.map(|refcell| refcell.borrow())
    }

    fn maybe_borrow_mut<'a, T>(lock: &Self::MaybeLock<'a, T>) -> Option<Self::GuardMut<'a, T>> {
        lock.map(|refcell| refcell.borrow_mut())
    }

    fn borrow<'a, T>(lock: &Self::Lock<'a, T>) -> Self::Guard<'a, T> {
        lock.borrow()
    }

    fn borrow_mut<'b, 'a, T>(lock: &'b Self::Lock<'a, T>) -> Self::GuardMut<'b, T> {
        lock.borrow_mut()
    }

    fn new_holder<'a, T>() -> Self::Holder<'a, T> {
        Holder::new()
    }

    fn empty<'a, T>() -> Self::MaybeLock<'a, T> {
        None
    }

    fn add_holder<'b, 'a, T>(holder: &'b mut Self::Holder<'a, T>, value: T) -> Self::Lock<'a, T> {
        holder.add(value)
    }
}

// pub struct RcRefCell<T>(Option<Rc<RefCell<T>>>);
// 
// pub struct RcRefCellSelector {}
// impl LockSelector for RcRefCellSelector {
//     type Lock<T> = RcRefCell<T>;
//     type Holder<T> = Rc<RefCell<T>>;
// 
//     fn empty<T>() -> Self::Lock<T> {
//         RcRefCell(None)
//     }
// 
//     fn new<T>(x: T) -> Self::Holder<T> {
//         Rc::new(RefCell::new(x))
//     }
// 
//     fn refer<T>(x: &mut Self::Holder<T>) -> Self::Lock<T> {
//         RcRefCell(Some(x.clone()))
//     }
// 
//     unsafe fn refer_uninit<T>(x: &mut Self::Holder<MaybeUninit<T>>) -> Self::Lock<T> {
//         std::mem::transmute::<_, Self::Lock<T>>(x).clone()
//     }
// 
//     fn borrow_mut_holder<T>(x: &mut Self::Holder<T>) -> RefMut<'_, T> {
//         x.borrow_mut()
//     }
// }
// 
// impl<T> Clone for RcRefCell<T> {
//     fn clone(&self) -> Self {
//         RcRefCell(self.0.clone())
//     }
// }
// 
// impl<T> LockSuper for RcRefCell<T> {
//     type Guard<'a, X: 'a> = Ref<'a, X>;
//     type GuardMut<'a, X: 'a> = RefMut<'a, X>;
// }
// 
// impl<T> Lock<T> for RcRefCell<T> {
//     fn borrow(&self) -> Option<Ref<'_, T>> {
//         self.0.as_ref().map(|x| x.borrow())
//     }
// 
//     fn borrow_mut(&self) -> Option<RefMut<'_, T>> {
//         self.0.as_ref().map(|x| x.borrow_mut())
//     }
// }
// 
// pub struct ArcMutex<T>(Option<Arc<Mutex<T>>>);
// 
// pub struct ArcMutexSelector {}
// impl LockSelector for ArcMutexSelector {
//     type Lock<T> = ArcMutex<T>;
//     type Holder<T> = Arc<Mutex<T>>;
// 
//     fn empty<T>() -> Self::Lock<T> {
//         ArcMutex(None)
//     }
// 
//     fn new<T>(x: T) -> Self::Holder<T> {
//         Arc::new(Mutex::new(x))
//     }
// 
//     fn refer<T>(x: &mut Self::Holder<T>) -> Self::Lock<T> {
//         ArcMutex(Some(x.clone()))
//     }
// 
//     unsafe fn refer_uninit<T>(x: &mut Self::Holder<MaybeUninit<T>>) -> Self::Lock<T> {
//         std::mem::transmute::<_, Self::Lock<T>>(x).clone()
//     }
// 
//     fn borrow_mut_holder<T>(x: &mut Self::Holder<T>) -> MutexGuard<'_, T> {
//         x.0.lock().unwrap()
//     }
// }
// 
// impl<T> From<T> for ArcMutex<T> {
//     fn from(x: T) -> Self {
//         ArcMutex(Arc::new(Mutex::new(x)))
//     }
// }
// 
// impl<T> Clone for ArcMutex<T> {
//     fn clone(&self) -> Self {
//         ArcMutex(self.0.clone())
//     }
// }
// 
// impl<T> LockSuper for ArcMutex<T> {
//     type Guard<'a, X: 'a> = MutexGuard<'a, X>;
//     type GuardMut<'a, X: 'a> = MutexGuard<'a, X>;
// }
// 
// impl<T> std::hash::Hash for ArcMutex<T> {
//     fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
//         std::ptr::hash(&*self.0, state);
//     }
// }
// 
// impl<T> std::fmt::Debug for ArcMutex<T> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "ArcMutex({:?})", self.0.as_ref() as *const _)
//     }
// }
// 
// impl<T> PartialEq for ArcMutex<T> {
//     fn eq(&self, other: &Self) -> bool {
//         std::ptr::eq(&*self.0, &*other.0)
//     }
// }
// 
// impl<T> Eq for ArcMutex<T> { }
// 
// impl<T> Lock<T> for ArcMutex<T> {
//     fn borrow(&self) -> MutexGuard<'_, T> {
//         self.0.lock().unwrap()
//     }
// 
//     fn borrow_mut(&self) -> MutexGuard<'_, T> {
//         self.0.lock().unwrap()
//     }
// }
// 
// 
// // This is unsafe and we sould allow it only with special compilation flag which marks caller
// // methods as unsafe.
// pub struct RawPtr<T>(*mut T);
// 
// pub struct RawPtrSelector {}
// impl LockSelector for RawPtrSelector {
//     type Lock<T> = RawPtr<T>;
//     type Holder<T> = T;
// 
//     fn new<T>(x: T) -> Self::Holder<T> {
//         x
//     }
// 
//     fn refer<T>(x: &mut Self::Holder<T>) -> Self::Lock<T> {
//         RawPtr(x)
//     }
// 
//     unsafe fn refer_uninit<T>(x: &mut Self::Holder<MaybeUninit<T>>) -> Self::Lock<T> {
//         RawPtr(x.as_mut_ptr())
//     }
// 
//     fn borrow_mut_holder<T>(x: &mut T) -> &mut T {
//         x
//     }
// }
// 
// impl<T> Clone for RawPtr<T> {
//     fn clone(&self) -> Self {
//         RawPtr(self.0)
//     }
// }
// 
// impl<T> std::fmt::Debug for RawPtr<T> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "RawPtr({:?})", self.0)
//     }
// }
// 
// impl<T> std::hash::Hash for RawPtr<T> {
//     fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
//         std::ptr::hash(self.0, state);
//     }
// }
// 
// impl<T> PartialEq for RawPtr<T> {
//     fn eq(&self, other: &Self) -> bool {
//         std::ptr::eq(self.0, other.0)
//     }
// }
// 
// impl<T> Eq for RawPtr<T> { }
// 
// impl<T> LockSuper for RawPtr<T> {
//     type Guard<'a, X: 'a> = &'a X;
//     type GuardMut<'a, X: 'a> = &'a mut X;
// }
// 
// impl<T> Lock<T> for RawPtr<T> {
//     fn borrow(&self) -> &T {
//         unsafe { &*self.0 }
//     }
// 
//     fn borrow_mut(&self) -> &mut T {
//         unsafe { &mut *self.0 }
//     }
// }
