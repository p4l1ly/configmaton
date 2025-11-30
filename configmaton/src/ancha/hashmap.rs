//! AnchaHashMap: Hash table with separate chaining.
//!
//! Memory layout: `[mask: usize][arr: [*const AList]][alists...]`

use super::{Anchize, BuildCursor, Deanchize, Reserve, Shifter};
use std::marker::PhantomData;

/// AnchaHashMap: hash table structure
#[repr(C)]
pub struct AnchaHashMap<'a, AList> {
    pub mask: usize,
    pub arr: *const AList,
    _phantom: PhantomData<&'a AList>,
}

// Note: get() and other usage methods would be added here when needed,
// following the exact pattern from BlobHashMap

// ============================================================================
// Composable anchization for AnchaHashMap
// ============================================================================

/// Anchization strategy for AnchaHashMap.
pub struct HashMapAncha<AListAnchize> {
    pub alist_ancha: AListAnchize,
}

impl<AListAnchize> HashMapAncha<AListAnchize> {
    pub fn new(alist_ancha: AListAnchize) -> Self {
        HashMapAncha { alist_ancha }
    }
}

/// Trait for checking if origin is empty (matches blob::IsEmpty)
pub trait IsEmpty {
    fn is_empty(&self) -> bool;
}

impl<T> IsEmpty for Vec<T> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<AListAnchize> Anchize for HashMapAncha<AListAnchize>
where
    AListAnchize: Anchize + 'static,
    AListAnchize::Origin: IsEmpty,
{
    type Origin = Vec<AListAnchize::Origin>;
    type Ancha<'a>
        = AnchaHashMap<'a, AListAnchize::Ancha<'a>>
    where
        Self: 'a;

    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<AnchaHashMap<AListAnchize::Ancha<'static>>>(0);
        let addr = sz.0;
        sz.add::<usize>(1);
        sz.add::<*const AListAnchize::Ancha<'static>>(origin.len());
        for alist_origin in origin.iter() {
            if !alist_origin.is_empty() {
                self.alist_ancha.reserve(alist_origin, sz);
            }
        }
        addr
    }

    unsafe fn anchize<'a, After>(
        &self,
        origin: &Self::Origin,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).mask = origin.len() - 1;
        let mut arr_cur = cur.transmute::<usize>().behind::<*const AListAnchize::Ancha<'a>>(1);
        let mut alist_cur = arr_cur.behind::<AListAnchize::Ancha<'a>>(origin.len());

        for alist_origin in origin.iter() {
            if alist_origin.is_empty() {
                *arr_cur.get_mut() = std::ptr::null();
            } else {
                *arr_cur.get_mut() = alist_cur.cur as *const _;
                alist_cur =
                    self.alist_ancha.anchize::<AListAnchize::Ancha<'a>>(alist_origin, alist_cur);
            }
            arr_cur.inc();
        }

        alist_cur.align()
    }
}

impl<AListAnchize> Deanchize for HashMapAncha<AListAnchize>
where
    AListAnchize: Deanchize + 'static,
{
    type Ancha<'a>
        = AnchaHashMap<'a, AListAnchize::Ancha<'a>>
    where
        Self: 'a;

    unsafe fn deanchize<'a, After>(
        &self,
        cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        let mut arr_cur = cur.transmute::<usize>().behind::<*const AListAnchize::Ancha<'a>>(1);
        let hashmap_cap = (*cur.get_mut()).mask + 1;
        let mut alist_cur = arr_cur.behind::<AListAnchize::Ancha<'a>>(hashmap_cap);

        for _ in 0..hashmap_cap {
            let arr_ptr = arr_cur.get_mut();
            if !(*arr_ptr).is_null() {
                Shifter(cur.buf).shift(&mut *arr_ptr);
                alist_cur = self.alist_ancha.deanchize::<AListAnchize::Ancha<'a>>(alist_cur);
            }
            arr_cur.inc();
        }

        alist_cur.align()
    }
}
