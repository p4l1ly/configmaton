//! Vector-based map with indirect value storage in the Ancha system.
//!
//! `AnchaVecMap<K, V>` stores a vector of key-value pairs where:
//! - Keys are stored inline in a vector
//! - Values are stored indirectly via pointers
//!
//! # Memory Layout
//!
//! ```text
//! [AnchaVec header] [Item 0 (key, *val)] [Item 1] ... [Value 0] [Value 1] ...
//! ```

use std::marker::PhantomData;

use super::{vec::AnchaVec, BuildCursor, Reserve, Shifter};

/// A single key-value item in the VecMap.
#[repr(C)]
pub struct VecMapItem<K, V> {
    pub key: K,
    pub val: *const V,
}

type VecMapVec<'a, K, V> = AnchaVec<'a, VecMapItem<K, V>>;

/// Vector-based map with indirect value storage.
#[repr(C)]
pub struct AnchaVecMap<'a, K, V> {
    pub keys: VecMapVec<'a, K, V>,
}

impl<'a, K, V> AnchaVecMap<'a, K, V> {
    /// Get the number of key-value pairs.
    pub fn len(&self) -> usize {
        self.keys.len
    }

    /// Check if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate over all key-value pairs.
    ///
    /// # Safety
    ///
    /// The VecMap must have been properly anchized and deanchized.
    pub unsafe fn iter(&self) -> AnchaVecMapIter<'a, K, V> {
        AnchaVecMapIter { items: self.keys.as_ref(), index: 0, _phantom: PhantomData }
    }
}

/// Iterator over VecMap entries.
pub struct AnchaVecMapIter<'a, K, V> {
    items: &'a [VecMapItem<K, V>],
    index: usize,
    _phantom: PhantomData<&'a (K, V)>,
}

impl<'a, K, V> Iterator for AnchaVecMapIter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.items.len() {
            let item = &self.items[self.index];
            self.index += 1;
            Some((&item.key, unsafe { &*item.val }))
        } else {
            None
        }
    }
}

// ============================================================================
// Anchization Strategy
// ============================================================================

use super::{Anchize, StaticAnchize};

/// Strategy for anchizing a Vec<(K, V)> into a VecMap.
pub struct VecMapAnchizeFromVec<'a, KeyAnchize, ValueAnchize> {
    pub key_ancha: KeyAnchize,
    pub value_ancha: ValueAnchize,
    _phantom: PhantomData<&'a (KeyAnchize, ValueAnchize)>,
}

impl<'a, KeyAnchize, ValueAnchize> VecMapAnchizeFromVec<'a, KeyAnchize, ValueAnchize> {
    pub fn new(key_ancha: KeyAnchize, value_ancha: ValueAnchize) -> Self {
        VecMapAnchizeFromVec { key_ancha, value_ancha, _phantom: PhantomData }
    }
}

