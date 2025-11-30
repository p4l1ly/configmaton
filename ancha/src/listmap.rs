//! List-based map with indirect value storage in the Ancha system.
//!
//! `AnchaListMap<K, V>` stores a linked list of key-value pairs where:
//! - Keys are stored inline in list nodes
//! - Values are stored indirectly via pointers
//!
//! # Memory Layout
//!
//! ```text
//! [List node 0: next, val*, key] [List node 1] ... [Value 0] [Value 1] ...
//! ```
//!
//! # Context Enrichment Pattern
//!
//! ListMap uses an **enriched context** to solve the pointer-patching problem:
//! - We need to serialize list nodes first, but they need pointers to values
//! - Values are serialized after the list
//! - Solution: Thread a Vec through the context to collect item cursors during list serialization
//! - Then patch the pointers in a second pass

use std::marker::PhantomData;

use super::{
    list::AnchaList, Anchize, Assocs, AssocsSuper, BuildCursor, Deanchize, Matches, Reserve,
    Shifter, StaticAnchize, StaticDeanchize,
};

/// A single key-value item in the ListMap (key inline, value via pointer).
#[repr(C)]
pub struct ListMapItem<K, V> {
    pub val: *const V,
    pub key: K,
}

type ListMapList<'a, K, V> = AnchaList<'a, ListMapItem<K, V>>;

/// List-based map with indirect value storage.
#[repr(C)]
pub struct AnchaListMap<'a, K, V> {
    pub keys: ListMapList<'a, K, V>,
}

// ============================================================================
// Anchization Strategy
// ============================================================================

/// Strategy for anchizing a Vec<(K, V)> into a ListMap.
///
/// Note: This manually implements the list traversal (doesn't delegate to
/// ListAnchizeFromVec) because we need to collect item cursors for later
/// pointer patching. Since we're doing the traversal manually anyway, we
/// don't need an enriched context - just a local Vec.
pub struct ListMapAnchizeFromVec<'a, KeyAnchize, ValueAnchize> {
    pub key_ancha: KeyAnchize,
    pub value_ancha: ValueAnchize,
    _phantom: PhantomData<&'a (KeyAnchize, ValueAnchize)>,
}

impl<'a, KeyAnchize, ValueAnchize> ListMapAnchizeFromVec<'a, KeyAnchize, ValueAnchize> {
    pub fn new(key_ancha: KeyAnchize, value_ancha: ValueAnchize) -> Self {
        ListMapAnchizeFromVec { key_ancha, value_ancha, _phantom: PhantomData }
    }
}

