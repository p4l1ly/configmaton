//! AnchaBdd: Binary Decision Diagram with composable var/leaf anchization.
//!
//! This showcases the power of composable anchization: you can customize
//! var anchization independently from leaf anchization!

use super::{Anchize, BuildCursor, Deanchize, Reserve, Shifter, StaticAnchize};
use hashbrown::HashMap;
use std::marker::PhantomData;

// ============================================================================
// Origin representation (same as before)
// ============================================================================

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
            BddOrigin::NodeNoOwned { var, .. }
            | BddOrigin::NodePosOwned { var, .. }
            | BddOrigin::NodeNegOwned { var, .. }
            | BddOrigin::NodeBothOwned { var, .. } => var,
        }
    }

    pub unsafe fn get_pos(&self) -> &BddOrigin<Var, Leaf> {
        match self {
            BddOrigin::Leaf(_) => panic!("Leaf has no positive child"),
            BddOrigin::NodeNoOwned { pos, .. } | BddOrigin::NodeNegOwned { pos, .. } => &**pos,
            BddOrigin::NodePosOwned { pos, .. } | BddOrigin::NodeBothOwned { pos, .. } => pos,
        }
    }

    pub unsafe fn get_neg(&self) -> &BddOrigin<Var, Leaf> {
        match self {
            BddOrigin::Leaf(_) => panic!("Leaf has no negative child"),
            BddOrigin::NodeNoOwned { neg, .. } | BddOrigin::NodePosOwned { neg, .. } => &**neg,
            BddOrigin::NodeNegOwned { neg, .. } | BddOrigin::NodeBothOwned { neg, .. } => neg,
        }
    }

    pub fn owns_pos(&self) -> bool {
        matches!(self, BddOrigin::NodePosOwned { .. } | BddOrigin::NodeBothOwned { .. })
    }

    pub fn owns_neg(&self) -> bool {
        matches!(self, BddOrigin::NodeNegOwned { .. } | BddOrigin::NodeBothOwned { .. })
    }
}

// ============================================================================
// Ancha representation
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum BddType {
    Leaf,
    NodeNoOwned,
    NodePosOwned,
    NodeNegOwned,
    NodeBothOwned,
}

#[repr(C)]
pub struct AnchaBdd<'a, Var, Leaf> {
    pub type_: BddType,
    _phantom: PhantomData<&'a (Var, Leaf)>,
}

#[repr(C)]
pub struct NodeNoOwned<'a, Var, Leaf> {
    pub var: Var,
    pub pos: *const AnchaBdd<'a, Var, Leaf>,
    pub neg: *const AnchaBdd<'a, Var, Leaf>,
}

#[repr(C)]
pub struct NodeOwned<'a, Var, Leaf> {
    pub var: Var,
    pub unowned: *const AnchaBdd<'a, Var, Leaf>,
    pub owned: AnchaBdd<'a, Var, Leaf>,
}

/// Helper to get pointer to data behind struct.
unsafe fn get_behind_struct<A, B>(a: &A) -> *const B {
    (a as *const A).add(1) as *const B
}

impl<'a, Var, Leaf> AnchaBdd<'a, Var, Leaf> {
    /// Evaluate the BDD with a variable assignment function.
    ///
    /// # Safety
    ///
    /// - The BDD must be properly initialized and deanchized
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
}

// ============================================================================
// Composable BDD anchization!
// ============================================================================

/// BDD anchization with separate strategies for vars and leaves.
///
/// This is the KEY to the composable approach:
/// - var_ancha: how to anchize variables (can be custom!)
/// - leaf_ancha: how to anchize leaves (can be custom!)
///
/// # Example
///
/// ```ignore
/// // Default: direct copy for both
/// let default_bdd = BddAncha {
///     var_ancha: DirectCopy::<u8>::new(),
///     leaf_ancha: VecAncha::new(DirectCopy::<u8>::new()),
/// };
///
/// // Custom: multiply vars by 2, keep leaves default
/// let custom_bdd = BddAncha {
///     var_ancha: MultiplyBy2,
///     leaf_ancha: VecAncha::new(DirectCopy::<u8>::new()),
/// };
/// ```
pub struct BddAncha<VarAnchize, LeafAnchize> {
    pub var_ancha: VarAnchize,
    pub leaf_ancha: LeafAnchize,
}

impl<VarAnchize, LeafAnchize> BddAncha<VarAnchize, LeafAnchize> {
    pub fn new(var_ancha: VarAnchize, leaf_ancha: LeafAnchize) -> Self {
        BddAncha { var_ancha, leaf_ancha }
    }
}