impl<'a, KeyAnchize: Default, ValueAnchize: Default> Default
    for VecMapAnchizeFromVec<'a, KeyAnchize, ValueAnchize>
{
    fn default() -> Self {
        VecMapAnchizeFromVec {
            key_ancha: Default::default(),
            value_ancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, KeyAnchize, ValueAnchize> Anchize<'a>
    for VecMapAnchizeFromVec<'a, KeyAnchize, ValueAnchize>
where
    KeyAnchize: StaticAnchize<'a>,
    ValueAnchize: Anchize<'a, Context = KeyAnchize::Context>,
{
    type Origin = Vec<(KeyAnchize::Origin, ValueAnchize::Origin)>;
    type Ancha = AnchaVecMap<'a, KeyAnchize::Ancha, ValueAnchize::Ancha>;
    type Context = KeyAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve) {
        // Reserve space for the VecMap header (which contains the vector header)
        sz.add::<Self::Ancha>(0);
        sz.add::<Self::Ancha>(1); // The header contains AnchaVec which has len field
        sz.add::<VecMapItem<KeyAnchize::Ancha, ValueAnchize::Ancha>>(origin.len());

        // Reserve space for values
        for (_, value_origin) in origin.iter() {
            sz.add::<ValueAnchize::Ancha>(0); // Align before each value
            self.value_ancha.reserve(value_origin, context, sz);
        }
        sz.add::<ValueAnchize::Ancha>(0); // Final alignment
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur = cur.align::<Self::Ancha>();

        // Set up the vector header
        let vecmap = &mut *cur.get_mut();
        vecmap.keys.len = origin.len();

        // Get cursor to the items array
        let mut item_cur: BuildCursor<VecMapItem<KeyAnchize::Ancha, ValueAnchize::Ancha>> =
            cur.behind(1);

        // Get cursor to where values will be stored (after all items)
        let mut vcur: BuildCursor<ValueAnchize::Ancha> = item_cur.behind(origin.len());

        // Anchize keys and store value pointers
        for (key_origin, value_origin) in origin.iter() {
            let item = &mut *item_cur.get_mut();

            // Anchize the key inline
            self.key_ancha.anchize_static(key_origin, context, &mut item.key);

            // Align and store the value pointer
            vcur = vcur.align::<ValueAnchize::Ancha>();
            item.val = vcur.cur as *const ValueAnchize::Ancha;

            // Anchize the value
            vcur = self.value_ancha.anchize(value_origin, context, vcur);

            item_cur.inc();
        }

        vcur.transmute()
    }
}

// ============================================================================
// Deanchization Strategy
// ============================================================================

use super::{Deanchize, StaticDeanchize};

/// Strategy for deanchizing a VecMap.
pub struct VecMapDeanchize<'a, KeyDeanchize, ValueDeanchize> {
    pub key_deancha: KeyDeanchize,
    pub value_deancha: ValueDeanchize,
    _phantom: PhantomData<&'a (KeyDeanchize, ValueDeanchize)>,
}

impl<'a, KeyDeanchize, ValueDeanchize> VecMapDeanchize<'a, KeyDeanchize, ValueDeanchize> {
    pub fn new(key_deancha: KeyDeanchize, value_deancha: ValueDeanchize) -> Self {
        VecMapDeanchize { key_deancha, value_deancha, _phantom: PhantomData }
    }
}

impl<'a, KeyDeanchize: Default, ValueDeanchize: Default> Default
    for VecMapDeanchize<'a, KeyDeanchize, ValueDeanchize>
{
    fn default() -> Self {
        VecMapDeanchize {
            key_deancha: Default::default(),
            value_deancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, KeyDeanchize, ValueDeanchize> Deanchize<'a>
    for VecMapDeanchize<'a, KeyDeanchize, ValueDeanchize>
where
    KeyDeanchize: StaticDeanchize<'a>,
    ValueDeanchize: Deanchize<'a>,
{
    type Ancha = AnchaVecMap<'a, KeyDeanchize::Ancha, ValueDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let cur = cur.align::<Self::Ancha>();
        let vecmap = &mut *cur.get_mut();
        let len = vecmap.keys.len;
        let shifter = Shifter(cur.buf);

        // Fix up keys and value pointers
        let mut item_cur: BuildCursor<VecMapItem<KeyDeanchize::Ancha, ValueDeanchize::Ancha>> =
            cur.behind(1);
        for _ in 0..len {
            let item = &mut *item_cur.get_mut();

            // Deanchize the key
            self.key_deancha.deanchize_static(&mut item.key);

            // Fix up the value pointer
            shifter.shift(&mut item.val);

            item_cur.inc();
        }

        // Deanchize all values
        let mut vcur: BuildCursor<ValueDeanchize::Ancha> = item_cur.transmute();
        for _ in 0..len {
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
    fn test_vecmap_basic() {
        let origin = vec![(1u32, vec![1u8, 2, 3]), (2u32, vec![4, 5]), (3u32, vec![6, 7, 8, 9])];

        let anchize: VecMapAnchizeFromVec<
            CopyAnchize<u32, ()>,
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
        > = VecMapAnchizeFromVec::default();
        let deanchize: VecMapDeanchize<NoopDeanchize<u32>, VecDeanchize<NoopDeanchize<u8>>> =
            VecMapDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &(), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &(), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let vecmap = unsafe { &*(buf.as_ptr() as *const AnchaVecMap<u32, AnchaVec<u8>>) };

        assert_eq!(vecmap.len(), 3);

        let collected: Vec<(u32, Vec<u8>)> =
            unsafe { vecmap.iter().map(|(k, v)| (*k, v.as_ref().to_vec())).collect() };

        assert_eq!(collected[0], (1u32, vec![1u8, 2, 3]));
        assert_eq!(collected[1], (2u32, vec![4u8, 5]));
        assert_eq!(collected[2], (3u32, vec![6u8, 7, 8, 9]));
    }

    #[test]
    fn test_vecmap_empty() {
        let origin: Vec<(u32, Vec<u8>)> = vec![];

        let anchize: VecMapAnchizeFromVec<
            CopyAnchize<u32, ()>,
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
        > = VecMapAnchizeFromVec::default();
        let deanchize: VecMapDeanchize<NoopDeanchize<u32>, VecDeanchize<NoopDeanchize<u8>>> =
            VecMapDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &(), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &(), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let vecmap = unsafe { &*(buf.as_ptr() as *const AnchaVecMap<u32, AnchaVec<u8>>) };

        assert_eq!(vecmap.len(), 0);
        assert!(vecmap.is_empty());
    }

    #[test]
    fn test_vecmap_single() {
        let origin = vec![(42u32, vec![1u8, 2])];

        let anchize: VecMapAnchizeFromVec<
            CopyAnchize<u32, ()>,
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
        > = VecMapAnchizeFromVec::default();
        let deanchize: VecMapDeanchize<NoopDeanchize<u32>, VecDeanchize<NoopDeanchize<u8>>> =
            VecMapDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &(), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &(), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let vecmap = unsafe { &*(buf.as_ptr() as *const AnchaVecMap<u32, AnchaVec<u8>>) };

        assert_eq!(vecmap.len(), 1);

        let collected: Vec<(u32, Vec<u8>)> =
            unsafe { vecmap.iter().map(|(k, v)| (*k, v.as_ref().to_vec())).collect() };

        assert_eq!(collected[0], (42u32, vec![1u8, 2]));
    }
}
