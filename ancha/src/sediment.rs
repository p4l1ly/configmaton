//! Packed array of variable-sized elements in the Ancha system.
//!
//! `AnchaSediment` stores elements sequentially without gaps, where each element
//! can have a different size. This is useful for storing structures that
//! internally have variable-sized components (like AnchaVec or Lists).
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
//! Unlike `AnchaVec` where all elements have the same size, `AnchaSediment` allows
//! heterogeneous sizes. This is critical for structures like state arrays
//! where each state may have different internal sizes.

use std::marker::PhantomData;

use super::{Anchize, BuildCursor, Deanchize, Reserve};

/// A packed array of variable-sized elements.
///
/// Each element is stored immediately after the previous one, with proper
/// alignment. The size of each element may vary.
#[repr(C)]
pub struct AnchaSediment<'a, X> {
    /// Number of elements in the array.
    pub len: usize,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> AnchaSediment<'a, X> {
    /// Iterate over elements with a callback that returns a pointer to the next element.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - The callback returns a valid pointer to the next element
    /// - The structure has been properly anchized/deanchized
    pub unsafe fn each<F: FnMut(&X) -> *const X>(&self, mut f: F) {
        let mut cur = (self as *const Self).add(1) as *const X;
        for _ in 0..self.len {
            cur = f(&*cur);
        }
    }
}

// ============================================================================
// Anchization Strategy
// ============================================================================

/// Strategy for anchizing a `Vec<Origin>` into `AnchaSediment<Ancha>`.
///
/// Each element can have a variable size in the serialized form.
#[derive(Clone, Copy)]
pub struct SedimentAnchizeFromVec<'a, ElemAnchize> {
    pub elem_ancha: ElemAnchize,
    _phantom: PhantomData<&'a ElemAnchize>,
}

impl<'a, ElemAnchize> SedimentAnchizeFromVec<'a, ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        SedimentAnchizeFromVec { elem_ancha, _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize: Default> Default for SedimentAnchizeFromVec<'a, ElemAnchize> {
    fn default() -> Self {
        SedimentAnchizeFromVec { elem_ancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize> Anchize<'a> for SedimentAnchizeFromVec<'a, ElemAnchize>
where
    ElemAnchize: Anchize<'a>,
{
    type Origin = Vec<ElemAnchize::Origin>;
    type Ancha = AnchaSediment<'a, ElemAnchize::Ancha>;
    type Context = ElemAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0); // Alignment at the beginning!
        sz.add::<Self::Ancha>(1);
        for elem_origin in origin.iter() {
            self.elem_ancha.reserve(elem_origin, context, sz);
        }
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align(); // Alignment at the beginning!
        (*cur.get_mut()).len = origin.len();
        let mut xcur: BuildCursor<ElemAnchize::Ancha> = cur.behind(1);

        for elem_origin in origin.iter() {
            xcur = self.elem_ancha.anchize(elem_origin, context, xcur);
        }
        xcur.transmute()
    }
}

// ============================================================================
// Deanchization Strategy
// ============================================================================

/// Strategy for deanchizing an `AnchaSediment`.
///
/// Fixes up pointers in variable-sized elements.
#[derive(Clone, Copy)]
pub struct SedimentDeanchize<'a, ElemDeanchize> {
    pub elem_deancha: ElemDeanchize,
    _phantom: PhantomData<&'a ElemDeanchize>,
}

impl<'a, ElemDeanchize> SedimentDeanchize<'a, ElemDeanchize> {
    pub fn new(elem_deancha: ElemDeanchize) -> Self {
        SedimentDeanchize { elem_deancha, _phantom: PhantomData }
    }
}

impl<'a, ElemDeanchize: Default> Default for SedimentDeanchize<'a, ElemDeanchize> {
    fn default() -> Self {
        SedimentDeanchize { elem_deancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, ElemDeanchize> Deanchize<'a> for SedimentDeanchize<'a, ElemDeanchize>
where
    ElemDeanchize: Deanchize<'a>,
{
    type Ancha = AnchaSediment<'a, ElemDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align(); // Alignment at the beginning!
        let len = (*cur.get_mut()).len;
        let mut xcur: BuildCursor<ElemDeanchize::Ancha> = cur.behind(1);
        for _ in 0..len {
            xcur = self.elem_deancha.deanchize(xcur);
        }
        xcur.transmute()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{vec::*, CopyAnchize, NoopDeanchize};

    #[test]
    fn test_sediment_of_vecs() {
        // Create a sediment of variable-sized vectors
        let origin = vec![vec![1u8, 2], vec![3, 4, 5, 6], vec![7]];

        let anchize: SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            SedimentAnchizeFromVec::default();
        let deanchize: SedimentDeanchize<VecDeanchize<NoopDeanchize<u8>>> =
            SedimentDeanchize::default();

        // Reserve phase
        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        // Allocate buffer
        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        // Anchize and deanchize
        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        // Verify structure
        let sediment = unsafe { &*(buf.as_ptr() as *const AnchaSediment<AnchaVec<u8>>) };
        assert_eq!(sediment.len, 3);

        // Verify each vector
        let mut idx = 0;
        unsafe {
            sediment.each(|vec| {
                let expected = &origin[idx];
                assert_eq!(vec.len, expected.len());
                assert_eq!(vec.as_ref(), expected.as_slice());
                idx += 1;
                vec.behind::<AnchaVec<u8>>() as *const AnchaVec<u8>
            });
        }
        assert_eq!(idx, 3);
    }

    #[test]
    fn test_sediment_empty() {
        let origin: Vec<Vec<u8>> = vec![];

        let anchize: SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            SedimentAnchizeFromVec::default();
        let deanchize: SedimentDeanchize<VecDeanchize<NoopDeanchize<u8>>> =
            SedimentDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let sediment = unsafe { &*(buf.as_ptr() as *const AnchaSediment<AnchaVec<u8>>) };
        assert_eq!(sediment.len, 0);
    }

    #[test]
    fn test_sediment_single_element() {
        let origin = vec![vec![42u8, 43, 44]];

        let anchize: SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            SedimentAnchizeFromVec::default();
        let deanchize: SedimentDeanchize<VecDeanchize<NoopDeanchize<u8>>> =
            SedimentDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let sediment = unsafe { &*(buf.as_ptr() as *const AnchaSediment<AnchaVec<u8>>) };
        assert_eq!(sediment.len, 1);

        unsafe {
            sediment.each(|vec| {
                assert_eq!(vec.as_ref(), &[42u8, 43, 44]);
                vec as *const AnchaVec<u8>
            });
        }
    }
}
