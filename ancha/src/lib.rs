//! Ancha: Composable serialization system for high-performance blob data structures.
//!
//! The name "ancha" comes from "anchor" and the Slovak name "Anča".
//!
//! # Core Concept
//!
//! Serialization strategies are **composable objects** with customizable defaults:

// Core data structures (migrated to ancha system)
pub mod arrmap;
pub mod assoc_list;
pub mod bdd;
pub mod flagellum;
pub mod list;
pub mod listmap;
pub mod sediment;
pub mod tupellum;
pub mod vec;
pub mod vecmap;

// TODO: Migrate these from blob
// pub mod arrmap;
// pub mod assoc_list;
// pub mod automaton;
// pub mod flagellum;
pub mod hashmap;
// pub mod keyval_state;
// pub mod listmap; // Duplicate - already defined above
// pub mod state; // Moved to configmaton::my_ancha
// pub mod vecmap;

use std::mem::{align_of, size_of};

// Re-export commonly used types
pub use hashbrown::HashMap;

// ============================================================================
// Core serialization traits
// ============================================================================

/// Anchization: serialization of Origin → Ancha (blob).
///
/// This trait defines how to convert an origin representation into
/// a serialized blob representation.
pub trait Anchize<'a> {
    /// The origin type (pre-serialization)
    type Origin;
    type Context;

    /// The ancha type family (blob, parameterized by lifetime)
    ///
    /// For example: `Ancha<'a> = AnchaVec<'a, u8>`
    ///
    /// Note: The lifetime 'a is the lifetime of the blob itself.
    type Ancha;

    /// Reserve space for the blob.
    fn reserve(&self, origin: &Self::Origin, context: &mut Self::Context, sz: &mut Reserve);

    /// Serialize the origin into the blob.
    ///
    /// # Safety
    ///
    /// - `cur` must point to valid, allocated memory
    /// - Buffer must have sufficient space (as computed by `reserve`)
    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After>;
}

/// Deanchization: pointer fixup in the blob (deserialization).
///
/// This is origin-agnostic - it just fixes up pointers in place.
pub trait Deanchize<'a> {
    /// The ancha type family (blob, parameterized by lifetime)
    type Ancha;

    /// Fix up pointers in the blob.
    ///
    /// # Safety
    ///
    /// - `cur` must point to a properly initialized structure
    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After>;
}

/// Static anchization: for fixed-size types that can be mutated in place.
///
/// Examples: primitives (u8, usize), fixed-size structs
pub trait StaticAnchize<'a> {
    /// The origin type
    type Origin;
    type Context;

    /// The ancha type (no lifetime needed for fixed-size types)
    type Ancha;

    /// Serialize by mutating the ancha in place.
    fn anchize_static(
        &self,
        origin: &Self::Origin,
        context: &mut Self::Context,
        ancha: &mut Self::Ancha,
    );
}

pub trait StaticDeanchize<'a> {
    type Ancha;
    fn deanchize_static(&self, ancha: &mut Self::Ancha);
}

// ============================================================================
// Memory management utilities
// ============================================================================

/// Reserve space calculator.
#[derive(Clone, Copy)]
pub struct Reserve(pub usize);

impl Reserve {
    /// Add space for `n` instances of type `T`.
    ///
    /// Handles alignment automatically.
    pub fn add<T>(&mut self, n: usize) {
        self.0 = align_up(self.0, align_of::<T>());
        self.0 += n * size_of::<T>();
    }
}

/// Build cursor: tracks position during serialization.
///
/// Generic over `A` to enable type-safe cursor operations.
pub struct BuildCursor<A> {
    pub cur: usize,
    pub buf: *mut u8,
    _phantom: std::marker::PhantomData<A>,
}

impl<A> Clone for BuildCursor<A> {
    fn clone(&self) -> Self {
        BuildCursor { cur: self.cur, buf: self.buf, _phantom: std::marker::PhantomData }
    }
}

impl<A> BuildCursor<A> {
    /// Create a new cursor at the start of a buffer.
    pub fn new(buf: *mut u8) -> Self {
        BuildCursor { cur: 0, buf, _phantom: std::marker::PhantomData }
    }

    /// Get a mutable reference to the current position.
    ///
    /// # Safety
    ///
    /// - `cur` must point to valid, allocated memory with proper alignment
    pub unsafe fn get_mut(&self) -> &mut A {
        &mut *(self.buf.add(self.cur) as *mut A)
    }

