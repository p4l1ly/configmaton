//! State machine state representation in the Ancha system.
//!
//! This module demonstrates **advanced context enrichment** where we thread
//! a pointer map (`qptrs`) through the serialization to handle state references.
//!
//! # Key Design Patterns
//!
//! 1. **GetQptrs Trait**: Context provides access to state pointer mappings
//! 2. **Union Types**: State can be sparse or dense (different memory layouts)
//! 3. **Deep Composition**: Leverages Vec, ArrMap, VecMap, HashMap strategies
//! 4. **Conditional Serialization**: Tags are optional (nullable pointer)

use std::marker::PhantomData;
use std::mem::ManuallyDrop;

use ancha::{
    arrmap::{AnchaArrMap, ArrMapAnchizeFromArray, ArrMapDeanchize},
    hashmap::{AnchaHashMap, HashMapAnchizeFromVec, HashMapDeanchize},
    vec::{AnchaVec, AnchaVecIter, VecAnchizeFromVec, VecDeanchize},
    vecmap::{AnchaVecMap, VecMapAnchizeFromVec, VecMapDeanchize, VecMapMatchIter},
    Anchize, Assocs, BuildCursor, CopyAnchize, Deanchize, Matches, NoopDeanchize, Reserve,
    Shifter, StaticAnchize, StaticDeanchize,
};

use crate::guards::Guard;

// ============================================================================
// Matches Implementation for Guard
// ============================================================================

impl Matches<Guard> for u8 {
    unsafe fn matches(&self, other: &Guard) -> bool {
        other.contains(*self)
    }
}

// ============================================================================
// Context Trait: GetQptrs
// ============================================================================

/// Trait for contexts that provide access to state pointer mappings.
///
/// During serialization, state indices (usize) need to be converted to
/// pointers (*const U8State). The qptrs vector maps: index → pointer address.
///
/// The caller is responsible for providing a context that implements this trait.
pub trait GetQptrs {
    fn get_qptrs(&self) -> &Vec<usize>;
}

// ============================================================================
// Type Aliases (matching blob structure)
// ============================================================================

type U8States<'a> = AnchaVec<'a, *const U8State<'a>>;
type U8AList<'a> = AnchaVecMap<'a, u8, U8States<'a>>;
type U8ExplicitTrans<'a> = AnchaHashMap<'a, U8AList<'a>>;
type U8Tags<'a> = AnchaVec<'a, usize>;
type U8PatternTrans<'a> = AnchaVecMap<'a, Guard, U8States<'a>>;
type U8ArrMap<'a> = AnchaArrMap<'a, 256, U8States<'a>>;

// ============================================================================
// State Structures
// ============================================================================

#[repr(C)]
pub struct U8SparseState<'a> {
    pub is_dense: bool,
    pub tags: *const U8Tags<'a>,
    pub explicit_trans: *const U8ExplicitTrans<'a>,
    pub pattern_trans: U8PatternTrans<'a>,
}

#[repr(C)]
pub struct U8DenseState<'a> {
    pub is_dense: bool,
    pub tags: *const U8Tags<'a>,
    pub trans: U8ArrMap<'a>,
}

#[repr(C)]
pub union U8State<'a> {
    pub sparse: ManuallyDrop<U8SparseState<'a>>,
    pub dense: ManuallyDrop<U8DenseState<'a>>,
}

// ============================================================================
// Origin Types (from blob)
// ============================================================================

#[derive(Debug)]
pub struct U8DenseStatePrepared {
    pub tags: Vec<usize>,
    pub trans: [Vec<usize>; 256],
}

#[derive(Debug)]
pub struct U8SparseStatePrepared {
    pub tags: Vec<usize>,
    pub pattern_trans: Vec<(Guard, Vec<usize>)>,
    pub explicit_trans: Vec<Vec<(u8, Vec<usize>)>>,
}

#[derive(Debug)]
pub enum U8StatePrepared {
    Sparse(U8SparseStatePrepared),
    Dense(U8DenseStatePrepared),
}

// ============================================================================
// State Pointer Strategy
// ============================================================================

/// Strategy for anchizing state pointer (usize index → *const U8State).
///
/// Uses the context's qptrs to convert indices to pointers.
#[derive(Clone, Copy)]
pub struct StatePointerAnchize<Ctx> {
    _phantom: PhantomData<Ctx>,
}

