//! Two-element tuple with sequential storage.
//!
//! `Tupellum` stores two values sequentially: the first inline, the second
//! immediately after (with proper alignment).
//!
//! # Memory Layout
//!
//! ```text
//! ┌─────────┬─────────┬─────────┐
//! │    A    │ padding │    B    │
//! └─────────┴─────────┴─────────┘
//! ```
//!
//! This is useful for pairing data where you want both elements stored
//! contiguously. For example:
//! - Key and value in a hash table
//! - Metadata and payload
//! - Header and body

use super::{Anchize, BuildCursor, Deanchize, Reserve};
use std::marker::PhantomData;

/// A two-element tuple with inline storage for both elements.
///
/// The first element (type `A`) is stored inline in the struct.
/// The second element (type `B`) is stored immediately after `A`,
/// properly aligned.
#[repr(C)]
pub struct Tupellum<'a, A, B> {
    /// The first element, stored inline.
    pub a: A,
    _phantom: PhantomData<&'a B>,
}

pub struct TupellumAnchizeFromTuple<'a, A, B> {
    pub a_ancha: A,
    pub b_ancha: B,
    _phantom: PhantomData<&'a (A, B)>,
}

impl<'a, A, B> TupellumAnchizeFromTuple<'a, A, B> {
    pub fn new(a_ancha: A, b_ancha: B) -> Self {
        TupellumAnchizeFromTuple { a_ancha, b_ancha, _phantom: PhantomData }
    }
}

impl<'a, A: Default, B: Default> Default for TupellumAnchizeFromTuple<'a, A, B> {
    fn default() -> Self {
        TupellumAnchizeFromTuple {
            a_ancha: Default::default(),
            b_ancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, A: Anchize<'a>, B: Anchize<'a, Context = A::Context>> Anchize<'a>
    for TupellumAnchizeFromTuple<'a, A, B>
{
    type Origin = (A::Origin, B::Origin);
    type Ancha = Tupellum<'a, A::Ancha, B::Ancha>;
    type Context = A::Context;

    fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve) {
        self.a_ancha.reserve(&origin.0, context, sz);
        self.b_ancha.reserve(&origin.1, context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let vcur = self.a_ancha.anchize(&origin.0, context, cur.transmute());
        self.b_ancha.anchize(&origin.1, context, vcur)
    }
}

pub struct TupellumDeanchizeFromTuple<'a, A, B> {
    pub a_deancha: A,
    pub b_deancha: B,
    _phantom: PhantomData<&'a (A, B)>,
}

impl<'a, A, B> TupellumDeanchizeFromTuple<'a, A, B> {
    pub fn new(a_deancha: A, b_deancha: B) -> Self {
        TupellumDeanchizeFromTuple { a_deancha, b_deancha, _phantom: PhantomData }
    }
}

impl<'a, A: Default, B: Default> Default for TupellumDeanchizeFromTuple<'a, A, B> {
    fn default() -> Self {
        TupellumDeanchizeFromTuple {
            a_deancha: Default::default(),
            b_deancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, A: Deanchize<'a>, B: Deanchize<'a>> Deanchize<'a>
    for TupellumDeanchizeFromTuple<'a, A, B>
{
    type Ancha = Tupellum<'a, A::Ancha, B::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let vcur = self.a_deancha.deanchize(cur.transmute());
        self.b_deancha.deanchize(vcur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec::{AnchaVec, VecAnchizeFromVec, VecDeanchize};
    use crate::{CopyAnchize, NoopDeanchize};

    #[test]
    fn test_tupellum() {
        // Create origin data: a tuple of two vectors
        let origin = (vec![1u8, 2, 3], vec![4u8, 5, 6, 7]);

        let anchize: TupellumAnchizeFromTuple<
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
        > = TupellumAnchizeFromTuple::default();

        let deanchize: TupellumDeanchizeFromTuple<
            VecDeanchize<NoopDeanchize<u8>>,
            VecDeanchize<NoopDeanchize<u8>>,
        > = TupellumDeanchizeFromTuple::default();

        // Reserve phase: calculate space needed
        let mut sz = Reserve(0);
        anchize.reserve(&origin, &(), &mut sz);

        // Allocate buffer
        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &(), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        // Use the deserialized structure
        let tupellum = unsafe { &*(buf.as_ptr() as *const Tupellum<AnchaVec<u8>, AnchaVec<u8>>) };

        // Verify first vector
        assert_eq!(unsafe { tupellum.a.as_ref() }, &[1u8, 2, 3]);

        // Verify second vector (stored behind the first)
        let second_vec = unsafe { tupellum.a.behind::<AnchaVec<u8>>() };
        assert_eq!(unsafe { second_vec.as_ref() }, &[4u8, 5, 6, 7]);
    }
}
