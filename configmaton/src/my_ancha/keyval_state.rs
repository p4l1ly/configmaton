//! Key-value state representation in the Ancha system.
//!
//! This module implements the serialization of key-value states used in the automaton.
//! It demonstrates deep composition of multiple Ancha structures:
//! - Tupellum (sequential composition)
//! - Sediment (packed arrays)
//! - List (intrusive linked lists)
//! - BlobVec (vectors)
//! - Bdd (binary decision diagrams)
//!
//! The structure involves two types of state pointers:
//! - KeyValState pointers (pointing to states in this structure)
//! - U8State pointers (pointing to character-level DFA states)

use ancha::{
    bdd::{AnchaBdd, BddAnchize, BddDeanchize},
    list::{AnchaList, ListAnchizeFromVec, ListDeanchize},
    sediment::{AnchaSediment, SedimentAnchizeFromVec, SedimentDeanchize},
    tupellum::{Tupellum, TupellumAnchizeFromRefs, TupellumDeanchizeFromTuple},
    vec::{AnchaVec, VecAnchizeFromVec, VecDeanchize},
    Anchize, BuildCursor, CopyAnchize, Deanchize, NoopDeanchize, Reserve, Shifter, StaticAnchize,
    StaticDeanchize,
};

use super::state::U8State;

// ============================================================================
// Type Aliases for the Structure
// ============================================================================

pub type Bytes<'a> = AnchaVec<'a, u8>;
pub type LeafMeta<'a> = Tupellum<'a, AnchaSediment<'a, Bytes<'a>>, AnchaSediment<'a, Bytes<'a>>>;
pub type Leaf0<'a> = Tupellum<'a, AnchaVec<'a, *const KeyValState<'a>>, LeafMeta<'a>>;
pub type Finals<'a> = AnchaBdd<'a, usize, Leaf0<'a>>;
pub type InitsAndFinals<'a> = Tupellum<'a, AnchaVec<'a, *const U8State<'a>>, Finals<'a>>;
pub type Tran0<'a> = Tupellum<'a, Bytes<'a>, InitsAndFinals<'a>>;
pub type KeyValStateSparse<'a> = AnchaList<'a, Tran0<'a>>;

// ============================================================================
// KeyValState Structure
// ============================================================================

#[repr(C)]
pub struct KeyValState<'a> {
    pub sparse: KeyValStateSparse<'a>,
}

// ============================================================================
// Origin Types
// ============================================================================

#[derive(Clone, Debug)]
pub struct LeafOrigin {
    pub states: Vec<usize>,
    pub get_olds: Vec<Vec<u8>>,
    pub exts: Vec<Vec<u8>>,
}

pub struct TranOrigin {
    pub key: Vec<u8>,
    pub dfa_inits: Vec<usize>,
    pub bdd: ancha::bdd::BddOrigin<usize, LeafOrigin>,
}

pub struct StateOrigin {
    pub transitions: Vec<TranOrigin>,
}

// ============================================================================
// Context Traits for State Pointers
// ============================================================================

/// Context trait for accessing both KeyValState and U8State pointer vectors.
pub trait GetQptrs {
    fn get_kvqptrs(&self) -> &Vec<usize>;
    fn get_u8qptrs(&self) -> &Vec<usize>;
}

// ============================================================================
// Pointer Anchization Strategies
// ============================================================================

/// Strategy for anchizing KeyValState pointers (usize index -> *const KeyValState).
#[derive(Clone, Copy, Default)]
pub struct KeyValStatePointerAnchize<Ctx> {
    _phantom: std::marker::PhantomData<Ctx>,
}

impl<'a, Ctx: GetQptrs> StaticAnchize<'a> for KeyValStatePointerAnchize<Ctx> {
    type Origin = usize;
    type Context = Ctx;
    type Ancha = *const KeyValState<'a>;

    fn anchize_static(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        ancha: &mut Self::Ancha,
    ) {
        let kvqptrs = context.get_kvqptrs();
        *ancha = kvqptrs[*origin] as *const KeyValState<'a>;
    }
}

