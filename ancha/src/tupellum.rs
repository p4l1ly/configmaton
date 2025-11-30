//! AnchaTupellum: Two-element tuple with sequential storage.
//!
//! Memory layout: `[A][padding][B]`
//!
//! **Note**: Both elements should be dynamic structures (Sediment, List, Vec, etc.).
//! Do not use with primitives - use a proper struct or specialized data structure instead.

use super::{Anchize, BuildCursor, Deanchize, Reserve};
use std::marker::PhantomData;

/// AnchaTupellum: stores two elements sequentially.
///
/// The first element `a` is stored inline, the second element `b`
/// is stored immediately after (properly aligned).
#[repr(C)]
pub struct AnchaTupellum<'a, A, B> {
    /// The first element
    pub a: A,
    _phantom: PhantomData<&'a B>,
}

impl<'a, A, B> AnchaTupellum<'a, A, B> {
    /// Get the first element.
    pub fn a(&self) -> &A {
        &self.a
    }

    // Note: No .b() method. Access the second element through .a's behind() method,
    // just like the original blob implementation.
}

// ============================================================================
// Composable anchization for AnchaTupellum
// ============================================================================

/// Anchization strategy for AnchaTupellum with customizable element strategies.
pub struct TupellumAncha<AAnchize, BAnchize> {
    pub a_ancha: AAnchize,
    pub b_ancha: BAnchize,
}

impl<AAnchize, BAnchize> TupellumAncha<AAnchize, BAnchize> {
    pub fn new(a_ancha: AAnchize, b_ancha: BAnchize) -> Self {
        TupellumAncha { a_ancha, b_ancha }
    }
}

impl<AAnchize, BAnchize> Anchize for TupellumAncha<AAnchize, BAnchize>
where
    AAnchize: Anchize + 'static,
    BAnchize: Anchize + 'static,
{
    type Origin = (AAnchize::Origin, BAnchize::Origin);
    type Ancha<'a>
        = AnchaTupellum<'a, AAnchize::Ancha<'a>, BAnchize::Ancha<'a>>
    where
        Self: 'a;

    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<AnchaTupellum<AAnchize::Ancha<'static>, BAnchize::Ancha<'static>>>(0);
        let addr = sz.0;
        self.a_ancha.reserve(&origin.0, sz);
        self.b_ancha.reserve(&origin.1, sz);
        addr
    }

    unsafe fn anchize<'a, After>(
        &self,
        origin: &Self::Origin,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        // Serialize first element
        let b_cur = self.a_ancha.anchize::<BAnchize::Ancha<'a>>(&origin.0, cur.transmute());
        // Serialize second element
        self.b_ancha.anchize::<After>(&origin.1, b_cur)
    }
}

impl<AAnchize, BAnchize> Deanchize for TupellumAncha<AAnchize, BAnchize>
where
    AAnchize: Deanchize + 'static,
    BAnchize: Deanchize + 'static,
{
    type Ancha<'a>
        = AnchaTupellum<'a, AAnchize::Ancha<'a>, BAnchize::Ancha<'a>>
    where
        Self: 'a;

    unsafe fn deanchize<'a, After>(
        &self,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        // Deanchize first element
        let b_cur = self.a_ancha.deanchize::<BAnchize::Ancha<'a>>(cur.transmute());
        // Deanchize second element
        self.b_ancha.deanchize::<After>(b_cur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec::{AnchaVec, VecAncha};
    use crate::DirectCopy;

    #[test]
    fn test_tupellum_with_vec() {
        // Tuple of (VecAncha<u8>, VecAncha<u8>)
        let a_ancha = VecAncha::new(DirectCopy::<u8>::new());
        let b_ancha = VecAncha::new(DirectCopy::<u8>::new());
        let tup_ancha = TupellumAncha::new(a_ancha, b_ancha);

        let origin = (b"hello".to_vec(), b"world".to_vec());

        let mut sz = Reserve(0);
        let addr = tup_ancha.reserve(&origin, &mut sz);
        println!("Reserved {} bytes, addr={}", sz.0, addr);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            let after_cur = tup_ancha.anchize::<()>(&origin, cur.clone());
            println!("After anchize, cursor at: {}", after_cur.cur);
            tup_ancha.deanchize::<()>(cur);
        }

        let tup = unsafe { &*(buf.as_ptr() as *const AnchaTupellum<AnchaVec<u8>, AnchaVec<u8>>) };
        println!("tup.a.len = {}", tup.a.len);
        println!("tup at {:p}, buf at {:p}", tup, buf.as_ptr());

        // Access first element directly
        assert_eq!(tup.a.len, 5, "First vec should have length 5");
        assert_eq!(unsafe { tup.a.as_ref() }, b"hello");

        // Access second element through first element's behind() method (original pattern)
        let vec_b: &AnchaVec<u8> = unsafe { tup.a.behind() };
        assert_eq!(vec_b.len, 5, "Second vec should have length 5");
        assert_eq!(unsafe { vec_b.as_ref() }, b"world");
    }
}
