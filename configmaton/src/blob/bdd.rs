use std::marker::PhantomData;

use hashbrown::HashMap;

use super::{get_behind_struct, Build, BuildCursor, Reserve, Shifter};

pub enum BddOrigin<Var, Leaf> {
    Leaf(Leaf),
    NodeNoOwned { var: Var, pos: *const BddOrigin<Var, Leaf>, neg: *const BddOrigin<Var, Leaf> },
    NodePosOwned { var: Var, pos: Box<BddOrigin<Var, Leaf>>, neg: *const BddOrigin<Var, Leaf> },
    NodeNegOwned { var: Var, pos: *const BddOrigin<Var, Leaf>, neg: Box<BddOrigin<Var, Leaf>> },
    NodeBothOwned { var: Var, pos: Box<BddOrigin<Var, Leaf>>, neg: Box<BddOrigin<Var, Leaf>> },
}

impl<Var, Leaf> BddOrigin<Var, Leaf> {
    pub fn get_var(&self) -> &Var {
        match self {
            BddOrigin::Leaf(_) => panic!("Leaf has no variable"),
            BddOrigin::NodeNoOwned { var, .. } => var,
            BddOrigin::NodePosOwned { var, .. } => var,
            BddOrigin::NodeNegOwned { var, .. } => var,
            BddOrigin::NodeBothOwned { var, .. } => var,
        }
    }

    pub unsafe fn get_pos(&self) -> &BddOrigin<Var, Leaf> {
        match self {
            BddOrigin::Leaf(_) => panic!("Leaf has no positive child"),
            BddOrigin::NodeNoOwned { pos, .. } => &**pos,
            BddOrigin::NodePosOwned { pos, .. } => pos,
            BddOrigin::NodeNegOwned { pos, .. } => &**pos,
            BddOrigin::NodeBothOwned { pos, .. } => pos,
        }
    }

    pub unsafe fn get_neg(&self) -> &BddOrigin<Var, Leaf> {
        match self {
            BddOrigin::Leaf(_) => panic!("Leaf has no negative child"),
            BddOrigin::NodeNoOwned { neg, .. } => &**neg,
            BddOrigin::NodePosOwned { neg, .. } => &**neg,
            BddOrigin::NodeNegOwned { neg, .. } => neg,
            BddOrigin::NodeBothOwned { neg, .. } => neg,
        }
    }

    pub fn owns_pos(&self) -> bool {
        match self {
            BddOrigin::Leaf(_) => false,
            BddOrigin::NodeNoOwned { .. } => false,
            BddOrigin::NodePosOwned { .. } => true,
            BddOrigin::NodeNegOwned { .. } => false,
            BddOrigin::NodeBothOwned { .. } => true,
        }
    }

    pub fn owns_neg(&self) -> bool {
        match self {
            BddOrigin::Leaf(_) => false,
            BddOrigin::NodeNoOwned { .. } => false,
            BddOrigin::NodePosOwned { .. } => false,
            BddOrigin::NodeNegOwned { .. } => true,
            BddOrigin::NodeBothOwned { .. } => true,
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub enum BddType {
    Leaf,
    NodeNoOwned,
    NodePosOwned,
    NodeNegOwned,
    NodeBothOwned,
}

#[repr(C)]
pub struct Bdd<'a, Var, Leaf> {
    type_: BddType,
    _phantom: PhantomData<&'a (Var, Leaf)>,
}

#[repr(C)]
pub struct NodeNoOwned<'a, Var, Leaf> {
    var: Var,
    pos: *const Bdd<'a, Var, Leaf>,
    neg: *const Bdd<'a, Var, Leaf>,
}

#[repr(C)]
pub struct NodeOwned<'a, Var, Leaf> {
    var: Var,
    unowned: *const Bdd<'a, Var, Leaf>,
    owned: Bdd<'a, Var, Leaf>,
}

