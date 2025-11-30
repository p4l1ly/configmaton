//! Intrusive linked list with inline node storage in the Ancha system.
//!
//! `AnchaList` stores elements as a linked list where each node contains:
//! - A pointer to the next node
//! - The value inline
//!
//! # Memory Layout
//!
//! ```text
//! ┌──────┬───────┐    ┌──────┬───────┐    ┌──────┬───────┐
//! │ next │ value │───▶│ next │ value │───▶│ null │ value │
//! └──────┴───────┘    └──────┴───────┘    └──────┴───────┘
//! ```
//!
//! This is useful when:
//! - The number of elements is small
//! - Elements have different sizes (via trait objects or enums)
//! - Order matters but random access is not needed

use std::marker::PhantomData;

use super::{Anchize, BuildCursor, Deanchize, Reserve, Shifter};

/// An intrusive linked list node.
///
/// Each node contains a value inline and a pointer to the next node.
/// The last node has a null `next` pointer.
#[repr(C)]
pub struct AnchaList<'a, X> {
    /// Pointer to the next list node, or null if this is the last node.
    pub next: *const Self,

    /// The value stored in this node.
    value: X,

    _phantom: PhantomData<&'a ()>,
}

/// Iterator over list elements.
///
/// # Safety
///
/// The list must have been properly anchized and deanchized.
pub struct AnchaListIter<'a, X> {
    current: *const AnchaList<'a, X>,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> Iterator for AnchaListIter<'a, X> {
    type Item = &'a X;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            None
        } else {
            unsafe {
                let item = &*self.current;
                self.current = item.next;
                Some(&item.value)
            }
        }
    }
}

impl<'a, X> AnchaList<'a, X> {
    /// Create an iterator over the list elements.
    ///
    /// # Safety
    ///
    /// The list must have been properly anchized and deanchized.
    pub unsafe fn iter(&self) -> AnchaListIter<'a, X> {
        AnchaListIter { current: self, _phantom: PhantomData }
    }
}

// ============================================================================
// Anchization Strategy
// ============================================================================

/// Strategy for anchizing a `Vec<Origin>` into a linked list.
#[derive(Clone, Copy)]
pub struct ListAnchizeFromVec<'a, ElemAnchize> {
    pub elem_ancha: ElemAnchize,
    _phantom: PhantomData<&'a ElemAnchize>,
}

impl<'a, ElemAnchize> ListAnchizeFromVec<'a, ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        ListAnchizeFromVec { elem_ancha, _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize: Default> Default for ListAnchizeFromVec<'a, ElemAnchize> {
    fn default() -> Self {
        ListAnchizeFromVec { elem_ancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize> Anchize<'a> for ListAnchizeFromVec<'a, ElemAnchize>
where
    ElemAnchize: Anchize<'a>,
{
    type Origin = Vec<ElemAnchize::Origin>;
    type Ancha = AnchaList<'a, ElemAnchize::Ancha>;
    type Context = ElemAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0); // Alignment at the beginning!
        for elem_origin in origin.iter() {
            sz.add::<*const Self::Ancha>(1); // Space for next pointer
            self.elem_ancha.reserve(elem_origin, context, sz);
        }
        sz.add::<Self::Ancha>(0); // Alignment at the end
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let mut cur: BuildCursor<Self::Ancha> = cur.align(); // Alignment at the beginning!

        if origin.is_empty() {
            // Empty list - just return
            return cur.transmute();
        }

        for (i, elem_origin) in origin.iter().enumerate() {
            if i == origin.len() - 1 {
                // Last element - set next to null
                (*cur.get_mut()).next = std::ptr::null();
                cur = self.elem_ancha.anchize(
                    elem_origin,
                    context,
                    cur.transmute::<*const Self::Ancha>().behind(1),
                );
            } else {
                // Not last - we'll fill in the pointer after anchizing the element
                let next_ptr = &mut (*cur.get_mut()).next as *mut *const Self::Ancha;
                cur = self.elem_ancha.anchize(
                    elem_origin,
                    context,
                    cur.transmute::<*const Self::Ancha>().behind(1),
                );

                // CRITICAL: Since elem_ancha doesn't align at the end (ancha design),
                // we must align here before storing as a pointer!
                // This ensures the next list node starts at proper alignment.
                cur = cur.align::<Self::Ancha>();

                // Fill in the next pointer with the ALIGNED cursor position
                *next_ptr = cur.cur as *const Self::Ancha;
            }
        }
        cur.transmute()
    }
}

// ============================================================================
// Deanchization Strategy
// ============================================================================

/// Strategy for deanchizing an `AnchaList`.
#[derive(Clone, Copy)]
pub struct ListDeanchize<'a, ElemDeanchize> {
    pub elem_deancha: ElemDeanchize,
    _phantom: PhantomData<&'a ElemDeanchize>,
}

impl<'a, ElemDeanchize> ListDeanchize<'a, ElemDeanchize> {
    pub fn new(elem_deancha: ElemDeanchize) -> Self {
        ListDeanchize { elem_deancha, _phantom: PhantomData }
    }
}

impl<'a, ElemDeanchize: Default> Default for ListDeanchize<'a, ElemDeanchize> {
    fn default() -> Self {
        ListDeanchize { elem_deancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, ElemDeanchize> Deanchize<'a> for ListDeanchize<'a, ElemDeanchize>
where
    ElemDeanchize: Deanchize<'a>,
{
    type Ancha = AnchaList<'a, ElemDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, mut cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        cur = cur.align(); // Alignment at the beginning!
        let shifter = Shifter(cur.buf);

        loop {
            // Align before accessing each list node
            cur = cur.align::<Self::Ancha>();
            let list_node = &mut *cur.get_mut();
            let is_last = list_node.next.is_null();

            if !is_last {
                // Fix up the next pointer before we lose the reference
                shifter.shift(&mut list_node.next);
            }

            cur = self.elem_deancha.deanchize(cur.transmute::<*const Self::Ancha>().behind(1));

            if is_last {
                // Last node - we're done with final alignment
                return cur.align();
            }
        }
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
    fn test_list_basic() {
        let origin = vec![vec![1u8, 2, 3], vec![4, 5], vec![6, 7, 8, 9]];

        let anchize: ListAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            ListAnchizeFromVec::default();
        let deanchize: ListDeanchize<VecDeanchize<NoopDeanchize<u8>>> = ListDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let list = unsafe { &*(buf.as_ptr() as *const AnchaList<AnchaVec<u8>>) };

        // Verify we can iterate
        let collected: Vec<Vec<u8>> =
            unsafe { list.iter().map(|vec| vec.as_ref().to_vec()).collect() };

        assert_eq!(collected, origin);
    }

    #[test]
    fn test_list_empty() {
        let origin: Vec<Vec<u8>> = vec![];

        let anchize: ListAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            ListAnchizeFromVec::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        // Empty list should just have alignment
        assert!(sz.0 <= 8);
    }

    #[test]
    fn test_list_single_element() {
        let origin = vec![vec![42u8]];

        let anchize: ListAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            ListAnchizeFromVec::default();
        let deanchize: ListDeanchize<VecDeanchize<NoopDeanchize<u8>>> = ListDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let list = unsafe { &*(buf.as_ptr() as *const AnchaList<AnchaVec<u8>>) };

        let collected: Vec<Vec<u8>> =
            unsafe { list.iter().map(|vec| vec.as_ref().to_vec()).collect() };

        assert_eq!(collected, vec![vec![42u8]]);
    }
}
