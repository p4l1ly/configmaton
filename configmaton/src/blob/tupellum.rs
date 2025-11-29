use std::marker::PhantomData;

use super::{Build, BuildCursor, Reserve};

#[repr(C)]
pub struct Tupellum<'a, A, B> {
    pub a: A,
    _phantom: PhantomData<&'a B>,
}

impl<'a, A, B> Tupellum<'a, A, B> {
    pub unsafe fn deserialize<
        After,
        FK: FnMut(BuildCursor<A>) -> BuildCursor<B>,
        FV: FnMut(BuildCursor<B>) -> BuildCursor<After>,
    >(
        cur: BuildCursor<Self>,
        mut fk: FK,
        mut fv: FV,
    ) -> BuildCursor<After> {
        let vcur = fk(cur.transmute());
        fv(vcur)
    }
}

pub trait TupellumBuild<A, B> {
    fn left(&self) -> &A;
    fn right(&self) -> &B;
}

impl<A, B> TupellumBuild<A, B> for (A, B) {
    fn left(&self) -> &A {
        &self.0
    }
    fn right(&self) -> &B {
        &self.1
    }
}

impl<'a, A, B> TupellumBuild<A, B> for (&'a A, &'a B) {
    fn left(&self) -> &A {
        self.0
    }
    fn right(&self) -> &B {
        self.1
    }
}

impl<'a, A: Build, B: Build> Build for Tupellum<'a, A, B> {
    type Origin = (A::Origin, B::Origin);
}

impl<'a, A, B> Tupellum<'a, A, B> {
    pub fn reserve<
        BldA,
        BldB,
        Bld: TupellumBuild<BldA, BldB>,
        FK: FnMut(&BldA, &mut Reserve),
        FV: FnMut(&BldB, &mut Reserve),
    >(
        origin: &Bld,
        sz: &mut Reserve,
        mut fk: FK,
        mut fv: FV,
    ) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        fk(origin.left(), sz);
        fv(origin.right(), sz);
        my_addr
    }

    pub unsafe fn serialize<
        After,
        BldA,
        BldB,
        Bld: TupellumBuild<BldA, BldB>,
        FK: FnMut(&BldA, BuildCursor<A>) -> BuildCursor<B>,
        FV: FnMut(&BldB, BuildCursor<B>) -> BuildCursor<After>,
    >(
        origin: &Bld,
        cur: BuildCursor<Self>,
        mut fk: FK,
        mut fv: FV,
    ) -> BuildCursor<After> {
        let vcur = fk(origin.left(), cur.transmute());
        fv(origin.right(), vcur)
    }
}
