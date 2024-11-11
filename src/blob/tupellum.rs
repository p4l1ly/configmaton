use std::marker::PhantomData;

use super::{Build, BuildCursor, Reserve};

#[repr(C)]
pub struct Tupellum<'a, A, B> {
    pub a: A,
    _phantom: PhantomData<&'a B>
}

impl<'a, A, B> Tupellum<'a, A, B> {
    pub unsafe fn deserialize
    <
        After,
        FK: FnMut(BuildCursor<A>) -> BuildCursor<B>,
        FV: FnMut(BuildCursor<B>) -> BuildCursor<After>,
    >
    (cur: BuildCursor<Self>, mut fk: FK, mut fv: FV) -> BuildCursor<After>
    {
        let vcur = fk(cur.transmute());
        fv(vcur)
    }
}

impl<'a, A: Build, B: Build> Build for Tupellum<'a, A, B> {
    type Origin = (A::Origin, B::Origin);
}

impl<'a, A: Build, B: Build> Tupellum<'a, A, B> {
    pub fn reserve<
        FK: FnMut(&A::Origin, &mut Reserve),
        FV: FnMut(&B::Origin, &mut Reserve),
    >
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, mut fk: FK, mut fv: FV) -> usize
    {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        fk(&origin.0, sz);
        fv(&origin.1, sz);
        my_addr
    }

    pub unsafe fn serialize
    <
        After,
        FK: FnMut(&A::Origin, BuildCursor<A>) -> BuildCursor<B>,
        FV: FnMut(&B::Origin, BuildCursor<B>) -> BuildCursor<After>,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut fk: FK, mut fv: FV)
    -> BuildCursor<After>
    {
        let vcur = fk(&origin.0, cur.transmute());
        fv(&origin.1, vcur)
    }
}
