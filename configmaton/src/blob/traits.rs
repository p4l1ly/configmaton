//! Enhanced traits for composable blob serialization.
//!
//! This module provides traits that reduce boilerplate by:
//! 1. Providing default implementations for primitive types
//! 2. Enabling automatic composition of complex structures
//! 3. Eliminating repetitive closure patterns
//!
//! # Design Goals
//!
//! - **Defaults for primitives**: Types like `u8`, `usize`, `Guard` get automatic serialization
//! - **Composability**: Complex structures can be built from simpler ones automatically
//! - **Flexibility**: Can override defaults when custom behavior is needed
//! - **Zero overhead**: All traits should compile to the same machine code as manual implementation
//!
//! # Note on `Sized` Bounds
//!
//! All traits require `Sized` because:
//! - Blob structures have fixed-size headers (e.g., `BlobVec` is just `usize` + phantom)
//! - Variable-length content follows the header, accessed via methods like `behind()`
//! - We need `size_of::<Self>()` for the header during serialization
//!
//! The blob system supports variable-length data, but the header structures themselves
//! are always sized.

use super::{Build, BuildCursor, Reserve};

/// Trait for types that can serialize individual elements with default behavior.
///
/// This trait provides the serialization half of the blob protocol with
/// automatic defaults for common types.
///
/// # Default Implementation
///
/// For `Copy` types where `Origin` is also `Copy`, the default implementation
/// simply copies the value.
pub trait BlobSerialize: Build + Sized {
    /// Optional context passed during serialization.
    ///
    /// This allows passing extra information (like pointer tables) through
    /// the serialization process.
    type SerializeCtx;

    /// Serialize a single element.
    ///
    /// # Safety
    ///
    /// - `cur` must point to valid, allocated memory with proper alignment for `Self`
    /// - The buffer must have sufficient space (as computed by `reserve`)
    unsafe fn serialize_elem(
        origin: &Self::Origin,
        cur: BuildCursor<Self>,
        ctx: &Self::SerializeCtx,
    ) -> BuildCursor<()>;
}

/// Trait for types that can deserialize individual elements with default behavior.
///
/// This trait provides the deserialization half of the blob protocol with
/// automatic defaults for common types.
pub trait BlobDeserialize: Build + Sized {
    /// Optional context passed during deserialization.
    ///
    /// This allows passing extra information (like the buffer base pointer
    /// for the Shifter) through the deserialization process.
    type DeserializeCtx;

    /// Deserialize a single element (fix up pointers in place).
    ///
    /// # Safety
    ///
    /// - `cur` must point to a properly initialized structure
    /// - All offsets must be valid within the buffer
    unsafe fn deserialize_elem(
        cur: BuildCursor<Self>,
        _ctx: &Self::DeserializeCtx,
    ) -> BuildCursor<()> {
        // Default implementation for types that don't need deserialization
        cur.behind(1)
    }
}

/// Trait for types that can compute their space requirements automatically.
///
/// This provides the reserve phase with defaults for simple types.
pub trait BlobReserve: Build + Sized {
    /// Compute space requirements and return the address where this structure will be placed.
    ///
    /// The default implementation aligns to `Self` and adds space for one instance.
    fn reserve(_origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        my_addr
    }
}

// ============================================================================
// Default implementations for primitive types
// ============================================================================

/// Macro to implement blob traits for simple Copy types.
macro_rules! impl_blob_copy {
    ($t:ty) => {
        impl BlobSerialize for $t {
            type SerializeCtx = ();

            unsafe fn serialize_elem(
                origin: &Self::Origin,
                cur: BuildCursor<Self>,
                _ctx: &Self::SerializeCtx,
            ) -> BuildCursor<()> {
                *cur.get_mut() = *origin;
                cur.behind(1)
            }
        }

        impl BlobDeserialize for $t {
            type DeserializeCtx = ();
            // Uses default implementation (no-op)
        }

        impl BlobReserve for $t {
            // Uses default implementation
        }
    };
}

