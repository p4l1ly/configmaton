//! Automaton type aliases and serialization for the Ancha system.
//!
//! This module defines the top-level automaton structure as a composition
//! of the keyval_state and state types, plus custom serialization that
//! tracks state offsets for pointer fixup.

use super::keyval_state::{
    Bytes, GetQptrs as KeyValGetQptrs, KeyValState, KeyValStateAnchize, KeyValStateDeanchize,
    StateOrigin,
};
use super::state::{GetQptrs, U8State, U8StateAnchize, U8StateDeanchize, U8StatePrepared};
use ancha::{
    sediment::AnchaSediment,
    sediment::{SedimentAnchizeFromVec, SedimentDeanchize},
    tupellum::Tupellum,
    tupellum::TupellumDeanchizeFromTuple,
    vec::AnchaVec,
    vec::{VecAnchizeFromVec, VecDeanchize},
    Anchize, BuildCursor, CopyAnchize, Deanchize, NoopDeanchize, Reserve,
};

/// Sediment containing KeyValState and U8State instances.
pub type States<'a> =
    Tupellum<'a, AnchaSediment<'a, KeyValState<'a>>, AnchaSediment<'a, U8State<'a>>>;

/// Vector of KeyValState pointers with the States structure.
pub type InitsAndStates<'a> = Tupellum<'a, AnchaVec<'a, *const KeyValState<'a>>, States<'a>>;

/// Extensions (Exts) and the automaton core.
pub type ExtsAndAut<'a> = Tupellum<
    'a,
    AnchaSediment<'a, Bytes<'a>>, // Exts
    InitsAndStates<'a>,
>;

/// Complete automaton structure: GetOlds + ExtsAndAut.
pub type Automaton<'a> = Tupellum<
    'a,
    AnchaSediment<'a, Bytes<'a>>, // GetOlds
    ExtsAndAut<'a>,
>;

// ============================================================================
// Context for Automaton Serialization with Offset Tracking
// ============================================================================

/// Trait for contexts that can track offsets during sediment anchization.
pub trait TrackKvOffset {
    fn push_kvqptr(&mut self, offset: usize);
}

pub trait TrackU8Offset {
    fn push_u8qptr(&mut self, offset: usize);
}

/// Context for automaton serialization that provides both U8State and KeyValState pointers.
#[derive(Default)]
pub struct AutomatonContext {
    pub u8qptrs: Vec<usize>,
    pub kvqptrs: Vec<usize>,
}

impl TrackKvOffset for AutomatonContext {
    fn push_kvqptr(&mut self, offset: usize) {
        self.kvqptrs.push(offset);
    }
}

impl TrackU8Offset for AutomatonContext {
    fn push_u8qptr(&mut self, offset: usize) {
        self.u8qptrs.push(offset);
    }
}

impl KeyValGetQptrs for AutomatonContext {
    fn get_kvqptrs(&self) -> &Vec<usize> {
        &self.kvqptrs
    }

    fn get_u8qptrs(&self) -> &Vec<usize> {
        &self.u8qptrs
    }
}

impl GetQptrs for AutomatonContext {
    fn get_qptrs(&self) -> &Vec<usize> {
        &self.u8qptrs
    }
}

// ============================================================================
// Wrapper Anchizers that Track Offsets
// ============================================================================

use std::marker::PhantomData;

/// Wrapper anchizer that tracks KeyValState offset before calling inner anchizer.
pub struct TrackKvAnchize<Inner> {
    pub inner: Inner,
    _phantom: PhantomData<Inner>,
}

