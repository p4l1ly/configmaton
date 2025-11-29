use std::marker::PhantomData;

use super::{
    Assocs, Build, BuildCursor, EqMatch, IsEmpty, MyHash, Reserve, Shifter, UnsafeIterator,
};

#[repr(C)]
pub struct BlobHashMap<'a, AList> {
    mask: usize,
    arr: *const AList,
    _phantom: PhantomData<&'a AList>,
}

impl<'a, AList: Assocs<'a>> BlobHashMap<'a, AList> {
    pub unsafe fn get(&self, key: &AList::Key) -> Option<&AList::Val>
    where
        AList::Key: Eq + MyHash,
    {
        let ix = key.my_hash() & self.mask;
        let alist_ptr = *(&self.arr as *const *const AList).add(ix);
        if alist_ptr.is_null() {
            return None;
        }
        let alist = &*alist_ptr;
        alist.iter_matches(&EqMatch(key)).next().map(|(_, val)| val)
    }
}

impl<'a, AList> BlobHashMap<'a, AList> {
    pub unsafe fn deserialize<F: FnMut(BuildCursor<AList>) -> BuildCursor<AList>, After>(
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        let mut arr_cur = cur.transmute::<usize>().behind::<*const AList>(1);
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

impl<'a, AList: Build> BlobHashMap<'a, AList>
where
    AList::Origin: IsEmpty,
{
    pub fn reserve<F: FnMut(&AList::Origin, &mut Reserve)>(
        origin: &<Self as Build>::Origin,
        sz: &mut Reserve,
        mut f: F,
    ) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<usize>(1);
        sz.add::<*const AList>(origin.len());
        for alist in origin.iter() {
            if !alist.is_empty() {
                f(alist, sz);
            }
        }
        my_addr
    }

    pub unsafe fn serialize<
        F: FnMut(&AList::Origin, BuildCursor<AList>) -> BuildCursor<AList>,
        After,
    >(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).mask = origin.len() - 1;
        let mut arr_cur = cur.transmute::<usize>().behind::<*const AList>(1);
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
