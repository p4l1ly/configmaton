//! Composable serialization strategies.
//!
//! This module provides a different approach to blob serialization:
//! serialization strategies are **objects** that can be composed, customized,
//! and passed around, rather than just trait methods.
//!
//! # Key Idea
//!
//! Serialization is a relationship between an Origin type and a Target type,
//! mediated by a strategy object that knows how to convert between them.
//!
//! ```ignore
//! let default_ser = DirectSer::new();  // Origin::Var → Target::Var (direct copy)
//! let custom_ser = MultiplyBy2Ser::new();  // Multiply var by 2
//! let mapped_ser = DictMapSer::new(dict);  // Map via external dictionary
//!
//! let bdd_ser = BddSerialization {
//!     var_ser: custom_ser,  // Use custom strategy for vars
//!     leaf_ser: default_ser, // Use default for leaves
//! };
//!
//! bdd_ser.serialize(&origin, cur);
//! ```
//!
//! This enables:
//! - Multiple origins for the same blob type
//! - Customizable defaults
//! - Partial overrides (change just the var serialization, keep leaf default)
//! - Serialization strategies as first-class values

use super::{Build, BuildCursor, Reserve};

/// Dynamic serialization: for types with variable size that need cursor management.
///
/// Examples: BlobVec, Sediment, Bdd
///
/// Note: Target is parameterized by a lifetime because blob structures contain
/// lifetimes (for references within the blob). The actual lifetime is provided
/// at serialization time.
pub trait DynamicSerialization {
    /// The origin type (pre-serialization representation)
    type Origin;

    /// The target type family (blob representation parameterized by lifetime)
    ///
    /// For example, for BlobVec serialization, Target<'a> = BlobVec<'a, X>
    ///
    /// Note: The lifetime 'a is the lifetime of the blob itself, not the serialization
    /// strategy. The strategy can be used to serialize blobs with any lifetime.
    type Target<'a>: Build;

    /// Reserve space for serialization.
    ///
    /// Returns the address where the structure will be placed.
    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize;

    /// Serialize the origin into the blob at the given cursor.
    ///
    /// # Safety
    ///
    /// - `cur` must point to valid, allocated memory
    /// - Buffer must have sufficient space (as computed by `reserve`)
    ///
    /// Returns a cursor positioned after the serialized structure.
    unsafe fn serialize<'a, After>(
        &self,
        origin: &Self::Origin,
        cur: BuildCursor<Self::Target<'a>>,
    ) -> BuildCursor<After>;

    /// Deserialize (fix up pointers) in the blob.
    ///
    /// # Safety
    ///
    /// - `cur` must point to a properly initialized structure
    ///
    /// Returns a cursor positioned after the deserialized structure.
    unsafe fn deserialize<'a, After>(
        &self,
        cur: BuildCursor<Self::Target<'a>>,
    ) -> BuildCursor<After>;
}

/// Static serialization: for types with fixed size that can be mutated in place.
///
/// Examples: primitives (u8, usize), Guard, fixed-size structs
pub trait StaticSerialization {
    /// The origin type
    type Origin;

    /// The target type
    type Target;

    /// Serialize by mutating the target in place.
    ///
    /// # Safety
    ///
    /// - `target` must point to valid, allocated memory
    fn serialize(&self, origin: &Self::Origin, target: &mut Self::Target);
}

// ============================================================================
// Default implementations for primitives
// ============================================================================

/// Direct serialization: just copy the value.
///
/// Works for any type where Origin == Target and both are Copy.
pub struct DirectCopy<T>(std::marker::PhantomData<T>);

impl<T> DirectCopy<T> {
    pub fn new() -> Self {
        DirectCopy(std::marker::PhantomData)
    }
}

impl<T> Default for DirectCopy<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy> StaticSerialization for DirectCopy<T> {
    type Origin = T;
    type Target = T;

    fn serialize(&self, origin: &Self::Origin, target: &mut Self::Target) {
        *target = *origin;
    }
}

// Convenient type aliases
pub type U8Ser = DirectCopy<u8>;
pub type UsizeSer = DirectCopy<usize>;

// ============================================================================
// Example: Custom serialization that multiplies by 2
// ============================================================================

/// Example custom serialization: multiply integer by 2.
pub struct MultiplyBy2;

impl StaticSerialization for MultiplyBy2 {
    type Origin = usize;
    type Target = usize;

    fn serialize(&self, origin: &Self::Origin, target: &mut Self::Target) {
        *target = *origin * 2;
    }
}

// ============================================================================
// Helper for making DynamicSerialization from StaticSerialization
// ============================================================================

/// Adapter to lift StaticSerialization into DynamicSerialization.
///
/// This is useful when you have a StaticSerialization strategy but need
/// to use it in a context that expects DynamicSerialization.
pub struct StaticToDynamic<S>(pub S);

impl<S> StaticToDynamic<S>
where
    S: StaticSerialization,
    S::Target: Build<Origin = S::Origin> + Sized,
{
    pub fn new(inner: S) -> Self {
        StaticToDynamic(inner)
    }
}

