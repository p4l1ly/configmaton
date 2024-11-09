use std::marker::PhantomData;

use super::{Build, BuildCursor, Reserve, Shifter};

#[repr(C)]
pub struct ArrMap<'a, const SIZE: usize, V> {
    arr: [*const V; SIZE],
    _phantom: PhantomData<&'a ()>
}

impl<'a, const SIZE: usize, V: Build> Build for ArrMap<'a, SIZE, V> {
    type Origin = [V::Origin; SIZE];
}

impl<'a, const SIZE: usize, V: Build> ArrMap<'a, SIZE, V> {
    pub fn reserve<RV, FV: Fn(&V::Origin, &mut Reserve) -> RV>
    (origin: &<Self as Build>::Origin, sz: &mut Reserve, fv: FV) -> (usize, [RV; SIZE])
    {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        let vaddrs = origin.each_ref().map(|v| fv(v, sz));
        (my_addr, vaddrs)
    }

    pub unsafe fn serialize
    <
        After,
        FV: FnMut(&V::Origin, BuildCursor<V>) -> BuildCursor<V>,
    >
    (origin: &<Self as Build>::Origin, cur: BuildCursor<Self>, mut fv: FV)
    -> BuildCursor<After>
    {
        let mut i = 0;
        let slf = &mut *cur.get_mut();
        let mut vcur = cur.behind::<V>(1);
        origin.each_ref().map(|v| {
            slf.arr[i] = vcur.cur as *const V;
            vcur = fv(v, vcur.clone());
            i += 1;
        });
        let r = vcur.behind(0);
        r
    }
}

impl<'a, const SIZE: usize, V> ArrMap<'a, SIZE, V> {
    pub unsafe fn get(&self, ix: usize) -> &V {
        &*self.arr[ix]
    }

    pub unsafe fn deserialize<
        After,
        FV: FnMut(BuildCursor<V>) -> BuildCursor<V>,
    >
    (cur: BuildCursor<Self>, mut fv: FV) -> BuildCursor<After>
    {
        let shifter = Shifter(cur.buf);
        (*cur.get_mut()).arr.each_mut().map(|v| shifter.shift(v));
        let mut vcur = cur.behind(1);
        for _ in 0..SIZE { vcur = fv(vcur); }
        vcur.behind(0)
    }
}
