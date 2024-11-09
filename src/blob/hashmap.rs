use std::marker::PhantomData;

use super::{
    Assocs, UnsafeIterator, Build, BuildCursor, IsEmpty, Reserve, Shifter, MyHash,
    get_behind_struct, EqMatch
};

#[repr(C)]
pub struct BlobHashMap<'a, AList> {
    mask: usize,
    _phantom: PhantomData<&'a AList>,
}

impl<'a, AList: Assocs<'a>> BlobHashMap<'a, AList> {
    pub unsafe fn get(&self, key: &AList::Key) -> Option<&AList::Val>
        where AList::Key: Eq + MyHash
    {
        let ix = key.my_hash() & self.mask;
        let alist_ptr = *get_behind_struct::<_, *const AList>(self).add(ix);
        if alist_ptr.is_null() {
            return None;
        }
        let alist = &*alist_ptr;
        alist.iter_matches(&EqMatch(key)).next().map(|(_, val)| val)
    }
}

impl<'a, AList> BlobHashMap<'a, AList> {
    pub unsafe fn deserialize
    <
        F: FnMut(BuildCursor<AList>) -> BuildCursor<AList>,
        After,
    >
    (cur: BuildCursor<Self>, mut f: F) -> BuildCursor<After> {
        let mut arr_cur = cur.behind::<*const AList>(1);
        let hashmap_cap = (*cur.get_mut()).mask + 1;
        let mut alist_cur = arr_cur.behind::<AList>(hashmap_cap);
        for _ in 0..(*cur.get_mut()).mask + 1 {
            let arr_ptr = arr_cur.get_mut();
            if !(*arr_ptr).is_null() {
                Shifter(cur.buf).shift(&mut *arr_ptr);
                alist_cur = f(alist_cur);
            }
            arr_cur.inc();
        }
        alist_cur.align()
    }
}

impl<'a, AList: Build> Build for BlobHashMap<'a, AList> {
    type Origin = Vec<AList::Origin>;
}

impl<'a, AList: Build> BlobHashMap<'a, AList> where AList::Origin: IsEmpty {
    pub fn reserve<R, F: Fn(&AList::Origin, &mut Reserve) -> R>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, f: F) -> (usize, Vec<R>) {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        sz.add::<*const AList>(origin.len());
        let mut results = Vec::with_capacity(origin.len());
        for alist in origin.iter() {
            if !alist.is_empty() {
                results.push(f(alist, sz));
            }
        }
        (my_addr, results)
    }

    pub unsafe fn serialize
    <
        F: FnMut(&AList::Origin, BuildCursor<AList>) -> BuildCursor<AList>,
        After,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut f: F) -> BuildCursor<After>
    {
        (*cur.get_mut()).mask = origin.len() - 1;
        let mut arr_cur = cur.behind::<*const AList>(1);
        let mut alist_cur = arr_cur.behind::<AList>(origin.len());
        for alist_origin in origin.iter() {
            if alist_origin.is_empty() {
                *arr_cur.get_mut() = std::ptr::null();
            } else {
                *arr_cur.get_mut() = alist_cur.cur as *const AList;
                alist_cur = f(alist_origin, alist_cur);
            }
            arr_cur.inc()
        }
        alist_cur.align()
    }
}
