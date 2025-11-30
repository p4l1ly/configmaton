//! # Blob Serialization System
//!
//! A high-performance, zero-copy serialization framework for complex data structures.
//! Similar in spirit to Cap'n Proto, but with more flexibility for direct memory layout control.
//!
//! ## Architecture
//!
//! The blob system provides three phases for each data structure:
//!
//! 1. **Reserve**: Calculate space requirements and determine layout
//! 2. **Serialize**: Write data to buffer, converting from origin types to blob format
//! 3. **Deserialize**: Fix up pointers in-place (convert offsets to absolute pointers)
//!
//! ## Key Features
//!
//! - **Zero-copy deserialization**: Pointers are fixed up in place, no data copying
//! - **Custom layouts**: Full control over memory layout (arrays, linked lists, hashmaps, etc.)
//! - **Type-safe cursors**: `BuildCursor<T>` provides type-safe buffer manipulation
//! - **Alignment handling**: Automatic alignment for all types
//! - **Flexible composition**: Structures can be nested arbitrarily
//!
//! ## Memory Safety
//!
//! This module uses extensive unsafe code for performance. The safety invariants are:
//! - Buffers must be large enough (verified via Reserve phase)
//! - Alignment requirements must be met (handled by BuildCursor)
//! - Pointers are only valid within the same buffer
//! - Lifetimes ensure buffer outlives all references
//!
//! ## Example Usage
//!
//! ```ignore
//! // 1. Reserve phase: calculate size
//! let origin = vec![1usize, 2, 3];
//! let mut sz = Reserve(0);
//! BlobVec::<usize>::reserve(&origin, &mut sz);
//!
//! // 2. Allocate buffer
//! let mut buf = vec![0u8; sz.0];
//!
//! // 3. Serialize phase: write to buffer
//! let mut cur = BuildCursor::new(buf.as_mut_ptr());
//! cur = unsafe { BlobVec::<usize>::serialize(&origin, cur, |x, y| *y = *x) };
//!
//! // 4. Deserialize phase: fix up pointers
//! let mut cur = BuildCursor::new(buf.as_mut_ptr());
//! cur = unsafe { BlobVec::<usize>::deserialize(cur, |_| ()) };
//!
//! // 5. Use the deserialized structure
//! let blobvec = unsafe { &*(buf.as_ptr() as *const BlobVec<usize>) };
//! assert_eq!(unsafe { blobvec.as_ref() }, &[1, 2, 3]);
//! ```
//!
//! ## Limitations
//!
//! - No endianness handling (only little-endian)
//! - Pointers are platform-specific (not portable across architectures)
//! - Requires unsafe code for usage

// WARNING: No endianness handling is implemented yet, as we have no use case for BigEndian.

use std::marker::PhantomData;
use std::mem::{align_of, size_of};

use twox_hash::XxHash64;

use crate::guards::Guard;
use vec::BlobVec;

pub mod arrmap;
pub mod assoc_list;
pub mod automaton;
pub mod bdd;
pub mod flagellum;
pub mod hashmap;
pub mod keyval_state;
pub mod list;
pub mod listmap;
pub mod sediment;
pub mod ser;
pub mod state;
pub mod tupellum;
pub mod vec;
pub mod vecmap;

/// Trait for computing hash values compatible with blob hash maps.
///
/// This trait provides a simple hashing interface for keys in `BlobHashMap`.
/// The hash value is used directly for bucket selection.
pub trait MyHash {
    /// Compute a hash value for this key.
    ///
    /// The hash should be well-distributed to minimize collisions.
    fn my_hash(&self) -> usize;
}

impl MyHash for u8 {
    fn my_hash(&self) -> usize {
        *self as usize
    }
}

impl MyHash for &[u8] {
    fn my_hash(&self) -> usize {
        XxHash64::oneshot(1234, self) as usize
    }
}

/// Trait for matching values during iteration over associative containers.
///
/// This trait allows flexible matching strategies:
/// - Exact equality matching
/// - Range matching (for guards)
/// - Wildcard matching (match everything)
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
///
/// This matcher uses `==` to check if values match.
pub struct EqMatch<'a, X>(pub &'a X);

impl<'a, X: Eq> Matches<X> for EqMatch<'a, X> {
    unsafe fn matches(&self, other: &X) -> bool {
        *self.0 == *other
    }
}

