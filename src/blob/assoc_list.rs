use super::{
    Build, BuildCursor, Reserve, list::List, Assoc, Assocs, AssocsSuper, Matches, UnsafeIterator,
};

#[repr(C)]
pub struct AssocList<'a, KV>(List<'a, KV>);

impl<'a, KV> AssocList<'a, KV> {
    pub unsafe fn deserialize
    <
        F: FnMut(BuildCursor<KV>) -> BuildCursor<List<'a, KV>>,
        After,
    >
    (cur: BuildCursor<Self>, f: F) -> BuildCursor<After> {
        <List<'a, KV>>::deserialize(cur.behind(0), f)
    }
}

impl<'a, KV: Build> Build for AssocList<'a, KV> {
    type Origin = Vec<KV::Origin>;
}

impl<'a, KV: Build> AssocList<'a, KV> {
    pub fn reserve<R, F: Fn(&KV::Origin, &mut Reserve) -> R>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, f: F) -> (usize, Vec<R>)
    { <List<'a, KV>>::reserve(origin, sz, f) }

    pub unsafe fn serialize
    <
        After,
        F: FnMut(&KV::Origin, BuildCursor<KV>) -> BuildCursor<List<'a, KV>>,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, f: F) -> BuildCursor<After>
    {
        <List<'a, KV>>::serialize(origin, cur.behind(0), f)
    }
}

pub struct AssocListIter<'a, 'b, X, KV> {
    x: &'b X,
    cur: *const List<'a, KV>,
}

impl<'a, 'b, KV: 'b + Assoc<'a>, X: Matches<KV::Key>> UnsafeIterator
for AssocListIter<'a, 'b, X, KV>
{
    type Item = (&'a KV::Key, &'a KV::Val);

    unsafe fn next(&mut self) -> Option<Self::Item> {
        while let Some(key_val) = self.cur.next() {
            let key = key_val.key();
            if self.x.matches(key) { return Some((key, key_val.val())); }
        }
        None
    }
}

impl<'a, KV: Assoc<'a>> AssocsSuper<'a> for AssocList<'a, KV> {
    type Key = KV::Key;
    type Val = KV::Val;
    type I<'b, X: 'b + Matches<KV::Key>> = AssocListIter<'a, 'b, X, KV> where 'a: 'b;
}

impl<'a, KV: Assoc<'a>> Assocs<'a> for AssocList<'a, KV> {
    unsafe fn iter_matches<'c, 'b, X: Matches<KV::Key>>(&'c self, key: &'b X) -> Self::I<'b, X>
        where 'a: 'b + 'c
    { AssocListIter { x: key, cur: &self.0 } }
}