impl<Ctx> Default for StatePointerAnchize<Ctx> {
    fn default() -> Self {
        StatePointerAnchize { _phantom: PhantomData }
    }
}

impl<'a, Ctx: GetQptrs> StaticAnchize<'a> for StatePointerAnchize<Ctx> {
    type Origin = usize;
    type Context = Ctx;
    type Ancha = *const U8State<'a>;

    fn anchize_static(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        ancha: &mut Self::Ancha,
    ) {
        // qptrs stores OFFSETS (not absolute pointers)
        // These will be converted to absolute pointers during deanchize
        let qptrs = context.get_qptrs();
        *ancha = qptrs[*origin] as *const U8State<'a>;
    }
}

/// Strategy for deanchizing state pointers.
///
/// This strategy CONTAINS the Shifter, allowing pointer fixup to compose!
#[derive(Clone, Copy)]
pub struct StatePointerDeanchize {
    shifter: Shifter,
}

impl StatePointerDeanchize {
    pub fn new(buf: *mut u8) -> Self {
        StatePointerDeanchize { shifter: Shifter(buf) }
    }
}

impl<'a> StaticDeanchize<'a> for StatePointerDeanchize {
    type Ancha = *const U8State<'a>;

    fn deanchize_static(&self, ancha: &mut Self::Ancha) {
        unsafe {
            self.shifter.shift(ancha);
        }
    }
}

// ============================================================================
// State Anchization Strategy
// ============================================================================

pub struct U8StateAnchize<Ctx> {
    _phantom: PhantomData<Ctx>,
}

impl<Ctx> Default for U8StateAnchize<Ctx> {
    fn default() -> Self {
        U8StateAnchize { _phantom: PhantomData }
    }
}