    /// Move cursor to position behind current struct (with alignment).
    pub fn behind<B>(&self, n: usize) -> BuildCursor<B> {
        BuildCursor {
            cur: align_up(self.cur + n * size_of::<A>(), align_of::<B>()),
            buf: self.buf,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Increment cursor by one element.
    pub fn inc(&mut self) {
        self.cur += size_of::<A>();
    }

    /// Align cursor for type B.
    pub fn align<B>(&self) -> BuildCursor<B> {
        BuildCursor {
            cur: align_up(self.cur, align_of::<B>()),
            buf: self.buf,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Transmute cursor to different type (same position).
    pub fn transmute<B>(&self) -> BuildCursor<B> {
        BuildCursor { cur: self.cur, buf: self.buf, _phantom: std::marker::PhantomData }
    }

    /// Go to a specific field within the structure.
    pub fn goto<B>(&self, field: &mut B) -> BuildCursor<B> {
        BuildCursor {
            cur: field as *mut B as usize - self.buf as usize,
            buf: self.buf,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Shifter: converts offsets to pointers during deanchization.
#[derive(Clone, Copy)]
pub struct Shifter(pub *mut u8);

impl Shifter {
    /// Shift a pointer from offset to absolute address.
    ///
    /// # Safety
    ///
    /// - The offset must be valid within the buffer
    pub unsafe fn shift<T>(&self, ptr: &mut *const T) {
        *ptr = self.0.offset(*ptr as isize) as *const T;
    }
}

/// Align an address up to the given alignment.
pub fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// Get a pointer to data behind a struct with proper alignment.
///
/// # Safety
///
/// - `a` must point to a valid, initialized structure of type A
/// - There must be valid data of type B after the structure
/// - The alignment requirements of B must be satisfied
#[inline]
pub unsafe fn get_behind_struct<A, B>(a: *const A) -> *const B {
    align_up((a as *const u8).add(size_of::<A>()) as usize, align_of::<B>()) as *const B
}

// ============================================================================
// Default implementations for primitives
// ============================================================================

/// Direct copy: the default anchization for Copy types.
#[derive(Clone, Copy)]
pub struct CopyAnchize<'a, T, Ctx>(std::marker::PhantomData<&'a (T, Ctx)>);

impl<'a, T, Ctx> CopyAnchize<'a, T, Ctx> {
    pub fn new() -> Self {
        CopyAnchize(std::marker::PhantomData)
    }
}

impl<'a, T, Ctx> Default for CopyAnchize<'a, T, Ctx> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T: Copy, Ctx> StaticAnchize<'a> for CopyAnchize<'a, T, Ctx> {
    type Origin = T;
    type Ancha = T;
    type Context = Ctx;

    fn anchize_static(
        &self,
        origin: &Self::Origin,
        _context: &mut Self::Context,
        ancha: &mut Self::Ancha,
    ) {
        *ancha = *origin;
    }
}

#[derive(Clone, Copy)]
pub struct NoopDeanchize<'a, T>(std::marker::PhantomData<&'a T>);

impl<'a, T> NoopDeanchize<'a, T> {
    pub fn new() -> Self {
        NoopDeanchize(std::marker::PhantomData)
    }
}

impl<'a, T> Default for NoopDeanchize<'a, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T> StaticDeanchize<'a> for NoopDeanchize<'a, T> {
    type Ancha = T;
    fn deanchize_static(&self, _ancha: &mut Self::Ancha) {}
}

// ============================================================================
// Traits for Associative Containers
// ============================================================================

/// Trait for matching keys in associative containers.
///
/// # Safety
///
/// Implementations must ensure the match predicate is consistent and doesn't
/// access invalid memory.
pub trait Matches<T> {
    /// Check if this matcher matches the given value.
    ///
    /// # Safety
    ///
    /// The `other` reference must be valid and properly initialized.
    unsafe fn matches(&self, other: &T) -> bool;
}

/// Wrapper for equality-based matching.
pub struct EqMatch<'a, X>(pub &'a X);

impl<'a, X: Eq> Matches<X> for EqMatch<'a, X> {
    unsafe fn matches(&self, other: &X) -> bool {
        *self.0 == *other
    }
}

/// Matcher that matches any value (wildcard).
pub struct AnyMatch;

impl<T> Matches<T> for AnyMatch {
    unsafe fn matches(&self, _: &T) -> bool {
        true
    }
}

/// Trait for types that provide a hash value.
pub trait MyHash {
    /// Compute a hash value for this key.
    fn my_hash(&self) -> usize;
}

/// Trait for types that can be checked for emptiness.
pub trait IsEmpty {
    fn is_empty(&self) -> bool;
}

impl<X> IsEmpty for Vec<X> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

// ============================================================================
// UnsafeIterator Trait
// ============================================================================

/// Iterator trait for unsafe iteration over blob structures.
///
/// # Safety
///
/// Implementations must ensure that:
/// - The iterator only accesses memory within the valid blob
/// - Returned references have appropriate lifetimes
pub trait UnsafeIterator {
    /// The type of elements yielded by this iterator.
    type Item;

    /// Advance the iterator and return the next element.
    ///
    /// # Safety
    ///
    /// Caller must ensure the blob structure is valid and properly initialized.
    unsafe fn next(&mut self) -> Option<Self::Item>;
}

// ============================================================================
// Assocs Traits
// ============================================================================

/// Super-trait for associative containers, defining associated types.
pub trait AssocsSuper<'a> {
    /// The key type for this associative container.
    type Key: 'a;

    /// The value type for this associative container.
    type Val: 'a;

    /// The iterator type returned by `iter_matches`.
    type I<'b, X: 'b + Matches<Self::Key>>: UnsafeIterator<Item = (&'a Self::Key, &'a Self::Val)>
    where
        'a: 'b;
}

/// Trait for associative containers (maps) in ancha format.
pub trait Assocs<'a>: AssocsSuper<'a> {
    /// Create an iterator over key-value pairs matching the given key predicate.
    ///
    /// # Safety
    ///
    /// The container must be properly initialized and the buffer must remain
    /// valid for the lifetime 'a.
    unsafe fn iter_matches<'c, 'b, X: Matches<Self::Key>>(&'c self, key: &'b X) -> Self::I<'b, X>
    where
        'a: 'b + 'c;
}

/// Trait for single key-value associations.
pub trait Assoc<'a> {
    /// The key type.
    type Key: 'a;

    /// The value type.
    type Val: 'a;

    /// Get a reference to the key.
    ///
    /// # Safety
    ///
    /// The structure must be properly initialized.
    unsafe fn key(&self) -> &'a Self::Key;

    /// Get a reference to the value.
    ///
    /// # Safety
    ///
    /// The structure must be properly initialized.
    unsafe fn val(&self) -> &'a Self::Val;
}
