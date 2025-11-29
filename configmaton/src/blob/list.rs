//! Intrusive linked list with inline node storage.
//!
//! `List` stores elements as a linked list where each node contains:
//! - A pointer to the next node
//! - The value inline
//!
//! # Memory Layout
//!
//! ```text
//! ┌──────┬───────┐    ┌──────┬───────┐    ┌──────┬───────┐
//! │ next │ value │───▶│ next │ value │───▶│ null │ value │
//! └──────┴───────┘    └──────┴───────┘    └──────┴───────┘
//! ```
//!
//! This is useful when:
//! - The number of elements is small
//! - Elements have different sizes (via trait objects or enums)
//! - Order matters but random access is not needed

use std::marker::PhantomData;

use super::{Build, BuildCursor, Reserve, Shifter, UnsafeIterator};

/// An intrusive linked list node.
///
/// Each node contains a value inline and a pointer to the next node.
/// The last node has a null `next` pointer.
#[repr(C)]
pub struct List<'a, X> {
    /// Pointer to the next list node, or null if this is the last node.
    pub next: *const Self,

    /// The value stored in this node.
    value: X,

    _phantom: PhantomData<&'a ()>,
}

impl<'a, X: 'a> UnsafeIterator for *const List<'a, X> {
    type Item = &'a X;
    unsafe fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.is_null() {
                return None;
            }
            let item = *self;
            *self = (*item).next;
            return Some(&(*item).value);
        }
    }
}

impl<'a, X> List<'a, X> {
    pub unsafe fn deserialize<F: FnMut(BuildCursor<X>) -> BuildCursor<Self>, After>(
        mut cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        loop {
            let alist = &mut *cur.get_mut();
            cur = f(cur.transmute::<*const Self>().behind(1));
            if alist.next.is_null() {
                return cur.align();
            }
            Shifter(cur.buf).shift(&mut alist.next);
        }
    }
}

impl<'a, X: Build> Build for List<'a, X> {
    type Origin = Vec<X::Origin>;
}

impl<'a, X: Build> List<'a, X> {
    pub fn reserve<F: FnMut(&X::Origin, &mut Reserve)>(
        origin: &<Self as Build>::Origin,
        sz: &mut Reserve,
        mut f: F,
    ) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        for x in origin.iter() {
            sz.add::<*const Self>(1);
            f(x, sz);
        }
        sz.add::<Self>(0);
        my_addr
    }

    pub unsafe fn serialize<After, F: FnMut(&X::Origin, BuildCursor<X>) -> BuildCursor<Self>>(
        origin: &<Self as Build>::Origin,
        mut cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        for (i, x) in origin.iter().enumerate() {
            if i == origin.len() - 1 {
                (*cur.get_mut()).next = std::ptr::null();
                cur = f(x, cur.transmute::<*const Self>().behind(1));
            } else {
                let next = &mut (*cur.get_mut()).next;
                cur = f(x, cur.transmute::<*const Self>().behind(1));
                *next = cur.cur as *const Self;
            }
        }
        cur.align()
    }
}
