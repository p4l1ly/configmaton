//! AnchaArrMap: Fixed-size array map with pointers to values.
//!
//! Memory layout: `[arr: [*const V; SIZE]][val0][val1]...`

use super::{Anchize, BuildCursor, Deanchize, Reserve, Shifter};
use std::marker::PhantomData;

/// AnchaArrMap: fixed-size array of pointers to values
#[repr(C)]
pub struct AnchaArrMap<'a, const SIZE: usize, V> {
    pub arr: [*const V; SIZE],
    _phantom: PhantomData<&'a ()>,
}

impl<'a, const SIZE: usize, V> AnchaArrMap<'a, SIZE, V> {
    pub unsafe fn get(&self, ix: usize) -> &V {
        &*self.arr[ix]
    }
}

// ============================================================================
// Composable anchization for AnchaArrMap
// ============================================================================

/// Anchization strategy for AnchaArrMap.
pub struct ArrMapAncha<const SIZE: usize, ValAnchize> {
    pub val_ancha: ValAnchize,
}

impl<const SIZE: usize, ValAnchize> ArrMapAncha<SIZE, ValAnchize> {
    pub fn new(val_ancha: ValAnchize) -> Self {
        ArrMapAncha { val_ancha }
    }
}

impl<const SIZE: usize, ValAnchize> Anchize for ArrMapAncha<SIZE, ValAnchize>
where
    ValAnchize: Anchize + 'static,
{
    type Origin = [ValAnchize::Origin; SIZE];
    type Ancha<'a>
        = AnchaArrMap<'a, SIZE, ValAnchize::Ancha<'a>>
    where
        Self: 'a;

    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<AnchaArrMap<SIZE, ValAnchize::Ancha<'static>>>(0);
        let addr = sz.0;
        sz.add::<AnchaArrMap<SIZE, ValAnchize::Ancha<'static>>>(1);
        for val_origin in origin.iter() {
            self.val_ancha.reserve(val_origin, sz);
        }
        addr
    }

    unsafe fn anchize<'a, After>(
        &self,
        origin: &Self::Origin,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        let slf = &mut *cur.get_mut();
        let mut val_cur: BuildCursor<ValAnchize::Ancha<'a>> = cur.behind(1);

        for (i, val_origin) in origin.iter().enumerate() {
            slf.arr[i] = val_cur.cur as *const _;
            val_cur = self.val_ancha.anchize::<ValAnchize::Ancha<'a>>(val_origin, val_cur);
        }

        val_cur.align()
    }
}

impl<const SIZE: usize, ValAnchize> Deanchize for ArrMapAncha<SIZE, ValAnchize>
where
    ValAnchize: Deanchize + 'static,
{
    type Ancha<'a>
        = AnchaArrMap<'a, SIZE, ValAnchize::Ancha<'a>>
    where
        Self: 'a;

    unsafe fn deanchize<'a, After>(
        &self,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        let shifter = Shifter(cur.buf);

        // Shift all pointers
        for ptr in (*cur.get_mut()).arr.iter_mut() {
            shifter.shift(ptr);
        }

        // Deanchize all values
        let mut val_cur: BuildCursor<ValAnchize::Ancha<'a>> = cur.behind(1);
        for _ in 0..SIZE {
            val_cur = self.val_ancha.deanchize::<ValAnchize::Ancha<'a>>(val_cur);
        }

        val_cur.align()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec::{AnchaVec, VecAncha};
    use crate::DirectCopy;

    #[test]
    fn test_arrmap_with_vectors() {
        // Array map of 3 vectors
        let val_ancha = VecAncha::new(DirectCopy::<u8>::new());
        let arrmap_ancha = ArrMapAncha::<3, _>::new(val_ancha);

        let origin = [b"foo".to_vec(), b"bar".to_vec(), b"baz".to_vec()];

        let mut sz = Reserve(0);
        arrmap_ancha.reserve(&origin, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            arrmap_ancha.anchize::<()>(&origin, cur.clone());
            arrmap_ancha.deanchize::<()>(cur);
        }

        let arrmap = unsafe { &*(buf.as_ptr() as *const AnchaArrMap<3, AnchaVec<u8>>) };

        assert_eq!(unsafe { arrmap.get(0).as_ref() }, b"foo");
        assert_eq!(unsafe { arrmap.get(1).as_ref() }, b"bar");
        assert_eq!(unsafe { arrmap.get(2).as_ref() }, b"baz");
    }
}
