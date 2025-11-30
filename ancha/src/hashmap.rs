//! Hash map implementation in the Ancha system.
//!
//! `AnchaHashMap<AList>` stores a hash table with:
//! - A mask (for calculating bucket index: `hash & mask`)
//! - An array of pointers to association lists
//! - The association lists themselves (inline)
//!
//! # Memory Layout
//!
//! ```text
//! [mask][arr: *AList, *AList, ...][AList 0][AList 1]...
//! ```
//!
//! Some array pointers may be null if that bucket is empty.

use std::marker::PhantomData;

use super::{Anchize, Assocs, BuildCursor, Deanchize, EqMatch, IsEmpty, MyHash, Reserve, Shifter};

/// Hash map structure.
#[repr(C)]
pub struct AnchaHashMap<'a, AList> {
    pub mask: usize,
    pub arr: *const AList,
    _phantom: PhantomData<&'a AList>,
}

impl<'a, AList: Assocs<'a>> AnchaHashMap<'a, AList> {
    pub unsafe fn get(&self, key: &AList::Key) -> Option<&AList::Val>
    where
        AList::Key: Eq + MyHash,
    {
        let ix = key.my_hash() & self.mask;
        let alist_ptr = *(&self.arr as *const *const AList).add(ix);
        if alist_ptr.is_null() {
            return None;
        }
        let alist = &*alist_ptr;
        alist.iter_matches(&EqMatch(key)).next().map(|(_, val)| val)
    }
}

// ============================================================================
// Anchization Strategy
// ============================================================================

/// Strategy for anchizing a Vec<AListOrigin> into a HashMap.
///
/// The Vec represents the buckets of the hash table (length must be a power of 2).
pub struct HashMapAnchizeFromVec<'a, AListAnchize> {
    pub alist_ancha: AListAnchize,
    _phantom: PhantomData<&'a AListAnchize>,
}

impl<'a, AListAnchize> HashMapAnchizeFromVec<'a, AListAnchize> {
    pub fn new(alist_ancha: AListAnchize) -> Self {
        HashMapAnchizeFromVec { alist_ancha, _phantom: PhantomData }
    }
}

impl<'a, AListAnchize: Default> Default for HashMapAnchizeFromVec<'a, AListAnchize> {
    fn default() -> Self {
        HashMapAnchizeFromVec { alist_ancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, AListAnchize> Anchize<'a> for HashMapAnchizeFromVec<'a, AListAnchize>
where
    AListAnchize: Anchize<'a>,
    AListAnchize::Origin: IsEmpty,
{
    type Origin = Vec<AListAnchize::Origin>;
    type Ancha = AnchaHashMap<'a, AListAnchize::Ancha>;
    type Context = AListAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0); // Initial alignment
        sz.add::<usize>(1); // mask
        sz.add::<*const AListAnchize::Ancha>(origin.len()); // array of pointers

        // Reserve space for each non-empty association list
        for alist_origin in origin.iter() {
            if !alist_origin.is_empty() {
                self.alist_ancha.reserve(alist_origin, context, sz);
            }
        }
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur = cur.align::<Self::Ancha>();

        // Set mask (capacity - 1, for fast modulo via bitwise AND)
        (*cur.get_mut()).mask = origin.len() - 1;

        // Position cursors
        let mut arr_cur = cur.transmute::<usize>().behind::<*const AListAnchize::Ancha>(1);
        let mut alist_cur: BuildCursor<AListAnchize::Ancha> =
            arr_cur.behind::<AListAnchize::Ancha>(origin.len());

        // Serialize each bucket
        for alist_origin in origin.iter() {
            if alist_origin.is_empty() {
                *arr_cur.get_mut() = std::ptr::null();
            } else {
                // Align before storing the pointer (anchize will also align at the beginning)
                alist_cur = alist_cur.align::<AListAnchize::Ancha>();
                // Store pointer to where this alist will be
                *arr_cur.get_mut() = alist_cur.cur as *const AListAnchize::Ancha;
                // Anchize the association list
                alist_cur = self.alist_ancha.anchize(alist_origin, context, alist_cur);
            }
            arr_cur.inc();
        }

        alist_cur.transmute()
    }
}

// ============================================================================
// Deanchization Strategy
// ============================================================================

/// Strategy for deanchizing a HashMap.
#[derive(Clone, Copy)]
pub struct HashMapDeanchize<'a, AListDeanchize> {
    pub alist_deancha: AListDeanchize,
    _phantom: PhantomData<&'a AListDeanchize>,
}

