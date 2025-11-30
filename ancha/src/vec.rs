//! AnchaVec: Dynamic array in blob format.
//!
//! Similar to `Vec<T>` but stored in a contiguous blob with inline elements.

use super::{Anchize, BuildCursor, Deanchize, Reserve, StaticAnchize, StaticDeanchize};
use std::marker::PhantomData;

/// AnchaVec: a vector stored in blob format.
///
/// Layout: `[len: usize][elements...]`
#[repr(C)]
pub struct AnchaVec<'a, X> {
    /// Number of elements
    pub len: usize,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> AnchaVec<'a, X> {
    /// Get the elements as a slice.
    ///
    /// # Safety
    ///
    /// - The AnchaVec must be properly initialized
    /// - Elements must be valid
    pub unsafe fn as_ref(&self) -> &'a [X] {
        std::slice::from_raw_parts(super::get_behind_struct::<Self, X>(self), self.len)
    }

    /// Get an element by index.
    ///
    /// # Panics
    ///
    /// Panics if index >= len.
    ///
    /// # Safety
    ///
    /// - The AnchaVec must be properly initialized
    pub unsafe fn get(&self, ix: usize) -> &'a X {
        assert!(ix < self.len);
        &*super::get_behind_struct::<Self, X>(self).add(ix)
    }

    /// Get a reference to data that follows this AnchaVec in memory.
    ///
    /// This is used when multiple structures are stored sequentially
    /// (e.g., in `Tupellum`).
    ///
    /// # Safety
    ///
    /// - The AnchaVec must be properly initialized
    /// - There must be valid data of type `After` following the elements
    pub unsafe fn behind<After>(&self) -> &'a After {
        let elem_ptr = (self as *const Self).add(1) as *const X;
        let after_elems = elem_ptr.add(self.len) as *const u8;
        let aligned = super::align_up(after_elems as usize, std::mem::align_of::<After>());
        &*(aligned as *const After)
    }
}

// ============================================================================
// Composable anchization for AnchaVec
// ============================================================================

/// Anchization strategy for AnchaVec with customizable element anchization.
///
/// This is the key to composability: you can plug in ANY element anchization strategy!
#[derive(Clone, Copy)]
pub struct VecAnchizeFromVec<'a, ElemAnchize> {
    pub elem_ancha: ElemAnchize,
    _phantom: PhantomData<&'a ElemAnchize>,
}

impl<'a, ElemAnchize> VecAnchizeFromVec<'a, ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        VecAnchizeFromVec { elem_ancha, _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize: Default> Default for VecAnchizeFromVec<'a, ElemAnchize> {
    fn default() -> Self {
        VecAnchizeFromVec { elem_ancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize> Anchize<'a> for VecAnchizeFromVec<'a, ElemAnchize>
where
    ElemAnchize: StaticAnchize<'a>,
{
    type Origin = Vec<ElemAnchize::Origin>;
    type Ancha = AnchaVec<'a, ElemAnchize::Ancha>;
    type Context = ElemAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, _context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0);
        sz.add::<Self::Ancha>(1);
        sz.add::<ElemAnchize::Ancha>(origin.len());
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align();
        (*cur.get_mut()).len = origin.len();
        let mut xcur: BuildCursor<ElemAnchize::Ancha> = cur.behind(1);

        for elem_origin in origin.iter() {
            // Use the element anchization strategy!
            self.elem_ancha.anchize_static(elem_origin, context, &mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.transmute()
    }
}

#[derive(Clone, Copy)]
pub struct VecDeanchize<'a, ElemDeanchize> {
    pub elem_deancha: ElemDeanchize,
    _phantom: PhantomData<&'a ElemDeanchize>,
}

impl<'a, ElemDeanchize> VecDeanchize<'a, ElemDeanchize> {
    pub fn new(elem_deancha: ElemDeanchize) -> Self {
        VecDeanchize { elem_deancha, _phantom: PhantomData }
    }
}

impl<'a, ElemDeanchize: Default> Default for VecDeanchize<'a, ElemDeanchize> {
    fn default() -> Self {
        VecDeanchize { elem_deancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, ElemDeanchize> Deanchize<'a> for VecDeanchize<'a, ElemDeanchize>
where
    ElemDeanchize: StaticDeanchize<'a>,
{
    type Ancha = AnchaVec<'a, ElemDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align();
        let len = (*cur.get_mut()).len;
        let mut xcur: BuildCursor<ElemDeanchize::Ancha> = cur.behind(1);
        for _ in 0..len {
            self.elem_deancha.deanchize_static(&mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.transmute()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CopyAnchize, NoopDeanchize};

    #[test]
    fn test_anchavec_basic() {
        let anchize: VecAnchizeFromVec<CopyAnchize<u8, ()>> = VecAnchizeFromVec::default();
        let deanchize: VecDeanchize<NoopDeanchize<u8>> = VecDeanchize::default();
        let origin = vec![1u8, 2, 3];

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let anchavec = unsafe { &*(buf.as_ptr() as *const AnchaVec<u8>) };
        assert_eq!(unsafe { anchavec.as_ref() }, &[1, 2, 3]);
    }
}

// ============================================================================
// Iterator
// ============================================================================

/// Iterator over elements in an AnchaVec.
pub struct AnchaVecIter<'a, X> {
    cur: *const X,
    pub end: *const X,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> AnchaVec<'a, X> {
    /// Create an iterator over the elements.
    ///
    /// # Safety
    ///
    /// - The AnchaVec must be properly initialized
    /// - Elements must be valid
    pub unsafe fn iter(&self) -> AnchaVecIter<'a, X> {
        let cur = super::get_behind_struct::<Self, X>(self);
        AnchaVecIter { cur, end: cur.add(self.len), _phantom: PhantomData }
    }
}

impl<'a, X> super::UnsafeIterator for AnchaVecIter<'a, X> {
    type Item = &'a X;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        if self.cur < self.end {
            let result = &*self.cur;
            self.cur = self.cur.add(1);
            Some(result)
        } else {
            None
        }
    }
}
