use std::marker::PhantomData;

use super::{UnsafeIterator, Build, BuildCursor, Reserve, Shifter, get_behind_struct};

#[repr(C)]
pub struct List<'a, X> {
    next: *const List<'a, X>,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X> UnsafeIterator for *const List<'a, X> {
    type Item = &'a X;
    unsafe fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.is_null() {
                return None;
            }
            let item = *self;
            *self = (*item).next;
            return Some(&*get_behind_struct::<_, X>(item));
        }
    }
}

impl<'a, X> List<'a, X> {
    pub unsafe fn deserialize
    <F: FnMut(BuildCursor<X>) -> BuildCursor<Self>, After>
    (mut cur: BuildCursor<Self>, mut f: F) -> BuildCursor<After>
    {
        loop {
            let alist = &mut *cur.get_mut();
            cur = f(cur.behind(1));
            if alist.next.is_null() { return cur.behind(0); }
            Shifter(cur.buf).shift(&mut alist.next);
        }
    }
}

impl<'a, X: Build> Build for List<'a, X> {
    type Origin = Vec<X::Origin>;
}

impl<'a, X: Build> List<'a, X> {
    pub fn reserve<R, F: Fn(&X::Origin, &mut Reserve) -> R>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, f: F) -> (usize, Vec<R>)
    {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        let mut results = Vec::with_capacity(origin.len());
        for x in origin.iter() { sz.add::<Self>(1); results.push(f(x, sz)); }
        sz.add::<Self>(0);
        (my_addr, results)
    }

    pub unsafe fn serialize
    <
        After,
        F: FnMut(&X::Origin, BuildCursor<X>) -> BuildCursor<Self>,
    >
    (origin: &<Self as Build>::Origin, mut cur: BuildCursor<Self>, mut f: F) -> BuildCursor<After>
    {
        for (i, x) in origin.iter().enumerate() {
            if i == origin.len() - 1 {
                (*cur.get_mut()).next = std::ptr::null();
                cur = f(x, cur.behind(1));
            } else {
                let next = &mut (*cur.get_mut()).next;
                cur = f(x, cur.behind(1));
                *next = cur.cur as *const Self;
            }
        }
        cur.behind(0)
    }
}