impl<'a, Ctx: GetQptrs> Anchize<'a> for U8StateAnchize<Ctx> {
    type Origin = U8StatePrepared;
    type Ancha = U8State<'a>;
    type Context = Ctx;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0);
        sz.add::<bool>(1);
        sz.add::<*const U8Tags>(1);

        match origin {
            U8StatePrepared::Sparse(sparse) => {
                sz.add::<*const U8ExplicitTrans>(1);

                // Pattern transitions
                let pattern_anch: VecMapAnchizeFromVec<
                    CopyAnchize<Guard, Ctx>,
                    VecAnchizeFromVec<StatePointerAnchize<Ctx>>,
                > = VecMapAnchizeFromVec::default();
                pattern_anch.reserve(&sparse.pattern_trans, context, sz);

                // Explicit transitions (HashMap of association lists)
                let explicit_anch: HashMapAnchizeFromVec<
                    VecMapAnchizeFromVec<
                        CopyAnchize<u8, Ctx>,
                        VecAnchizeFromVec<StatePointerAnchize<Ctx>>,
                    >,
                > = HashMapAnchizeFromVec::default();
                explicit_anch.reserve(&sparse.explicit_trans, context, sz);

                // Tags
                if !sparse.tags.is_empty() {
                    let tags_anch: VecAnchizeFromVec<CopyAnchize<usize, Ctx>> =
                        VecAnchizeFromVec::default();
                    tags_anch.reserve(&sparse.tags, context, sz);
                }
            }
            U8StatePrepared::Dense(dense) => {
                // Dense transitions (array map)
                let trans_anch: ArrMapAnchizeFromArray<
                    256,
                    VecAnchizeFromVec<StatePointerAnchize<Ctx>>,
                > = ArrMapAnchizeFromArray::default();
                trans_anch.reserve(&dense.trans, context, sz);

                // Tags
                if !dense.tags.is_empty() {
                    let tags_anch: VecAnchizeFromVec<CopyAnchize<usize, Ctx>> =
                        VecAnchizeFromVec::default();
                    tags_anch.reserve(&dense.tags, context, sz);
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
        let cur = cur.align::<Self::Ancha>();
        let state = &mut *cur.get_mut();
        let f_is_dense_cur = cur.transmute::<bool>();
        let f_tags_cur = f_is_dense_cur.behind::<*const U8Tags>(1);

        match origin {
            U8StatePrepared::Sparse(sparse_origin) => {
                let sparse = &mut state.sparse;
                sparse.is_dense = false;

                let f_explicit_trans_cur = f_tags_cur.behind::<*const U8ExplicitTrans>(1);
                let f_pattern_trans_cur = f_explicit_trans_cur.behind::<U8PatternTrans>(1);

                // Anchize pattern transitions
                let pattern_anch: VecMapAnchizeFromVec<
                    CopyAnchize<Guard, Ctx>,
                    VecAnchizeFromVec<StatePointerAnchize<Ctx>>,
                > = VecMapAnchizeFromVec::default();
                let exp_cur = pattern_anch.anchize(
                    &sparse_origin.pattern_trans,
                    context,
                    f_pattern_trans_cur,
                );

                // Store pointer to explicit transitions
                sparse.explicit_trans = exp_cur.cur as *const U8ExplicitTrans;

                // Anchize explicit transitions
                let explicit_anch: HashMapAnchizeFromVec<
                    VecMapAnchizeFromVec<
                        CopyAnchize<u8, Ctx>,
                        VecAnchizeFromVec<StatePointerAnchize<Ctx>>,
                    >,
                > = HashMapAnchizeFromVec::default();
                let tags_cur: BuildCursor<u8> =
                    explicit_anch.anchize(&sparse_origin.explicit_trans, context, exp_cur);

                // Handle tags
                if sparse_origin.tags.is_empty() {
                    sparse.tags = std::ptr::null();
                    tags_cur.transmute()
                } else {
                    let tags_cur = tags_cur.align::<U8Tags>();
                    sparse.tags = tags_cur.cur as *const U8Tags;
                    let tags_anch: VecAnchizeFromVec<CopyAnchize<usize, Ctx>> =
                        VecAnchizeFromVec::default();
                    tags_anch.anchize(&sparse_origin.tags, context, tags_cur)
                }
            }
            U8StatePrepared::Dense(dense_origin) => {
                let dense = &mut state.dense;
                dense.is_dense = true;

                let f_trans_cur = f_tags_cur.behind::<U8ArrMap>(1);

                // Anchize dense transitions
                let trans_anch: ArrMapAnchizeFromArray<
                    256,
                    VecAnchizeFromVec<StatePointerAnchize<Ctx>>,
                > = ArrMapAnchizeFromArray::default();
                let tags_cur: BuildCursor<u8> =
                    trans_anch.anchize(&dense_origin.trans, context, f_trans_cur);

                // Handle tags
                if dense_origin.tags.is_empty() {
                    dense.tags = std::ptr::null();
                    tags_cur.transmute()
                } else {
                    let tags_cur = tags_cur.align::<U8Tags>();
                    dense.tags = tags_cur.cur as *const U8Tags;
                    let tags_anch: VecAnchizeFromVec<CopyAnchize<usize, Ctx>> =
                        VecAnchizeFromVec::default();
                    tags_anch.anchize(&dense_origin.tags, context, tags_cur)
                }
            }
        }
    }
}

// ============================================================================
// State Deanchization Strategy
// ============================================================================

pub struct U8StateDeanchize {
    _phantom: PhantomData<()>,
}

impl Default for U8StateDeanchize {
    fn default() -> Self {
        U8StateDeanchize { _phantom: PhantomData }
    }
}

impl<'a> Deanchize<'a> for U8StateDeanchize {
    type Ancha = U8State<'a>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let shifter = Shifter(cur.buf);
        let state = &mut *cur.get_mut();
        let f_is_dense_cur = cur.transmute::<bool>();
        let f_tags_cur = f_is_dense_cur.behind::<*const U8Tags>(1);

        // Create pointer deanchize strategy with the buffer
        let ptr_deanch = StatePointerDeanchize::new(cur.buf);

        if state.sparse.is_dense {
            // Dense state
            let dense = &mut state.dense;
            let f_trans_cur = f_tags_cur.behind::<U8ArrMap>(1);

            // Composable! ArrMap of Vec of state pointers
            let trans_deanch: ArrMapDeanchize<'a, 256, VecDeanchize<'a, StatePointerDeanchize>> =
                ArrMapDeanchize::new(VecDeanchize::new(ptr_deanch));
            let tags_cur: BuildCursor<u8> = trans_deanch.deanchize(f_trans_cur);

            if dense.tags.is_null() {
                tags_cur.transmute()
            } else {
                shifter.shift(&mut dense.tags);
                let tags_deanch: VecDeanchize<'a, NoopDeanchize<'a, usize>> =
                    VecDeanchize::default();
                tags_deanch.deanchize(tags_cur.align())
            }
        } else {
            // Sparse state
            let sparse = &mut state.sparse;
            shifter.shift(&mut sparse.explicit_trans);

            let f_explicit_trans_cur = f_tags_cur.behind::<*const U8ExplicitTrans>(1);
            let f_pattern_trans_cur = f_explicit_trans_cur.behind::<U8PatternTrans>(1);

            // Composable! VecMap<Guard, Vec<*const U8State>>
            let pattern_deanch: VecMapDeanchize<
                'a,
                NoopDeanchize<'a, Guard>,
                VecDeanchize<'a, StatePointerDeanchize>,
            > = VecMapDeanchize::new(NoopDeanchize::default(), VecDeanchize::new(ptr_deanch));
            let exp_cur = pattern_deanch.deanchize(f_pattern_trans_cur);

            // Composable! HashMap<VecMap<u8, Vec<*const U8State>>>
            let explicit_deanch: HashMapDeanchize<
                'a,
                VecMapDeanchize<
                    'a,
                    NoopDeanchize<'a, u8>,
                    VecDeanchize<'a, StatePointerDeanchize>,
                >,
            > = HashMapDeanchize::new(VecMapDeanchize::new(
                NoopDeanchize::default(),
                VecDeanchize::new(ptr_deanch),
            ));
            let tags_cur: BuildCursor<u8> = explicit_deanch.deanchize(exp_cur);

            if sparse.tags.is_null() {
                tags_cur.transmute()
            } else {
                shifter.shift(&mut sparse.tags);
                let tags_deanch: VecDeanchize<'a, NoopDeanchize<'a, usize>> =
                    VecDeanchize::default();
                tags_deanch.deanchize(tags_cur.align())
            }
        }
    }
}