impl Matches<Guard> for u8 {
    unsafe fn matches(&self, other: &Guard) -> bool {
        other.contains(*self)
    }
}

impl<'a, 'b> Matches<BlobVec<'a, u8>> for &'b [u8] {
    unsafe fn matches(&self, other: &BlobVec<'a, u8>) -> bool {
        *self == other.as_ref()
    }
}

/// Matcher that matches any value (wildcard).
///
/// Used when iterating over all elements in an associative container.
pub struct AnyMatch;

impl<T> Matches<T> for AnyMatch {
    unsafe fn matches(&self, _: &T) -> bool {
        true
    }
}

/// Iterator trait for blob structures.
///
/// Unlike standard `Iterator`, this trait has an unsafe `next()` method because:
/// - Iterators may access uninitialized or raw memory
/// - References returned may have incorrect lifetimes if buffer is invalid
/// - Used for performance-critical code where safety checks are externalized
///
/// # Safety
///
/// Callers must ensure:
/// - The blob structure being iterated is properly initialized
/// - The buffer containing the data remains valid for the lifetime 'a
/// - No concurrent modifications to the buffer occur during iteration
pub trait UnsafeIterator {
    /// The type of elements yielded by this iterator.
    type Item;

    /// Advance the iterator and return the next element.
    ///
    /// # Safety
    ///
    /// See trait-level safety documentation.
    unsafe fn next(&mut self) -> Option<Self::Item>;
}

pub struct FakeSafeIterator<T: UnsafeIterator>(pub T);

impl<T: UnsafeIterator> Iterator for FakeSafeIterator<T> {
    type Item = T::Item;
    fn next(&mut self) -> Option<Self::Item> {
        unsafe { self.0.next() }
    }
}

/// Align an offset up to the next alignment boundary.
///
/// # Example
///
/// ```ignore
/// align_up(5, 4) == 8
/// align_up(8, 4) == 8
/// ```
fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}

/// Align a mutable pointer up to the alignment of type B.
///
/// # Safety
///
/// The resulting pointer may point past the original allocation.
/// Caller must ensure sufficient space is available.
pub fn align_up_mut_ptr<A, B>(a: *mut A) -> *mut B {
    align_up(a as usize, align_of::<B>()) as *mut B
}

/// Align a const pointer up to the alignment of type B.
///
/// # Safety
///
/// The resulting pointer may point past the original allocation.
/// Caller must ensure sufficient space is available.
pub fn align_up_ptr<A, B>(a: *const A) -> *const B {
    align_up(a as usize, align_of::<B>()) as *const B
}

/// Get a pointer to the data immediately after a struct, with proper alignment.
///
/// This is commonly used to access inline data that follows a header structure.
///
/// # Safety
///
/// - `a` must point to a valid, initialized structure of type A
/// - There must be valid data of type B after the structure
/// - The alignment requirements of B must be satisfied
pub unsafe fn get_behind_struct<A, B>(a: *const A) -> *const B {
    align_up((a as *const u8).add(size_of::<A>()) as usize, align_of::<B>()) as *const B
}

/// Tracks the size and alignment requirements during the reserve phase.
///
/// This accumulator is used to calculate how much space a blob structure
/// needs before serialization. It automatically handles alignment padding.
///
/// # Example
///
/// ```ignore
/// let mut sz = Reserve(0);
/// sz.add::<u8>(5);      // Add 5 bytes
/// sz.add::<u64>(2);     // Add 2×8 bytes, with alignment padding
/// let total = sz.0;     // Get total size including padding
/// ```
pub struct Reserve(pub usize);

impl Reserve {
    /// Add space for `n` elements of type `T`, including alignment padding.
    ///
    /// This method:
    /// 1. Aligns the current offset to T's alignment requirement
    /// 2. Adds space for `n` instances of T
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut sz = Reserve(0);
    /// sz.add::<u8>(3);      // offset = 3
    /// sz.add::<u32>(1);     // offset = 4 (aligned) + 4 = 8
    /// ```
    pub fn add<T>(&mut self, n: usize) {
        self.0 = align_up(self.0, align_of::<T>()) + size_of::<T>() * n;
    }
}