impl<VarAnchize, LeafAnchize> Anchize for BddAncha<VarAnchize, LeafAnchize>
where
    VarAnchize: StaticAnchize + 'static,
    VarAnchize::Ancha: Sized + 'static,
    LeafAnchize: Anchize + 'static,
{
    type Origin = BddOrigin<VarAnchize::Origin, LeafAnchize::Origin>;
    type Ancha<'a>
        = AnchaBdd<'a, VarAnchize::Ancha, LeafAnchize::Ancha<'a>>
    where
        LeafAnchize::Ancha<'a>: 'a;

    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<Self::Ancha<'static>>(0);
        let my_addr = sz.0;

        let mut todo: Vec<&BddOrigin<VarAnchize::Origin, LeafAnchize::Origin>> = vec![origin];
        while let Some(origin) = todo.pop() {
            sz.add::<Self::Ancha<'static>>(1);
            match origin {
                BddOrigin::Leaf(leaf) => {
                    self.leaf_ancha.reserve(leaf, sz);
                }
                BddOrigin::NodeNoOwned { .. } => {
                    sz.add::<NodeNoOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'static>>>(1);
                }
                BddOrigin::NodePosOwned { pos, .. } => {
                    sz.add::<NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'static>>>(0);
                    sz.add::<VarAnchize::Ancha>(1);
                    sz.add::<*const Self::Ancha<'static>>(1);
                    todo.push(&pos);
                }
                BddOrigin::NodeNegOwned { neg, .. } => {
                    sz.add::<NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'static>>>(0);
                    sz.add::<VarAnchize::Ancha>(1);
                    sz.add::<*const Self::Ancha<'static>>(1);
                    todo.push(&neg);
                }
                BddOrigin::NodeBothOwned { pos, neg, .. } => {
                    sz.add::<NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'static>>>(0);
                    sz.add::<VarAnchize::Ancha>(1);
                    sz.add::<*const Self::Ancha<'static>>(1);
                    todo.push(&neg);
                    todo.push(&pos);
                }
            }
            sz.add::<Self::Ancha<'static>>(0);
        }
        my_addr
    }

    unsafe fn anchize<'a, After>(
        &self,
        origin: &Self::Origin,
        mut cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        let mut todo: Vec<&BddOrigin<VarAnchize::Origin, LeafAnchize::Origin>> = vec![origin];
        let mut ptrmap: HashMap<*const BddOrigin<VarAnchize::Origin, LeafAnchize::Origin>, usize> =
            HashMap::new();
        let mut curs = Vec::new();

        // First pass: initialize everything except pointers
        while let Some(origin) = todo.pop() {
            ptrmap.insert(origin, cur.cur);
            curs.push(cur.clone());
            let bdd = &mut *cur.get_mut();
            match origin {
                BddOrigin::Leaf(leaf) => {
                    bdd.type_ = BddType::Leaf;
                    cur = self.leaf_ancha.anchize(leaf, cur.behind(1));
                }
                BddOrigin::NodeNoOwned { var, .. } => {
                    bdd.type_ = BddType::NodeNoOwned;
                    let node_cur =
                        cur.behind::<NodeNoOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>>>(1);
                    let node = &mut *node_cur.get_mut();
                    // Use var anchization strategy!
                    self.var_ancha.anchize_static(var, &mut node.var);
                    cur = node_cur.behind(1);
                }
                BddOrigin::NodePosOwned { var, pos, .. } => {
                    bdd.type_ = BddType::NodePosOwned;
                    let node_cur = cur.behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
                    self.var_ancha.anchize_static(var, &mut node.var);
                    cur = cur.goto(&mut node.owned);
                    todo.push(&pos);
                }
                BddOrigin::NodeNegOwned { var, neg, .. } => {
                    bdd.type_ = BddType::NodeNegOwned;
                    let node_cur = cur.behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
                    self.var_ancha.anchize_static(var, &mut node.var);
                    cur = cur.goto(&mut node.owned);
                    todo.push(&neg);
                }
                BddOrigin::NodeBothOwned { var, pos, neg, .. } => {
                    bdd.type_ = BddType::NodeBothOwned;
                    let node_cur = cur.behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
                    self.var_ancha.anchize_static(var, &mut node.var);
                    cur = cur.goto(&mut node.owned);
                    todo.push(&neg);
                    todo.push(&pos);
                }
            }
        }

        // Second pass: fill in pointers
        let mut i = 0;
        todo.push(origin);
        while let Some(origin) = todo.pop() {
            match origin {
                BddOrigin::Leaf(_) => {}
                BddOrigin::NodeNoOwned { pos, neg, .. } => {
                    let node_cur = curs[i].behind(1);
                    let node: &mut NodeNoOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
                    node.pos = *ptrmap.get(pos).unwrap() as *const _;
                    node.neg = *ptrmap.get(neg).unwrap() as *const _;
                }
                BddOrigin::NodePosOwned { pos, neg, .. } => {
                    let node_cur = curs[i].behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
                    node.unowned = *ptrmap.get(neg).unwrap() as *const _;
                    todo.push(&pos);
                }
                BddOrigin::NodeNegOwned { neg, pos, .. } => {
                    let node_cur = curs[i].behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
                    node.unowned = *ptrmap.get(pos).unwrap() as *const _;
                    todo.push(&neg);
                }
                BddOrigin::NodeBothOwned { pos, neg, .. } => {
                    let node_cur = curs[i].behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
                    node.unowned = *ptrmap.get(&(&**neg as *const _)).unwrap() as *const _;
                    todo.push(&neg);
                    todo.push(&pos);
                }
            }
            i += 1;
        }
        cur.align()
    }
}