// ============================================================================
// State Query Methods
// ============================================================================

impl<'a> U8State<'a> {
    pub unsafe fn get_tags(&self) -> &[usize] {
        if self.sparse.tags.is_null() {
            &[]
        } else {
            (*self.sparse.tags).as_ref()
        }
    }

    pub unsafe fn is_dense(&self) -> bool {
        self.sparse.is_dense
    }

    pub unsafe fn iter_matches<'c, 'b>(&'c self, key: &'b u8) -> U8StateIterator<'a, 'b>
    where
        'a: 'b + 'c,
    {
        if self.sparse.is_dense {
            U8StateIterator::Dense(self.dense.trans.get(*key as usize).iter())
        } else {
            let sparse = &self.sparse;
            U8StateIterator::Sparse(U8SparseStateIterator {
                pattern_iter: sparse.pattern_trans.iter_matches(key),
                states_iter: None,
                explicit_trans: sparse.explicit_trans,
            })
        }
    }
}

// ============================================================================
// Iterators
// ============================================================================

pub struct U8SparseStateIterator<'a, 'b> {
    states_iter: Option<AnchaVecIter<'a, *const U8State<'a>>>,
    pattern_iter: VecMapMatchIter<'a, 'b, u8, Guard, U8States<'a>>,
    explicit_trans: *const U8ExplicitTrans<'a>,
}

pub type U8DenseStateIterator<'a> = AnchaVecIter<'a, *const U8State<'a>>;

