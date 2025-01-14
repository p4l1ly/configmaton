use std::marker::PhantomData;

use super::{Assoc, Build, BuildCursor, Reserve};

#[repr(C)]
pub struct Flagellum<'a, K, V> {
    key: K,
    val: V,
    _phantom: PhantomData<&'a ()>
}

impl<'a, K, V> Flagellum<'a, K, V> {
    pub unsafe fn deserialize
    <
        After,
        FK: FnMut(&mut K),
        FV: FnMut(BuildCursor<V>) -> BuildCursor<After>,
    >
    (cur: BuildCursor<Self>, mut fk: FK, mut fv: FV) -> BuildCursor<After>
    {
        fk(&mut (*cur.get_mut()).key);
        fv(cur.transmute::<K>().behind(1))
    }
}

impl<'a, K: Build, V: Build> Build for Flagellum<'a, K, V> {
    type Origin = (K::Origin, V::Origin);
}

impl<'a, K: Build, V: Build> Flagellum<'a, K, V> {
    pub fn reserve<FV: Fn(&V::Origin, &mut Reserve)>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, fv: FV) -> usize
    {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<K>(1);
        fv(&origin.1, sz);
        my_addr
    }

    pub unsafe fn serialize
    <
        After,
        FK: FnMut(&K::Origin, &mut K),
        FV: FnMut(&V::Origin, BuildCursor<V>) -> BuildCursor<After>,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut fk: FK, mut fv: FV)
    -> BuildCursor<After>
    {
        fk(&origin.0, &mut (*cur.get_mut()).key);
        fv(&origin.1, cur.behind::<K>(0).behind(1))
    }
}

impl<'a, K: 'a, V: 'a> Assoc<'a> for Flagellum<'a, K, V> {
    type Key = K;
    type Val = V;

    unsafe fn key(&self) -> &'a K { &*(&self.key as *const _) }
    unsafe fn val(&self) -> &'a V { &*(&self.val as *const _) }
}