// Implement only for types that already have Build implementations
impl_blob_copy!(u8);
impl_blob_copy!(usize);
impl_blob_copy!(());

// Special case for Guard (from configmaton)
impl BlobSerialize for crate::guards::Guard {
    type SerializeCtx = ();

    unsafe fn serialize_elem(
        origin: &Self::Origin,
        cur: BuildCursor<Self>,
        _ctx: &Self::SerializeCtx,
    ) -> BuildCursor<()> {
        *cur.get_mut() = *origin;
        cur.behind(1)
    }
}

impl BlobDeserialize for crate::guards::Guard {
    type DeserializeCtx = ();
}

impl BlobReserve for crate::guards::Guard {
    // Uses default implementation
}

// ============================================================================
// Implementations for blob data structures
// ============================================================================

use super::vec::BlobVec;

impl<'a, X> BlobSerialize for BlobVec<'a, X>
where
    X: BlobSerialize + Build + Sized,
{
    type SerializeCtx = X::SerializeCtx;

    unsafe fn serialize_elem(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        ctx: &Self::SerializeCtx,
    ) -> BuildCursor<()> {
        (*cur.get_mut()).len = origin.len();
        let mut xcur: BuildCursor<X> = cur.behind(1);
        for x in origin.iter() {
            X::serialize_elem(x, xcur.clone(), ctx);
            xcur.inc();
        }
        xcur.align()
    }
}

impl<'a, X> BlobDeserialize for BlobVec<'a, X>
where
    X: BlobDeserialize + Build + Sized,
{
    type DeserializeCtx = X::DeserializeCtx;

    unsafe fn deserialize_elem(
        cur: BuildCursor<Self>,
        ctx: &Self::DeserializeCtx,
    ) -> BuildCursor<()> {
        let mut xcur: BuildCursor<X> = cur.behind(1);
        for _ in 0..(*cur.get_mut()).len {
            X::deserialize_elem(xcur.clone(), ctx);
            xcur.inc();
        }
        xcur.align()
    }
}

impl<'a, X> BlobReserve for BlobVec<'a, X>
where
    X: BlobReserve + Build,
{
    fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        for x in origin.iter() {
            X::reserve(x, sz);
        }
        sz.add::<X>(0);
        my_addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_u8() {
        // Test u8
        let origin = 42u8;
        let mut sz = Reserve(0);
        let addr = u8::reserve(&origin, &mut sz);
        assert_eq!(addr, 0);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());
        unsafe {
            u8::serialize_elem(&origin, cur.clone(), &());
            u8::deserialize_elem(cur, &());
        }

        let value = unsafe { *(buf.as_ptr() as *const u8) };
        assert_eq!(value, 42);
    }

    #[test]
    fn test_serialize_usize() {
        let origin = 12345usize;
        let mut sz = Reserve(0);
        let addr = usize::reserve(&origin, &mut sz);
        assert_eq!(addr, 0);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());
        unsafe {
            usize::serialize_elem(&origin, cur.clone(), &());
            usize::deserialize_elem(cur, &());
        }

        let value = unsafe { *(buf.as_ptr() as *const usize) };
        assert_eq!(value, 12345);
    }

    #[test]
    fn test_blobvec_with_traits() {
        use super::super::vec::BlobVec;

        // Test BlobVec<usize> using the new traits
        let origin = vec![1usize, 3, 5];
        let mut sz = Reserve(0);
        let addr = BlobVec::<usize>::reserve(&origin, &mut sz);
        assert_eq!(addr, 0);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            BlobVec::<usize>::serialize_elem(&origin, cur.clone(), &());
            BlobVec::<usize>::deserialize_elem(cur, &());
        }

        let blobvec = unsafe { &*(buf.as_ptr() as *const BlobVec<usize>) };
        assert_eq!(unsafe { blobvec.as_ref() }, &[1, 3, 5]);
    }
}
