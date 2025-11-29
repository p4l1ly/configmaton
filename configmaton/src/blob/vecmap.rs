use std::marker::PhantomData;

use super::{
    vec::{BlobVec, BlobVecIter},
    Assocs, AssocsSuper, Build, BuildCursor, Matches, Reserve, Shifter, UnsafeIterator,
};

#[repr(C)]
pub struct VecMapItem<K, V> {
    key: K,
    val: *const V,
}

impl<K: Build, V: Build> Build for VecMapItem<K, V> {
    type Origin = (K::Origin, V::Origin);
}

type VecMapVec<'a, K, V> = BlobVec<'a, VecMapItem<K, V>>;

#[repr(C)]
pub struct VecMap<'a, K, V> {
    keys: VecMapVec<'a, K, V>,
}

impl<'a, K: Build, V: Build> Build for VecMap<'a, K, V> {
    type Origin = Vec<(K::Origin, V::Origin)>;
}

impl<'a, K: Build, V: Build> VecMap<'a, K, V> {
    pub fn reserve<FV: FnMut(&V::Origin, &mut Reserve)>(
        origin: &<Self as Build>::Origin,
        sz: &mut Reserve,
        mut fv: FV,
    ) -> usize {
        let my_addr = <VecMapVec<'a, K, V>>::reserve(origin, sz);
        for (_, v) in origin.iter() {
            fv(v, sz);
        }
        sz.add::<V>(0);
        my_addr
    }

    pub fn key_addr(my_addr: usize, ix: usize) -> usize {
        <VecMapVec<'a, K, V>>::elem_addr(my_addr, ix)
    }

    pub unsafe fn serialize<
        After,
        FK: FnMut(&K::Origin, &mut K),
        FV: FnMut(&V::Origin, BuildCursor<V>) -> BuildCursor<V>,
    >(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        mut fk: FK,
        mut fv: FV,
    ) -> BuildCursor<After> {
        let kcur = cur.behind::<VecMapVec<'a, K, V>>(0);
        let item_cur = kcur.behind::<VecMapItem<K, V>>(1);
        let mut vcur = item_cur.behind::<V>(origin.len());
        <VecMapVec<'a, K, V>>::serialize::<_, V>(origin, kcur, |kv, bk| {
            fk(&kv.0, &mut bk.key);
            bk.val = vcur.cur as *const V;
            vcur = fv(&kv.1, vcur.clone());
        });
        vcur.align()
    }
}

impl<'a, K, V> VecMap<'a, K, V> {
    pub unsafe fn deserialize<
        After,
        FK: FnMut(&mut K),
        FV: FnMut(BuildCursor<V>) -> BuildCursor<V>,
    >(
        cur: BuildCursor<Self>,
        mut fk: FK,
        mut fv: FV,
    ) -> BuildCursor<After> {
        let kcur = cur.behind::<VecMapVec<'a, K, V>>(0);
        let len = (*kcur.get_mut()).len;
        let shifter = Shifter(cur.buf);
        let mut vcur = BlobVec::deserialize(kcur, |kv| {
            fk(&mut kv.key);
            shifter.shift(&mut kv.val);
        });
        for _ in 0..len {
            vcur = fv(vcur);
        }
        vcur.align()
    }
}

pub struct VecMapIter<'a, 'b, X, K, V> {
    pub(super) x: &'b X,
    vec_iter: BlobVecIter<'a, VecMapItem<K, V>>,
    _phantom: PhantomData<&'a K>,
}

impl<'a, 'b, X: Matches<K>, K, V: 'b> UnsafeIterator for VecMapIter<'a, 'b, X, K, V> {
    type Item = (&'a K, &'a V);

    unsafe fn next(&mut self) -> Option<Self::Item> {
        while let Some(VecMapItem { key, val }) = self.vec_iter.next() {
            if self.x.matches(key) {
                return Some((&key, &**val));
            }
        }
        None
    }
}

impl<'a, K: 'a, V: 'a> AssocsSuper<'a> for VecMap<'a, K, V> {
    type Key = K;
    type Val = V;
    type I<'b, X: 'b + Matches<K>>
        = VecMapIter<'a, 'b, X, K, V>
    where
        'a: 'b;
}

impl<'a, K: 'a, V: 'a> Assocs<'a> for VecMap<'a, K, V> {
    unsafe fn iter_matches<'c, 'b, X: Matches<K>>(&'c self, key: &'b X) -> Self::I<'b, X>
    where
        'a: 'b + 'c,
    {
        VecMapIter { x: key, vec_iter: self.keys.iter(), _phantom: PhantomData }
    }
}
