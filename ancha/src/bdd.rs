//! Binary Decision Diagram (BDD) in the Ancha system.
//!
//! A BDD is a directed acyclic graph (DAG) used for efficient representation
//! of boolean functions. Nodes can be shared, and we track ownership to
//! determine which nodes are stored inline vs referenced by pointer.

use std::marker::PhantomData;

use hashbrown::HashMap;

use super::{Anchize, BuildCursor, Deanchize, Reserve, Shifter, StaticAnchize, StaticDeanchize};

// ============================================================================
// Origin Types
// ============================================================================

/// Origin representation of a BDD node.
///
/// Tracks ownership of child nodes to determine serialization layout.
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
        matches!(self, BddOrigin::NodePosOwned { .. } | BddOrigin::NodeBothOwned { .. })
    }

    pub fn owns_neg(&self) -> bool {
        matches!(self, BddOrigin::NodeNegOwned { .. } | BddOrigin::NodeBothOwned { .. })
    }
}

// ============================================================================
// Ancha Types
// ============================================================================

/// Discriminant for BDD node types in the serialized form.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum BddType {
    Leaf,
    NodeNoOwned,
    NodePosOwned,
    NodeNegOwned,
    NodeBothOwned,
}

/// The main BDD structure in the ancha system.
///
/// Contains only the type discriminant; actual data is stored behind it.
#[repr(C)]
pub struct AnchaBdd<'a, Var, Leaf> {
    pub type_: BddType,
    _phantom: PhantomData<&'a (Var, Leaf)>,
}

/// Node with no owned children (both are pointers).
#[repr(C)]
pub struct NodeNoOwned<'a, Var, Leaf> {
    pub var: Var,
    pub pos: *const AnchaBdd<'a, Var, Leaf>,
    pub neg: *const AnchaBdd<'a, Var, Leaf>,
}

/// Node with one owned child and one unowned pointer.
#[repr(C)]
pub struct NodeOwned<'a, Var, Leaf> {
    pub var: Var,
    pub unowned: *const AnchaBdd<'a, Var, Leaf>,
    pub owned: AnchaBdd<'a, Var, Leaf>,
}

impl<'a, Var, Leaf> AnchaBdd<'a, Var, Leaf> {
    /// Evaluate the BDD by following the decision path.
    ///
    /// # Safety
    ///
    /// The BDD must have been properly anchized and deanchized.
    pub unsafe fn evaluate<F: FnMut(&Var) -> bool>(&self, mut f: F) -> &'a Leaf {
        let mut cur = self;
        loop {
            match cur.type_ {
                BddType::Leaf => {
                    return &*super::get_behind_struct(cur);
                }
                BddType::NodeNoOwned => {
                    let node: &NodeNoOwned<Var, Leaf> = &*super::get_behind_struct(cur);
                    cur = if f(&node.var) { &*node.pos } else { &*node.neg };
                }
                BddType::NodePosOwned => {
                    let node: &NodeOwned<Var, Leaf> = &*super::get_behind_struct(cur);
                    cur = if f(&node.var) { &node.owned } else { &*node.unowned };
                }
                BddType::NodeNegOwned => {
                    let node: &NodeOwned<Var, Leaf> = &*super::get_behind_struct(cur);
                    cur = if f(&node.var) { &*node.unowned } else { &node.owned };
                }
                BddType::NodeBothOwned => {
                    let node: &NodeOwned<Var, Leaf> = &*super::get_behind_struct(cur);
                    cur = if f(&node.var) { &node.owned } else { &*node.unowned };
                }
            }
        }
    }
}

// ============================================================================
// Anchization Strategy
// ============================================================================

/// Strategy for anchizing a BDD.
#[derive(Clone, Copy)]
pub struct BddAnchize<'a, VarAnchize, LeafAnchize> {
    pub var_ancha: VarAnchize,
    pub leaf_ancha: LeafAnchize,
    _phantom: PhantomData<&'a (VarAnchize, LeafAnchize)>,
}

impl<'a, VarAnchize, LeafAnchize> BddAnchize<'a, VarAnchize, LeafAnchize> {
    pub fn new(var_ancha: VarAnchize, leaf_ancha: LeafAnchize) -> Self {
        BddAnchize { var_ancha, leaf_ancha, _phantom: PhantomData }
    }
}

