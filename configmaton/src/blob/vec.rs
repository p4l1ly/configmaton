use std::marker::PhantomData;

use super::{
    align_up, align_up_ptr, get_behind_struct, Build, BuildCursor, Reserve, UnsafeIterator,
};

#[repr(C)]
pub struct BlobVec<'a, X> {
    pub(super) len: usize,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X: Build> Build for BlobVec<'a, X> {
    type Origin = Vec<X::Origin>;
}

pub struct BlobVecIter<'a, X> {
    cur: *const X,
    pub end: *const X,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> BlobVec<'a, X> {
    pub unsafe fn iter(&self) -> BlobVecIter<'a, X> {
        let cur = get_behind_struct::<_, X>(self);
        BlobVecIter { cur, end: cur.add(self.len), _phantom: PhantomData }
    }

    pub unsafe fn behind<After>(&self) -> &'a After {
        let cur = get_behind_struct::<_, X>(self);
        &*align_up_ptr(cur.add(self.len))
    }

    pub unsafe fn get(&self, ix: usize) -> &X {
        assert!(ix < self.len);
        &*get_behind_struct::<_, X>(self).add(ix)
    }

    pub unsafe fn as_ref(&self) -> &'a [X] {
        std::slice::from_raw_parts(get_behind_struct::<_, X>(self), self.len)
    }

    pub unsafe fn deserialize<F: FnMut(&mut X), After>(
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        let mut xcur = cur.behind(1);
        for _ in 0..(*cur.get_mut()).len {
            f(&mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.align()
    }
}

impl<'a, X: Build> BlobVec<'a, X> {
    pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        sz.add::<X>(origin.len());
        my_addr
    }

    pub fn elem_addr(my_addr: usize, ix: usize) -> usize {
        align_up(my_addr + size_of::<Self>(), align_of::<X>()) + size_of::<X>() * ix
    }

    pub unsafe fn serialize<F: FnMut(&X::Origin, &mut X), After>(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).len = origin.len();
        let mut xcur = cur.behind(1);
        for x in origin.iter() {
            f(x, &mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.align()
    }
}

impl<'a, X> UnsafeIterator for BlobVecIter<'a, X> {
    type Item = &'a X;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        if self.cur == self.end {
            return None;
        }
        let ret = self.cur;
        self.cur = self.cur.add(1);
        Some(&*ret)
    }
}
