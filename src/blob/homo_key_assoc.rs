use std::marker::PhantomData;

use super::{get_behind_struct, Assoc, Build, BuildCursor, Reserve};

#[repr(C)]
pub struct HomoKeyAssoc<'a, K, V> {
    key: K,
    _phantom: PhantomData<&'a V>
}

impl<'a, K, V> HomoKeyAssoc<'a, K, V> {
    pub unsafe fn deserialize
    <
        After,
        FK: FnMut(&mut K),
        FV: FnMut(BuildCursor<V>) -> BuildCursor<After>,
    >
    (cur: BuildCursor<Self>, mut fk: FK, mut fv: FV) -> BuildCursor<After>
    {
        fk(&mut (*cur.get_mut()).key);
        fv(cur.behind(1))
    }
}

impl<'a, K: Build, V: Build> Build for HomoKeyAssoc<'a, K, V> {
    type Origin = (K::Origin, V::Origin);
}

impl<'a, K: Build, V: Build> HomoKeyAssoc<'a, K, V> {
    pub fn reserve<RV, FV: Fn(&V::Origin, &mut Reserve) -> RV>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, fv: FV) -> (usize, RV)
    {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        let rv = fv(&origin.1, sz);
        (my_addr, rv)
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
        fv(&origin.1, cur.behind(1))
    }
}

impl<'a, K: 'a, V: 'a> Assoc<'a> for HomoKeyAssoc<'a, K, V> {
    type Key = K;
    type Val = V;

    unsafe fn key(&self) -> &'a K { &*(&self.key as *const _) }
    unsafe fn val(&self) -> &'a V { &*get_behind_struct(self) }
}
