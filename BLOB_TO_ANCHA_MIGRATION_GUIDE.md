# Blob to Ancha Migration Guide

This document provides a detailed, step-by-step guide for migrating data structures from the `blob` system to the `ancha` system. It is based on the completed migrations of `vec.rs` and `tupellum.rs`.

## Table of Contents

1. [Overview](#overview)
2. [Key Differences](#key-differences)
3. [**CRITICAL: Alignment Strategy Difference**](#critical-alignment-strategy-difference)
4. [Transformation Steps](#transformation-steps)
5. [Test Migration](#test-migration)
6. [Alignment Unification](#alignment-unification)
7. [Examples](#examples)

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

## CRITICAL: Alignment Strategy Difference

**⚠️ THIS IS THE MOST IMPORTANT CONCEPTUAL DIFFERENCE BETWEEN BLOB AND ANCHA ⚠️**

### Blob's Alignment Strategy: "Align at the End" (Conservative)

In the blob system, each structure aligns **after** serializing its content:

```rust
// Blob pattern
pub unsafe fn serialize<F, After>(
    origin: &Origin,
    cur: BuildCursor<Self>,
    mut f: F,
) -> BuildCursor<After> {
    // ... serialize content ...
    xcur.align()  // ← Align at the END
}
```

**Result**: The cursor is **always** aligned after any operation. This is **safer** - you can't accidentally use a misaligned cursor.

### Ancha's Alignment Strategy: "Align at the Beginning" (Efficient)

In the ancha system, each structure aligns **before** processing, especially in loops:

```rust
// Ancha pattern
unsafe fn anchize<After>(...) -> BuildCursor<After> {
    while let Some(origin) = todo.pop() {
        cur = cur.align::<Self::Ancha>();  // ← Align at the BEGINNING
        // ... serialize content ...
    }
    xcur.transmute()  // ← NO alignment at the end
}
```

**Result**: The cursor may **not** be aligned after operations. This allows **tighter packing**.

### Why This Matters: Space Efficiency vs Safety

#### Space Efficiency Benefit

Consider this scenario:
```rust
// You have:
// 1. Sediment with 8-byte aligned elements (e.g., pointers)
// 2. Followed by a u8

// Blob layout (always align at end):
// [Sediment header | elem1 | elem2 | align_padding] [u8 | 7_bytes_waste]
//                                    ← forced 8-byte alignment here

// Ancha layout (align at beginning only when needed):
// [Sediment header | elem1 | elem2 | u8] [next_structure]
//                                     ↑ u8 fits in the gap!
```

In the ancha system, the `u8` can fall **before** the big alignment of the next coarsely-aligned structure. This saves space.

#### Safety Risk

The risk is that if you're not systematic about alignment, you can get:
- **Misaligned pointer dereferences** (crashes or undefined behavior)
- **Subtle bugs** where cursors aren't aligned when they should be
- **Pointer map issues** (like we had in BDD) where stored addresses are misaligned

### The Ancha Discipline: Be Explicit About Alignment

**In Reserve:**
```rust
fn reserve(&self, origin: &Self::Origin, context: &Self::Context, sz: &mut Reserve) {
    sz.add::<Self::Ancha>(0);  // ← Align FIRST (at beginning)
    // For loop-based structures:
    while let Some(origin) = todo.pop() {
        sz.add::<Self::Ancha>(0);  // ← Align BEFORE each iteration
        sz.add::<Self::Ancha>(1);  // Then add space
        // ...
    }
}
```

**In Anchize:**
```rust
unsafe fn anchize<After>(...) -> BuildCursor<After> {
    let cur = cur.align();  // ← Align FIRST (at beginning)
    // For loop-based structures:
    while let Some(origin) = todo.pop() {
        cur = cur.align::<Self::Ancha>();  // ← Align BEFORE each iteration
        ptrmap.insert(origin, cur.cur);     // Store ALIGNED address!
        // ...
    }
    xcur.transmute()  // ← NO alignment at end
}
```

**In Deanchize:**
```rust
unsafe fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After> {
    let cur = cur.align();  // ← Align FIRST (at beginning)
    // For loop-based structures:
    while todo_count > 0 {
        cur = cur.align::<Self::Ancha>();  // ← Align BEFORE each iteration
        // ...
    }
    xcur.transmute()  // ← NO alignment at end
}
```

### Critical Rules for Ancha Alignment

1. **Always align at the beginning** of reserve/anchize/deanchize
2. **For loop-based structures**, align BEFORE each iteration
3. **Never align at the end** of anchize/deanchize (use `transmute()`, not `align()`)
4. **Exception**: Structures with headers that don't wrap other structures need explicit alignment
5. **When storing pointers** (like in BDD's ptrmap), ensure you align BEFORE storing the address

### Why Align Before in Loops?

For structures like BDD that use a work queue and store pointers:

```rust
while let Some(origin) = todo.pop() {
    cur = cur.align::<Self::Ancha>();  // ← MUST align first!
    ptrmap.insert(origin, cur.cur);     // ← Store aligned address
    // ...
    // In phase 2, we'll use ptrmap addresses as pointers
    // They MUST be aligned!
}
```

If we aligned at the end instead, we'd store **misaligned** addresses in the ptrmap, causing crashes when dereferencing them.

### Migration Checklist: Alignment

When migrating from blob to ancha:

- [ ] **Remove** `xcur.align()` at the end of anchize → change to `xcur.transmute()`
- [ ] **Remove** `xcur.align()` at the end of deanchize → change to `xcur.transmute()`
- [ ] **Add** `cur = cur.align()` at the BEGINNING of anchize
- [ ] **Add** `cur = cur.align()` at the BEGINNING of deanchize
- [ ] **For loop-based structures**: Add `cur = cur.align::<Self::Ancha>()` BEFORE each iteration
- [ ] **For loop-based structures**: Add `sz.add::<Self::Ancha>(0)` BEFORE each iteration in reserve
- [ ] **Test thoroughly** with various data to catch alignment issues

### Red Flags: Signs of Alignment Issues

Watch out for these symptoms:
- ❌ Misaligned pointer dereference panics
- ❌ Different behavior between debug and release builds
- ❌ Crashes with `SIGBUS` or `SIGSEGV`
- ❌ "address must be a multiple of X" errors
- ❌ Tests pass individually but fail when run together

### Trade-off Summary

| Aspect | Blob (Align at End) | Ancha (Align at Beginning) |
|--------|---------------------|----------------------------|
| **Safety** | ✅ Very safe - cursor always aligned | ⚠️ Requires discipline |
| **Space Efficiency** | ❌ Can waste space with padding | ✅ Tighter packing possible |
| **Complexity** | ✅ Simple - automatic alignment | ⚠️ Must be explicit |
| **Composability** | ❌ Each structure adds alignment | ✅ Flexible composition |
| **Performance** | ❌ More padding = more cache misses | ✅ Denser data = better cache |

**The ancha approach trades safety for efficiency. We must be systematic and disciplined to avoid alignment bugs.**

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

2. **⚠️ CRITICAL: Change alignment pattern**:
   - In `reserve()`: Add `sz.add::<Self::Ancha>(0)` as the first line
   - In `anchize()`: Add `let cur = cur.align()` as the first line
   - **REMOVE** any `xcur.align()` at the end → change to `xcur.transmute()`
   - **For loops**: Add alignment BEFORE each iteration (see [Alignment Strategy](#critical-alignment-strategy-difference))

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
2. **⚠️ REMOVE alignment at the end**: Change `xcur.align()` to `xcur.transmute()`
3. **Replace closure `f` with strategy call**: `self.elem_deancha.deanchize_static(...)`
4. **Store len first**: For safety, read `len` before iterating

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

**⚠️ READ [CRITICAL: Alignment Strategy Difference](#critical-alignment-strategy-difference) FIRST!**

**Critical rule**: Alignment must be done **at the beginning** of each phase and at the beginning of each loop iteration.

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
- **Do NOT** add explicit alignment at the beginning
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
- **Has header fields (like len, type_, etc.)?** → Align explicitly at the beginning
- **Only wraps first element inline?** → Delegate alignment to first element
- **Uses a work queue/loop?** → Align at the beginning of each iteration

**Note**: The first element's anchize will align at its beginning, which provides the alignment for the whole Tupellum.

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
  - [ ] Add alignment at beginning of `reserve()`: `sz.add::<Self::Ancha>(0)`
  - [ ] Add alignment at beginning of `anchize()`: `let cur = cur.align()`
  - [ ] **⚠️ REMOVE** any `xcur.align()` at the end → use `xcur.transmute()`
  - [ ] For loop structures: align BEFORE each iteration
  - [ ] Replace closures with strategy calls
  - [ ] Thread context through
- [ ] Create deanchization strategy struct
- [ ] Implement `new()` and `Default` for deanchization strategy
- [ ] Implement `Deanchize` trait
  - [ ] Add alignment at beginning of `deanchize()`: `let cur = cur.align()`
  - [ ] **⚠️ REMOVE** any `xcur.align()` at the end → use `xcur.transmute()`
  - [ ] For loop structures: align BEFORE each iteration
  - [ ] Replace closures with strategy calls
- [ ] Find tests in `blob.rs`
- [ ] Copy tests to new module
- [ ] Transform tests to use strategy objects
- [ ] **⚠️ Test with various data sizes** to catch alignment bugs
- [ ] Verify tests pass
- [ ] Update imports in other modules
- [ ] Update documentation

---

## Common Pitfalls

1. **⚠️ CRITICAL: Wrong alignment pattern**:
   - ❌ BAD: Aligning at the end like blob (`xcur.align()`)
   - ✅ GOOD: Aligning at the beginning and using `xcur.transmute()` at the end
   - See [CRITICAL: Alignment Strategy Difference](#critical-alignment-strategy-difference)

2. **Forgetting alignment in loops**: For work-queue structures, align BEFORE each iteration
   - In reserve: `sz.add::<Self::Ancha>(0)` before processing
   - In anchize: `cur = cur.align::<Self::Ancha>()` before processing
   - In deanchize: `cur = cur.align::<Self::Ancha>()` before processing

3. **Storing misaligned pointers**: When building pointer maps (like BDD), align BEFORE storing addresses

4. **Wrong closure replacement**: Map `f(x, xcur)` to `self.elem_ancha.anchize_static(x, context, xcur)`

5. **Missing context**: Thread `context` parameter through all calls

6. **Type mismatches**: Use `ElemAnchize::Origin` not `X::Origin`

7. **Test transformation**: Remember to create strategy objects first

8. **Visibility changes**: Check if field visibility needs adjustment

9. **Not testing with misaligned data**: Test with various data sizes to catch alignment bugs early

---

## Summary

The migration from blob to ancha requires careful attention to detail, especially regarding **alignment strategy**:

1. **Rename structures** (Blob→Ancha)
2. **Create strategy structs** (Anchize and Deanchize)
3. **Transform methods to traits** (reserve/serialize/deserialize → Anchize/Deanchize)
4. **Replace closures with strategies** (trait-based composition)
5. **⚠️ CRITICAL: Change alignment pattern**:
   - Remove alignment at the END of anchize/deanchize
   - Add alignment at the BEGINNING of each phase
   - For loops: align BEFORE each iteration
6. **Migrate tests** (use strategy objects instead of static methods)

### The Key Insights

1. **Closures become traits**, enabling full composability
2. **Alignment moves from end to beginning**, enabling tighter packing but requiring discipline
3. **Safety requires systematic approach** - the ancha pattern is more efficient but less forgiving

### The Critical Trade-off

**Blob**: Conservative, safe, wastes some space
**Ancha**: Efficient, composable, requires careful alignment discipline

**The ancha system prioritizes space efficiency and composability over automatic safety. This design enables better cache utilization and more flexible data layouts, but demands systematic adherence to the alignment rules.**
