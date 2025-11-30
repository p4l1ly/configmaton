//! Simple key-value pair structure in the Ancha system.
//!
//! `AnchaFlagellum<K, V>` stores a key and value pair where:
//! - Key is stored inline
//! - Value is stored inline after the key
//!
//! # Memory Layout
//!
//! ```text
//! [Flagellum header: key][value]
//! ```

use std::marker::PhantomData;

use super::{Anchize, BuildCursor, Deanchize, Reserve, StaticAnchize, StaticDeanchize};

/// A simple key-value pair.
#[repr(C)]
pub struct AnchaFlagellum<'a, K, V> {
    pub key: K,
    pub val: V,
    _phantom: PhantomData<&'a ()>,
}

impl<'a, K, V> AnchaFlagellum<'a, K, V> {
    /// Get a reference to the key.
    ///
    /// # Safety
    ///
    /// The Flagellum must have been properly anchized and deanchized.
    pub unsafe fn key(&self) -> &'a K {
        &*(&self.key as *const _)
    }

    /// Get a reference to the value.
    ///
    /// # Safety
    ///
    /// The Flagellum must have been properly anchized and deanchized.
    pub unsafe fn val(&self) -> &'a V {
        &*(&self.val as *const _)
    }
}

impl<'a, K: 'a, V: 'a> super::Assoc<'a> for AnchaFlagellum<'a, K, V> {
    type Key = K;
    type Val = V;

    unsafe fn key(&self) -> &'a K {
        self.key()
    }

    unsafe fn val(&self) -> &'a V {
        self.val()
    }
}

// ============================================================================
// Anchization Strategy
// ============================================================================

/// Strategy for anchizing a (K, V) tuple into a Flagellum.
#[derive(Clone, Copy)]
pub struct FlagellumAnchizeFromTuple<'a, KeyAnchize, ValueAnchize> {
    pub key_ancha: KeyAnchize,
    pub value_ancha: ValueAnchize,
    _phantom: PhantomData<&'a (KeyAnchize, ValueAnchize)>,
}

impl<'a, KeyAnchize, ValueAnchize> FlagellumAnchizeFromTuple<'a, KeyAnchize, ValueAnchize> {
    pub fn new(key_ancha: KeyAnchize, value_ancha: ValueAnchize) -> Self {
        FlagellumAnchizeFromTuple { key_ancha, value_ancha, _phantom: PhantomData }
    }
}

impl<'a, KeyAnchize: Default, ValueAnchize: Default> Default
    for FlagellumAnchizeFromTuple<'a, KeyAnchize, ValueAnchize>
{
    fn default() -> Self {
        FlagellumAnchizeFromTuple {
            key_ancha: Default::default(),
            value_ancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, KeyAnchize, ValueAnchize> Anchize<'a>
    for FlagellumAnchizeFromTuple<'a, KeyAnchize, ValueAnchize>
where
    KeyAnchize: StaticAnchize<'a>,
    ValueAnchize: Anchize<'a, Context = KeyAnchize::Context>,
{
    type Origin = (KeyAnchize::Origin, ValueAnchize::Origin);
    type Ancha = AnchaFlagellum<'a, KeyAnchize::Ancha, ValueAnchize::Ancha>;
    type Context = KeyAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0); // Alignment at the beginning!
        sz.add::<KeyAnchize::Ancha>(1); // Space for the key
        self.value_ancha.reserve(&origin.1, context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align(); // Alignment at the beginning!

        let flagellum = &mut *cur.get_mut();

        // Anchize the key inline
        self.key_ancha.anchize_static(&origin.0, context, &mut flagellum.key);

        // Anchize the value inline after the key
        let vcur: BuildCursor<ValueAnchize::Ancha> = cur.behind::<KeyAnchize::Ancha>(0).behind(1);
        self.value_ancha.anchize(&origin.1, context, vcur)
    }
}

// ============================================================================
// Deanchization Strategy
// ============================================================================

/// Strategy for deanchizing a Flagellum.
#[derive(Clone, Copy)]
pub struct FlagellumDeanchize<'a, KeyDeanchize, ValueDeanchize> {
    pub key_deancha: KeyDeanchize,
    pub value_deancha: ValueDeanchize,
    _phantom: PhantomData<&'a (KeyDeanchize, ValueDeanchize)>,
}

impl<'a, KeyDeanchize, ValueDeanchize> FlagellumDeanchize<'a, KeyDeanchize, ValueDeanchize> {
    pub fn new(key_deancha: KeyDeanchize, value_deancha: ValueDeanchize) -> Self {
        FlagellumDeanchize { key_deancha, value_deancha, _phantom: PhantomData }
    }
}

impl<'a, KeyDeanchize: Default, ValueDeanchize: Default> Default
    for FlagellumDeanchize<'a, KeyDeanchize, ValueDeanchize>
{
    fn default() -> Self {
        FlagellumDeanchize {
            key_deancha: Default::default(),
            value_deancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, KeyDeanchize, ValueDeanchize> Deanchize<'a>
    for FlagellumDeanchize<'a, KeyDeanchize, ValueDeanchize>
where
    KeyDeanchize: StaticDeanchize<'a>,
    ValueDeanchize: Deanchize<'a>,
{
    type Ancha = AnchaFlagellum<'a, KeyDeanchize::Ancha, ValueDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align(); // Alignment at the beginning!

        let flagellum = &mut *cur.get_mut();

        // Deanchize the key
        self.key_deancha.deanchize_static(&mut flagellum.key);

        // Deanchize the value
        let vcur: BuildCursor<ValueDeanchize::Ancha> =
            cur.behind::<KeyDeanchize::Ancha>(0).behind(1);
        self.value_deancha.deanchize(vcur)
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
    fn test_flagellum_basic() {
        let origin = (42u32, vec![1u8, 2, 3]);

        let anchize: FlagellumAnchizeFromTuple<
            CopyAnchize<u32, ()>,
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
        > = FlagellumAnchizeFromTuple::default();
        let deanchize: FlagellumDeanchize<NoopDeanchize<u32>, VecDeanchize<NoopDeanchize<u8>>> =
            FlagellumDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let flagellum = unsafe { &*(buf.as_ptr() as *const AnchaFlagellum<u32, AnchaVec<u8>>) };

        assert_eq!(unsafe { *flagellum.key() }, 42u32);
        assert_eq!(unsafe { flagellum.val().as_ref() }, &[1u8, 2, 3]);
    }

    #[test]
    fn test_flagellum_multiple() {
        let origin1 = (1u8, vec![10u8, 20]);
        let origin2 = (2u8, vec![30u8, 40, 50]);

        let anchize: FlagellumAnchizeFromTuple<
            CopyAnchize<u8, ()>,
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
        > = FlagellumAnchizeFromTuple::default();
        let deanchize: FlagellumDeanchize<NoopDeanchize<u8>, VecDeanchize<NoopDeanchize<u8>>> =
            FlagellumDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin1, &mut (), &mut sz);
        anchize.reserve(&origin2, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let mut cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            cur = anchize.anchize(&origin1, &mut (), cur);
            anchize.anchize::<()>(&origin2, &mut (), cur);

            cur = BuildCursor::new(buf.as_mut_ptr());
            cur = deanchize.deanchize(cur);
            deanchize.deanchize::<()>(cur);
        }

        let flag1 = unsafe { &*(buf.as_ptr() as *const AnchaFlagellum<u8, AnchaVec<u8>>) };
        assert_eq!(unsafe { *flag1.key() }, 1u8);
        assert_eq!(unsafe { flag1.val().as_ref() }, &[10u8, 20]);
    }
}
