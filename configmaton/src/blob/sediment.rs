//! Packed array of variable-sized elements.
//!
//! `Sediment` stores elements sequentially without gaps, where each element
//! can have a different size. This is useful for storing structures that
//! internally have variable-sized components (like BlobVec or Lists).
//!
//! # Memory Layout
//!
//! ```text
//! ┌─────┬───────────┬───────────────┬─────────┐
//! │ len │  elem[0]  │   elem[1]     │ elem[2] │
//! └─────┴───────────┴───────────────┴─────────┘
//!        ← var size → ← var size →   ← var size →
//! ```
//!
//! Unlike `BlobVec` where all elements have the same size, `Sediment` allows
//! heterogeneous sizes. This is critical for structures like `U8State` arrays
//! where each state may have different internal sizes.
//!
//! # Iteration
//!
//! Elements must be accessed sequentially via the `each()` method, which
//! takes a closure that returns a pointer to the next element.

use std::marker::PhantomData;

use super::{get_behind_struct, Build, BuildCursor, Reserve};

/// A packed array of variable-sized elements.
///
/// Each element is stored immediately after the previous one, with proper
/// alignment. The size of each element may vary.
#[repr(C)]
pub struct Sediment<'a, X> {
    /// Number of elements in the array.
    pub len: usize,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X: Build> Build for Sediment<'a, X> {
    type Origin = Vec<X::Origin>;
}

impl<'a, X> Sediment<'a, X> {
    pub unsafe fn each<F: FnMut(&X) -> *const X>(&self, mut f: F) {
        let mut cur = get_behind_struct::<_, X>(self);
        for _ in 0..self.len {
            cur = f(&*cur);
        }
    }

    pub unsafe fn deserialize<F: FnMut(BuildCursor<X>) -> BuildCursor<X>, After>(
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        let mut xcur = cur.behind(1);
        for _ in 0..(*cur.get_mut()).len {
            xcur = f(xcur);
        }
        xcur.align()
    }
}

impl<'a, X: Build> Sediment<'a, X> {
    pub fn reserve<F: FnMut(&X::Origin, &mut Reserve)>(
        origin: &<Self as Build>::Origin,
        sz: &mut Reserve,
        mut f: F,
    ) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        for x in origin.iter() {
            f(x, sz);
        }
        sz.add::<X>(0);
        my_addr
    }

    pub unsafe fn serialize<F: FnMut(&X::Origin, BuildCursor<X>) -> BuildCursor<X>, After>(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).len = origin.len();
        let mut xcur = cur.behind(1);
        for x in origin.iter() {
            xcur = f(x, xcur);
        }
        xcur.align()
    }
}