pub enum U8StateIterator<'a, 'b> {
    Sparse(U8SparseStateIterator<'a, 'b>),
    Dense(U8DenseStateIterator<'a>),
}

use ancha::UnsafeIterator;

impl<'a, 'b> UnsafeIterator for U8SparseStateIterator<'a, 'b>
where
    'a: 'b,
{
    type Item = *const U8State<'a>;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        // Check if we have states from a previous match
        if let Some(states_iter) = self.states_iter.as_mut() {
            if let Some(state) = states_iter.next() {
                return Some(*state);
            }
        }

        // Try pattern transitions
        loop {
            if let Some((_, states)) = self.pattern_iter.next() {
                let mut states_iter = states.iter();
                if let Some(state) = states_iter.next() {
                    self.states_iter = Some(states_iter);
                    return Some(*state);
                }
            } else {
                // Pattern exhausted, try explicit transitions
                if self.explicit_trans.is_null() {
                    return None;
                } else {
                    let explicit_trans = &*self.explicit_trans;
                    self.explicit_trans = std::ptr::null();
                    if let Some(states) = explicit_trans.get(self.pattern_iter.x) {
                        let mut states_iter = states.iter();
                        if let Some(state) = states_iter.next() {
                            self.states_iter = Some(states_iter);
                            return Some(*state);
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
            }
        }
    }
}

impl<'a, 'b> UnsafeIterator for U8StateIterator<'a, 'b>
where
    'a: 'b,
{
    type Item = *const U8State<'a>;

    unsafe fn next(&mut self) -> Option<Self::Item> {
        match self {
            U8StateIterator::Sparse(iter) => iter.next(),
            U8StateIterator::Dense(iter) => iter.next().map(|s| *s),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper context for tests
    struct TestContext {
        qptrs: Vec<usize>,
    }

    impl GetQptrs for TestContext {
        fn get_qptrs(&self) -> &Vec<usize> {
            &self.qptrs
        }
    }

    #[test]
    fn test_dense_state_basic() {
        // Create a simple dense state with a few transitions
        let origin = U8StatePrepared::Dense(U8DenseStatePrepared {
            tags: vec![42, 100],
            trans: {
                let mut arr = std::array::from_fn(|_| Vec::new());
                arr[b'a' as usize] = vec![1, 2];
                arr[b'b' as usize] = vec![3];
                arr
            },
        });

        // Create anchize strategy
        let anchize = U8StateAnchize::<TestContext>::default();

        // Set up context with qptrs (mapping indices to fake addresses)
        let mut context = TestContext { qptrs: vec![0x1000, 0x2000, 0x3000, 0x4000] };

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut context, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut context, cur.clone());

            // Create deanchize strategy with buffer
            let deanch = U8StateDeanchize::default();
            deanch.deanchize::<()>(cur.clone());
        }

        let state = unsafe { &*(buf.as_ptr() as *const U8State) };

        // Verify it's dense
        assert!(unsafe { state.is_dense() });

        // Verify tags
        let tags = unsafe { state.get_tags() };
        assert_eq!(tags, &[42, 100]);
    }

    #[test]
    fn test_sparse_state_basic() {
        // Create a sparse state
        let origin = U8StatePrepared::Sparse(U8SparseStatePrepared {
            tags: vec![7, 8, 9],
            pattern_trans: vec![(Guard(1, 0), vec![0, 1]), (Guard(2, 0), vec![2])],
            explicit_trans: vec![vec![(b'x', vec![3])], vec![], vec![(b'y', vec![4, 5])], vec![]],
        });

        let anchize = U8StateAnchize::<TestContext>::default();

        let mut context =
            TestContext { qptrs: vec![0x1000, 0x2000, 0x3000, 0x4000, 0x5000, 0x6000] };

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut context, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut context, cur.clone());

            let deanch = U8StateDeanchize::default();
            deanch.deanchize::<()>(cur.clone());
        }

        let state = unsafe { &*(buf.as_ptr() as *const U8State) };

        // Verify it's sparse
        assert!(!unsafe { state.is_dense() });

        // Verify tags
        let tags = unsafe { state.get_tags() };
        assert_eq!(tags, &[7, 8, 9]);
    }

    #[test]
    fn test_state_no_tags() {
        // State without tags
        let origin = U8StatePrepared::Dense(U8DenseStatePrepared {
            tags: vec![], // Empty tags
            trans: std::array::from_fn(|_| Vec::new()),
        });

        let anchize = U8StateAnchize::<TestContext>::default();

        let mut context = TestContext { qptrs: vec![] };

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut context, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut context, cur.clone());

            let deanch = U8StateDeanchize::default();
            deanch.deanchize::<()>(cur.clone());
        }

        let state = unsafe { &*(buf.as_ptr() as *const U8State) };