/// Type-safe cursor for building blob structures in a buffer.
///
/// `BuildCursor<A>` tracks:
/// - Current offset in the buffer (`cur`)
/// - Base pointer to the buffer (`buf`)
/// - Expected type at current position (phantom `A`)
///
/// The type parameter `A` provides compile-time safety: methods ensure
/// the cursor points to memory suitable for type `A`.
///
/// # Type Safety
///
/// Cursors can be transmuted to different types, but this is only safe
/// if the new type's alignment and size requirements are compatible with
/// the current buffer position.
///
/// # Example
///
/// ```ignore
/// let mut buf = vec![0u8; 100];
/// let mut cur = BuildCursor::<MyStruct>::new(buf.as_mut_ptr());
///
/// // Write MyStruct at current position
/// unsafe { *cur.get_mut() = MyStruct { ... }; }
///
/// // Move cursor to next structure
/// cur = cur.behind::<NextStruct>(1);
/// ```
#[derive(Copy)]
pub struct BuildCursor<A> {
    /// Offset from the start of the buffer.
    pub cur: usize,

    /// Base pointer to the buffer.
    pub buf: *mut u8,

    _phantom: PhantomData<A>,
}

impl<A> BuildCursor<A> {
    /// Create a new cursor at the start of a buffer.
    pub fn new(buf: *mut u8) -> Self {
        Self { cur: 0, buf, _phantom: PhantomData }
    }

    /// Create a cursor pointing to a specific location within the buffer.
    ///
    /// # Safety
    ///
    /// The pointer `at` must be within the same buffer as this cursor.
    pub fn goto<B>(&self, at: *mut B) -> BuildCursor<B> {
        BuildCursor { cur: at as usize - self.buf as usize, buf: self.buf, _phantom: PhantomData }
    }

    /// Increment the cursor by one element of type A.
    pub fn inc(&mut self) {
        self.cur += size_of::<A>();
    }

    /// Get a cursor positioned after `n` elements of type A, aligned for type B.
    ///
    /// This is used to access data that follows an array or structure.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // After writing 3 u32s, get cursor for next f64
    /// let next_cur = cur.behind::<f64>(3);
    /// ```
    pub fn behind<B>(&self, n: usize) -> BuildCursor<B> {
        BuildCursor {
            cur: align_up(self.cur + size_of::<A>() * n, align_of::<B>()),
            buf: self.buf,
            _phantom: PhantomData,
        }
    }

    /// Align the cursor for type B without advancing past any data.
    ///
    /// Used when the current position needs to be aligned before writing.
    pub fn align<B>(&self) -> BuildCursor<B> {
        BuildCursor {
            cur: align_up(self.cur, align_of::<B>()),
            buf: self.buf,
            _phantom: PhantomData,
        }
    }

    /// Reinterpret the cursor as pointing to a different type.
    ///
    /// # Safety
    ///
    /// The caller must ensure that type B is compatible with the current
    /// buffer position (alignment and initialization requirements).
    pub fn transmute<B>(&self) -> BuildCursor<B> {
        BuildCursor { cur: self.cur, buf: self.buf, _phantom: PhantomData }
    }

    /// Get a mutable pointer to the current position.
    ///
    /// # Safety
    ///
    /// - The cursor must point to valid, allocated memory
    /// - The memory must be properly aligned for type A
    /// - Caller must ensure no aliasing violations
    pub unsafe fn get_mut(&self) -> *mut A {
        self.buf.add(self.cur) as *mut A
    }
}

impl<A> Clone for BuildCursor<A> {
    fn clone(&self) -> Self {
        Self { cur: self.cur, buf: self.buf, _phantom: PhantomData }
    }
}

/// Helper for converting relative offsets to absolute pointers during deserialization.
///
/// During serialization, pointers are stored as offsets from the buffer start.
/// During deserialization, `Shifter` converts these offsets back to absolute pointers.
///
/// # Example
///
/// ```ignore
/// let shifter = Shifter(buf.as_ptr());
/// // ptr currently contains offset 100
/// shifter.shift(&mut ptr);  // Now ptr points to buf + 100
/// ```
pub struct Shifter(pub *const u8);

