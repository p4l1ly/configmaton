//! AnchaSediment: Packed array of variable-sized elements.
//!
//! Unlike AnchaVec where all elements have the same size, AnchaSediment
//! allows heterogeneous sizes. Each element is stored sequentially.

use super::{get_behind_struct, Anchize, BuildCursor, Deanchize, Reserve};
use std::marker::PhantomData;

/// AnchaSediment: a packed array where each element can have different size.
///
/// Layout: `[len: usize][elem[0]][elem[1]]...`
///
/// Elements are stored sequentially with proper alignment between them.
#[repr(C)]
pub struct AnchaSediment<'a, X> {
    /// Number of elements
    pub len: usize,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> AnchaSediment<'a, X> {
    /// Iterate over elements.
    ///
    /// The closure receives each element and must return a pointer to the next one.
    ///
    /// # Safety
    ///
    /// - The AnchaSediment must be properly initialized
    /// - The closure must return valid pointers
    pub unsafe fn each<F: FnMut(&X) -> *const X>(&self, mut f: F) {
        let mut cur = get_behind_struct::<_, X>(self);
        for _ in 0..self.len {
            cur = f(&*cur);
        }
    }
}

// ============================================================================
// Composable anchization for AnchaSediment
// ============================================================================

/// Anchization strategy for AnchaSediment with customizable element anchization.
///
/// Unlike VecAncha which uses StaticAnchize, SedimentAncha uses the full Anchize
/// trait because elements are variable-size and return the next cursor position.
///
/// # Example
///
/// ```ignore
/// // Sediment of vectors with different sizes
/// let elem_ancha = VecAncha::new(DirectCopy::<u8>::new());
/// let sediment_ancha = SedimentAncha::new(elem_ancha);
///
/// let origin = vec![b"hello".to_vec(), b"world".to_vec()];
/// sediment_ancha.anchize(&origin, cur);
/// ```
pub struct SedimentAncha<ElemAnchize> {
    pub elem_ancha: ElemAnchize,
}

impl<ElemAnchize> SedimentAncha<ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        SedimentAncha { elem_ancha }
    }
}

impl<ElemAnchize> Anchize for SedimentAncha<ElemAnchize>
where
    ElemAnchize: Anchize + 'static,
{
    type Origin = Vec<ElemAnchize::Origin>;
    type Ancha<'a>
        = AnchaSediment<'a, ElemAnchize::Ancha<'a>>
    where
        ElemAnchize::Ancha<'a>: 'a;

    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<AnchaSediment<ElemAnchize::Ancha<'static>>>(0);
        let addr = sz.0;
        sz.add::<AnchaSediment<ElemAnchize::Ancha<'static>>>(1);
        for elem_origin in origin.iter() {
            // Each element reserves its own space
            self.elem_ancha.reserve(elem_origin, sz);
        }
        sz.add::<ElemAnchize::Ancha<'static>>(0); // Align
        addr
    }

    unsafe fn anchize<'a, After>(
        &self,
        origin: &Self::Origin,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).len = origin.len();
        let mut xcur: BuildCursor<ElemAnchize::Ancha<'a>> = cur.behind(1);

        for elem_origin in origin.iter() {
            // anchize returns cursor for next element (same type)
            xcur = self.elem_ancha.anchize::<ElemAnchize::Ancha<'a>>(elem_origin, xcur);
        }

        xcur.align()
    }
}

impl<ElemAnchize> Deanchize for SedimentAncha<ElemAnchize>
where
    ElemAnchize: Deanchize + 'static,
{
    type Ancha<'a>
        = AnchaSediment<'a, ElemAnchize::Ancha<'a>>
    where
        ElemAnchize::Ancha<'a>: 'a;

    unsafe fn deanchize<'a, After>(
        &self,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        let len = (*cur.get_mut()).len;
        let mut xcur: BuildCursor<ElemAnchize::Ancha<'a>> = cur.behind(1);

        for _ in 0..len {
            // deanchize returns cursor for next element (same type)
            xcur = self.elem_ancha.deanchize::<ElemAnchize::Ancha<'a>>(xcur);
        }

        xcur.align()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec::{AnchaVec, VecAncha};
    use crate::DirectCopy;

    #[test]
    fn test_sediment_with_vectors() {
        // Sediment of variable-size vectors
        let elem_ancha = VecAncha::new(DirectCopy::<u8>::new());
        let sediment_ancha = SedimentAncha::new(elem_ancha);

        let origin = vec![b"hello".to_vec(), b"world".to_vec(), b"!".to_vec()];

        let mut sz = Reserve(0);
        sediment_ancha.reserve(&origin, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            sediment_ancha.anchize::<()>(&origin, cur.clone());
            sediment_ancha.deanchize::<()>(cur);
        }

        let sediment = unsafe { &*(buf.as_ptr() as *const AnchaSediment<AnchaVec<u8>>) };
        let mut results = vec![];
        unsafe {
            sediment.each(|vec| {
                results.push(vec.as_ref().to_vec());
                vec.behind()
            });
        }

        assert_eq!(results[0], b"hello");
        assert_eq!(results[1], b"world");
        assert_eq!(results[2], b"!");
    }
}