impl<'a, AListDeanchize> HashMapDeanchize<'a, AListDeanchize> {
    pub fn new(alist_deancha: AListDeanchize) -> Self {
        HashMapDeanchize { alist_deancha, _phantom: PhantomData }
    }
}

impl<'a, AListDeanchize: Default> Default for HashMapDeanchize<'a, AListDeanchize> {
    fn default() -> Self {
        HashMapDeanchize { alist_deancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, AListDeanchize> Deanchize<'a> for HashMapDeanchize<'a, AListDeanchize>
where
    AListDeanchize: Deanchize<'a>,
{
    type Ancha = AnchaHashMap<'a, AListDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let mut arr_cur = cur.transmute::<usize>().behind::<*const AListDeanchize::Ancha>(1);
        let hashmap_cap = (*cur.get_mut()).mask + 1;
        let mut alist_cur = arr_cur.behind::<AListDeanchize::Ancha>(hashmap_cap);

        // Fix up pointers and deanchize each non-null association list
        for _ in 0..hashmap_cap {
            let arr_ptr = arr_cur.get_mut();
            if !(*arr_ptr).is_null() {
                Shifter(cur.buf).shift(&mut *arr_ptr);
                alist_cur = self.alist_deancha.deanchize(alist_cur);
            }
            arr_cur.inc();
        }

        alist_cur.transmute()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{assoc_list::*, flagellum::*, CopyAnchize, NoopDeanchize};

    // Implement MyHash for u32 for testing
    impl MyHash for u32 {
        fn my_hash(&self) -> usize {
            *self as usize
        }
    }

    #[test]
    fn test_hashmap_basic() {
        // Create a hash table with 4 buckets
        // Bucket 0: [(0, 100), (4, 400)]
        // Bucket 1: [(1, 100)]
        // Bucket 2: []
        // Bucket 3: [(3, 300)]

        use crate::vec::*;

        type FlagOrigin = (u32, Vec<u8>);
        type AListOrigin = Vec<FlagOrigin>;

        let origin: Vec<AListOrigin> = vec![
            vec![(0u32, vec![100u8]), (4u32, vec![104u8])], // bucket 0
            vec![(1u32, vec![101u8])],                      // bucket 1
            vec![],                                         // bucket 2
            vec![(3u32, vec![103u8])],                      // bucket 3
        ];

        let anchize: HashMapAnchizeFromVec<
            AssocListAnchizeFromVec<
                FlagellumAnchizeFromTuple<
                    CopyAnchize<u32, ()>,
                    VecAnchizeFromVec<CopyAnchize<u8, ()>>,
                >,
            >,
        > = HashMapAnchizeFromVec::default();
        let deanchize: HashMapDeanchize<
            AssocListDeanchize<
                FlagellumDeanchize<NoopDeanchize<u32>, VecDeanchize<NoopDeanchize<u8>>>,
            >,
        > = HashMapDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        type AnchaFlag<'a> = AnchaFlagellum<'a, u32, AnchaVec<'a, u8>>;
        type AnchaAList<'a> = AnchaAssocList<'a, AnchaFlag<'a>>;
        let hashmap = unsafe { &*(buf.as_ptr() as *const AnchaHashMap<AnchaAList>) };

        // Test lookups
        assert_eq!(unsafe { hashmap.get(&0u32).map(|v| v.as_ref()[0]) }, Some(100u8));
        assert_eq!(unsafe { hashmap.get(&1u32).map(|v| v.as_ref()[0]) }, Some(101u8));
        assert_eq!(unsafe { hashmap.get(&3u32).map(|v| v.as_ref()[0]) }, Some(103u8));
        assert_eq!(unsafe { hashmap.get(&4u32).map(|v| v.as_ref()[0]) }, Some(104u8));
        assert!(unsafe { hashmap.get(&2u32) }.is_none()); // Empty bucket
        assert!(unsafe { hashmap.get(&5u32) }.is_none()); // Not in table
    }

    #[test]
    fn test_hashmap_empty() {
        use crate::vec::*;

        type AListOrigin = Vec<(u32, Vec<u8>)>;
        let origin: Vec<AListOrigin> = vec![vec![], vec![], vec![], vec![]];

        let anchize: HashMapAnchizeFromVec<
            AssocListAnchizeFromVec<
                FlagellumAnchizeFromTuple<
                    CopyAnchize<u32, ()>,
                    VecAnchizeFromVec<CopyAnchize<u8, ()>>,
                >,
            >,
        > = HashMapAnchizeFromVec::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        // Should just have mask + pointer array
        assert!(sz.0 <= 64);
    }
}
