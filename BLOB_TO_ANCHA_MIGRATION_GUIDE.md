# Blob to Ancha Migration Guide

This document provides a detailed, step-by-step guide for migrating data structures from the `blob` system to the `ancha` system. It is based on the completed migrations of `vec.rs` and `tupellum.rs`.

## Table of Contents

1. [Overview](#overview)
2. [Key Differences](#key-differences)
3. [Transformation Steps](#transformation-steps)
4. [Test Migration](#test-migration)
5. [Alignment Unification](#alignment-unification)
6. [Examples](#examples)

---

## Overview

### The Old System (Blob)

The blob system uses:
- **Function-based serialization**: Static methods like `serialize()`, `deserialize()`, and `reserve()`
- **Closures for element transformation**: Callers provide closures that define how to transform each element
- **`Build` trait**: Associates blob types with their origin types
- **No composability**: Each structure implements its own serialization logic

Example from `blob/vec.rs`:
```rust
pub unsafe fn serialize<F: FnMut(&X::Origin, &mut X), After>(
    origin: &<Self as Build>::Origin,
    cur: BuildCursor<Self>,
    mut f: F,
) -> BuildCursor<After>
```

### The New System (Ancha)

The ancha system uses:
- **Object-based strategies**: Serialization strategies are composable objects
- **Trait-based transformation**: Strategies implement `Anchize`, `Deanchize`, `StaticAnchize`, `StaticDeanchize`
- **Context threading**: A `Context` type parameter allows threading state through the serialization process
- **Full composability**: Strategies can be nested and composed

Example from `ancha/vec.rs`:
```rust
pub struct VecAnchizeFromVec<'a, ElemAnchize> {
    pub elem_ancha: ElemAnchize,
    _phantom: PhantomData<&'a ElemAnchize>,
}

impl<'a, ElemAnchize: StaticAnchize<'a>> Anchize<'a> for VecAnchizeFromVec<'a, ElemAnchize>
```

---

## Key Differences

### 1. Structure Naming

| Blob | Ancha |
|------|-------|
| `BlobVec<'a, X>` | `AnchaVec<'a, X>` |
| `Tupellum<'a, A, B>` | `Tupellum<'a, A, B>` (unchanged) |

### 2. Trait Changes

| Blob | Ancha |
|------|-------|
| `Build` | `Anchize<'a>` |
| Static methods on structs | Separate strategy structs |
| Closures in `serialize()` | `StaticAnchize` trait implementations |
| Closures in `deserialize()` | `StaticDeanchize` trait implementations |

### 3. Strategy Structs

For each data structure, you need:

1. **Anchization strategy**: Converts `Origin → Ancha`
   - Named `<Type>AnchizeFrom<Source>`
   - Implements `Anchize<'a>`
   - Example: `VecAnchizeFromVec`, `TupellumAnchizeFromTuple`

2. **Deanchization strategy**: Fixes up pointers in the blob
   - Named `<Type>Deanchize`
   - Implements `Deanchize<'a>`
   - Example: `VecDeanchize`, `TupellumDeanchizeFromTuple`

### 4. Method Signatures

**Blob reserve:**
```rust
pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize
```

**Ancha reserve:**
```rust
fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve)
```

**Blob serialize:**
```rust
pub unsafe fn serialize<F: FnMut(&X::Origin, &mut X), After>(
    origin: &<Self as Build>::Origin,
    cur: BuildCursor<Self>,
    mut f: F,
) -> BuildCursor<After>
```

**Ancha anchize:**
```rust
unsafe fn anchize<After>(
    &self,
    origin: &Self::Origin,
    context: &Self::Context,
    cur: BuildCursor<Self::Ancha>,
) -> BuildCursor<After>
```

---

## Transformation Steps

### Step 1: Create the Ancha Data Structure

Transform the blob struct to an ancha struct.

**Blob version (`configmaton/src/blob/vec.rs`):**
```rust
#[repr(C)]
pub struct BlobVec<'a, X> {
    pub(super) len: usize,
    _phantom: PhantomData<&'a X>,
}
```

**Ancha version (`ancha/src/vec.rs`):**
```rust
#[repr(C)]
pub struct AnchaVec<'a, X> {
    pub len: usize,  // Note: visibility can be changed from pub(super) to pub
    _phantom: PhantomData<&'a X>,
}
```

**Changes:**
- Rename `BlobVec` → `AnchaVec`
- Keep the same layout and fields
- Can adjust visibility as appropriate

### Step 2: Migrate Helper Methods

Keep all helper methods on the struct (like `as_ref()`, `get()`, `behind()`, `iter()`).

**Blob version:**
```rust
impl<'a, X> BlobVec<'a, X> {
    pub unsafe fn as_ref(&self) -> &'a [X] {
        std::slice::from_raw_parts(get_behind_struct::<_, X>(self), self.len)
    }

    pub unsafe fn get(&self, ix: usize) -> &X {
        assert!(ix < self.len);
        &*get_behind_struct::<_, X>(self).add(ix)
    }

    pub unsafe fn behind<After>(&self) -> &'a After {
        let cur = get_behind_struct::<_, X>(self);
        &*align_up_ptr(cur.add(self.len))
    }
}
```

**Ancha version:**
```rust
impl<'a, X> AnchaVec<'a, X> {
    pub unsafe fn as_ref(&self) -> &'a [X] {
        let ptr = (self as *const Self).add(1) as *const X;
        std::slice::from_raw_parts(ptr, self.len)
    }

    pub unsafe fn get(&self, ix: usize) -> &'a X {
        assert!(ix < self.len);
        let ptr = (self as *const Self).add(1) as *const X;
        &*ptr.add(ix)
    }

    pub unsafe fn behind<After>(&self) -> &'a After {
        let elem_ptr = (self as *const Self).add(1) as *const X;
        let after_elems = elem_ptr.add(self.len) as *const u8;
        let aligned = super::align_up(after_elems as usize, std::mem::align_of::<After>());
        &*(aligned as *const After)
    }
}
```

**Changes:**
- Update imports: `get_behind_struct` from blob might not be available, inline the logic
- Update `align_up_ptr` calls to use `super::align_up`
- Keep the same unsafe contracts and panics

### Step 3: Create Anchization Strategy Struct

Create a struct that implements the `Anchize` trait.

**Pattern:**
```rust
pub struct <Type>AnchizeFrom<Source><'a, ElemAnchize> {
    pub elem_ancha: ElemAnchize,
    _phantom: PhantomData<&'a ElemAnchize>,
}

impl<'a, ElemAnchize> <Type>AnchizeFrom<Source><'a, ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        Self { elem_ancha, _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize: Default> Default for <Type>AnchizeFrom<Source><'a, ElemAnchize> {
    fn default() -> Self {
        Self { elem_ancha: Default::default(), _phantom: PhantomData }
    }
}
```

**Example for Vec:**
```rust
pub struct VecAnchizeFromVec<'a, ElemAnchize> {
    pub elem_ancha: ElemAnchize,
    _phantom: PhantomData<&'a ElemAnchize>,
}

impl<'a, ElemAnchize> VecAnchizeFromVec<'a, ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        VecAnchizeFromVec { elem_ancha, _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize: Default> Default for VecAnchizeFromVec<'a, ElemAnchize> {
    fn default() -> Self {
        VecAnchizeFromVec { elem_ancha: Default::default(), _phantom: PhantomData }
    }
}
```

### Step 4: Implement the Anchize Trait

Transform the `reserve()` and `serialize()` methods into `Anchize` trait implementation.

**Blob version:**
```rust
impl<'a, X: Build> BlobVec<'a, X> {
    pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        sz.add::<X>(origin.len());
        my_addr
    }

    pub unsafe fn serialize<F: FnMut(&X::Origin, &mut X), After>(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).len = origin.len();
        let mut xcur = cur.behind(1);
        for x in origin.iter() {
            f(x, &mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.align()
    }
}
```

**Ancha version:**
```rust
impl<'a, ElemAnchize> Anchize<'a> for VecAnchizeFromVec<'a, ElemAnchize>
where
    ElemAnchize: StaticAnchize<'a>,
    ElemAnchize::Ancha: Sized,
{
    type Origin = Vec<ElemAnchize::Origin>;
    type Ancha = AnchaVec<'a, ElemAnchize::Ancha>;
    type Context = ElemAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, _context: &Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0);  // Alignment at the beginning!
        sz.add::<Self::Ancha>(1);
        sz.add::<ElemAnchize::Ancha>(origin.len());
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align();  // Alignment at the beginning!
        (*cur.get_mut()).len = origin.len();
        let mut xcur: BuildCursor<ElemAnchize::Ancha> = cur.behind(1);

        for elem_origin in origin.iter() {
            self.elem_ancha.anchize_static(elem_origin, context, &mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.transmute()
    }
}
```

**Key transformations:**

1. **Remove `my_addr` return value**: The ancha system doesn't return addresses from reserve
2. **Add alignment at the beginning**:
   - In `reserve()`: `sz.add::<Self::Ancha>(0)` as the first line
   - In `anchize()`: `let cur = cur.align()` as the first line
3. **Replace closure `f` with strategy call**: `self.elem_ancha.anchize_static(...)`
4. **Thread context**: Add `context` parameter and pass it through
5. **Rename types**: `X` → `ElemAnchize::Ancha`, `X::Origin` → `ElemAnchize::Origin`

### Step 5: Create Deanchization Strategy Struct

Create a struct that implements the `Deanchize` trait.

**Pattern:**
```rust
pub struct <Type>Deanchize<'a, ElemDeanchize> {
    pub elem_deancha: ElemDeanchize,
    _phantom: PhantomData<&'a ElemDeanchize>,
}

impl<'a, ElemDeanchize> <Type>Deanchize<'a, ElemDeanchize> {
    pub fn new(elem_deancha: ElemDeanchize) -> Self {
        Self { elem_deancha, _phantom: PhantomData }
    }
}

impl<'a, ElemDeanchize: Default> Default for <Type>Deanchize<'a, ElemDeanchize> {
    fn default() -> Self {
        Self { elem_deancha: Default::default(), _phantom: PhantomData }
    }
}
```

### Step 6: Implement the Deanchize Trait

Transform the `deserialize()` method into `Deanchize` trait implementation.

**Blob version:**
```rust
pub unsafe fn deserialize<F: FnMut(&mut X), After>(
    cur: BuildCursor<Self>,
    mut f: F,
) -> BuildCursor<After> {
    let mut xcur = cur.behind(1);
    for _ in 0..(*cur.get_mut()).len {
        f(&mut *xcur.get_mut());
        xcur.inc();
    }
    xcur.align()
}
```

**Ancha version:**
```rust
impl<'a, ElemDeanchize> Deanchize<'a> for VecDeanchize<'a, ElemDeanchize>
where
    ElemDeanchize: StaticDeanchize<'a>,
    ElemDeanchize::Ancha: Sized,
{
    type Ancha = AnchaVec<'a, ElemDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align();  // Alignment at the beginning!
        let len = (*cur.get_mut()).len;
        let mut xcur: BuildCursor<ElemDeanchize::Ancha> = cur.behind(1);
        for _ in 0..len {
            self.elem_deancha.deanchize_static(&mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.transmute()
    }
}
```

**Key transformations:**

1. **Add alignment at the beginning**: `let cur = cur.align()`
2. **Replace closure `f` with strategy call**: `self.elem_deancha.deanchize_static(...)`
3. **Store len first**: For safety, read `len` before iterating

---

## Test Migration

Tests need to be migrated from `blob.rs` to the respective module files.

### Finding Tests

Original tests are in `configmaton/src/blob.rs` under the `#[cfg(test)]` module.

**Example from blob.rs:**
```rust
#[test]
pub fn test_blobvec() {
    let origin = vec![1usize, 3, 5];
    let mut sz = Reserve(0);
    let my_addr = BlobVec::<usize>::reserve(&origin, &mut sz);
    // ... rest of test
}
```

### Transforming Tests

**Ancha version:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CopyAnchize, NoopDeanchize};

    #[test]
    fn test_anchavec_basic() {
        // 1. Create strategy objects
        let anchize: VecAnchizeFromVec<CopyAnchize<u8, ()>> = VecAnchizeFromVec::default();
        let deanchize: VecDeanchize<NoopDeanchize<u8>> = VecDeanchize::default();
        let origin = vec![1u8, 2, 3];

        // 2. Reserve phase
        let mut sz = Reserve(0);
        anchize.reserve(&origin, &(), &mut sz);

        // 3. Allocate buffer
        let mut buf = vec![0u8; sz.0];
        let cur = BuildCursor::new(buf.as_mut_ptr());

        // 4. Anchize and deanchize
        unsafe {
            anchize.anchize::<()>(&origin, &(), cur.clone());
            deanchize.deanchize::<()>(cur);
        }

        // 5. Verify
        let anchavec = unsafe { &*(buf.as_ptr() as *const AnchaVec<u8>) };
        assert_eq!(unsafe { anchavec.as_ref() }, &[1, 2, 3]);
    }
}
```

**Key transformations:**

1. **Create strategy objects**: Instantiate anchize and deanchize strategies
2. **Specify element strategies**: Use `CopyAnchize` for direct copy, `NoopDeanchize` for no-op
3. **Pass context**: Usually `()` for simple tests
4. **Remove address tracking**: No need for `my_addr` return values
5. **Call through strategy objects**: `anchize.reserve()` instead of `BlobVec::reserve()`

### Test for Composite Structures (Tupellum)

**Blob version:**
```rust
#[test]
fn test_sediment_and_tupellum() {
    let origin = (vec![b"".to_vec(), b"foo".to_vec()], b"barr".to_vec());
    let mut sz = Reserve(0);
    Tupellum::<Sediment<BlobVec<u8>>, BlobVec<u8>>::reserve(
        &origin,
        &mut sz,
        |xs, sz| { Sediment::<BlobVec<u8>>::reserve(xs, sz, ...) },
        |xs, sz| { BlobVec::<u8>::reserve(xs, sz) },
    );
    // ... serialize with nested closures ...
}
```

**Ancha version:**
```rust
#[test]
fn test_tupellum() {
    let origin = (vec![1u8, 2, 3], vec![4u8, 5, 6, 7]);

    let anchize:
        TupellumAnchizeFromTuple<
            VecAnchizeFromVec<CopyAnchize<u8, ()>>,
            VecAnchizeFromVec<CopyAnchize<u8, ()>>
        > =
        TupellumAnchizeFromTuple::default();

    let deanchize:
        TupellumDeanchizeFromTuple<
            VecDeanchize<NoopDeanchize<u8>>,
            VecDeanchize<NoopDeanchize<u8>>
        > =
        TupellumDeanchizeFromTuple::default();

    let mut sz = Reserve(0);
    anchize.reserve(&origin, &(), &mut sz);

    let mut buf = vec![0u8; sz.0];
    let cur = BuildCursor::new(buf.as_mut_ptr());

    unsafe {
        anchize.anchize::<()>(&origin, &(), cur.clone());
        deanchize.deanchize::<()>(cur);
    }

    let tupellum =
        unsafe { &*(buf.as_ptr() as *const Tupellum<AnchaVec<u8>, AnchaVec<u8>>) };

    assert_eq!(unsafe { tupellum.a.as_ref() }, &[1u8, 2, 3]);
    let second_vec = unsafe { tupellum.a.behind::<AnchaVec<u8>>() };
    assert_eq!(unsafe { second_vec.as_ref() }, &[4u8, 5, 6, 7]);
}
```

**Key differences:**

1. **Explicit type parameters**: The composite type is fully spelled out in the anchize/deanchize types
2. **No nested closures**: Composability is handled by the type system, not closures
3. **Single call**: One `anchize()` call, one `deanchize()` call - composition happens internally

---

## Alignment Unification

**Critical rule**: Alignment must be done **exactly once** at the **beginning** of each phase.

### In Reserve Phase

```rust
fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve) {
    sz.add::<Self::Ancha>(0);  // ← ALIGNMENT HAPPENS HERE (first line)
    sz.add::<Self::Ancha>(1);
    // ... rest of reservation
}
```

**Why `sz.add::<T>(0)`?**
- Adds 0 bytes of `T`
- But still aligns to `align_of::<T>()`
- This ensures the structure starts at the correct alignment

### In Anchize Phase

```rust
unsafe fn anchize<After>(
    &self,
    origin: &Self::Origin,
    context: &Self::Context,
    cur: BuildCursor<Self::Ancha>,
) -> BuildCursor<After> {
    let cur: BuildCursor<Self::Ancha> = cur.align();  // ← ALIGNMENT HAPPENS HERE (first line)
    // ... rest of serialization
}
```

### In Deanchize Phase

```rust
unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
    let cur: BuildCursor<Self::Ancha> = cur.align();  // ← ALIGNMENT HAPPENS HERE (first line)
    // ... rest of deserialization
}
```

### Why This Matters

1. **Consistency**: Reserve and anchize/deanchize must match exactly
2. **Safety**: Misaligned access causes undefined behavior
3. **Portability**: Different platforms have different alignment requirements
4. **Simplicity**: One place to align, not scattered throughout

### Exception: Structures Without Headers

Some structures (like `Tupellum`) have **no header** - they only contain their first element inline:

```rust
#[repr(C)]
pub struct Tupellum<'a, A, B> {
    pub a: A,  // First element stored inline
    _phantom: PhantomData<&'a B>,  // Zero-size, no layout impact
}
```

For such structures:
- **Do NOT** add explicit alignment
- **Delegate** alignment to the first element's strategy
- The alignment of `Tupellum<A, B>` equals the alignment of `A`

**Example (Tupellum):**
```rust
fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve) {
    // NO sz.add::<Self>(0) here - delegate to first element
    self.a_ancha.reserve(&origin.0, context, sz);
    self.b_ancha.reserve(&origin.1, context, sz);
}