impl<VarAnchize, LeafAnchize> Deanchize for BddAncha<VarAnchize, LeafAnchize>
where
    VarAnchize: StaticAnchize + 'static,
    VarAnchize::Ancha: Sized + 'static,
    LeafAnchize: Deanchize + 'static,
{
    type Ancha<'a>
        = AnchaBdd<'a, VarAnchize::Ancha, LeafAnchize::Ancha<'a>>
    where
        LeafAnchize::Ancha<'a>: 'a;

    unsafe fn deanchize<'a, After>(
        &self,
        mut cur: BuildCursor<Self::Ancha<'a>>,
    ) -> BuildCursor<After> {
        let shifter = Shifter(cur.buf);
        let mut todo_count: usize = 1;
        while todo_count > 0 {
            let bdd = &mut *cur.get_mut();
            match bdd.type_ {
                BddType::Leaf => {
                    cur = self.leaf_ancha.deanchize(cur.behind(1));
                }
                BddType::NodeNoOwned => {
                    let node_cur = cur.behind(1);
                    let node: &mut NodeNoOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
                    // Var needs no deanchization (it's a Copy type)
                    shifter.shift(&mut node.pos);
                    shifter.shift(&mut node.neg);
                    cur = node_cur.behind(1);
                }
                BddType::NodeBothOwned => {
                    let node_cur = cur.behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
                    shifter.shift(&mut node.unowned);
                    todo_count += 2;
                    cur = cur.goto(&mut node.owned);
                }
                _ => {
                    let node_cur = cur.behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha<'a>> =
                        &mut *node_cur.get_mut();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ancha::vec::{AnchaVec, VecAncha};
    use crate::ancha::DirectCopy;

    // c & (a == b)
    #[test]
    fn test_bdd_with_default_anchization() {
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

        // Use default anchization: DirectCopy for vars, VecAncha for leaves
        let ancha = BddAncha::new(DirectCopy::<u8>::new(), VecAncha::new(DirectCopy::<u8>::new()));

        let mut sz = Reserve(0);
        ancha.reserve(&node_c, &mut sz);
        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            ancha.anchize::<()>(&node_c, cur.clone());
            ancha.deanchize::<()>(cur);
        }

        let bdd = unsafe { &*(buf.as_ptr() as *const AnchaBdd<u8, AnchaVec<u8>>) };

        let leaf = unsafe { bdd.evaluate(|x| [false, false, false][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"false".to_vec());
        let leaf = unsafe { bdd.evaluate(|x| [false, false, true][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"true".to_vec());
        let leaf = unsafe { bdd.evaluate(|x| [false, true, true][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"false".to_vec());
        let leaf = unsafe { bdd.evaluate(|x| [true, true, true][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"true".to_vec());
    }

    #[test]
    fn test_bdd_with_custom_var_anchization() {
        // Custom: multiply vars by 10
        struct MultiplyBy10;
        impl StaticAnchize for MultiplyBy10 {
            type Origin = u8;
            type Ancha = u8;
            fn anchize_static(&self, origin: &Self::Origin, ancha: &mut Self::Ancha) {
                *ancha = *origin * 10;
            }
        }

        let leaf_a = Box::new(BddOrigin::Leaf(b"a".to_vec()));
        let leaf_b = Box::new(BddOrigin::Leaf(b"b".to_vec()));
        let ptr_a: *const _ = &*leaf_a;
        let node = BddOrigin::NodePosOwned {
            var: 5u8, // Will become 50!
            pos: leaf_b,
            neg: ptr_a,
        };

        // Use custom var anchization!
        let ancha = BddAncha::new(MultiplyBy10, VecAncha::new(DirectCopy::<u8>::new()));

        let mut sz = Reserve(0);
        ancha.reserve(&node, &mut sz);
        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            ancha.anchize::<()>(&node, cur.clone());
            ancha.deanchize::<()>(cur);
        }

        let bdd = unsafe { &*(buf.as_ptr() as *const AnchaBdd<u8, AnchaVec<u8>>) };

        // The var should be multiplied by 10!
        let leaf = unsafe { bdd.evaluate(|x| *x >= 50).as_ref() };
        assert_eq!(leaf, &b"b".to_vec());
        let leaf = unsafe { bdd.evaluate(|x| *x < 50).as_ref() };
        assert_eq!(leaf, &b"a".to_vec());
    }
}