/// Strategy for deanchizing KeyValState pointers.
#[derive(Clone, Copy)]
pub struct KeyValStatePointerDeanchize {
    shifter: Shifter,
}

impl KeyValStatePointerDeanchize {
    pub fn new(buf: *mut u8) -> Self {
        KeyValStatePointerDeanchize { shifter: Shifter(buf) }
    }
}

impl<'a> StaticDeanchize<'a> for KeyValStatePointerDeanchize {
    type Ancha = *const KeyValState<'a>;

    fn deanchize_static(&self, ancha: &mut Self::Ancha) {
        unsafe {
            self.shifter.shift(ancha);
        }
    }
}

/// Strategy for anchizing U8State pointers (reuse from state.rs).
#[derive(Clone, Copy, Default)]
pub struct U8StatePointerAnchize<Ctx> {
    _phantom: std::marker::PhantomData<Ctx>,
}

impl<'a, Ctx: GetQptrs> StaticAnchize<'a> for U8StatePointerAnchize<Ctx> {
    type Origin = usize;
    type Context = Ctx;
    type Ancha = *const U8State<'a>;

    fn anchize_static(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        ancha: &mut Self::Ancha,
    ) {
        let u8qptrs = context.get_u8qptrs();
        *ancha = u8qptrs[*origin] as *const U8State<'a>;
    }
}

/// Strategy for deanchizing U8State pointers.
#[derive(Clone, Copy)]
pub struct U8StatePointerDeanchize {
    shifter: Shifter,
}

impl U8StatePointerDeanchize {
    pub fn new(buf: *mut u8) -> Self {
        U8StatePointerDeanchize { shifter: Shifter(buf) }
    }
}

impl<'a> StaticDeanchize<'a> for U8StatePointerDeanchize {
    type Ancha = *const U8State<'a>;

    fn deanchize_static(&self, ancha: &mut Self::Ancha) {
        unsafe {
            self.shifter.shift(ancha);
        }
    }
}

// ============================================================================
// Leaf Anchization
// ============================================================================

pub struct LeafAnchize<Ctx> {
    _phantom: std::marker::PhantomData<Ctx>,
}

impl<Ctx> Default for LeafAnchize<Ctx> {
    fn default() -> Self {
        LeafAnchize { _phantom: std::marker::PhantomData }
    }
}

impl<'a, Ctx: GetQptrs> Anchize<'a> for LeafAnchize<Ctx> {
    type Origin = LeafOrigin;
    type Ancha = Leaf0<'a>;
    type Context = Ctx;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        // Tupellum of (Vec<*const KeyValState>, (Sediment<Vec<u8>>, Sediment<Vec<u8>>))
        let states_ancha: VecAnchizeFromVec<KeyValStatePointerAnchize<Ctx>> =
            VecAnchizeFromVec::new(KeyValStatePointerAnchize {
                _phantom: std::marker::PhantomData,
            });
        let get_olds_ancha: SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>> =
            SedimentAnchizeFromVec::new(VecAnchizeFromVec::default());
        let exts_ancha: SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>> =
            SedimentAnchizeFromVec::new(VecAnchizeFromVec::default());

        let meta_ancha: TupellumAnchizeFromRefs<
            SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>>,
            SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>>,
        > = TupellumAnchizeFromRefs::new(get_olds_ancha, exts_ancha);

        let leaf_ancha: TupellumAnchizeFromRefs<
            VecAnchizeFromVec<KeyValStatePointerAnchize<Ctx>>,
            TupellumAnchizeFromRefs<
                SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>>,
                SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>>,
            >,
        > = TupellumAnchizeFromRefs::new(states_ancha, meta_ancha);

