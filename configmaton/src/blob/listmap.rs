use std::marker::PhantomData;

use super::{
    list::List, AssocsSuper, Build, BuildCursor, Matches, Reserve, Shifter,
    UnsafeIterator, Assocs,
};

#[repr(C)]
pub struct ListMapItem<K, V> {
    val: *const V,
    key: K,
}

impl<K: Build, V: Build> Build for ListMapItem<K, V> {
    type Origin = (K::Origin, V::Origin);
}

type ListMapList<'a, K, V> = List<'a, ListMapItem<K, V>>;

#[repr(C)]
pub struct ListMap<'a, K, V> {
    keys: ListMapList<'a, K, V>,
}

impl<'a, K: Build, V: Build> Build for ListMap<'a, K, V> {
    type Origin = Vec<(K::Origin, V::Origin)>;
}

impl<'a, K: Build, V: Build> ListMap<'a, K, V> {
    pub fn reserve<
        FK: FnMut(&K::Origin, &mut Reserve),
        FV: FnMut(&V::Origin, &mut Reserve),
    >
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, mut fk: FK, mut fv: FV) -> usize {
        let my_addr = <ListMapList<'a, K, V>>::reserve(origin, sz,
            |(k, _), sz| { sz.add::<ListMapItem<K, V>>(1); fk(k, sz) });
        for (_, v) in origin.iter() { fv(v, sz); }
        sz.add::<V>(0);
        my_addr
    }

    pub unsafe fn serialize
    <
        After,
        FK: FnMut(&K::Origin, BuildCursor<K>) -> BuildCursor<ListMapList<'a, K, V>>,
        FV: FnMut(&V::Origin, BuildCursor<V>) -> BuildCursor<V>,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut fk: FK, mut fv: FV)
    -> BuildCursor<After>
    {
        let kcur = cur.behind::<ListMapList<'a, K, V>>(0);
        let mut item_curs = Vec::with_capacity(origin.len());
        let mut vcur = <ListMapList<'a, K, V>>::serialize(origin, kcur, |kv, item_cur| {
            item_curs.push(item_cur.clone());
            fk(&kv.0, item_cur.transmute::<*const V>().behind(1))
        });
        for (kv, item_cur) in origin.iter().zip(item_curs) {
            (*item_cur.get_mut()).val = vcur.cur as *const V;
            vcur = fv(&kv.1, vcur);
        }
        vcur.align()
    }
}

impl<'a, K, V> ListMap<'a, K, V> {
    pub unsafe fn deserialize<
        After,
        FK: FnMut(BuildCursor<K>) -> BuildCursor<ListMapList<'a, K, V>>,
        FV: FnMut(BuildCursor<V>) -> BuildCursor<V>,
    >
    (cur: BuildCursor<Self>, mut fk: FK, mut fv: FV) -> BuildCursor<After>
    {
        let kcur = cur.behind::<ListMapList<'a, K, V>>(0);
        let mut len = 0;
        let shifter = Shifter(cur.buf);
        let mut vcur = ListMapList::deserialize(kcur, |item_cur| {
            len += 1;
            shifter.shift(&mut (*item_cur.get_mut()).val);
            fk(item_cur.transmute::<*const V>().behind(1))
        });
        for _ in 0..len { vcur = fv(vcur); }
        vcur.align()
    }
}

pub struct ListMapIter<'a, 'b, X, K, V> {
    x: &'b X,
    list_iter: *const List<'a, ListMapItem<K, V>>,
    _phantom: PhantomData<&'a K>,
}

impl<'a, 'b, X: Matches<K>, K, V: 'a + 'b> UnsafeIterator for ListMapIter<'a, 'b, X, K, V> {
    type Item = (&'a K, &'a V);

    unsafe fn next(&mut self) -> Option<Self::Item> {
        while let Some(item) = self.list_iter.next() {
            if self.x.matches(&item.key) {
                return Some((&item.key, &*item.val));
            }
        }
        None
    }
}

impl<'a, K: 'a, V: 'a> AssocsSuper<'a> for ListMap<'a, K, V> {
    type Key = K;
    type Val = V;
    type I<'b, X: 'b + Matches<K>> = ListMapIter<'a, 'b, X, K, V> where 'a: 'b;
}

impl<'a, K: 'a, V: 'a> Assocs<'a> for ListMap<'a, K, V> {
    unsafe fn iter_matches<'c, 'b, X: Matches<K>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c
    { ListMapIter { x: key, list_iter: &self.keys, _phantom: PhantomData } }
}