        // Verify no tags
        let tags = unsafe { state.get_tags() };
        assert_eq!(tags.len(), 0);
    }

    #[test]
    fn test_state_pointer_shifting() {
        // Test that state pointers are correctly shifted
        let origin = U8StatePrepared::Dense(U8DenseStatePrepared {
            tags: vec![],
            trans: {
                let mut arr = std::array::from_fn(|_| Vec::new());
                arr[0] = vec![0, 1, 2]; // Multiple state pointers
                arr
            },
        });

        let anchize = U8StateAnchize::<TestContext>::default();

        // qptrs maps indices to actual addresses in the buffer
        let mut context = TestContext { qptrs: vec![0x100, 0x200, 0x300] };

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut context, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let base_addr = buf.as_ptr() as usize;
        let cur = BuildCursor::new(buf.as_mut_ptr());

        // Update qptrs to actual buffer addresses
        context.qptrs = vec![base_addr + 0x10, base_addr + 0x20, base_addr + 0x30];

        unsafe {
            anchize.anchize::<()>(&origin, &mut context, cur.clone());

            let deanch = U8StateDeanchize::default();
            deanch.deanchize::<()>(cur.clone());
        }

        let state = unsafe { &*(buf.as_ptr() as *const U8State) };

        // Access the dense transitions
        unsafe {
            let trans_array = &state.dense.trans;
            let states_vec = trans_array.get(0);

            // Verify we have 3 state pointers
            assert_eq!(states_vec.len, 3);

            // The pointers should now point to buffer addresses (shifted from offsets)
            let ptrs = states_vec.as_ref();
            assert!(!ptrs[0].is_null());
            assert!(!ptrs[1].is_null());
            assert!(!ptrs[2].is_null());
        }
    }

    #[test]
    fn test_composition_demonstration() {
        // This test demonstrates the deep composition:
        // ArrMap<256, Vec<*const U8State>>
        //   where the Vec uses StatePointerDeanchize
        //   which contains a Shifter
        //   all composing perfectly!

        let origin = U8StatePrepared::Dense(U8DenseStatePrepared {
            tags: vec![999],
            trans: {
                let mut arr = std::array::from_fn(|_| Vec::new());
                // Multiple entries with state pointers
                arr[b'a' as usize] = vec![0, 1];
                arr[b'b' as usize] = vec![2, 3, 4];
                arr[b'z' as usize] = vec![5];
                arr
            },
        });

        let anchize = U8StateAnchize::<TestContext>::default();

        let mut context = TestContext {
            qptrs: vec![0, 0, 0, 0, 0, 0], // Will be filled
        };

        let mut sz = Reserve(0);
        anchize.reserve(&origin, &mut context, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let base = buf.as_ptr() as usize;
        context.qptrs = (0..6).map(|i| base + i * 8).collect();

        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            anchize.anchize::<()>(&origin, &mut context, cur.clone());
            let deanch = U8StateDeanchize::default();
            deanch.deanchize::<()>(cur.clone());
        }

        let state = unsafe { &*(buf.as_ptr() as *const U8State) };

        // Verify structure
        assert!(unsafe { state.is_dense() });
        assert_eq!(unsafe { state.get_tags() }, &[999]);

        // Verify all transitions are accessible
        unsafe {
            let trans = &state.dense.trans;
            assert_eq!(trans.get(b'a' as usize).len, 2);
            assert_eq!(trans.get(b'b' as usize).len, 3);
            assert_eq!(trans.get(b'z' as usize).len, 1);
            assert_eq!(trans.get(b'c' as usize).len, 0);
        }
    }

    // ============================================================================
    // Integration Test Helpers
    // ============================================================================

    // Context for state creation - implements GetQptrs
    struct StateCreationContext {
        qptrs: Vec<usize>,
    }

    impl GetQptrs for StateCreationContext {
        fn get_qptrs(&self) -> &Vec<usize> {
            &self.qptrs
        }
    }

    pub unsafe fn create_states<'a>(
        buf: &'a mut Vec<u8>,
        states: Vec<U8StatePrepared>,
    ) -> Vec<&'a U8State<'a>> {
        // Reserve space with dummy context (context not used in reserve phase)
        let mut sz = Reserve(0);
        let mut addrs = Vec::<usize>::new();
        let anch: U8StateAnchize<StateCreationContext> = U8StateAnchize::default();
        for state in &states {
            // Align and record address
            sz.add::<U8State>(0);
            addrs.push(sz.0);
            // Reserve the state itself
            let mut dummy_ctx = StateCreationContext { qptrs: vec![] };
            anch.reserve(state, &mut dummy_ctx, &mut sz);
        }

        // Allocate buffer
        buf.resize(sz.0 + std::mem::size_of::<usize>() * 16, 0); // Extra padding for alignment

        // Calculate actual base address after alignment
        let base = crate::blob::align_up_mut_ptr::<u8, u128>(buf.as_mut_ptr()) as *mut u8;

        // Anchize phase - write all states
        // qptrs stores OFFSETS relative to buffer start (not absolute pointers)
        let mut context = StateCreationContext { qptrs: addrs.clone() };

        let mut cur = BuildCursor::new(base);
        for state in &states {
            cur = cur.align::<U8State>();
            cur = anch.anchize(state, &mut context, cur);
        }

        // Deanchize phase - fix up pointers
        let deanch: U8StateDeanchize = U8StateDeanchize::default();

        let mut cur = BuildCursor::new(base);
        for _ in &states {
            cur = cur.align::<U8State>();
            cur = deanch.deanchize(cur);
        }

        // Return references to states
        addrs.iter().map(|&addr| &*(base.add(addr) as *const U8State)).collect()
    }

    pub fn expect_dense<'a, 'b>(iter: U8StateIterator<'a, 'b>) -> U8DenseStateIterator<'a> {
        match iter {
            U8StateIterator::Dense(iter) => iter,
            _ => unreachable!("Expected dense iterator"),
        }
    }

    pub fn expect_sparse<'a, 'b>(iter: U8StateIterator<'a, 'b>) -> U8SparseStateIterator<'a, 'b> {
        match iter {
            U8StateIterator::Sparse(iter) => iter,
            _ => unreachable!("Expected sparse iterator"),
        }
    }

    #[test]
    fn test_states() {
        use ancha::UnsafeIterator;

        // Manually construct prepared states that match the blob test
        // State 0: Dense state with transitions for 'a','b' -> multiple successors
        // State 1: Sparse state with a single pattern transition
        let prepared_states = vec![
            // State 0: Will be dense (256 array map)
            U8StatePrepared::Dense(U8DenseStatePrepared {
                tags: vec![],
                trans: {
                    let mut arr = std::array::from_fn(|_| Vec::new());
                    // 'a' (97) and 'b' (98) both point to state 0
                    arr[97] = vec![0, 1]; // 'a' -> states 0, 1
                    arr[98] = vec![0, 1]; // 'b' -> states 0, 1
                                          // 'd' through 'z' point to state 1
                    for i in b'd'..=b'z' {
                        arr[i as usize] = vec![1];
                    }
                    arr
                },
            }),
            // State 1: Sparse state with 2 empty buckets (hashmap_cap = 2)
            U8StatePrepared::Sparse(U8SparseStatePrepared {
                tags: vec![1, 2],
                explicit_trans: vec![vec![], vec![]], // 2 empty buckets
                pattern_trans: vec![(Guard::from_range((b'b', b'm')), vec![0])],
            }),
        ];

        let mut buf = vec![];
        let states = unsafe { create_states(&mut buf, prepared_states) };
        let state0 = states[0];
        let state1 = states[1];

        // Test case 1: Dense state, no match for 'c'
        let mut iter = expect_dense(unsafe { state0.iter_matches(&b'c') });
        assert!(unsafe { iter.next() }.is_none());

        // Test case 2: Dense state, match for 'a' -> two successors
        let mut iter = expect_dense(unsafe { state0.iter_matches(&b'a') });
        let mut succs = vec![unsafe { *iter.next().unwrap() }, unsafe { *iter.next().unwrap() }];
        assert!(unsafe { iter.next() }.is_none());
        succs.sort();
        assert_eq!(succs, [state0 as *const U8State, state1 as *const U8State]);

        // Test case 3: Dense state, match for 'p' -> one successor
        let mut iter = expect_dense(unsafe { state0.iter_matches(&b'p') });
        let succs = vec![unsafe { *iter.next().unwrap() }];
        assert!(unsafe { iter.next() }.is_none());
        assert_eq!(succs, vec![state1 as *const U8State]);

        // Test case 4: Sparse state, no match for 'a'
        let mut iter = expect_sparse(unsafe { state1.iter_matches(&b'a') });
        assert!(unsafe { iter.next() }.is_none());

        // Test case 5: Sparse state, match for 'c' -> one successor
        let mut iter = expect_sparse(unsafe { state1.iter_matches(&b'c') });
        let succs = vec![unsafe { iter.next().unwrap() }];
        assert!(unsafe { iter.next() }.is_none());
        assert_eq!(succs, vec![state0 as *const U8State]);

        // Test case 6: get_tags
        let no_tags: &[usize] = &[];
        assert_eq!(unsafe { state0.get_tags() }, no_tags);
        assert_eq!(unsafe { state1.get_tags() }, &[1usize, 2]);
    }
}
