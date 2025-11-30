//! Associative list - a linked list implementing the Assocs trait.
//!
//! `AnchaAssocList` is a wrapper around `AnchaList` that implements
//! the `Assocs` trait, allowing iteration with key matching.

use super::{
    list::{AnchaList, ListAnchizeFromVec, ListDeanchize},
    Anchize, Assoc, Assocs, AssocsSuper, BuildCursor, Deanchize, Matches, Reserve,
};

/// An associative list - a linked list of key-value pairs.
#[repr(C)]
pub struct AnchaAssocList<'a, KV>(pub AnchaList<'a, KV>);

impl<'a, KV> AnchaAssocList<'a, KV> {
    /// Get a reference to the underlying list.
    pub fn list(&self) -> &AnchaList<'a, KV> {
        &self.0
    }
}

// ============================================================================
// Anchization Strategy
// ============================================================================

/// Strategy for anchizing a Vec into an AssocList.
/// This is a simple wrapper around ListAnchizeFromVec.
pub struct AssocListAnchizeFromVec<'a, ElemAnchize> {
    pub list_ancha: ListAnchizeFromVec<'a, ElemAnchize>,
}

impl<'a, ElemAnchize> AssocListAnchizeFromVec<'a, ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        AssocListAnchizeFromVec { list_ancha: ListAnchizeFromVec::new(elem_ancha) }
    }
}

impl<'a, ElemAnchize: Default> Default for AssocListAnchizeFromVec<'a, ElemAnchize> {
    fn default() -> Self {
        AssocListAnchizeFromVec { list_ancha: ListAnchizeFromVec::default() }
    }
}

impl<'a, ElemAnchize> Anchize<'a> for AssocListAnchizeFromVec<'a, ElemAnchize>
where
    ElemAnchize: Anchize<'a>,
{
    type Origin = Vec<ElemAnchize::Origin>;
    type Ancha = AnchaAssocList<'a, ElemAnchize::Ancha>;
    type Context = ElemAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        // Delegate to list reserve
        self.list_ancha.reserve(origin, context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        // Delegate to list anchize
        self.list_ancha.anchize(origin, context, cur.transmute())
    }
}

// ============================================================================
// Deanchization Strategy
// ============================================================================

/// Strategy for deanchizing an AssocList.
/// This is a simple wrapper around ListDeanchize.
pub struct AssocListDeanchize<'a, ElemDeanchize> {
    pub list_deancha: ListDeanchize<'a, ElemDeanchize>,
}

impl<'a, ElemDeanchize> AssocListDeanchize<'a, ElemDeanchize> {
    pub fn new(elem_deancha: ElemDeanchize) -> Self {
        AssocListDeanchize { list_deancha: ListDeanchize::new(elem_deancha) }
    }
}

impl<'a, ElemDeanchize: Default> Default for AssocListDeanchize<'a, ElemDeanchize> {
    fn default() -> Self {
        AssocListDeanchize { list_deancha: ListDeanchize::default() }
    }
}

impl<'a, ElemDeanchize> Deanchize<'a> for AssocListDeanchize<'a, ElemDeanchize>
where
    ElemDeanchize: Deanchize<'a>,
{
    type Ancha = AnchaAssocList<'a, ElemDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        // Delegate to list deanchize
        self.list_deancha.deanchize(cur.transmute())
    }
}

// ============================================================================
// Assocs Implementation
// ============================================================================

/// Iterator over AssocList entries matching a key.
pub struct AssocListIter<'a, 'b, X, KV> {
    x: &'b X,
    current: *const AnchaList<'a, KV>,
}

impl<'a, 'b, KV: 'b + Assoc<'a> + 'a, X: Matches<KV::Key>> Iterator
    for AssocListIter<'a, 'b, X, KV>
{
    type Item = (&'a KV::Key, &'a KV::Val);

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            while !self.current.is_null() {
                let node = &*self.current;
                // The list iterator logic - need to get the value from the node
                // Using the list's internal structure
                let kv_ptr = (self.current as *const u8)
                    .add(std::mem::size_of::<*const AnchaList<'a, KV>>())
                    as *const KV;
                let key_val = &*kv_ptr;

                // Move to next node
                self.current = node.next;

                let key = key_val.key();
                if self.x.matches(key) {
                    return Some((key, key_val.val()));
                }
            }
            None
        }
    }
}

impl<'a, KV: Assoc<'a> + 'a> AssocsSuper<'a> for AnchaAssocList<'a, KV> {
    type Key = KV::Key;
    type Val = KV::Val;
    type I<'b, X: 'b + Matches<KV::Key>>
        = AssocListIter<'a, 'b, X, KV>
    where
        'a: 'b;
}

impl<'a, KV: Assoc<'a> + 'a> Assocs<'a> for AnchaAssocList<'a, KV> {
    unsafe fn iter_matches<'c, 'b, X: Matches<KV::Key>>(&'c self, key: &'b X) -> Self::I<'b, X>
    where
        'a: 'b + 'c,
    {
        AssocListIter { x: key, current: &self.0 }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{flagellum::*, vec::*, AnyMatch, CopyAnchize, EqMatch, NoopDeanchize};

    type TestFlagellum<'a> =
        crate::flagellum::AnchaFlagellum<'a, u32, crate::vec::AnchaVec<'a, u8>>;

    #[test]
    fn test_assoc_list_basic() {
        let origin = vec![(1u32, vec![10u8, 20]), (2u32, vec![30u8, 40]), (3u32, vec![50u8, 60])];

        let anchize: AssocListAnchizeFromVec<
            FlagellumAnchizeFromTuple<
                CopyAnchize<u32, ()>,
                VecAnchizeFromVec<CopyAnchize<u8, ()>>,
            >,
        > = AssocListAnchizeFromVec::default();

        let deanchize: AssocListDeanchize<
            FlagellumDeanchize<NoopDeanchize<u32>, VecDeanchize<NoopDeanchize<u8>>>,
        > = AssocListDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let assoc_list = unsafe { &*(buf.as_ptr() as *const AnchaAssocList<TestFlagellum>) };

        // Search for key 2
        let mut iter = unsafe { assoc_list.iter_matches(&EqMatch(&2u32)) };
        let (k, v) = iter.next().unwrap();
        assert_eq!(*k, 2u32);
        assert_eq!(unsafe { v.as_ref() }, &[30u8, 40]);
        assert!(iter.next().is_none());

        // Iterate all with AnyMatch
        let collected: Vec<(u32, Vec<u8>)> = unsafe {
            assoc_list.iter_matches(&AnyMatch).map(|(k, v)| (*k, v.as_ref().to_vec())).collect()
        };

        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0], (1u32, vec![10u8, 20]));
        assert_eq!(collected[1], (2u32, vec![30u8, 40]));
        assert_eq!(collected[2], (3u32, vec![50u8, 60]));
    }

    #[test]
    fn test_assoc_list_empty() {
        let origin: Vec<(u32, Vec<u8>)> = vec![];

        let anchize: AssocListAnchizeFromVec<
            FlagellumAnchizeFromTuple<
                CopyAnchize<u32, ()>,
                VecAnchizeFromVec<CopyAnchize<u8, ()>>,
            >,
        > = AssocListAnchizeFromVec::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        // Empty list should have minimal size
        assert!(sz.0 <= 8);
    }
}
