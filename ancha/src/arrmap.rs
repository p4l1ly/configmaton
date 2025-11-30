//! Fixed-size array map with indirect storage in the Ancha system.
//!
//! `AnchaArrMap<SIZE, V>` stores an array of SIZE pointers to values of type V.
//! The values are stored sequentially after the header.
//!
//! # Memory Layout
//!
//! ```text
//! [AnchaArrMap header: array of pointers] [value 0] [value 1] ... [value SIZE-1]
//! ```

use std::marker::PhantomData;

use super::{Anchize, BuildCursor, Deanchize, Reserve, Shifter};

/// Fixed-size array map with pointers to values.
#[repr(C)]
pub struct AnchaArrMap<'a, const SIZE: usize, V> {
    arr: [*const V; SIZE],
    _phantom: PhantomData<&'a ()>,
}

impl<'a, const SIZE: usize, V> AnchaArrMap<'a, SIZE, V> {
    /// Get a value by index.
    ///
    /// # Safety
    ///
    /// The ArrMap must have been properly anchized and deanchized.
    /// The index must be less than SIZE.
    pub unsafe fn get(&self, ix: usize) -> &'a V {
        assert!(ix < SIZE);
        &*self.arr[ix]
    }
}

// ============================================================================
// Anchization Strategy
// ============================================================================

/// Strategy for anchizing an array into an ArrMap.
#[derive(Clone, Copy)]
pub struct ArrMapAnchizeFromArray<'a, const SIZE: usize, ValueAnchize> {
    pub value_ancha: ValueAnchize,
    _phantom: PhantomData<&'a ValueAnchize>,
}

impl<'a, const SIZE: usize, ValueAnchize> ArrMapAnchizeFromArray<'a, SIZE, ValueAnchize> {
    pub fn new(value_ancha: ValueAnchize) -> Self {
        ArrMapAnchizeFromArray { value_ancha, _phantom: PhantomData }
    }
}

impl<'a, const SIZE: usize, ValueAnchize: Default> Default
    for ArrMapAnchizeFromArray<'a, SIZE, ValueAnchize>
{
    fn default() -> Self {
        ArrMapAnchizeFromArray { value_ancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, const SIZE: usize, ValueAnchize> Anchize<'a>
    for ArrMapAnchizeFromArray<'a, SIZE, ValueAnchize>
where
    ValueAnchize: Anchize<'a>,
{
    type Origin = [ValueAnchize::Origin; SIZE];
    type Ancha = AnchaArrMap<'a, SIZE, ValueAnchize::Ancha>;
    type Context = ValueAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0); // Alignment at the beginning!
        sz.add::<Self::Ancha>(1); // Space for the header (array of pointers)
        for value_origin in origin.iter() {
            // Align before each value (matching anchize alignment)
            sz.add::<ValueAnchize::Ancha>(0);
            self.value_ancha.reserve(value_origin, context, sz);
        }
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align(); // Alignment at the beginning!
        let arrmap = &mut *cur.get_mut();
        let mut vcur: BuildCursor<ValueAnchize::Ancha> = cur.behind(1);

        for (i, value_origin) in origin.iter().enumerate() {
            // Align before storing the pointer (ancha design: elements don't align at end)
            vcur = vcur.align::<ValueAnchize::Ancha>();
            // Store the ALIGNED position as the pointer for this value
            arrmap.arr[i] = vcur.cur as *const ValueAnchize::Ancha;
            // Anchize the value
            vcur = self.value_ancha.anchize(value_origin, context, vcur);
        }
        vcur.transmute()
    }
}

// ============================================================================
// Deanchization Strategy
// ============================================================================

/// Strategy for deanchizing an AnchaArrMap.
#[derive(Clone, Copy)]
pub struct ArrMapDeanchize<'a, const SIZE: usize, ValueDeanchize> {
    pub value_deancha: ValueDeanchize,
    _phantom: PhantomData<&'a ValueDeanchize>,
}

impl<'a, const SIZE: usize, ValueDeanchize> ArrMapDeanchize<'a, SIZE, ValueDeanchize> {
    pub fn new(value_deancha: ValueDeanchize) -> Self {
        ArrMapDeanchize { value_deancha, _phantom: PhantomData }
    }
}

impl<'a, const SIZE: usize, ValueDeanchize: Default> Default
    for ArrMapDeanchize<'a, SIZE, ValueDeanchize>
{
    fn default() -> Self {
        ArrMapDeanchize { value_deancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, const SIZE: usize, ValueDeanchize> Deanchize<'a>
    for ArrMapDeanchize<'a, SIZE, ValueDeanchize>
where
    ValueDeanchize: Deanchize<'a>,
{
    type Ancha = AnchaArrMap<'a, SIZE, ValueDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align(); // Alignment at the beginning!
        let shifter = Shifter(cur.buf);

        // Fix up all the pointers in the array
        let arrmap = &mut *cur.get_mut();
        for ptr in arrmap.arr.iter_mut() {
            shifter.shift(ptr);
        }

        // Deanchize all the values
        let mut vcur: BuildCursor<ValueDeanchize::Ancha> = cur.behind(1);
        for _ in 0..SIZE {
            // Align before accessing each value (ancha design)
            vcur = vcur.align::<ValueDeanchize::Ancha>();
            vcur = self.value_deancha.deanchize(vcur);
        }
        vcur.transmute()
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
    fn test_arrmap_basic() {
        let origin = [vec![1u8, 2, 3], vec![4, 5], vec![6, 7, 8, 9]];

        let anchize: ArrMapAnchizeFromArray<3, VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            ArrMapAnchizeFromArray::default();
        let deanchize: ArrMapDeanchize<3, VecDeanchize<NoopDeanchize<u8>>> =
            ArrMapDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &(), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &(), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let arrmap = unsafe { &*(buf.as_ptr() as *const AnchaArrMap<3, AnchaVec<u8>>) };

        // Verify each value
        assert_eq!(unsafe { arrmap.get(0).as_ref() }, &[1u8, 2, 3]);
        assert_eq!(unsafe { arrmap.get(1).as_ref() }, &[4u8, 5]);
        assert_eq!(unsafe { arrmap.get(2).as_ref() }, &[6u8, 7, 8, 9]);
    }

    #[test]
    fn test_arrmap_single_element() {
        let origin = [vec![42u8]];

        let anchize: ArrMapAnchizeFromArray<1, VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            ArrMapAnchizeFromArray::default();
        let deanchize: ArrMapDeanchize<1, VecDeanchize<NoopDeanchize<u8>>> =
            ArrMapDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &(), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &(), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let arrmap = unsafe { &*(buf.as_ptr() as *const AnchaArrMap<1, AnchaVec<u8>>) };

        assert_eq!(unsafe { arrmap.get(0).as_ref() }, &[42u8]);
    }

    #[test]
    fn test_arrmap_large() {
        let origin = [vec![1u8], vec![2u8], vec![3u8], vec![4u8], vec![5u8]];

        let anchize: ArrMapAnchizeFromArray<5, VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            ArrMapAnchizeFromArray::default();
        let deanchize: ArrMapDeanchize<5, VecDeanchize<NoopDeanchize<u8>>> =
            ArrMapDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &(), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &(), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let arrmap = unsafe { &*(buf.as_ptr() as *const AnchaArrMap<5, AnchaVec<u8>>) };

        for i in 0..5 {
            assert_eq!(unsafe { arrmap.get(i).as_ref() }, &[(i + 1) as u8]);
        }
    }
}