impl<S> DynamicSerialization for StaticToDynamic<S>
where
    S: StaticSerialization,
    S::Target: Build<Origin = S::Origin> + Sized + Copy,
{
    type Origin = S::Origin;
    type Target<'a> = S::Target;

    fn reserve(&self, _origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<S::Target>(0);
        let my_addr = sz.0;
        sz.add::<S::Target>(1);
        my_addr
    }

    unsafe fn serialize<'a, After>(
        &self,
        origin: &Self::Origin,
        cur: BuildCursor<Self::Target<'a>>,
    ) -> BuildCursor<After> {
        self.0.serialize(origin, &mut *cur.get_mut());
        cur.behind(1)
    }

    unsafe fn deserialize<'a, After>(
        &self,
        cur: BuildCursor<Self::Target<'a>>,
    ) -> BuildCursor<After> {
        // No fixup needed for primitives
        cur.behind(1)
    }
}

// ============================================================================
// BlobVec serialization
// ============================================================================

use super::vec::BlobVec;

/// Serialization strategy for BlobVec with customizable element serialization.
///
/// This shows the power of the composable approach: you can choose
/// different element serialization strategies!
pub struct BlobVecSer<ElemSer> {
    pub elem_ser: ElemSer,
}

impl<ElemSer> BlobVecSer<ElemSer> {
    pub fn new(elem_ser: ElemSer) -> Self {
        BlobVecSer { elem_ser }
    }
}

impl<ElemSer> DynamicSerialization for BlobVecSer<ElemSer>
where
    ElemSer: StaticSerialization,
    ElemSer::Target: Build<Origin = ElemSer::Origin> + Sized + 'static,
{
    type Origin = Vec<ElemSer::Origin>;
    type Target<'a> = BlobVec<'a, ElemSer::Target>;

    fn reserve(&self, origin: &Self::Origin, sz: &mut Reserve) -> usize {
        sz.add::<BlobVec<ElemSer::Target>>(0);
        let my_addr = sz.0;
        sz.add::<BlobVec<ElemSer::Target>>(1);
        for _x in origin.iter() {
            // For fixed-size elements, reserve doesn't need the strategy
            // But we could extend this to support DynamicSerialization elements too
            sz.add::<ElemSer::Target>(1);
        }
        sz.add::<ElemSer::Target>(0);
        my_addr
    }

    unsafe fn serialize<'a, After>(
        &self,
        origin: &Self::Origin,
        cur: BuildCursor<Self::Target<'a>>,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).len = origin.len();
        let mut xcur: BuildCursor<ElemSer::Target> = cur.behind(1);

        for x in origin.iter() {
            // Use the element serialization strategy!
            self.elem_ser.serialize(x, &mut *xcur.get_mut());
            xcur.inc();
        }

        xcur.align()
    }

    unsafe fn deserialize<'a, After>(
        &self,
        cur: BuildCursor<Self::Target<'a>>,
    ) -> BuildCursor<After> {
        // No fixup needed for fixed-size elements
        let xcur: BuildCursor<ElemSer::Target> = cur.behind(1);
        let len = (*cur.get_mut()).len;
        let mut result_cur = xcur.clone();
        for _ in 0..len {
            result_cur.inc();
        }
        result_cur.align()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_copy() {
        let ser = DirectCopy::<u8>::new();
        let origin = 42u8;
        let mut target = 0u8;
        ser.serialize(&origin, &mut target);
        assert_eq!(target, 42);
    }

    #[test]
    fn test_multiply_by_2() {
        let ser = MultiplyBy2;
        let origin = 21usize;
        let mut target = 0usize;
        ser.serialize(&origin, &mut target);
        assert_eq!(target, 42);
    }

    #[test]
    fn test_static_to_dynamic() {
        let ser = StaticToDynamic::new(DirectCopy::<usize>::new());
        let origin = 42usize;

        let mut sz = Reserve(0);
        let addr = ser.reserve(&origin, &mut sz);
        assert_eq!(addr, 0);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            ser.serialize::<()>(&origin, cur.clone());
            ser.deserialize::<()>(cur);
        }

        let value = unsafe { *(buf.as_ptr() as *const usize) };
        assert_eq!(value, 42);
    }

    #[test]
    fn test_blobvec_with_default() {
        // BlobVec<u8> with default (direct copy) serialization
        let ser = BlobVecSer::new(DirectCopy::<u8>::new());
        let origin = vec![1u8, 2, 3];

        let mut sz = Reserve(0);
        let addr = ser.reserve(&origin, &mut sz);
        assert_eq!(addr, 0);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            ser.serialize::<()>(&origin, cur.clone());
            ser.deserialize::<()>(cur);
        }

        let blobvec = unsafe { &*(buf.as_ptr() as *const BlobVec<u8>) };
        assert_eq!(unsafe { blobvec.as_ref() }, &[1, 2, 3]);
    }

    #[test]
    fn test_blobvec_with_custom() {
        // BlobVec<usize> with custom serialization (multiply by 2)
        let ser = BlobVecSer::new(MultiplyBy2);
        let origin = vec![1usize, 2, 3];

        let mut sz = Reserve(0);
        let addr = ser.reserve(&origin, &mut sz);
        assert_eq!(addr, 0);

        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        unsafe {
            ser.serialize::<()>(&origin, cur.clone());
            ser.deserialize::<()>(cur);
        }

        let blobvec = unsafe { &*(buf.as_ptr() as *const BlobVec<usize>) };
        // Elements should be multiplied by 2!
        assert_eq!(unsafe { blobvec.as_ref() }, &[2, 4, 6]);
    }
}
