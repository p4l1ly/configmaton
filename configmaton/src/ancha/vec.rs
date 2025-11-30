//! AnchaVec: Dynamic array in blob format.
//!
//! Similar to `Vec<T>` but stored in a contiguous blob with inline elements.

use super::{Anchize, BuildCursor, Deanchize, Reserve, StaticAnchize};
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
        let ptr = (self as *const Self).add(1) as *const X;
        std::slice::from_raw_parts(ptr, self.len)
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
        let ptr = (self as *const Self).add(1) as *const X;
        &*ptr.add(ix)
    }
}

// ============================================================================
// Composable anchization for AnchaVec
// ============================================================================

/// Anchization strategy for AnchaVec with customizable element anchization.
///
/// This is the key to composability: you can plug in ANY element anchization strategy!
///
/// # Example
///
/// ```ignore
/// // Default: direct copy
/// let default_ancha = VecAncha::new(DirectCopy::<u8>::new());
///
/// // Custom: multiply elements by 2
/// let custom_ancha = VecAncha::new(MultiplyBy2);
///
/// // Use it!
/// custom_ancha.anchize(&vec![1,2,3], cur);  // → [2,4,6]
/// ```
pub struct VecAncha<ElemAnchize> {
    pub elem_ancha: ElemAnchize,
}

impl<ElemAnchize> VecAncha<ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        VecAncha { elem_ancha }
    }
}

impl<ElemAnchize> Anchize for VecAncha<ElemAnchize>
where
    ElemAnchize: StaticAnchize,
    ElemAnchize::Ancha: Sized + 'static,
{
    type Origin = Vec<ElemAnchize::Origin>;
    type Ancha<'a> = AnchaVec<'a, ElemAnchize::Ancha>;

    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<AnchaVec<ElemAnchize::Ancha>>(0);
        let addr = sz.0;
        sz.add::<AnchaVec<ElemAnchize::Ancha>>(1);
        sz.add::<ElemAnchize::Ancha>(origin.len());
        sz.add::<ElemAnchize::Ancha>(0); // Align
        addr
    }

    unsafe fn anchize<'a, After>(
        &self,
        origin: &Self::Origin,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).len = origin.len();
        let mut xcur: BuildCursor<ElemAnchize::Ancha> = cur.behind(1);

        for elem_origin in origin.iter() {
            // Use the element anchization strategy!
            self.elem_ancha.anchize_static(elem_origin, &mut *xcur.get_mut());
            xcur.inc();
        }

        xcur.align()
    }
}

impl<ElemAnchize> Deanchize for VecAncha<ElemAnchize>
where
    ElemAnchize: StaticAnchize,
    ElemAnchize::Ancha: Sized + 'static,
{
    type Ancha<'a> = AnchaVec<'a, ElemAnchize::Ancha>;

    unsafe fn deanchize<'a, After>(
        &self,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        // For fixed-size elements, no pointer fixup needed
        let len = (*cur.get_mut()).len;
        let mut xcur: BuildCursor<ElemAnchize::Ancha> = cur.behind(1);
        for _ in 0..len {
            xcur.inc();
        }
        xcur.align()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ancha::DirectCopy;

    #[test]
    fn test_anchavec_basic() {
        let ancha = VecAncha::new(DirectCopy::<u8>::new());
        let origin = vec![1u8, 2, 3];

        let mut sz = Reserve(0);
        let addr = ancha.reserve(&origin, &mut sz);
        assert_eq!(addr, 0);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            ancha.anchize::<()>(&origin, cur.clone());
            ancha.deanchize::<()>(cur);
        }

        let anchavec = unsafe { &*(buf.as_ptr() as *const AnchaVec<u8>) };
        assert_eq!(unsafe { anchavec.as_ref() }, &[1, 2, 3]);
    }

    #[test]
    fn test_anchavec_with_custom() {
        // Custom anchization: multiply by 2
        struct MultiplyBy2;
        impl StaticAnchize for MultiplyBy2 {
            type Origin = usize;
            type Ancha = usize;
            fn anchize_static(&self, origin: &Self::Origin, ancha: &mut Self::Ancha) {
                *ancha = *origin * 2;
            }
        }

        let ancha = VecAncha::new(MultiplyBy2);
        let origin = vec![1usize, 2, 3];

        let mut sz = Reserve(0);
        ancha.reserve(&origin, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            ancha.anchize::<()>(&origin, cur.clone());
            ancha.deanchize::<()>(cur);
        }

        let anchavec = unsafe { &*(buf.as_ptr() as *const AnchaVec<usize>) };
        // Elements should be multiplied by 2!
        assert_eq!(unsafe { anchavec.as_ref() }, &[2, 4, 6]);
    }
}