impl<'a, VarAnchize: Default, LeafAnchize: Default> Default
    for BddAnchize<'a, VarAnchize, LeafAnchize>
{
    fn default() -> Self {
        BddAnchize {
            var_ancha: Default::default(),
            leaf_ancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, VarAnchize, LeafAnchize> Anchize<'a> for BddAnchize<'a, VarAnchize, LeafAnchize>
where
    VarAnchize: StaticAnchize<'a>,
    LeafAnchize: Anchize<'a, Context = VarAnchize::Context>,
{
    type Origin = BddOrigin<VarAnchize::Origin, LeafAnchize::Origin>;
    type Ancha = AnchaBdd<'a, VarAnchize::Ancha, LeafAnchize::Ancha>;
    type Context = VarAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0); // Alignment at the beginning!
        let mut todo: Vec<&Self::Origin> = vec![origin];

        while let Some(origin) = todo.pop() {
            sz.add::<Self::Ancha>(0);
            sz.add::<Self::Ancha>(1);
            match origin {
                BddOrigin::Leaf(leaf) => {
                    self.leaf_ancha.reserve(leaf, context, sz);
                }
                BddOrigin::NodeNoOwned { .. } => {
                    sz.add::<NodeNoOwned<VarAnchize::Ancha, LeafAnchize::Ancha>>(1);
                }
                BddOrigin::NodePosOwned { pos, .. } => {
                    sz.add::<NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha>>(0);
                    sz.add::<VarAnchize::Ancha>(1);
                    sz.add::<*const Self::Ancha>(1);
                    todo.push(pos);
                }
                BddOrigin::NodeNegOwned { neg, .. } => {
                    sz.add::<NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha>>(0);
                    sz.add::<VarAnchize::Ancha>(1);
                    sz.add::<*const Self::Ancha>(1);
                    todo.push(neg);
                }
                BddOrigin::NodeBothOwned { pos, neg, .. } => {
                    sz.add::<NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha>>(0);
                    sz.add::<VarAnchize::Ancha>(1);
                    sz.add::<*const Self::Ancha>(1);
                    todo.push(neg);
                    todo.push(pos);
                }
            }
        }
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let mut cur: BuildCursor<Self::Ancha> = cur.align(); // Alignment at the beginning!
        let mut todo: Vec<&Self::Origin> = vec![origin];
        let mut ptrmap: HashMap<*const Self::Origin, usize> = HashMap::new();
        let mut curs = Vec::new();

        // Phase 1: Initialize everything except pointers
        while let Some(origin) = todo.pop() {
            // Align between loop iterations (matching sz.add::<Self::Ancha>(0) in reserve)
            cur = cur.align::<Self::Ancha>();

            ptrmap.insert(origin, cur.cur);
            curs.push(cur.clone());
            let bdd = &mut *cur.get_mut();

            match origin {
                BddOrigin::Leaf(leaf) => {
                    bdd.type_ = BddType::Leaf;
                    cur = self.leaf_ancha.anchize(leaf, context, cur.behind(1));
                }
                BddOrigin::NodeNoOwned { var, .. } => {
                    bdd.type_ = BddType::NodeNoOwned;
                    let node_cur =
                        cur.behind::<NodeNoOwned<VarAnchize::Ancha, LeafAnchize::Ancha>>(1);
                    let node = &mut *node_cur.get_mut();
                    self.var_ancha.anchize_static(var, context, &mut node.var);
                    cur = node_cur.behind(1);
                }
                BddOrigin::NodePosOwned { var, pos, .. } => {
                    bdd.type_ = BddType::NodePosOwned;
                    let node_ptr = cur.behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha> =
                        &mut *node_ptr.get_mut();
                    self.var_ancha.anchize_static(var, context, &mut node.var);
                    cur = cur.goto(&mut node.owned);
                    todo.push(pos);
                }
                BddOrigin::NodeNegOwned { var, neg, .. } => {
                    bdd.type_ = BddType::NodeNegOwned;
                    let node_ptr = cur.behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha> =
                        &mut *node_ptr.get_mut();
                    self.var_ancha.anchize_static(var, context, &mut node.var);
                    cur = cur.goto(&mut node.owned);
                    todo.push(neg);
                }
                BddOrigin::NodeBothOwned { var, pos, neg, .. } => {
                    bdd.type_ = BddType::NodeBothOwned;
                    let node_ptr = cur.behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha> =
                        &mut *node_ptr.get_mut();
                    self.var_ancha.anchize_static(var, context, &mut node.var);
                    cur = cur.goto(&mut node.owned);
                    todo.push(neg);
                    todo.push(pos);
                }
            }
        }

        // Phase 2: Fill in the pointers using the pointer map
        let mut i = 0;
        todo.push(origin);
        while let Some(origin) = todo.pop() {
            match origin {
                BddOrigin::Leaf(_) => {}
                BddOrigin::NodeNoOwned { pos, neg, .. } => {
                    let node_ptr = curs[i].behind(1);
                    let node: &mut NodeNoOwned<VarAnchize::Ancha, LeafAnchize::Ancha> =
                        &mut *node_ptr.get_mut();
                    node.pos = *ptrmap.get(pos).unwrap() as *const Self::Ancha;
                    node.neg = *ptrmap.get(neg).unwrap() as *const Self::Ancha;
                }
                BddOrigin::NodePosOwned { pos, neg, .. } => {
                    let node_ptr = curs[i].behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha> =
                        &mut *node_ptr.get_mut();
                    node.unowned = *ptrmap.get(neg).unwrap() as *const Self::Ancha;
                    todo.push(pos);
                }
                BddOrigin::NodeNegOwned { neg, pos, .. } => {
                    let node_ptr = curs[i].behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha> =
                        &mut *node_ptr.get_mut();
                    node.unowned = *ptrmap.get(pos).unwrap() as *const Self::Ancha;
                    todo.push(neg);
                }
                BddOrigin::NodeBothOwned { pos, neg, .. } => {
                    let node_ptr = curs[i].behind(1);
                    let node: &mut NodeOwned<VarAnchize::Ancha, LeafAnchize::Ancha> =
                        &mut *node_ptr.get_mut();
                    node.unowned =
                        *ptrmap.get(&(&**neg as *const _)).unwrap() as *const Self::Ancha;
                    todo.push(neg);
                    todo.push(pos);
                }
            }
            i += 1;
        }
        cur.transmute()
    }
}

// ============================================================================
// Deanchization Strategy
// ============================================================================

/// Strategy for deanchizing a BDD.
#[derive(Clone, Copy)]
pub struct BddDeanchize<'a, VarDeanchize, LeafDeanchize> {
    pub var_deancha: VarDeanchize,
    pub leaf_deancha: LeafDeanchize,
    _phantom: PhantomData<&'a (VarDeanchize, LeafDeanchize)>,
}

