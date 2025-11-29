# Blob Serialization System Refactoring Plan

## Overview

The blob serialization system is a high-performance, zero-copy serialization framework similar to Cap'n Proto, designed for representing complex data structures with direct memory layout control.

## Current Architecture

### Core Components

1. **Primitive Containers**
   - `BlobVec<'a, X>`: Dynamic array with inline storage
   - `List<'a, X>`: Linked list structure
   - `Sediment<'a, X>`: Packed array of variable-sized elements

2. **Composition Structures**
   - `Tupellum<'a, A, B>`: Two-element tuple
   - `Flagellum<'a, K, V>`: Key-value pair (inline key, pointer to value)

3. **Map Structures**
   - `VecMap<'a, K, V>`: Array-based map
   - `ListMap<'a, K, V>`: Linked-list-based map
   - `ArrMap<'a, SIZE, V>`: Fixed-size array map
   - `BlobHashMap<'a, AList>`: Hash map with configurable bucket type
   - `AssocList<'a, KV>`: Association list

4. **Advanced Structures**
   - `Bdd<'a, Var, Leaf>`: Binary Decision Diagram with DAG sharing

5. **Application-Specific** (Configmaton-specific)
   - `U8State<'a>`: Character automaton state
   - `KeyValState<'a>`: Key-value automaton state

### Serialization Protocol

Each blob structure implements three phases:

1. **Reserve** (`fn reserve(origin: &Origin, sz: &mut Reserve) -> usize`)
   - Calculate space requirements
   - Return the address where the structure will be placed
   - Must handle alignment

2. **Serialize** (`unsafe fn serialize(origin: &Origin, cur: BuildCursor<Self>) -> BuildCursor<After>`)
   - Write structure to buffer
   - Convert origin data to blob format
   - Return cursor for next structure

3. **Deserialize** (`unsafe fn deserialize(cur: BuildCursor<Self>) -> BuildCursor<After>`)
   - Fix up pointers (convert offsets to absolute pointers)
   - No allocation, pure in-place transformation
   - Return cursor for next structure

### Helper Types

- `Reserve`: Tracks size and alignment requirements
- `BuildCursor<A>`: Type-safe pointer with offset tracking
- `Shifter`: Helper for converting offsets to pointers
- `Build` trait: Associates a blob type with its origin type
- `UnsafeIterator`: Iterator trait for blob structures
- `Assocs`/`AssocsSuper`: Traits for associative containers

## Problems Identified

### 1. **Excessive Boilerplate**

Example from `keyval_state.rs` (lines 131-184):

```rust
pub fn reserve(origin: &<Self as Build>::Origin, sz: &mut Reserve) -> usize {
    sz.add::<KeyValState>(0);
    let result = sz.0;
    KeyValStateSparse::reserve(&origin.transitions, sz, |tran, sz| {
        Tran0::reserve(
            &(&tran.key, &(&tran.dfa_inits, &tran.bdd)),
            sz,
            |key, sz| {
                Bytes::reserve(key, sz);  // Manual delegation
            },
            |iaf, sz| {
                InitsAndFinals::reserve(
                    iaf,
                    sz,
                    |inits, sz| {
                        BlobVec::<*const U8State>::reserve(inits, sz);  // Manual delegation
                    },
                    |finals, sz| {
                        Finals::reserve(finals, sz, |leaf, sz| {
                            // Even more nesting...
                        });
                    },
                );
            },
        );
    });
    result
}
```

**Issue**: Every level of nesting requires explicit manual delegation with closures. For a deeply nested structure, this becomes unmanageable.

### 2. **Repetitive Primitive Serialization**

Throughout the codebase, we see patterns like:
- `|x, y| *y = *x` for copying primitives (u8, usize, etc.)
- `|_| ()` for no-op deserialization of primitives
- These are repeated hundreds of times

### 3. **Poor Composability**

When composing structures, you must:
- Explicitly handle every nested field
- Thread through all intermediate closures
- No way to say "use default behavior"
- Can't easily extend or modify structures

### 4. **No Separation of Concerns**

- Core serialization logic is mixed with application-specific types
- U8State and KeyValState are in the blob module but are configmaton-specific
- Hard to extract blob system as a standalone library

## Proposed Solutions

### Phase 1: Document Current System ✓

**Status**: IN PROGRESS
- [x] Read and understand all blob data structures
- [x] Document architecture in this file
- [ ] Add inline documentation to key files
- [ ] Create examples showing usage patterns

### Phase 2: Add Default Trait Implementations

**Goal**: Eliminate repetitive primitive handling