impl<'a, Var, Leaf> Bdd<'a, Var, Leaf> {
    pub unsafe fn evaluate<F: FnMut(&Var) -> bool>(&self, mut f: F) -> &'a Leaf {
        let mut cur = self;
        loop {
            match cur.type_ {
                BddType::Leaf => {
                    return &*get_behind_struct(cur);
                }
                BddType::NodeNoOwned => {
                    let node: &NodeNoOwned<Var, Leaf> = &*get_behind_struct(cur);
                    cur = if f(&node.var) { &*node.pos } else { &*node.neg };
                }
                BddType::NodePosOwned => {
                    let node: &NodeOwned<Var, Leaf> = &*get_behind_struct(cur);
                    cur = if f(&node.var) { &node.owned } else { &*node.unowned };
                }
                BddType::NodeNegOwned => {
                    let node: &NodeOwned<Var, Leaf> = &*get_behind_struct(cur);
                    cur = if f(&node.var) { &*node.unowned } else { &node.owned };
                }
                BddType::NodeBothOwned => {
                    let node: &NodeOwned<Var, Leaf> = &*get_behind_struct(cur);
                    cur = if f(&node.var) { &node.owned } else { &*node.unowned };
                }
            }
        }
    }

    pub unsafe fn deserialize<
        After,
        FLeaf: FnMut(BuildCursor<Leaf>) -> BuildCursor<Self>,
        FVar: FnMut(&mut Var),
    >(
        mut cur: BuildCursor<Self>,
        mut f_leaf: FLeaf,
        mut f_var: FVar,
    ) -> BuildCursor<After> {
        let shifter = Shifter(cur.buf);
        let mut todo_count: usize = 1;
        while todo_count > 0 {
            let bdd = &mut *cur.get_mut();
            match bdd.type_ {
                BddType::Leaf => {
                    cur = f_leaf(cur.behind(1));
                }
                BddType::NodeNoOwned => {
                    let node_cur = cur.behind(1);
                    let node: &mut NodeNoOwned<Var, Leaf> = &mut *node_cur.get_mut();
                    f_var(&mut node.var);
                    shifter.shift(&mut node.pos);
                    shifter.shift(&mut node.neg);
                    cur = node_cur.behind(1);
                }
                BddType::NodeBothOwned => {
                    let node: &mut NodeOwned<Var, Leaf> = &mut *cur.behind(1).get_mut();
                    f_var(&mut node.var);
                    shifter.shift(&mut node.unowned);
                    todo_count += 2;
                    cur = cur.goto(&mut node.owned);
                }
                _ => {
                    let node: &mut NodeOwned<Var, Leaf> = &mut *cur.behind(1).get_mut();
                    f_var(&mut node.var);
                    shifter.shift(&mut node.unowned);
                    todo_count += 1;
                    cur = cur.goto(&mut node.owned);
                }
            }
            todo_count -= 1;
        }
        cur.align()
    }
}

impl<'a, Var: Build, Leaf: Build> Build for Bdd<'a, Var, Leaf> {
    type Origin = BddOrigin<Var::Origin, Leaf::Origin>;
}

impl<'a, Var: Build, Leaf: Build> Bdd<'a, Var, Leaf> {
    pub fn reserve<FLeaf: FnMut(&Leaf::Origin, &mut Reserve)>(
        origin: &<Self as Build>::Origin,
        sz: &mut Reserve,
        mut fleaf: FLeaf,
    ) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        let mut todo: Vec<&BddOrigin<Var::Origin, Leaf::Origin>> = vec![origin];
        while let Some(origin) = todo.pop() {
            sz.add::<Self>(1);
            match origin {
                BddOrigin::Leaf(leaf) => {
                    fleaf(leaf, sz);
                }
                BddOrigin::NodeNoOwned { .. } => {
                    sz.add::<NodeNoOwned<Var::Origin, Leaf::Origin>>(1);
                }
                BddOrigin::NodePosOwned { pos, .. } => {
                    sz.add::<NodeOwned<Var::Origin, Leaf::Origin>>(0);
                    sz.add::<Var>(1);
                    sz.add::<*const Self>(1);
                    todo.push(&pos);
                }
                BddOrigin::NodeNegOwned { neg, .. } => {
                    sz.add::<NodeOwned<Var::Origin, Leaf::Origin>>(0);
                    sz.add::<Var>(1);
                    sz.add::<*const Self>(1);
                    todo.push(&neg);
                }
                BddOrigin::NodeBothOwned { pos, neg, .. } => {
                    sz.add::<NodeOwned<Var::Origin, Leaf::Origin>>(0);
                    sz.add::<Var>(1);
                    sz.add::<*const Self>(1);
                    todo.push(&neg);
                    todo.push(&pos);
                }
            }
            sz.add::<Self>(0);
        }
        my_addr
    }

    pub unsafe fn serialize<
        After,
        FLeaf: FnMut(&Leaf::Origin, BuildCursor<Leaf>) -> BuildCursor<Self>,
        FVar: FnMut(&Var::Origin, &mut Var),
    >(
        origin: &<Self as Build>::Origin,
        mut cur: BuildCursor<Self>,
        mut fleaf: FLeaf,
        mut fvar: FVar,
    ) -> BuildCursor<After> {
        let mut todo: Vec<&BddOrigin<Var::Origin, Leaf::Origin>> = vec![origin];
        let mut ptrmap: HashMap<*const <Self as Build>::Origin, usize> = HashMap::new();
        let mut curs = Vec::new();

        // First, initialize everything except the pointers, as they are not yet known.
        // We will store the pointers in a hashmap and fill them in later.
        while let Some(origin) = todo.pop() {
            ptrmap.insert(origin, cur.cur);
            curs.push(cur.clone());
            let bdd = &mut *cur.get_mut();
            match origin {
                BddOrigin::Leaf(leaf) => {
                    bdd.type_ = BddType::Leaf;
                    cur = fleaf(leaf, cur.behind(1));
                }
                BddOrigin::NodeNoOwned { var, .. } => {
                    bdd.type_ = BddType::NodeNoOwned;
                    let node_cur = cur.behind::<NodeNoOwned<Var, Leaf>>(1);
                    let node = &mut *node_cur.get_mut();
                    fvar(var, &mut node.var);
                    cur = node_cur.behind(1);
                }
                BddOrigin::NodePosOwned { var, pos, .. } => {
                    bdd.type_ = BddType::NodePosOwned;
                    let node: &mut NodeOwned<Var, Leaf> = &mut *cur.behind(1).get_mut();
                    fvar(var, &mut node.var);
                    cur = cur.goto(&mut node.owned);
                    todo.push(&pos);
                }
                BddOrigin::NodeNegOwned { var, neg, .. } => {
                    bdd.type_ = BddType::NodeNegOwned;
                    let node: &mut NodeOwned<Var, Leaf> = &mut *cur.behind(1).get_mut();
                    fvar(var, &mut node.var);
                    cur = cur.goto(&mut node.owned);
                    todo.push(&neg);
                }
                BddOrigin::NodeBothOwned { var, pos, neg, .. } => {
                    bdd.type_ = BddType::NodeBothOwned;
                    let node: &mut NodeOwned<Var, Leaf> = &mut *cur.behind(1).get_mut();
                    fvar(var, &mut node.var);
                    cur = cur.goto(&mut node.owned);
                    todo.push(&neg);
                    todo.push(&pos);
                }
            }
        }

        let mut i = 0;
        todo.push(origin);
        while let Some(origin) = todo.pop() {
            match origin {
                BddOrigin::Leaf(_) => {}
                BddOrigin::NodeNoOwned { pos, neg, .. } => {
                    let node: &mut NodeNoOwned<Var, Leaf> = &mut *curs[i].behind(1).get_mut();
                    node.pos = *ptrmap.get(pos).unwrap() as *const Bdd<'a, Var, Leaf>;
                    node.neg = *ptrmap.get(neg).unwrap() as *const Bdd<'a, Var, Leaf>;
                }
                BddOrigin::NodePosOwned { pos, neg, .. } => {
                    let node: &mut NodeOwned<Var, Leaf> = &mut *curs[i].behind(1).get_mut();
                    node.unowned = *ptrmap.get(neg).unwrap() as *const Bdd<'a, Var, Leaf>;
                    todo.push(&pos);
                }
                BddOrigin::NodeNegOwned { neg, pos, .. } => {
                    let node: &mut NodeOwned<Var, Leaf> = &mut *curs[i].behind(1).get_mut();
                    node.unowned = *ptrmap.get(pos).unwrap() as *const Bdd<'a, Var, Leaf>;
                    todo.push(&neg);
                }
                BddOrigin::NodeBothOwned { pos, neg, .. } => {
                    let node: &mut NodeOwned<Var, Leaf> = &mut *curs[i].behind(1).get_mut();
                    node.unowned =
                        *ptrmap.get(&(&**neg as *const _)).unwrap() as *const Bdd<'a, Var, Leaf>;
                    todo.push(&neg);
                    todo.push(&pos);
                }
            }
            i += 1;
        }
        cur.align()
    }
}