impl<'a, KeyAnchize: Default, ValueAnchize: Default> Default
    for ListMapAnchizeFromVec<'a, KeyAnchize, ValueAnchize>
{
    fn default() -> Self {
        ListMapAnchizeFromVec {
            key_ancha: Default::default(),
            value_ancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, KeyAnchize, ValueAnchize> Anchize<'a>
    for ListMapAnchizeFromVec<'a, KeyAnchize, ValueAnchize>
where
    KeyAnchize: StaticAnchize<'a>,
    ValueAnchize: Anchize<'a, Context = KeyAnchize::Context>,
{
    type Origin = Vec<(KeyAnchize::Origin, ValueAnchize::Origin)>;
    type Ancha = AnchaListMap<'a, KeyAnchize::Ancha, ValueAnchize::Ancha>;
    type Context = KeyAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        // Reserve space for the list nodes (each contains key + pointer)
        sz.add::<Self::Ancha>(0); // Initial alignment
        for (_key_origin, _value_origin) in origin.iter() {
            // Align before each list node
            sz.add::<ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>>(0);
            sz.add::<*const ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>>(1); // next pointer
            sz.add::<*const ValueAnchize::Ancha>(1); // val pointer
            sz.add::<KeyAnchize::Ancha>(1); // key
        }

        // Reserve space for values
        for (_key_origin, value_origin) in origin.iter() {
            sz.add::<ValueAnchize::Ancha>(0); // Align before each value
            self.value_ancha.reserve(value_origin, context, sz);
        }
        sz.add::<ValueAnchize::Ancha>(0); // Final alignment
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur = cur.align::<Self::Ancha>();

        if origin.is_empty() {
            return cur.transmute();
        }

        // Phase 1: Build the list structure, collecting item cursors (local Vec!)
        let mut item_cursors: Vec<
            BuildCursor<ListMapItem<KeyAnchize::Ancha, ValueAnchize::Ancha>>,
        > = Vec::new();
        let mut list_cur: BuildCursor<ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>> =
            cur.transmute();

        for (i, (key_origin, _)) in origin.iter().enumerate() {
            // Align before each node
            list_cur = list_cur.align::<ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>>();

            // Set next pointer for previous node
            if i > 0 {
                let prev_list_cur: BuildCursor<
                    ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>,
                > = BuildCursor {
                    cur: item_cursors[i - 1].cur
                        - std::mem::size_of::<
                            *const ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>,
                        >(),
                    buf: list_cur.buf,
                    _phantom: PhantomData,
                };
                (*prev_list_cur.get_mut()).next =
                    list_cur.cur as *const ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>;
            }

            // Get cursor to the item (after next pointer)
            let item_cur: BuildCursor<ListMapItem<KeyAnchize::Ancha, ValueAnchize::Ancha>> =
                list_cur
                    .transmute::<*const ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>>()
                    .behind(1);

            // Anchize the key inline
            let item = &mut *item_cur.get_mut();
            self.key_ancha.anchize_static(key_origin, context, &mut item.key);

            // Collect the item cursor
            item_cursors.push(item_cur.clone());

            // Move to next node position
            list_cur = item_cur.behind(1);
        }

        // Set last node's next to null
        if !origin.is_empty() {
            let last_list_cur: BuildCursor<ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>> =
                BuildCursor {
                    cur: item_cursors[origin.len() - 1].cur
                        - std::mem::size_of::<
                            *const ListMapList<KeyAnchize::Ancha, ValueAnchize::Ancha>,
                        >(),
                    buf: list_cur.buf,
                    _phantom: PhantomData,
                };
            (*last_list_cur.get_mut()).next = std::ptr::null();
        }

        // Phase 2: Anchize values and fill in val pointers
        let mut vcur: BuildCursor<ValueAnchize::Ancha> = list_cur.transmute();
        for (i, (_, value_origin)) in origin.iter().enumerate() {
            // Align before each value
            vcur = vcur.align::<ValueAnchize::Ancha>();

            // Store the value pointer in the item
            (*item_cursors[i].get_mut()).val = vcur.cur as *const ValueAnchize::Ancha;

            // Anchize the value
            vcur = self.value_ancha.anchize(value_origin, context, vcur);
        }

        vcur.transmute()
    }
}

// ============================================================================
// Deanchization Strategy
// ============================================================================

/// Strategy for deanchizing a ListMap.
#[derive(Clone, Copy)]
pub struct ListMapDeanchize<'a, KeyDeanchize, ValueDeanchize> {
    pub key_deancha: KeyDeanchize,
    pub value_deancha: ValueDeanchize,
    _phantom: PhantomData<&'a (KeyDeanchize, ValueDeanchize)>,
}

impl<'a, KeyDeanchize, ValueDeanchize> ListMapDeanchize<'a, KeyDeanchize, ValueDeanchize> {
    pub fn new(key_deancha: KeyDeanchize, value_deancha: ValueDeanchize) -> Self {
        ListMapDeanchize { key_deancha, value_deancha, _phantom: PhantomData }
    }
}

impl<'a, KeyDeanchize: Default, ValueDeanchize: Default> Default
    for ListMapDeanchize<'a, KeyDeanchize, ValueDeanchize>
{
    fn default() -> Self {
        ListMapDeanchize {
            key_deancha: Default::default(),
            value_deancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, KeyDeanchize, ValueDeanchize> Deanchize<'a>
    for ListMapDeanchize<'a, KeyDeanchize, ValueDeanchize>
where
    KeyDeanchize: StaticDeanchize<'a>,
    ValueDeanchize: Deanchize<'a>,
{
    type Ancha = AnchaListMap<'a, KeyDeanchize::Ancha, ValueDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let cur = cur.align::<Self::Ancha>();
        let shifter = Shifter(cur.buf);
        let mut len = 0;

        // Phase 1: Traverse the list, fix up pointers, and count
        let mut list_cur: BuildCursor<ListMapList<KeyDeanchize::Ancha, ValueDeanchize::Ancha>> =
            cur.transmute();

        loop {
            // Align before each node
            list_cur = list_cur.align::<ListMapList<KeyDeanchize::Ancha, ValueDeanchize::Ancha>>();

            let node = &mut *list_cur.get_mut();
            let is_last = node.next.is_null();

            if !is_last {
                shifter.shift(&mut node.next);
            }

            // Get the item cursor
            let item_cur: BuildCursor<ListMapItem<KeyDeanchize::Ancha, ValueDeanchize::Ancha>> =
                list_cur
                    .transmute::<*const ListMapList<KeyDeanchize::Ancha, ValueDeanchize::Ancha>>()
                    .behind(1);

            let item = &mut *item_cur.get_mut();

            // Deanchize the key
            self.key_deancha.deanchize_static(&mut item.key);

            // Fix up the value pointer
            shifter.shift(&mut item.val);

            len += 1;

            list_cur = item_cur.behind(1);

            if is_last {
                break;
            }
        }

        // Phase 2: Deanchize all values
        let mut vcur: BuildCursor<ValueDeanchize::Ancha> = list_cur.transmute();
        for _ in 0..len {
            vcur = vcur.align::<ValueDeanchize::Ancha>();
            vcur = self.value_deancha.deanchize(vcur);
        }

        vcur.transmute()
    }
}

// ============================================================================
// Assocs Implementation
// ============================================================================

/// Iterator over ListMap entries matching a key.
pub struct ListMapIter<'a, 'b, X, K, V> {
    x: &'b X,
    current: *const AnchaList<'a, ListMapItem<K, V>>,
}

impl<'a, 'b, X: Matches<K>, K: 'a, V: 'a + 'b> Iterator for ListMapIter<'a, 'b, X, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            while !self.current.is_null() {
                let node = &*self.current;
                // Get the item from the node
                let item_ptr = (self.current as *const u8)
                    .add(std::mem::size_of::<*const AnchaList<'a, ListMapItem<K, V>>>())
                    as *const ListMapItem<K, V>;
                let item = &*item_ptr;

                // Move to next node
                self.current = node.next;

                if self.x.matches(&item.key) {
                    return Some((&item.key, &*item.val));
                }
            }
            None
        }
    }
}