impl<'a, VarDeanchize, LeafDeanchize> BddDeanchize<'a, VarDeanchize, LeafDeanchize> {
    pub fn new(var_deancha: VarDeanchize, leaf_deancha: LeafDeanchize) -> Self {
        BddDeanchize { var_deancha, leaf_deancha, _phantom: PhantomData }
    }
}

impl<'a, VarDeanchize: Default, LeafDeanchize: Default> Default
    for BddDeanchize<'a, VarDeanchize, LeafDeanchize>
{
    fn default() -> Self {
        BddDeanchize {
            var_deancha: Default::default(),
            leaf_deancha: Default::default(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, VarDeanchize, LeafDeanchize> Deanchize<'a>
    for BddDeanchize<'a, VarDeanchize, LeafDeanchize>
where
    VarDeanchize: StaticDeanchize<'a>,
    LeafDeanchize: Deanchize<'a>,
{
    type Ancha = AnchaBdd<'a, VarDeanchize::Ancha, LeafDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, mut cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        cur = cur.align(); // Alignment at the beginning!
        let shifter = Shifter(cur.buf);
        let mut todo_count: usize = 1;

        while todo_count > 0 {
            // Align between loop iterations
            cur = cur.align::<Self::Ancha>();
            let bdd = &mut *cur.get_mut();
            match bdd.type_ {
                BddType::Leaf => {
                    cur = self.leaf_deancha.deanchize(cur.behind(1));
                }
                BddType::NodeNoOwned => {
                    let node_cur = cur.behind(1);
                    let node: &mut NodeNoOwned<VarDeanchize::Ancha, LeafDeanchize::Ancha> =
                        &mut *node_cur.get_mut();
                    self.var_deancha.deanchize_static(&mut node.var);
                    shifter.shift(&mut node.pos);
                    shifter.shift(&mut node.neg);
                    cur = node_cur.behind(1);
                }
                BddType::NodeBothOwned => {
                    let node_ptr = cur.behind(1);
                    let node: &mut NodeOwned<VarDeanchize::Ancha, LeafDeanchize::Ancha> =
                        &mut *node_ptr.get_mut();
                    self.var_deancha.deanchize_static(&mut node.var);
                    shifter.shift(&mut node.unowned);
                    todo_count += 2;
                    cur = cur.goto(&mut node.owned);
                }
                _ => {
                    let node_ptr = cur.behind(1);
                    let node: &mut NodeOwned<VarDeanchize::Ancha, LeafDeanchize::Ancha> =
                        &mut *node_ptr.get_mut();
                    self.var_deancha.deanchize_static(&mut node.var);
                    shifter.shift(&mut node.unowned);
                    todo_count += 1;
                    cur = cur.goto(&mut node.owned);
                }
            }
            todo_count -= 1;
        }
        cur.transmute()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{vec::*, CopyAnchize, NoopDeanchize};

    // Test: c & (a == b)
    // This creates a BDD that evaluates to true when c is true AND a equals b
    #[test]
    fn test_bdd_complex() {
        let (a, b, c) = (0u8, 1u8, 2u8);

        // Create leaf nodes
        let leaf_false = Box::new(BddOrigin::Leaf(b"false".to_vec()));
        let leaf_true = Box::new(BddOrigin::Leaf(b"true".to_vec()));
        let ptr_false: *const _ = &*leaf_false;
        let ptr_true: *const _ = &*leaf_true;

        // node_b_pos: if b is true, return true; else return false
        let node_b_pos = BddOrigin::NodePosOwned { var: b, pos: leaf_true, neg: ptr_false };

        // node_b_neg: if b is true, return false; else return true (inverted)
        let node_b_neg = BddOrigin::NodeNoOwned { var: b, pos: ptr_false, neg: ptr_true };

        // node_a: if a is true, use node_b_pos; else use node_b_neg
        // This implements (a == b)
        let node_a = BddOrigin::NodeBothOwned {
            var: a,
            pos: Box::new(node_b_pos),
            neg: Box::new(node_b_neg),
        };

        // node_c: if c is true, use node_a; else return false
        // This implements c & (a == b)
        let node_c = BddOrigin::NodeBothOwned { var: c, pos: Box::new(node_a), neg: leaf_false };

        // Create strategies
        let anchize: BddAnchize<CopyAnchize<u8, ()>, VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            BddAnchize::default();
        let deanchize: BddDeanchize<NoopDeanchize<u8>, VecDeanchize<NoopDeanchize<u8>>> =
            BddDeanchize::default();

        // Reserve and serialize
        let mut sz = Reserve(0);
        anchize.reserve(&node_c, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&node_c, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let bdd = unsafe { &*(buf.as_ptr() as *const AnchaBdd<u8, AnchaVec<u8>>) };

        // Test evaluations
        // [a, b, c] = [false, false, false] -> false (c is false)
        let leaf = unsafe { bdd.evaluate(|x| [false, false, false][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"false".to_vec());

        // [a, b, c] = [false, false, true] -> true (c is true, a == b)
        let leaf = unsafe { bdd.evaluate(|x| [false, false, true][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"true".to_vec());

        // [a, b, c] = [false, true, true] -> false (c is true, but a != b)
        let leaf = unsafe { bdd.evaluate(|x| [false, true, true][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"false".to_vec());

        // [a, b, c] = [true, true, true] -> true (c is true, a == b)
        let leaf = unsafe { bdd.evaluate(|x| [true, true, true][*x as usize]).as_ref() };
        assert_eq!(leaf, &b"true".to_vec());
    }

    #[test]
    fn test_bdd_simple_leaf() {
        let origin = BddOrigin::Leaf(vec![42u8, 43, 44]);

        let anchize: BddAnchize<CopyAnchize<u8, ()>, VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            BddAnchize::default();
        let deanchize: BddDeanchize<NoopDeanchize<u8>, VecDeanchize<NoopDeanchize<u8>>> =
            BddDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let bdd = unsafe { &*(buf.as_ptr() as *const AnchaBdd<u8, AnchaVec<u8>>) };
        assert!(matches!(bdd.type_, BddType::Leaf));

        // A leaf BDD always returns the leaf value
        let leaf = unsafe { bdd.evaluate(|_| panic!("Should not evaluate variables")).as_ref() };
        assert_eq!(leaf, &[42u8, 43, 44]);
    }

    #[test]
    fn test_bdd_single_node() {
        let leaf_true = Box::new(BddOrigin::Leaf(vec![1u8]));
        let leaf_false = Box::new(BddOrigin::Leaf(vec![0u8]));

        let node = BddOrigin::NodeBothOwned { var: 0u8, pos: leaf_true, neg: leaf_false };

        let anchize: BddAnchize<CopyAnchize<u8, ()>, VecAnchizeFromVec<CopyAnchize<u8, ()>>> =
            BddAnchize::default();
        let deanchize: BddDeanchize<NoopDeanchize<u8>, VecDeanchize<NoopDeanchize<u8>>> =
            BddDeanchize::default();

        let mut sz = Reserve(0);
        anchize.reserve(&node, &mut (), &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&node, &mut (), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        let bdd = unsafe { &*(buf.as_ptr() as *const AnchaBdd<u8, AnchaVec<u8>>) };

        // Variable 0 = true -> return [1]
        let leaf = unsafe { bdd.evaluate(|_| true).as_ref() };
        assert_eq!(leaf, &[1u8]);

        // Variable 0 = false -> return [0]
        let leaf = unsafe { bdd.evaluate(|_| false).as_ref() };
        assert_eq!(leaf, &[0u8]);
    }
}