#[cfg(test)]
mod test {
    use super::super::vec::BlobVec;
    use super::*;

    // c & (a == b)
    #[test]
    fn test_bdd() {
        let (a, b, c) = (0u8, 1u8, 2u8);
        let leaf_false = Box::new(BddOrigin::Leaf(b"false".to_vec()));
        let leaf_true = Box::new(BddOrigin::Leaf(b"true".to_vec()));
        let ptr_false: *const _ = &*leaf_false;
        let ptr_true: *const _ = &*leaf_true;
        let node_b_pos = BddOrigin::NodePosOwned { var: b, pos: leaf_true, neg: ptr_false };
        let node_b_neg = BddOrigin::NodeNoOwned { var: b, pos: ptr_false, neg: ptr_true };
        let node_a = BddOrigin::NodeBothOwned {
            var: a,
            pos: Box::new(node_b_pos),
            neg: Box::new(node_b_neg),
        };
        let node_c = BddOrigin::NodeBothOwned { var: c, pos: Box::new(node_a), neg: leaf_false };

        let mut sz = Reserve(0);
        Bdd::<u8, BlobVec<u8>>::reserve(&node_c, &mut sz, |xs, sz| {
            BlobVec::<u8>::reserve(xs, sz);
        });
        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());
        unsafe {
            Bdd::<u8, BlobVec<u8>>::serialize::<(), _, _>(
                &node_c,
                cur.clone(),
                |x, xcur| BlobVec::<u8>::serialize(x, xcur, |y, ycur| *ycur = *y),
                |x, xcur| {
                    *xcur = *x;
                },
            );
            Bdd::<u8, BlobVec<u8>>::deserialize::<(), _, _>(
                cur,
                |xcur| BlobVec::<u8>::deserialize(xcur, |_| ()),
                |_| (),
            );
        }
        let bdd = unsafe { &*(buf.as_ptr() as *const Bdd<u8, BlobVec<u8>>) };

        let leaf = unsafe { bdd.evaluate(|x| [false, false, false][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"false".to_vec());
        let leaf = unsafe { bdd.evaluate(|x| [false, false, true][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"true".to_vec());
        let leaf = unsafe { bdd.evaluate(|x| [false, true, true][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"false".to_vec());
        let leaf = unsafe { bdd.evaluate(|x| [true, true, true][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"true".to_vec());
    }
}