        leaf_ancha.reserve(&(&origin.states, &(&origin.get_olds, &origin.exts)), context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let states_ancha: VecAnchizeFromVec<KeyValStatePointerAnchize<Ctx>> =
            VecAnchizeFromVec::new(KeyValStatePointerAnchize {
                _phantom: std::marker::PhantomData,
            });
        let get_olds_ancha: SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>> =
            SedimentAnchizeFromVec::new(VecAnchizeFromVec::default());
        let exts_ancha: SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>> =
            SedimentAnchizeFromVec::new(VecAnchizeFromVec::default());

        let meta_ancha: TupellumAnchizeFromRefs<
            SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>>,
            SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>>,
        > = TupellumAnchizeFromRefs::new(get_olds_ancha, exts_ancha);

        let leaf_ancha: TupellumAnchizeFromRefs<
            VecAnchizeFromVec<KeyValStatePointerAnchize<Ctx>>,
            TupellumAnchizeFromRefs<
                SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>>,
                SedimentAnchizeFromVec<VecAnchizeFromVec<CopyAnchize<u8, Ctx>>>,
            >,
        > = TupellumAnchizeFromRefs::new(states_ancha, meta_ancha);

        leaf_ancha.anchize(&(&origin.states, &(&origin.get_olds, &origin.exts)), context, cur)
    }
}

// ============================================================================
// Leaf Deanchization
// ============================================================================

pub struct LeafDeanchize {
    _phantom: std::marker::PhantomData<()>,
}

impl Default for LeafDeanchize {
    fn default() -> Self {
        LeafDeanchize { _phantom: std::marker::PhantomData }
    }
}

impl<'a> Deanchize<'a> for LeafDeanchize {
    type Ancha = Leaf0<'a>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let _shifter = Shifter(cur.buf);

        let leaf_deancha: TupellumDeanchizeFromTuple<
            VecDeanchize<'a, KeyValStatePointerDeanchize>,
            TupellumDeanchizeFromTuple<
                'a,
                SedimentDeanchize<'a, VecDeanchize<'a, NoopDeanchize<'a, u8>>>,
                SedimentDeanchize<'a, VecDeanchize<'a, NoopDeanchize<'a, u8>>>,
            >,
        > = TupellumDeanchizeFromTuple::new(
            VecDeanchize::new(KeyValStatePointerDeanchize::new(cur.buf)),
            TupellumDeanchizeFromTuple::new(
                SedimentDeanchize::new(VecDeanchize::default()),
                SedimentDeanchize::new(VecDeanchize::default()),
            ),
        );

        leaf_deancha.deanchize(cur)
    }
}

// ============================================================================
// Tran Anchization
// ============================================================================

pub struct TranAnchize<Ctx> {
    _phantom: std::marker::PhantomData<Ctx>,
}

impl<Ctx> Default for TranAnchize<Ctx> {
    fn default() -> Self {
        TranAnchize { _phantom: std::marker::PhantomData }
    }
}

impl<'a, Ctx: GetQptrs> Anchize<'a> for TranAnchize<Ctx> {
    type Origin = TranOrigin;
    type Ancha = Tran0<'a>;
    type Context = Ctx;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        // Tupellum of (Bytes, (Vec<*const U8State>, Finals))
        let key_ancha: VecAnchizeFromVec<CopyAnchize<u8, Ctx>> = VecAnchizeFromVec::default();
        let inits_ancha: VecAnchizeFromVec<U8StatePointerAnchize<Ctx>> =
            VecAnchizeFromVec::new(U8StatePointerAnchize { _phantom: std::marker::PhantomData });
        let finals_ancha: BddAnchize<CopyAnchize<usize, Ctx>, LeafAnchize<Ctx>> =
            BddAnchize::new(CopyAnchize::default(), LeafAnchize::default());

        let iaf_ancha: TupellumAnchizeFromRefs<
            VecAnchizeFromVec<U8StatePointerAnchize<Ctx>>,
            BddAnchize<CopyAnchize<usize, Ctx>, LeafAnchize<Ctx>>,
        > = TupellumAnchizeFromRefs::new(inits_ancha, finals_ancha);

        let tran_ancha: TupellumAnchizeFromRefs<
            VecAnchizeFromVec<CopyAnchize<u8, Ctx>>,
            TupellumAnchizeFromRefs<
                VecAnchizeFromVec<U8StatePointerAnchize<Ctx>>,
                BddAnchize<CopyAnchize<usize, Ctx>, LeafAnchize<Ctx>>,
            >,
        > = TupellumAnchizeFromRefs::new(key_ancha, iaf_ancha);