impl<Inner: Default> Default for TrackKvAnchize<Inner> {
    fn default() -> Self {
        TrackKvAnchize { inner: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, Inner> Anchize<'a> for TrackKvAnchize<Inner>
where
    Inner: Anchize<'a>,
    Inner::Context: TrackKvOffset,
{
    type Origin = Inner::Origin;
    type Ancha = Inner::Ancha;
    type Context = Inner::Context;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0);
        context.push_kvqptr(sz.0);
        self.inner.reserve(origin, context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        self.inner.anchize(origin, context, cur)
    }
}

/// Wrapper anchizer that tracks U8State offset before calling inner anchizer.
pub struct TrackU8Anchize<Inner> {
    pub inner: Inner,
    _phantom: PhantomData<Inner>,
}

impl<Inner: Default> Default for TrackU8Anchize<Inner> {
    fn default() -> Self {
        TrackU8Anchize { inner: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, Inner> Anchize<'a> for TrackU8Anchize<Inner>
where
    Inner: Anchize<'a>,
    Inner::Context: TrackU8Offset,
{
    type Origin = Inner::Origin;
    type Ancha = Inner::Ancha;
    type Context = Inner::Context;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0);
        context.push_u8qptr(sz.0);
        self.inner.reserve(origin, context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        self.inner.anchize(origin, context, cur)
    }
}

// ============================================================================
// Automaton Anchization
// ============================================================================

// Origin types are not needed - the composite anchizer defines its own Origin type

pub struct AutomatonAnchize {
    _phantom: std::marker::PhantomData<()>,
}

impl Default for AutomatonAnchize {
    fn default() -> Self {
        AutomatonAnchize { _phantom: std::marker::PhantomData }
    }
}

impl<'a> Anchize<'a> for AutomatonAnchize {
    // The origin type is determined by the composition of TupellumAnchizeFromRefs
    // It has the structure: (&Vec<Vec<u8>>, &(&Vec<Vec<u8>>, &(&Vec<usize>, &(&Vec<StateOrigin>, &Vec<U8StatePrepared>))))
    type Origin = (
        &'a Vec<Vec<u8>>, // GetOlds
        &'a (
            &'a Vec<Vec<u8>>, // Exts
            &'a (
                &'a Vec<usize>,                                       // Inits
                &'a (&'a Vec<StateOrigin>, &'a Vec<U8StatePrepared>), // States
            ),
        ),
    );
    type Ancha = Automaton<'a>;
    type Context = AutomatonContext;

    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve) {
        use ancha::tupellum::TupellumAnchizeFromRefs;

        // GetOlds sediment (standard)
        let getolds_anch: SedimentAnchizeFromVec<
            VecAnchizeFromVec<CopyAnchize<u8, AutomatonContext>>,
        > = SedimentAnchizeFromVec::new(VecAnchizeFromVec::default());

        // Exts sediment (standard)
        let exts_anch: SedimentAnchizeFromVec<
            VecAnchizeFromVec<CopyAnchize<u8, AutomatonContext>>,
        > = SedimentAnchizeFromVec::new(VecAnchizeFromVec::default());

        // Inits vector
        let inits_anch: VecAnchizeFromVec<
            super::keyval_state::KeyValStatePointerAnchize<AutomatonContext>,
        > = VecAnchizeFromVec::new(super::keyval_state::KeyValStatePointerAnchize::default());

        // KV states sediment - wraps KeyValStateAnchize to track offsets
        let kvstates_anch: SedimentAnchizeFromVec<
            TrackKvAnchize<KeyValStateAnchize<AutomatonContext>>,
        > = SedimentAnchizeFromVec::default();

        // U8 states sediment - wraps U8StateAnchize to track offsets
        let u8states_anch: SedimentAnchizeFromVec<
            TrackU8Anchize<U8StateAnchize<AutomatonContext>>,
        > = SedimentAnchizeFromVec::default();

        // Build the compositional structure
        let states_anch = TupellumAnchizeFromRefs::new(kvstates_anch, u8states_anch);
        let inits_and_states_anch = TupellumAnchizeFromRefs::new(inits_anch, states_anch);
        let exts_and_aut_anch = TupellumAnchizeFromRefs::new(exts_anch, inits_and_states_anch);
        let automaton_anch = TupellumAnchizeFromRefs::new(getolds_anch, exts_and_aut_anch);

        automaton_anch.reserve(origin, context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        use ancha::tupellum::TupellumAnchizeFromRefs;

        // Same structure as reserve
        let getolds_anch: SedimentAnchizeFromVec<
            VecAnchizeFromVec<CopyAnchize<u8, AutomatonContext>>,
        > = SedimentAnchizeFromVec::new(VecAnchizeFromVec::default());
        let exts_anch: SedimentAnchizeFromVec<
            VecAnchizeFromVec<CopyAnchize<u8, AutomatonContext>>,
        > = SedimentAnchizeFromVec::new(VecAnchizeFromVec::default());
        let inits_anch: VecAnchizeFromVec<
            super::keyval_state::KeyValStatePointerAnchize<AutomatonContext>,
        > = VecAnchizeFromVec::new(super::keyval_state::KeyValStatePointerAnchize::default());
        let kvstates_anch: SedimentAnchizeFromVec<
            TrackKvAnchize<KeyValStateAnchize<AutomatonContext>>,
        > = SedimentAnchizeFromVec::default();
        let u8states_anch: SedimentAnchizeFromVec<
            TrackU8Anchize<U8StateAnchize<AutomatonContext>>,
        > = SedimentAnchizeFromVec::default();

        let states_anch = TupellumAnchizeFromRefs::new(kvstates_anch, u8states_anch);
        let inits_and_states_anch = TupellumAnchizeFromRefs::new(inits_anch, states_anch);
        let exts_and_aut_anch = TupellumAnchizeFromRefs::new(exts_anch, inits_and_states_anch);
        let automaton_anch = TupellumAnchizeFromRefs::new(getolds_anch, exts_and_aut_anch);

        automaton_anch.anchize(origin, context, cur)
    }
}

// ============================================================================
// Automaton Deanchization
// ============================================================================

pub struct AutomatonDeanchize {
    _phantom: std::marker::PhantomData<()>,
}

impl Default for AutomatonDeanchize {
    fn default() -> Self {
        AutomatonDeanchize { _phantom: std::marker::PhantomData }
    }
}

impl<'a> Deanchize<'a> for AutomatonDeanchize {
    type Ancha = Automaton<'a>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        // GetOlds sediment
        let getolds_deancha: SedimentDeanchize<VecDeanchize<NoopDeanchize<u8>>> =
            SedimentDeanchize::new(VecDeanchize::default());

        // Exts sediment
        let exts_deancha: SedimentDeanchize<VecDeanchize<NoopDeanchize<u8>>> =
            SedimentDeanchize::new(VecDeanchize::default());

        // Inits vector (KV state pointers)
        let inits_deancha: VecDeanchize<super::keyval_state::KeyValStatePointerDeanchize> =
            VecDeanchize::new(super::keyval_state::KeyValStatePointerDeanchize::new(cur.buf));

        // KV states sediment
        let kvstates_deancha: SedimentDeanchize<KeyValStateDeanchize> =
            SedimentDeanchize::new(KeyValStateDeanchize::default());

        // U8 states sediment
        let u8states_deancha: SedimentDeanchize<U8StateDeanchize> =
            SedimentDeanchize::new(U8StateDeanchize::default());

        // States = Tupellum<Sediment<KV>, Sediment<U8>>
        let states_deancha = TupellumDeanchizeFromTuple::new(kvstates_deancha, u8states_deancha);

        // InitsAndStates = Tupellum<Vec<*const KV>, States>
        let inits_and_states_deancha =
            TupellumDeanchizeFromTuple::new(inits_deancha, states_deancha);

        // ExtsAndAut = Tupellum<Sediment<Bytes>, InitsAndStates>
        let exts_and_aut_deancha =
            TupellumDeanchizeFromTuple::new(exts_deancha, inits_and_states_deancha);

        // Automaton = Tupellum<Sediment<Bytes>, ExtsAndAut>
        let automaton_deancha =
            TupellumDeanchizeFromTuple::new(getolds_deancha, exts_and_aut_deancha);

        automaton_deancha.deanchize(cur)
    }
}