impl Shifter {
    /// Convert a pointer from relative offset to absolute address.
    ///
    /// # Safety
    ///
    /// - The pointer must currently contain a valid offset within the buffer
    /// - The resulting absolute pointer must point to valid, initialized data
    /// - This should only be called during the deserialize phase
    pub unsafe fn shift<T>(&self, x: &mut *const T) {
        *x = self.0.add(*x as *const u8 as usize) as *const T
    }
}

/// Super-trait for associative containers, defining associated types.
///
/// This trait is split from `Assocs` to work around Rust's trait system limitations
/// with generic associated types (GATs).
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

/// Trait for associative containers (maps) in blob format.
///
/// Associative containers support iteration over key-value pairs that match
/// a given predicate. This allows both exact lookups and filtered iteration.
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

/// Trait for types that can be checked for emptiness.
///
/// Used during serialization to skip empty containers in hash maps.
pub trait IsEmpty {
    fn is_empty(&self) -> bool;
}

impl<X> IsEmpty for Vec<X> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

/// Trait for single key-value associations.
///
/// Unlike `Assocs` which represents a collection, `Assoc` represents
/// a single key-value pair (like `Flagellum`).
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

/// Associates a blob type with its origin (source) type.
///
/// During serialization, we convert from `Origin` types (standard Rust types like Vec)
/// to blob types (like BlobVec). The `Build` trait establishes this relationship.
///
/// # Example
///
/// ```ignore
/// impl<'a, X: Build> Build for BlobVec<'a, X> {
///     type Origin = Vec<X::Origin>;
/// }
/// ```
///
/// This means a `BlobVec<'a, u8>` is built from a `Vec<u8>`.
pub trait Build {
    /// The origin type that this blob type is built from.
    type Origin;
}

impl Build for u8 {
    type Origin = u8;
}
impl Build for Guard {
    type Origin = Guard;
}
impl Build for usize {
    type Origin = usize;
}
impl Build for () {
    type Origin = ();
}

#[cfg(test)]
pub mod tests {
    use tupellum::Tupellum;

    use crate::char_enfa::OrderedIxs;

    use super::*;
    use super::{
        assoc_list::*,
        flagellum::*,
        hashmap::*,
        listmap::*,
        sediment::*,
        state::{build::*, *},
        vecmap::*,
    };
    use crate::char_nfa;

    #[test]
    pub fn test_blobvec() {
        let origin = vec![1usize, 3, 5];
        let mut sz = Reserve(0);
        let my_addr = BlobVec::<usize>::reserve(&origin, &mut sz);
        assert_eq!(my_addr, 0);
        assert_eq!(sz.0, 4 * size_of::<usize>());
        let mut buf = vec![0u8; sz.0];
        let mut cur = BuildCursor::new(buf.as_mut_ptr());
        cur = unsafe {
            BlobVec::<usize>::serialize(&origin, cur, |x, xcur| {
                *xcur = *x;
            })
        };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        let mut cur = BuildCursor::new(buf.as_mut_ptr());
        cur = unsafe { BlobVec::<usize>::deserialize(cur, |_| ()) };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        let blobvec = unsafe { &*(buf.as_ptr() as *const BlobVec<usize>) };
        assert_eq!(blobvec.len, 3);
        assert_eq!(unsafe { blobvec.get(0) }, &1);
        assert_eq!(unsafe { blobvec.get(1) }, &3);
        assert_eq!(unsafe { blobvec.get(2) }, &5);
        let mut iter = unsafe { blobvec.iter() };
        assert_eq!(unsafe { iter.next() }, Some(&1));
        assert_eq!(unsafe { iter.next() }, Some(&3));
        assert_eq!(unsafe { iter.next() }, Some(&5));
        assert_eq!(unsafe { iter.next() }, None);
        assert_eq!(unsafe { blobvec.as_ref() }, &[1, 3, 5]);
    }