        tran_ancha.reserve(&(&origin.key, &(&origin.dfa_inits, &origin.bdd)), context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let key_ancha: VecAnchizeFromVec<CopyAnchize<u8, Ctx>> = VecAnchizeFromVec::default();
        let inits_ancha: VecAnchizeFromVec<U8StatePointerAnchize<Ctx>> =
            VecAnchizeFromVec::new(U8StatePointerAnchize { _phantom: std::marker::PhantomData });
        let finals_ancha: BddAnchize<CopyAnchize<usize, Ctx>, LeafAnchize<Ctx>> =
            BddAnchize::new(CopyAnchize::default(), LeafAnchize::default());

        let iaf_ancha: TupellumAnchizeFromRefs<
            VecAnchizeFromVec<U8StatePointerAnchize<Ctx>>,
            BddAnchize<CopyAnchize<usize, Ctx>, LeafAnchize<Ctx>>,
        > = TupellumAnchizeFromRefs::new(inits_ancha, finals_ancha);

        let tran_ancha: TupellumAnchizeFromRefs<
            VecAnchizeFromVec<CopyAnchize<u8, Ctx>>,
            TupellumAnchizeFromRefs<
                VecAnchizeFromVec<U8StatePointerAnchize<Ctx>>,
                BddAnchize<CopyAnchize<usize, Ctx>, LeafAnchize<Ctx>>,
            >,
        > = TupellumAnchizeFromRefs::new(key_ancha, iaf_ancha);

        tran_ancha.anchize(&(&origin.key, &(&origin.dfa_inits, &origin.bdd)), context, cur)
    }
}

// ============================================================================
// Tran Deanchization
// ============================================================================

pub struct TranDeanchize {
    _phantom: std::marker::PhantomData<()>,
}

impl Default for TranDeanchize {
    fn default() -> Self {
        TranDeanchize { _phantom: std::marker::PhantomData }
    }
}

impl<'a> Deanchize<'a> for TranDeanchize {
    type Ancha = Tran0<'a>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let _shifter = Shifter(cur.buf);

        let tran_deancha: TupellumDeanchizeFromTuple<
            VecDeanchize<'a, NoopDeanchize<'a, u8>>,
            TupellumDeanchizeFromTuple<
                'a,
                VecDeanchize<'a, U8StatePointerDeanchize>,
                BddDeanchize<'a, NoopDeanchize<'a, usize>, LeafDeanchize>,
            >,
        > = TupellumDeanchizeFromTuple::new(
            VecDeanchize::default(),
            TupellumDeanchizeFromTuple::new(
                VecDeanchize::new(U8StatePointerDeanchize::new(cur.buf)),
                BddDeanchize::new(NoopDeanchize::default(), LeafDeanchize::default()),
            ),
        );

        tran_deancha.deanchize(cur)
    }
}

// ============================================================================
// KeyValState Anchization
// ============================================================================

pub struct KeyValStateAnchize<Ctx> {
    _phantom: std::marker::PhantomData<Ctx>,
}

impl<Ctx> Default for KeyValStateAnchize<Ctx> {
    fn default() -> Self {
        KeyValStateAnchize { _phantom: std::marker::PhantomData }
    }
}

impl<'a, Ctx: GetQptrs> Anchize<'a> for KeyValStateAnchize<Ctx> {
    type Origin = StateOrigin;
    type Ancha = KeyValState<'a>;
    type Context = Ctx;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0);

        let sparse_ancha: ListAnchizeFromVec<TranAnchize<Ctx>> = ListAnchizeFromVec::default();
        sparse_ancha.reserve(&origin.transitions, context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur = cur.align::<Self::Ancha>();
        let state = &mut *cur.get_mut();
        let sparse_cur = cur.goto(&mut state.sparse);

        let sparse_ancha: ListAnchizeFromVec<TranAnchize<Ctx>> = ListAnchizeFromVec::default();
        sparse_ancha.anchize(&origin.transitions, context, sparse_cur)
    }
}

