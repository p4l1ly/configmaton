//! AnchaList: Intrusive linked list with inline node storage.

use super::{Anchize, BuildCursor, Deanchize, Reserve, Shifter};
use std::marker::PhantomData;

/// AnchaList: a linked list node.
///
/// Layout: `[next: *const Self][value: X]`
#[repr(C)]
pub struct AnchaList<'a, X> {
    /// Pointer to next node, or null if last node
    pub next: *const Self,
    /// The value stored in this node
    value: X,
    _phantom: PhantomData<&'a ()>,
}

impl<'a, X> AnchaList<'a, X> {
    /// Get the value.
    pub fn value(&self) -> &X {
        &self.value
    }

    /// Iterate through the list.
    ///
    /// # Safety
    ///
    /// - The list must be properly initialized and deanchized
    pub unsafe fn iter(&self) -> AnchaListIter<'a, X> {
        AnchaListIter { cur: self as *const Self }
    }
}

pub struct AnchaListIter<'a, X> {
    cur: *const AnchaList<'a, X>,
}

impl<'a, X: 'a> Iterator for AnchaListIter<'a, X> {
    type Item = &'a X;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cur.is_null() {
            return None;
        }
        unsafe {
            let item = &*self.cur;
            self.cur = item.next;
            Some(&item.value)
        }
    }
}

// ============================================================================
// Composable anchization for AnchaList
// ============================================================================

/// Anchization strategy for AnchaList with customizable element anchization.
///
/// Like Sediment, List has variable-size elements and uses the full Anchize trait.
pub struct ListAncha<ElemAnchize> {
    pub elem_ancha: ElemAnchize,
}

impl<ElemAnchize> ListAncha<ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        ListAncha { elem_ancha }
    }
}

impl<ElemAnchize> Anchize for ListAncha<ElemAnchize>
where
    ElemAnchize: Anchize + 'static,
{
    type Origin = Vec<ElemAnchize::Origin>;
    type Ancha<'a>
        = AnchaList<'a, ElemAnchize::Ancha<'a>>
    where
        ElemAnchize::Ancha<'a>: 'a;

    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<AnchaList<ElemAnchize::Ancha<'static>>>(0);
        let addr = sz.0;
        for elem_origin in origin.iter() {
            sz.add::<*const AnchaList<ElemAnchize::Ancha<'static>>>(1);
            self.elem_ancha.reserve(elem_origin, sz);
        }
        sz.add::<AnchaList<ElemAnchize::Ancha<'static>>>(0);
        addr
    }

    unsafe fn anchize<'a, After>(
        &self,
        origin: &Self::Origin,
        mut cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        for (i, elem_origin) in origin.iter().enumerate() {
            if i == origin.len() - 1 {
                // Last element
                (*cur.get_mut()).next = std::ptr::null();
                let value_cur: BuildCursor<ElemAnchize::Ancha<'a>> =
                    cur.transmute::<*const AnchaList<ElemAnchize::Ancha<'a>>>().behind(1);
                // anchize returns aligned cursor for next List node
                cur = self
                    .elem_ancha
                    .anchize::<AnchaList<ElemAnchize::Ancha<'a>>>(elem_origin, value_cur);
            } else {
                // Store address of next field as raw pointer to avoid borrow issues
                let next_ptr = &mut (*cur.get_mut()).next as *mut _;
                let value_cur: BuildCursor<ElemAnchize::Ancha<'a>> =
                    cur.transmute::<*const AnchaList<ElemAnchize::Ancha<'a>>>().behind(1);
                // anchize returns aligned cursor for next List node
                cur = self
                    .elem_ancha
                    .anchize::<AnchaList<ElemAnchize::Ancha<'a>>>(elem_origin, value_cur);
                *next_ptr = cur.cur as *const _;
            }
        }
        cur.align()
    }
}

impl<ElemAnchize> Deanchize for ListAncha<ElemAnchize>
where
    ElemAnchize: Deanchize + 'static,
{
    type Ancha<'a>
        = AnchaList<'a, ElemAnchize::Ancha<'a>>
    where
        ElemAnchize::Ancha<'a>: 'a;

    unsafe fn deanchize<'a, After>(
        &self,
        mut cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        let shifter = Shifter(cur.buf);
        loop {
            let node_ptr = cur.get_mut() as *mut AnchaList<ElemAnchize::Ancha<'a>>;
            let is_null = (*node_ptr).next.is_null();

            let value_cur: BuildCursor<ElemAnchize::Ancha<'a>> =
                cur.transmute::<*const AnchaList<ElemAnchize::Ancha<'a>>>().behind(1);
            // deanchize returns aligned cursor for next List node
            cur = self.elem_ancha.deanchize::<AnchaList<ElemAnchize::Ancha<'a>>>(value_cur);

            if is_null {
                return cur.align();
            }

            // Shift the next pointer
            shifter.shift(&mut (*node_ptr).next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ancha::vec::{AnchaVec, VecAncha};
    use crate::ancha::DirectCopy;

    #[test]
    fn test_list_with_vectors() {
        // List of variable-size vectors
        let elem_ancha = VecAncha::new(DirectCopy::<u8>::new());
        let list_ancha = ListAncha::new(elem_ancha);

        let origin = vec![b"hello".to_vec(), b"world".to_vec()];

        let mut sz = Reserve(0);
        list_ancha.reserve(&origin, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            list_ancha.anchize::<()>(&origin, cur.clone());
            list_ancha.deanchize::<()>(cur);
        }

        let list = unsafe { &*(buf.as_ptr() as *const AnchaList<AnchaVec<u8>>) };
        let results: Vec<_> = unsafe { list.iter().map(|v| v.as_ref().to_vec()).collect() };

        assert_eq!(results[0], b"hello");
        assert_eq!(results[1], b"world");
    }
}