**Approach**:
1. Create `BlobSerialize` and `BlobDeserialize` traits with default implementations for primitives
2. Implement `AutoReserve`, `AutoSerialize`, `AutoDeserialize` for common patterns

**Example**:
```rust
pub trait BlobSerialize: Build {
    type SerializeCtx = ();

    unsafe fn serialize_elem(
        origin: &Self::Origin,
        cur: BuildCursor<Self>,
        ctx: &Self::SerializeCtx,
    ) -> BuildCursor<()>;
}

// Default for Copy types
impl<T: Build + Copy> BlobSerialize for T
where
    T::Origin: Copy
{
    unsafe fn serialize_elem(origin: &T::Origin, cur: BuildCursor<T>, _: &()) -> BuildCursor<()> {
        *cur.get_mut() = *origin;
        cur.behind(1)
    }
}
```

**Benefits**:
- `|x, y| *y = *x` becomes automatic
- `|_| ()` becomes automatic
- Reduces code by ~40%

### Phase 3: Improve Composability with Trait Bounds

**Goal**: Make nested structures compose naturally

**Approach**:
1. Add trait bounds that allow automatic delegation
2. Create `Nested<A, B>` helper that implements all three phases automatically
3. Use const generics for tuple-like structures

**Example**:
```rust
pub struct AutoCompose<A, B> {
    a: A,
    b: B,
}

impl<A: BlobSerialize, B: BlobSerialize> BlobSerialize for AutoCompose<A, B> {
    unsafe fn serialize_elem(...) -> ... {
        let b_cur = A::serialize_elem(&origin.a, cur.transmute(), ctx);
        B::serialize_elem(&origin.b, b_cur, ctx)
    }
}
```

### Phase 4: Separate Core from Application

**Goal**: Extract blob system as reusable library

**Structure**:
```
configmaton/
├── blob/              (move to configmaton-blob crate)
│   ├── core.rs       (Reserve, BuildCursor, Shifter, Build)
│   ├── vec.rs        (BlobVec)
│   ├── list.rs       (List)
│   ├── maps.rs       (VecMap, ListMap, HashMap, etc.)
│   ├── compose.rs    (Tupellum, Flagellum)
│   ├── bdd.rs        (Bdd)
│   └── traits.rs     (New composability traits)
└── automaton/         (configmaton-specific)
    ├── u8_state.rs   (U8State moved here)
    └── keyval_state.rs (KeyValState moved here)
```

### Phase 5: Add Derive Macros (Optional, Future)

**Goal**: Ultimate convenience

```rust
#[derive(BlobSerialize)]
pub struct MyState<'a> {
    #[blob(inline)]
    count: usize,

    #[blob(nested)]
    transitions: BlobVec<'a, u8>,

    #[blob(custom = "my_serializer")]
    special: MySpecialType,
}
```

## Testing Strategy

1. **Keep existing tests passing**: All tests in `blob.rs::tests` must continue to pass
2. **Add regression tests**: Before refactoring each module, add tests for current behavior
3. **Performance benchmarks**: Ensure no performance regression
4. **Incremental migration**: Old and new APIs can coexist during transition

## Success Criteria

- [ ] Reduce boilerplate by at least 50% in complex structures (like KeyValState)
- [ ] Maintain same performance (zero-copy, no additional allocations)
- [ ] All existing tests pass
- [ ] Blob system extracted to separate crate
- [ ] Better documentation and examples
- [ ] Cleaner, more maintainable code

## Current Status

- **Phase 1**: ✅ COMPLETED - Understanding and documenting (100%)
  - Added comprehensive documentation to core blob module
  - Documented BlobVec, List, Sediment, Tupellum
  - All existing tests pass

- **Phase 2**: ✅ COMPLETED - Default trait implementations (100%)
  - Created new `traits.rs` module with `BlobSerialize`, `BlobDeserialize`, `BlobReserve`
  - Implemented defaults for primitive types (u8, usize, ())
  - All tests pass (10 tests in blob module)
  - Next: Demonstrate usage by refactoring complex structures

- **Phase 3**: Not started - Composability improvements
- **Phase 4**: Not started - Separation of concerns
- **Phase 5**: Future work - Derive macros

## Notes

- No endianness handling currently implemented (only little-endian)
- Heavy use of unsafe code (by design for performance)
- Zero-copy deserialization is a key requirement
- Must maintain alignment requirements

## Timeline Estimate

- Phase 1: 2-3 hours
- Phase 2: 4-6 hours
- Phase 3: 6-8 hours
- Phase 4: 3-4 hours
- **Total**: ~20 hours of work

---

*Last updated: 2025-11-29*
