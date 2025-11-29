//! Dynamic-length vector with inline storage.
//!
//! `BlobVec` is similar to `Vec` but with elements stored inline in the blob buffer.
//! The layout is:
//!
//! ```text
//! [len: usize][padding][elem0, elem1, ..., elemN][padding]
//! ```
//!
//! Elements are stored contiguously after the header, properly aligned.

use std::marker::PhantomData;

use super::{
    align_up, align_up_ptr, get_behind_struct, Build, BuildCursor, Reserve, UnsafeIterator,
};

/// A dynamic-length vector with inline element storage.
///
/// # Memory Layout
///
/// ```text
/// ┌─────────┬─────────┬─────────┬─────────┬─────────┐
/// │ len     │ padding │ elem[0] │ elem[1] │ elem[2] │
/// └─────────┴─────────┴─────────┴─────────┴─────────┘
/// ```
///
/// # Example
///
/// ```ignore
/// // Serialize a Vec<u32> to BlobVec<u32>
/// let origin = vec![1u32, 2, 3];
/// let mut sz = Reserve(0);
/// BlobVec::<u32>::reserve(&origin, &mut sz);
///
/// let mut buf = vec![0u8; sz.0];
/// let cur = BuildCursor::new(buf.as_mut_ptr());
/// unsafe {
///     BlobVec::<u32>::serialize(&origin, cur, |x, y| *y = *x);
///     BlobVec::<u32>::deserialize(BuildCursor::new(buf.as_mut_ptr()), |_| ());
/// }
///
/// let blobvec = unsafe { &*(buf.as_ptr() as *const BlobVec<u32>) };
/// assert_eq!(unsafe { blobvec.as_ref() }, &[1, 2, 3]);
/// ```
#[repr(C)]
pub struct BlobVec<'a, X> {
    /// Number of elements in the vector.
    pub(super) len: usize,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X: Build> Build for BlobVec<'a, X> {
    type Origin = Vec<X::Origin>;
}

/// Iterator over elements in a `BlobVec`.
pub struct BlobVecIter<'a, X> {
    cur: *const X,
    pub end: *const X,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> BlobVec<'a, X> {
    /// Create an iterator over the elements.
    ///
    /// # Safety
    ///
    /// The BlobVec must be properly initialized with valid elements.
    pub unsafe fn iter(&self) -> BlobVecIter<'a, X> {
        let cur = get_behind_struct::<_, X>(self);
        BlobVecIter { cur, end: cur.add(self.len), _phantom: PhantomData }
    }

    /// Get a reference to data that follows this BlobVec in memory.
    ///
    /// This is used when multiple structures are stored sequentially
    /// (e.g., in `Sediment` or `Tupellum`).
    ///
    /// # Safety
    ///
    /// - The BlobVec must be properly initialized
    /// - There must be valid data of type `After` following the elements
    /// - Proper alignment for `After` is assumed
    pub unsafe fn behind<After>(&self) -> &'a After {
        let cur = get_behind_struct::<_, X>(self);
        &*align_up_ptr(cur.add(self.len))
    }

    /// Get a reference to the element at index `ix`.
    ///
    /// # Panics
    ///
    /// Panics if `ix >= len`.
    ///
    /// # Safety
    ///
    /// The BlobVec must be properly initialized.
    pub unsafe fn get(&self, ix: usize) -> &X {
        assert!(ix < self.len);
        &*get_behind_struct::<_, X>(self).add(ix)
    }

    /// Get the elements as a slice.
    ///
    /// # Safety
    ///
    /// The BlobVec must be properly initialized with valid elements.
    pub unsafe fn as_ref(&self) -> &'a [X] {
        std::slice::from_raw_parts(get_behind_struct::<_, X>(self), self.len)
    }

    pub unsafe fn deserialize<F: FnMut(&mut X), After>(
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        let mut xcur = cur.behind(1);
        for _ in 0..(*cur.get_mut()).len {
            f(&mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.align()
    }
}

impl<'a, X: Build> BlobVec<'a, X> {
    pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        sz.add::<X>(origin.len());
        my_addr
    }

    pub fn elem_addr(my_addr: usize, ix: usize) -> usize {
        align_up(my_addr + size_of::<Self>(), align_of::<X>()) + size_of::<X>() * ix
    }

    pub unsafe fn serialize<F: FnMut(&X::Origin, &mut X), After>(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).len = origin.len();
        let mut xcur = cur.behind(1);
        for x in origin.iter() {
            f(x, &mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.align()
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
