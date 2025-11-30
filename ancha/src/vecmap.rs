//! AnchaVecMap: Map stored as vector of keys with pointers to values.
//!
//! Memory layout: `[len][key0,val_ptr0][key1,val_ptr1]...[val0][val1]...`

use super::{get_behind_struct, Anchize, BuildCursor, Deanchize, Reserve, Shifter, StaticAnchize};
use std::marker::PhantomData;

/// VecMapItem: a key with a pointer to its value
#[repr(C)]
pub struct AnchaVecMapItem<K, V> {
    pub key: K,
    pub val: *const V,
}

/// AnchaVecMap: map structure with keys and value pointers
#[repr(C)]
pub struct AnchaVecMap<'a, K, V> {
    pub len: usize,
    _phantom: PhantomData<&'a (K, V)>,
}

impl<'a, K, V> AnchaVecMap<'a, K, V> {
    /// Get the vec of items (keys + val pointers).
    ///
    /// # Safety
    ///
    /// - The vecmap must be properly initialized and deanchized
    pub unsafe fn items(&self) -> &'a [AnchaVecMapItem<K, V>] {
        let ptr = get_behind_struct::<_, AnchaVecMapItem<K, V>>(self);
        std::slice::from_raw_parts(ptr, self.len)
    }
}

// ============================================================================
// Composable anchization for AnchaVecMap
// ============================================================================

/// Anchization strategy for AnchaVecMap.
pub struct VecMapAncha<KeyAnchize, ValAnchize> {
    pub key_ancha: KeyAnchize,
    pub val_ancha: ValAnchize,
}

impl<KeyAnchize, ValAnchize> VecMapAncha<KeyAnchize, ValAnchize> {
    pub fn new(key_ancha: KeyAnchize, val_ancha: ValAnchize) -> Self {
        VecMapAncha { key_ancha, val_ancha }
    }
}

impl<KeyAnchize, ValAnchize> Anchize for VecMapAncha<KeyAnchize, ValAnchize>
where
    KeyAnchize: StaticAnchize + 'static,
    ValAnchize: Anchize + 'static,
    KeyAnchize::Ancha: Sized,
{
    type Origin = Vec<(KeyAnchize::Origin, ValAnchize::Origin)>;
    type Ancha<'a>
        = AnchaVecMap<'a, KeyAnchize::Ancha, ValAnchize::Ancha<'a>>
    where
        Self: 'a;

    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<AnchaVecMap<KeyAnchize::Ancha, ValAnchize::Ancha<'static>>>(1);
        let addr = sz.0;
        sz.add::<AnchaVecMapItem<KeyAnchize::Ancha, ValAnchize::Ancha<'static>>>(origin.len());
        for (_, val_origin) in origin.iter() {
            self.val_ancha.reserve(val_origin, sz);
        }
        sz.add::<ValAnchize::Ancha<'static>>(0);
        addr
    }

    unsafe fn anchize<'a, After>(
        &self,
        origin: &Self::Origin,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).len = origin.len();
        let mut item_cur: BuildCursor<AnchaVecMapItem<KeyAnchize::Ancha, ValAnchize::Ancha<'a>>> =
            cur.behind(1);
        let mut val_cur: BuildCursor<ValAnchize::Ancha<'a>> = item_cur.behind(origin.len());

        for (key_origin, val_origin) in origin.iter() {
            // Anchize key in-place
            self.key_ancha.anchize_static(key_origin, &mut (*item_cur.get_mut()).key);
            // Store pointer to where value will be
            (*item_cur.get_mut()).val = val_cur.cur as *const _;
            // Anchize value
            val_cur = self.val_ancha.anchize::<ValAnchize::Ancha<'a>>(val_origin, val_cur);
            // Move to next item
            item_cur.inc();
        }

        val_cur.align()
    }
}

impl<KeyAnchize, ValAnchize> Deanchize for VecMapAncha<KeyAnchize, ValAnchize>
where
    KeyAnchize: StaticAnchize + 'static,
    ValAnchize: Deanchize + 'static,
    KeyAnchize::Ancha: Sized,
{
    type Ancha<'a>
        = AnchaVecMap<'a, KeyAnchize::Ancha, ValAnchize::Ancha<'a>>
    where
        Self: 'a;

    unsafe fn deanchize<'a, After>(
        &self,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        let len = (*cur.get_mut()).len;
        let shifter = Shifter(cur.buf);
        let mut item_cur: BuildCursor<AnchaVecMapItem<KeyAnchize::Ancha, ValAnchize::Ancha<'a>>> =
            cur.behind(1);

        // Shift all value pointers
        for _ in 0..len {
            shifter.shift(&mut (*item_cur.get_mut()).val);
            item_cur.inc();
        }

        // Deanchize all values
        let mut val_cur: BuildCursor<ValAnchize::Ancha<'a>> = item_cur.behind(0);
        for _ in 0..len {
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
    fn test_vecmap_with_vectors() {
        // Map from u8 to VecAncha<u8>
        let key_ancha = DirectCopy::<u8>::new();
        let val_ancha = VecAncha::new(DirectCopy::<u8>::new());
        let map_ancha = VecMapAncha::new(key_ancha, val_ancha);

        let origin = vec![(1u8, b"hello".to_vec()), (2u8, b"world".to_vec())];

        let mut sz = Reserve(0);
        map_ancha.reserve(&origin, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            map_ancha.anchize::<()>(&origin, cur.clone());
            map_ancha.deanchize::<()>(cur);
        }

        let map = unsafe { &*(buf.as_ptr() as *const AnchaVecMap<u8, AnchaVec<u8>>) };
        let items = unsafe { map.items() };

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].key, 1);
        assert_eq!(unsafe { (*items[0].val).as_ref() }, b"hello");
        assert_eq!(items[1].key, 2);
        assert_eq!(unsafe { (*items[1].val).as_ref() }, b"world");
    }
}
