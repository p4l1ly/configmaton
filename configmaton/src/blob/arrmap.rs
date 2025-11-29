use std::marker::PhantomData;

use super::{Build, BuildCursor, Reserve, Shifter};

#[repr(C)]
pub struct ArrMap<'a, const SIZE: usize, V> {
    arr: [*const V; SIZE],
    _phantom: PhantomData<&'a ()>,
}

impl<'a, const SIZE: usize, V: Build> Build for ArrMap<'a, SIZE, V> {
    type Origin = [V::Origin; SIZE];
}

impl<'a, const SIZE: usize, V: Build> ArrMap<'a, SIZE, V> {
    pub fn reserve<FV: FnMut(&V::Origin, &mut Reserve)>(
        origin: &<Self as Build>::Origin,
        sz: &mut Reserve,
        mut fv: FV,
    ) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        for v in origin.iter() {
            fv(v, sz);
        }
        my_addr
    }

    pub unsafe fn serialize<After, FV: FnMut(&V::Origin, BuildCursor<V>) -> BuildCursor<V>>(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        mut fv: FV,
    ) -> BuildCursor<After> {
        let slf = &mut *cur.get_mut();
        let mut vcur = cur.behind::<V>(1);
        for (i, v) in origin.each_ref().iter().enumerate() {
            slf.arr[i] = vcur.cur as *const V;
            vcur = fv(v, vcur.clone());
        }
        vcur.align()
    }
}

impl<'a, const SIZE: usize, V> ArrMap<'a, SIZE, V> {
    pub unsafe fn get(&self, ix: usize) -> &V {
        &*self.arr[ix]
    }

    pub unsafe fn deserialize<After, FV: FnMut(BuildCursor<V>) -> BuildCursor<V>>(
        cur: BuildCursor<Self>,
        mut fv: FV,
    ) -> BuildCursor<After> {
        let shifter = Shifter(cur.buf);
        for v in (*cur.get_mut()).arr.each_mut().iter_mut() {
            shifter.shift(v);
        }
        let mut vcur = cur.behind(1);
        for _ in 0..SIZE {
            vcur = fv(vcur);
        }
        vcur.align()
    }
}
