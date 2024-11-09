use std::marker::PhantomData;

use super::{Build, BuildCursor, Reserve, UnsafeIterator, get_behind_struct};

#[repr(C)]
pub struct Sediment<'a, X> {
    len: usize,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X: Build> Build for Sediment<'a, X> {
    type Origin = Vec<X::Origin>;
}

pub struct SedimentIter<'a, F, X> {
    len: usize,
    f: F,
    cur: *const X,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> Sediment<'a, X> {
    pub unsafe fn iter<F: FnMut(&X) -> *const X>(&self, f: F) -> SedimentIter<'a, F, X> {
        let cur = get_behind_struct::<_, X>(self);
        SedimentIter { len: self.len, f, cur, _phantom: PhantomData }
    }

    pub unsafe fn deserialize<F: FnMut(BuildCursor<X>) -> BuildCursor<X>, After>
    (cur: BuildCursor<Self>, mut f: F) -> BuildCursor<After>
    {
        let mut xcur = cur.behind(1);
        for _ in 0..(*cur.get_mut()).len { xcur = f(xcur); }
        xcur.behind(0)
    }
}

impl<'a, X: Build> Sediment<'a, X> {
    pub fn reserve<R, F: Fn(&X::Origin, &mut Reserve) -> R>
        (origin: &<Self as Build>::Origin, sz: &mut Reserve, f: F) -> (usize, Vec<R>)
    {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        let mut xaddrs = Vec::with_capacity(origin.len());
        for x in origin.iter() { xaddrs.push(f(x, sz)); }
        sz.add::<X>(0);
        (my_addr, xaddrs)
    }

    pub unsafe fn serialize<F: FnMut(&X::Origin, BuildCursor<X>) -> BuildCursor<X>, After>
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut f: F) -> BuildCursor<After>
    {
        (*cur.get_mut()).len = origin.len();
        let mut xcur = cur.behind(1);
        for x in origin.iter() { xcur = f(x, xcur); }
        xcur.behind(0)
    }
}

impl<'a, X, F: FnMut(&X) -> *const X> UnsafeIterator for SedimentIter<'a, F, X> {
    type Item = ();

    unsafe fn next(&mut self) -> Option<Self::Item> {
        if self.len == 0 { return None; }
        self.len -= 1;
        self.cur = (self.f)(&*self.cur);
        Some(())
    }
}