    #[test]
    pub fn test_vecmap() {
        let origin = vec![(1, b"foo".to_vec()), (3, b"hello".to_vec()), (5, b"".to_vec())];
        let mut sz = Reserve(1);
        let addr = VecMap::<usize, BlobVec<u8>>::reserve(&origin, &mut sz, |x, sz| {
            BlobVec::<u8>::reserve(x, sz);
        });
        assert_eq!(addr, if align_of::<usize>() == 1 { 0 } else { align_of::<usize>() });
        let mut buf = vec![0u8; sz.0];
        let mut cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(addr) });
        cur = unsafe {
            VecMap::<usize, BlobVec<u8>>::serialize(
                &origin,
                cur,
                |x, xcur| {
                    *xcur = *x;
                },
                |x, xcur| {
                    BlobVec::<u8>::serialize(x, xcur, |y, ycur| {
                        *ycur = *y;
                    })
                },
            )
        };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        let mut cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(addr) });
        cur = unsafe {
            VecMap::<usize, BlobVec<u8>>::deserialize(
                cur,
                |_| (),
                |xcur| BlobVec::<u8>::deserialize(xcur, |_| ()),
            )
        };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        let vecmap = unsafe { &*(buf.as_ptr().add(addr) as *const VecMap<usize, BlobVec<u8>>) };

        let mut iter = unsafe { vecmap.iter_matches(&EqMatch(&3)) };
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&3, b"hello".as_ref()));
        assert_eq!(unsafe { iter.next() }.is_none(), true);

        let mut iter = unsafe { vecmap.iter_matches(&AnyMatch) };
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&1, b"foo".as_ref()));
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&3, b"hello".as_ref()));
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!((k, unsafe { v.as_ref() }), (&5, b"".as_ref()));
    }

    #[test]
    pub fn test_listmap() {
        let origin = vec![
            (b"aa".to_vec(), b"foo".to_vec()),
            (b"bb".to_vec(), b"hello".to_vec()),
            (b"aa".to_vec(), b"".to_vec()),
        ];
        let mut sz = Reserve(1);
        let addr = ListMap::<BlobVec<u8>, BlobVec<u8>>::reserve(
            &origin,
            &mut sz,
            |x, sz| {
                BlobVec::<u8>::reserve(x, sz);
            },
            |x, sz| {
                BlobVec::<u8>::reserve(x, sz);
            },
        );
        assert_eq!(addr, if align_of::<usize>() == 1 { 0 } else { align_of::<usize>() });
        let mut buf = vec![0u8; sz.0];
        let mut cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(addr) });
        cur = unsafe {
            ListMap::<BlobVec<u8>, BlobVec<u8>>::serialize(
                &origin,
                cur,
                |x, xcur| {
                    BlobVec::<u8>::serialize(x, xcur, |y, ycur| {
                        *ycur = *y;
                    })
                },
                |x, xcur| {
                    BlobVec::<u8>::serialize(x, xcur, |y, ycur| {
                        *ycur = *y;
                    })
                },
            )
        };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        let mut cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(addr) });
        cur = unsafe {
            ListMap::<BlobVec<u8>, BlobVec<u8>>::deserialize(
                cur,
                |xcur| BlobVec::<u8>::deserialize(xcur, |_| ()),
                |xcur| BlobVec::<u8>::deserialize(xcur, |_| ()),
            )
        };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        let vecmap =
            unsafe { &*(buf.as_ptr().add(addr) as *const ListMap<BlobVec<u8>, BlobVec<u8>>) };

        let key = b"aa".as_ref();
        let mut iter = unsafe { vecmap.iter_matches(&key) };
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!(unsafe { (k.as_ref(), v.as_ref()) }, (b"aa".as_ref(), b"foo".as_ref()));
        let (k, v) = unsafe { iter.next().unwrap() };
        assert_eq!(unsafe { (k.as_ref(), v.as_ref()) }, (b"aa".as_ref(), b"".as_ref()));
        assert_eq!(unsafe { iter.next() }.is_none(), true);
    }

    #[test]
    pub fn test_blobhashmap() {
        let origin0 = vec![(1, b"foo".to_vec()), (3, b"hello".to_vec()), (5, b"".to_vec())];
        let mut origin = vec![vec![], vec![], vec![], vec![]];
        for (k, v) in origin0 {
            origin[k as usize & 3].push((k, v));
        }
        let mut sz = Reserve(0);
        let my_addr = BlobHashMap::<AssocList<Flagellum<u8, BlobVec<u8>>>>::reserve(
            &origin,
            &mut sz,
            |alist, sz| {
                AssocList::<Flagellum<u8, BlobVec<u8>>>::reserve(alist, sz, |kv, sz| {
                    Flagellum::<u8, BlobVec<u8>>::reserve(kv, sz, |v, sz| {
                        BlobVec::<u8>::reserve(v, sz);
                    });
                });
            },
        );
        assert_eq!(my_addr, 0);
        let mut buf = vec![0u8; sz.0];
        let mut cur = BuildCursor::new(buf.as_mut_ptr());
        cur = unsafe {
            BlobHashMap::<AssocList<Flagellum<u8, BlobVec<u8>>>>::serialize(
                &origin,
                cur,
                |alist, alist_cur| {
                    AssocList::<Flagellum<u8, BlobVec<u8>>>::serialize(
                        alist,
                        alist_cur,
                        |kv, kv_cur| {
                            Flagellum::<u8, BlobVec<u8>>::serialize(
                                kv,
                                kv_cur,
                                |k, k_cur| {
                                    *k_cur = *k;
                                },
                                |v, v_cur| {
                                    BlobVec::<u8>::serialize(v, v_cur, |x, x_cur| {
                                        *x_cur = *x;
                                    })
                                },
                            )
                        },
                    )
                },
            )
        };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        let mut cur = BuildCursor::new(buf.as_mut_ptr());
        cur = unsafe {
            BlobHashMap::<AssocList<Flagellum<u8, BlobVec<u8>>>>::deserialize(cur, |alist_cur| {
                AssocList::<Flagellum<u8, BlobVec<u8>>>::deserialize(alist_cur, |kv_cur| {
                    Flagellum::<u8, BlobVec<u8>>::deserialize(
                        kv_cur,
                        |_| (),
                        |v_cur| BlobVec::<u8>::deserialize(v_cur, |_| ()),
                    )
                })
            })
        };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        let hash = unsafe {
            &*(buf.as_ptr() as *const BlobHashMap<AssocList<Flagellum<u8, BlobVec<u8>>>>)
        };
        assert_eq!(unsafe { hash.get(&3).unwrap().as_ref() }, b"hello".as_ref());
    }

    #[test]
    fn test_sediment_and_tupellum() {
        let origin = (vec![b"".to_vec(), b"foo".to_vec(), b"hello".to_vec()], b"barr".to_vec());
        let mut sz = Reserve(0);
        Tupellum::<Sediment<BlobVec<u8>>, BlobVec<u8>>::reserve(
            &origin,
            &mut sz,
            |xs, sz| {
                Sediment::<BlobVec<u8>>::reserve(xs, sz, |xs, sz| {
                    BlobVec::<u8>::reserve(xs, sz);
                });
            },
            |xs, sz| {
                BlobVec::<u8>::reserve(xs, sz);
            },
        );
        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(0) });
        let _: BuildCursor<()> = unsafe {
            Tupellum::<Sediment<BlobVec<u8>>, BlobVec<u8>>::serialize(
                &origin,
                cur,
                |x, xcur| {
                    Sediment::<BlobVec<u8>>::serialize(x, xcur, |x, bcur| {
                        BlobVec::<u8>::serialize(x, bcur, |y, ycur| {
                            *ycur = *y;
                        })
                    })
                },
                |x, xcur| {
                    BlobVec::<u8>::serialize(x, xcur, |y, ycur| {
                        *ycur = *y;
                    })
                },
            )
        };
        let cur = BuildCursor::new(unsafe { buf.as_mut_ptr().add(0) });
        unsafe {
            Tupellum::<Sediment<BlobVec<u8>>, BlobVec<u8>>::deserialize::<(), _, _>(
                cur,
                |xcur| {
                    Sediment::<BlobVec<u8>>::deserialize(xcur, |xcur| {
                        BlobVec::<u8>::deserialize(xcur, |_| ())
                    })
                },
                |xcur| BlobVec::<u8>::deserialize(xcur, |_| ()),
            )
        };
        let tupellum =
            unsafe { &*(buf.as_ptr() as *const Tupellum<Sediment<BlobVec<u8>>, BlobVec<u8>>) };
        let mut behind: *const BlobVec<u8> = std::ptr::null();
        let mut contents = vec![];
        unsafe {
            tupellum.a.each(|x| {
                contents.push(x.as_ref());
                behind = x.behind();
                behind
            })
        };
        assert_eq!(contents, vec![b"".as_slice(), b"foo".as_slice(), b"hello".as_slice()]);
        assert_eq!(unsafe { (*behind).as_ref() }, b"barr".as_slice());
    }

    pub struct TestU8BuildConfig;
    impl U8BuildConfig for TestU8BuildConfig {
        fn guard_size_keep(&self) -> u32 {
            2
        }
        fn hashmap_cap_power_fn(&self, _len: usize) -> usize {
            1
        }
        fn dense_guard_count(&self) -> usize {
            3
        }
    }

    pub unsafe fn create_states<'a>(
        buf: &'a mut Vec<u8>,
        qs: Vec<char_nfa::State>,
    ) -> Vec<&'a U8State<'a>> {
        let states = qs.iter().map(|q| U8StatePrepared::prepare(&q, &TestU8BuildConfig)).collect();
        let mut sz = Reserve(0);
        let mut addrs = Vec::<usize>::new();
        let list_addr = Sediment::<U8State>::reserve(&states, &mut sz, |state, sz| {
            addrs.push(U8State::reserve(state, sz));
        });
        assert_eq!(list_addr, 0);
        buf.resize(sz.0 + size_of::<usize>(), 0);
        let buf = align_up_mut_ptr::<u8, u128>(buf.as_mut_ptr()) as *mut u8;
        let mut cur = BuildCursor::new(buf);
        cur = unsafe {
            Sediment::<U8State>::serialize(&states, cur, |state, state_cur| {
                U8State::serialize(state, state_cur, &addrs)
            })
        };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        let mut cur = BuildCursor::new(buf);
        cur = unsafe {
            Sediment::<U8State>::deserialize(cur, |state_cur| U8State::deserialize(state_cur))
        };
        assert_eq!(cur.cur, cur.cur); // suppress unused_assign warning
        (0..qs.len()).map(|i| &*(buf.add(addrs[i]) as *const U8State)).collect()
    }

    pub fn expect_dense<'a, 'b>(iter: U8StateIterator<'a, 'b>) -> U8DenseStateIterator<'a> {
        match iter {
            U8StateIterator::Dense(iter) => iter,
            _ => unreachable!(),
        }
    }

    pub fn expect_sparse<'a, 'b>(iter: U8StateIterator<'a, 'b>) -> U8SparseStateIterator<'a, 'b> {
        match iter {
            U8StateIterator::Sparse(iter) => iter,
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_states() {
        let states = vec![
            char_nfa::State {
                tags: OrderedIxs(vec![]),
                transitions: vec![
                    (Guard::from_range((b'a', b'b')), 0),
                    (Guard::from_range((b'a', b'a')), 1),
                    (Guard::from_range((b'd', b'z')), 1),
                ],
                is_deterministic: false,
            },
            char_nfa::State {
                tags: OrderedIxs(vec![1, 2]),
                transitions: vec![(Guard::from_range((b'b', b'm')), 0)],
                is_deterministic: false,
            },
        ];
        let mut buf = vec![];
        let states = unsafe { create_states(&mut buf, states) };
        let state0 = states[0];
        let state1 = states[1];

        let mut iter = expect_dense(unsafe { state0.iter_matches(&b'c') });
        assert!(unsafe { iter.next() }.is_none());

        let mut iter = expect_dense(unsafe { state0.iter_matches(&b'a') });
        let mut succs = vec![*unsafe { iter.next() }.unwrap(), *unsafe { iter.next() }.unwrap()];
        assert!(unsafe { iter.next() }.is_none());
        succs.sort();
        assert_eq!(succs, [state0 as *const U8State, state1]);

        let mut iter = expect_dense(unsafe { state0.iter_matches(&b'p') });
        let succs = vec![*unsafe { iter.next() }.unwrap()];
        assert!(unsafe { iter.next() }.is_none());
        assert_eq!(succs, vec![state1 as *const U8State]);

        let mut iter = expect_sparse(unsafe { state1.iter_matches(&b'a') });
        assert!(unsafe { iter.next() }.is_none());

        let mut iter = expect_sparse(unsafe { state1.iter_matches(&b'c') });
        let succs = vec![unsafe { iter.next() }.unwrap()];
        assert!(unsafe { iter.next() }.is_none());
        assert_eq!(succs, vec![state0 as *const U8State]);

        let no_tags: &[usize] = &[];
        assert_eq!(unsafe { state0.get_tags() }, no_tags);
        assert_eq!(unsafe { state1.get_tags() }, &[1usize, 2]);
    }
}