unsafe fn anchize<After>(...) -> BuildCursor<After> {
    // NO cur.align() here - delegate to first element
    let vcur = self.a_ancha.anchize(&origin.0, context, cur.transmute());
    self.b_ancha.anchize(&origin.1, context, vcur)
}
```

**Rule of thumb:**
- **Has header fields?** → Align explicitly at the beginning
- **Only wraps first element?** → Delegate alignment to first element

### Common Mistake

**❌ Wrong:**
```rust
fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve) {
    sz.add::<Self::Ancha>(1);  // No alignment before!
    sz.add::<ElementType>(origin.len());
}
```

**✅ Correct:**
```rust
fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve) {
    sz.add::<Self::Ancha>(0);  // Align first
    sz.add::<Self::Ancha>(1);  // Then add the struct
    sz.add::<ElementType>(origin.len());  // Then add elements
}
```

---

## Examples

### Example 1: Simple Vec Migration

**Before (blob/vec.rs):**
```rust
#[repr(C)]
pub struct BlobVec<'a, X> {
    pub(super) len: usize,
    _phantom: PhantomData<&'a X>,
}

impl<'a, X: Build> BlobVec<'a, X> {
    pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        sz.add::<Self>(1);
        sz.add::<X>(origin.len());
        my_addr
    }

    pub unsafe fn serialize<F: FnMut(&X::Origin, &mut X), After>(
        origin: &<Self as Build>::Origin,
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        (*cur.get_mut()).len = origin.len();
        let mut xcur = cur.behind(1);
        for x in origin.iter() {
            f(x, &mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.align()
    }

    pub unsafe fn deserialize<F: FnMut(&mut X), After>(
        cur: BuildCursor<Self>,
        mut f: F,
    ) -> BuildCursor<After> {
        let mut xcur = cur.behind(1);
        for _ in 0..(*cur.get_mut()).len {
            f(&mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.align()
    }
}
```

**After (ancha/vec.rs):**
```rust
#[repr(C)]
pub struct AnchaVec<'a, X> {
    pub len: usize,
    _phantom: PhantomData<&'a X>,
}

pub struct VecAnchizeFromVec<'a, ElemAnchize> {
    pub elem_ancha: ElemAnchize,
    _phantom: PhantomData<&'a ElemAnchize>,
}

impl<'a, ElemAnchize> VecAnchizeFromVec<'a, ElemAnchize> {
    pub fn new(elem_ancha: ElemAnchize) -> Self {
        VecAnchizeFromVec { elem_ancha, _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize: Default> Default for VecAnchizeFromVec<'a, ElemAnchize> {
    fn default() -> Self {
        VecAnchizeFromVec { elem_ancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, ElemAnchize> Anchize<'a> for VecAnchizeFromVec<'a, ElemAnchize>
where
    ElemAnchize: StaticAnchize<'a>,
    ElemAnchize::Ancha: Sized,
{
    type Origin = Vec<ElemAnchize::Origin>;
    type Ancha = AnchaVec<'a, ElemAnchize::Ancha>;
    type Context = ElemAnchize::Context;

    fn reserve(&self, origin: &Self::Origin, _context: &Self::Context, sz: &mut Reserve) {
        sz.add::<Self::Ancha>(0);  // Alignment!
        sz.add::<Self::Ancha>(1);
        sz.add::<ElemAnchize::Ancha>(origin.len());
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align();  // Alignment!
        (*cur.get_mut()).len = origin.len();
        let mut xcur: BuildCursor<ElemAnchize::Ancha> = cur.behind(1);

        for elem_origin in origin.iter() {
            self.elem_ancha.anchize_static(elem_origin, context, &mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.transmute()
    }
}

pub struct VecDeanchize<'a, ElemDeanchize> {
    pub elem_deancha: ElemDeanchize,
    _phantom: PhantomData<&'a ElemDeanchize>,
}

impl<'a, ElemDeanchize> VecDeanchize<'a, ElemDeanchize> {
    pub fn new(elem_deancha: ElemDeanchize) -> Self {
        VecDeanchize { elem_deancha, _phantom: PhantomData }
    }
}

impl<'a, ElemDeanchize: Default> Default for VecDeanchize<'a, ElemDeanchize> {
    fn default() -> Self {
        VecDeanchize { elem_deancha: Default::default(), _phantom: PhantomData }
    }
}

impl<'a, ElemDeanchize> Deanchize<'a> for VecDeanchize<'a, ElemDeanchize>
where
    ElemDeanchize: StaticDeanchize<'a>,
    ElemDeanchize::Ancha: Sized,
{
    type Ancha = AnchaVec<'a, ElemDeanchize::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let cur: BuildCursor<Self::Ancha> = cur.align();  // Alignment!
        let len = (*cur.get_mut()).len;
        let mut xcur: BuildCursor<ElemDeanchize::Ancha> = cur.behind(1);
        for _ in 0..len {
            self.elem_deancha.deanchize_static(&mut *xcur.get_mut());
            xcur.inc();
        }
        xcur.transmute()
    }
}
```

### Example 2: Tupellum Migration

**Before (blob/tupellum.rs):**
```rust
#[repr(C)]
pub struct Tupellum<'a, A, B> {
    pub a: A,
    _phantom: PhantomData<&'a B>,
}

impl<'a, A, B> Tupellum<'a, A, B> {
    pub fn reserve<
        BldA, BldB, Bld: TupellumBuild<BldA, BldB>,
        FK: FnMut(&BldA, &mut Reserve),
        FV: FnMut(&BldB, &mut Reserve),
    >(
        origin: &Bld,
        sz: &mut Reserve,
        mut fk: FK,
        mut fv: FV,
    ) -> usize {
        sz.add::<Self>(0);
        let my_addr = sz.0;
        fk(origin.left(), sz);
        fv(origin.right(), sz);
        my_addr
    }

    pub unsafe fn serialize<
        After, BldA, BldB, Bld: TupellumBuild<BldA, BldB>,
        FK: FnMut(&BldA, BuildCursor<A>) -> BuildCursor<B>,
        FV: FnMut(&BldB, BuildCursor<B>) -> BuildCursor<After>,
    >(
        origin: &Bld,
        cur: BuildCursor<Self>,
        mut fk: FK,
        mut fv: FV,
    ) -> BuildCursor<After> {
        let vcur = fk(origin.left(), cur.transmute());
        fv(origin.right(), vcur)
    }

    pub unsafe fn deserialize<
        After,
        FK: FnMut(BuildCursor<A>) -> BuildCursor<B>,
        FV: FnMut(BuildCursor<B>) -> BuildCursor<After>,
    >(
        cur: BuildCursor<Self>,
        mut fk: FK,
        mut fv: FV,
    ) -> BuildCursor<After> {
        let vcur = fk(cur.transmute());
        fv(vcur)
    }
}
```

**After (ancha/tupellum.rs):**
```rust
#[repr(C)]
pub struct Tupellum<'a, A, B> {
    pub a: A,
    _phantom: PhantomData<&'a B>,
}

pub struct TupellumAnchizeFromTuple<'a, A, B> {
    pub a_ancha: A,
    pub b_ancha: B,
    _phantom: PhantomData<&'a (A, B)>,
}

impl<'a, A, B> TupellumAnchizeFromTuple<'a, A, B> {
    pub fn new(a_ancha: A, b_ancha: B) -> Self {
        TupellumAnchizeFromTuple { a_ancha, b_ancha, _phantom: PhantomData }
    }
}

impl<'a, A: Default, B: Default> Default for TupellumAnchizeFromTuple<'a, A, B> {
    fn default() -> Self {
        TupellumAnchizeFromTuple {
            a_ancha: Default::default(),
            b_ancha: Default::default(),
            _phantom: PhantomData
        }
    }
}

impl<'a, A: Anchize<'a>, B: Anchize<'a, Context = A::Context>> Anchize<'a>
    for TupellumAnchizeFromTuple<'a, A, B>
{
    type Origin = (A::Origin, B::Origin);
    type Ancha = Tupellum<'a, A::Ancha, B::Ancha>;
    type Context = A::Context;

    fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve) {
        self.a_ancha.reserve(&origin.0, context, sz);
        self.b_ancha.reserve(&origin.1, context, sz);
    }

    unsafe fn anchize<After>(
        &self,
        origin: &Self::Origin,
        context: &Self::Context,
        cur: BuildCursor<Self::Ancha>,
    ) -> BuildCursor<After> {
        let vcur = self.a_ancha.anchize(&origin.0, context, cur.transmute());
        self.b_ancha.anchize(&origin.1, context, vcur)
    }
}

pub struct TupellumDeanchizeFromTuple<'a, A, B> {
    pub a_deancha: A,
    pub b_deancha: B,
    _phantom: PhantomData<&'a (A, B)>,
}

impl<'a, A, B> TupellumDeanchizeFromTuple<'a, A, B> {
    pub fn new(a_deancha: A, b_deancha: B) -> Self {
        TupellumDeanchizeFromTuple { a_deancha, b_deancha, _phantom: PhantomData }
    }
}

impl<'a, A: Default, B: Default> Default for TupellumDeanchizeFromTuple<'a, A, B> {
    fn default() -> Self {
        TupellumDeanchizeFromTuple {
            a_deancha: Default::default(),
            b_deancha: Default::default(),
            _phantom: PhantomData
        }
    }
}

impl<'a, A: Deanchize<'a>, B: Deanchize<'a>> Deanchize<'a>
    for TupellumDeanchizeFromTuple<'a, A, B>
{
    type Ancha = Tupellum<'a, A::Ancha, B::Ancha>;

    unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
        let vcur = self.a_deancha.deanchize(cur.transmute());
        self.b_deancha.deanchize(vcur)
    }
}
```

**Note**: In Tupellum, alignment is **delegated** to the sub-strategies. There's no `sz.add::<Self>(0)` or `cur.align()` in Tupellum itself because:
1. The first element `A` handles its own alignment
2. Tupellum just stores `A` inline
3. The `B` element is positioned by `A`'s anchization

---

## Checklist

Use this checklist when migrating a structure:

- [ ] Create new ancha struct (rename Blob→Ancha)
- [ ] Migrate helper methods (`as_ref()`, `get()`, `behind()`, etc.)
- [ ] Create anchization strategy struct
- [ ] Implement `new()` and `Default` for anchization strategy
- [ ] Implement `Anchize` trait
  - [ ] Add alignment at beginning of `reserve()`
  - [ ] Add alignment at beginning of `anchize()`
  - [ ] Replace closures with strategy calls
  - [ ] Thread context through
- [ ] Create deanchization strategy struct
- [ ] Implement `new()` and `Default` for deanchization strategy
- [ ] Implement `Deanchize` trait
  - [ ] Add alignment at beginning of `deanchize()`
  - [ ] Replace closures with strategy calls
- [ ] Find tests in `blob.rs`
- [ ] Copy tests to new module
- [ ] Transform tests to use strategy objects
- [ ] Verify tests pass
- [ ] Update imports in other modules
- [ ] Update documentation

---

## Common Pitfalls

1. **Forgetting alignment**: Always align at the beginning of reserve/anchize/deanchize
2. **Wrong closure replacement**: Map `f(x, xcur)` to `self.elem_ancha.anchize_static(x, context, xcur)`
3. **Missing context**: Thread `context` parameter through all calls
4. **Type mismatches**: Use `ElemAnchize::Origin` not `X::Origin`
5. **Test transformation**: Remember to create strategy objects first
6. **Visibility changes**: Check if field visibility needs adjustment

---

## Summary

The migration from blob to ancha is straightforward but requires careful attention to detail:

1. **Rename structures** (Blob→Ancha)
2. **Create strategy structs** (Anchize and Deanchize)
3. **Transform methods to traits** (reserve/serialize/deserialize → Anchize/Deanchize)
4. **Replace closures with strategies** (trait-based composition)
5. **Align at the beginning** (exactly once per phase)
6. **Migrate tests** (use strategy objects instead of static methods)

The key insight is that **closures become traits**, enabling full composability while maintaining the same memory layout and performance characteristics.