impl<'a, K: 'a, V: 'a> AssocsSuper<'a> for AnchaListMap<'a, K, V> {
    type Key = K;
    type Val = V;
    type I<'b, X: 'b + Matches<K>>
        = ListMapIter<'a, 'b, X, K, V>
    where
        'a: 'b;
}

impl<'a, K: 'a, V: 'a> Assocs<'a> for AnchaListMap<'a, K, V> {
    unsafe fn iter_matches<'c, 'b, X: Matches<K>>(&'c self, key: &'b X) -> Self::I<'b, X>
    where
        'a: 'b + 'c,
    {
        ListMapIter { x: key, current: &self.keys }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{vec::*, AnyMatch, CopyAnchize, EqMatch, NoopDeanchize};

    #[test]
    fn test_listmap_basic() {
        let origin = vec![(1u32, vec![10u8, 20]), (2u32, vec![30u8, 40]), (3u32, vec![50u8, 60])];

        let anchize: ListMapAnchizeFromVec<
            CopyAnchize<u32, ()>,
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
        > = ListMapAnchizeFromVec::default();
        let deanchize: ListMapDeanchize<NoopDeanchize<u32>, VecDeanchize<NoopDeanchize<u8>>> =
            ListMapDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let listmap = unsafe { &*(buf.as_ptr() as *const AnchaListMap<u32, AnchaVec<u8>>) };

        // Search for key 2
        let mut iter = unsafe { listmap.iter_matches(&EqMatch(&2u32)) };
        let (k, v) = iter.next().unwrap();
        assert_eq!(*k, 2u32);
        assert_eq!(unsafe { v.as_ref() }, &[30u8, 40]);
        assert!(iter.next().is_none());

        // Iterate all with AnyMatch
        let collected: Vec<(u32, Vec<u8>)> = unsafe {
            listmap.iter_matches(&AnyMatch).map(|(k, v)| (*k, v.as_ref().to_vec())).collect()
        };

        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0], (1u32, vec![10u8, 20]));
        assert_eq!(collected[1], (2u32, vec![30u8, 40]));
        assert_eq!(collected[2], (3u32, vec![50u8, 60]));
    }

    #[test]
    fn test_listmap_empty() {
        let origin: Vec<(u32, Vec<u8>)> = vec![];

        let anchize: ListMapAnchizeFromVec<
            CopyAnchize<u32, ()>,
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
        > = ListMapAnchizeFromVec::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        // Empty map should have minimal size
        assert!(sz.0 <= 8);
    }
}