// ============================================================================
// KeyValState Deanchization
// ============================================================================

pub struct KeyValStateDeanchize {
    _phantom: std::marker::PhantomData<()>,
}

impl Default for KeyValStateDeanchize {
    fn default() -> Self {
        KeyValStateDeanchize { _phantom: std::marker::PhantomData }
    }
}

impl<'a> Deanchize<'a> for KeyValStateDeanchize {
    type Ancha = KeyValState<'a>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let cur = cur.align::<Self::Ancha>();
        let state = &mut *cur.get_mut();
        let sparse_cur = cur.goto(&mut state.sparse);

        let sparse_deancha: ListDeanchize<TranDeanchize> = ListDeanchize::default();
        sparse_deancha.deanchize(sparse_cur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ancha::bdd::BddOrigin;
    use ancha::sediment::{AnchaSediment, SedimentAnchizeFromVec, SedimentDeanchize};
    use ancha::BuildCursor;

    /// Test context that implements GetQptrs for both KV and U8 state pointers.
    struct TestContext {
        u8qptrs: Vec<usize>,
        kvqptrs: Vec<usize>,
    }

    impl GetQptrs for TestContext {
        fn get_kvqptrs(&self) -> &Vec<usize> {
            &self.kvqptrs
        }
        fn get_u8qptrs(&self) -> &Vec<usize> {
            &self.u8qptrs
        }
    }

    #[test]
    fn test_keyval_state_basic() {
        // Simple test: create a single KeyValState
        let state_origin = StateOrigin {
            transitions: vec![TranOrigin {
                key: b"test".to_vec(),
                dfa_inits: vec![0],
                bdd: BddOrigin::Leaf(LeafOrigin {
                    states: vec![0],
                    get_olds: vec![b"get1".to_vec()],
                    exts: vec![],
                }),
            }],
        };

        // Create mock state pointers
        let u8qptrs = vec![256usize];
        let kvqptrs = vec![1000usize];

        let mut context = TestContext { u8qptrs, kvqptrs };

        // Reserve phase
        let mut sz = Reserve(0);
        let kv_ancha: KeyValStateAnchize<TestContext> = KeyValStateAnchize::default();
        kv_ancha.reserve(&state_origin, &mut context, &mut sz);

        // Allocate buffer
        let mut buf = vec![0u8; sz.0];
        let base = buf.as_mut_ptr();

        // Anchize phase
        let cur = BuildCursor::new(base);
        let _cur = unsafe { kv_ancha.anchize::<()>(&state_origin, &mut context, cur) };

        // Deanchize phase
        let kv_deancha: KeyValStateDeanchize = KeyValStateDeanchize::default();
        let cur = BuildCursor::new(base);
        let _cur = unsafe { kv_deancha.deanchize::<()>(cur) };

        // Verify the structure was created
        let q0 = unsafe { &*(base as *const KeyValState) };

        // Basic sanity check - the structure exists and has expected alignment
        assert_eq!(q0 as *const _ as usize % std::mem::align_of::<KeyValState>(), 0);
    }

    #[test]
    fn test_leaf_origin() {
        let leaf = LeafOrigin {
            states: vec![0, 1, 2],
            get_olds: vec![b"old1".to_vec(), b"old2".to_vec()],
            exts: vec![b"ext1".to_vec()],
        };

        let mut context = TestContext { u8qptrs: Vec::new(), kvqptrs: vec![100, 200, 300] };

        let ancha: LeafAnchize<TestContext> = LeafAnchize::default();

        let mut sz = Reserve(0);
        ancha.reserve(&leaf, &mut context, &mut sz);

        let mut buf = vec![0u8; sz.0];
        let base = buf.as_mut_ptr();
        let cur = BuildCursor::new(base);

        unsafe {
            ancha.anchize::<()>(&leaf, &mut context, cur);
        }

        // Verify the leaf was created
        let leaf0 = unsafe { &*(base as *const Leaf0) };
        assert_eq!(leaf0.a.len, 3);
    }
}
