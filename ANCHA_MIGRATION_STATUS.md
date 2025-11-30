# Ancha System Migration Status

## Overview

Successfully migrated the blob library to the new "Ancha" system with composable serialization objects.

## Core Concepts

### Key Design Principles (from user feedback)
- **Serialize returns SAME TYPE cursor**: `anchize(elem, cur)` returns `BuildCursor<SameType>` positioned for the next element
- **Alignment at start, not end**: `behind()` aligns at the beginning; return values are unaligned
- **Preserve original logic**: Don't rework the logic - add flexibility and remove boilerplate while keeping the well-thought-out algorithms intact
- **Composable strategies**: Serialization/deserialization are objects that can be composed and customized, not just trait methods

### Traits

- **`StaticAnchize`**: For fixed-size types (primitives), handles in-place mutation
  ```rust
  fn anchize_static(&self, origin: &Self::Origin, target: &mut Self::Ancha);
  ```

- **`Anchize`**: For dynamic/variable-size types
  ```rust
  fn anchize<After>(&self, origin: &Self::Origin, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After>;
  ```
  - Returns cursor of type `After` (often same as `Self::Ancha` for iteration)
  - The returned cursor is positioned for the next element

- **`Deanchize`**: Origin-agnostic deserialization
  ```rust
  fn deanchize<After>(&self, cur: BuildCursor<Self::Ancha>) -> BuildCursor<After>;
  ```

## Completed Migrations

### ✅ Core Structures (All tests passing: 39/39)

1. **`VecAncha`** (`ancha/vec.rs`)
   - Replaces `BlobVec` serialization
   - Takes `ElemAnchize: StaticAnchize` for elements
   - Fixed-size elements stored sequentially

2. **`SedimentAncha`** (`ancha/sediment.rs`)
   - Replaces `Sediment` serialization
   - Takes `ElemAnchize: Anchize` for variable-size elements
   - Elements stored sequentially without gaps

3. **`ListAncha`** (`ancha/list.rs`)
   - Replaces `List` serialization
   - Intrusive linked list with inline node storage
   - Takes `ElemAnchize: Anchize` for variable-size elements

4. **`TupellumAncha`** (`ancha/tupellum.rs`)
   - Replaces `Tupellum` serialization
   - Two-element tuple with sequential storage
   - Takes `AAnchize` and `BAnchize` for both elements

5. **`BddAncha`** (`ancha/bdd.rs`)
   - Replaces `Bdd` serialization
   - Binary Decision Diagram with DAG sharing
   - Takes `VarAnchize` and `LeafAnchize` for customization
   - Handles complex graph traversal and pointer fixup

6. **`VecMapAncha`** (`ancha/vecmap.rs`)
   - Replaces `VecMap` serialization
   - Map stored as vector of keys with pointers to values
   - Takes `KeyAnchize: StaticAnchize` and `ValAnchize: Anchize`
   - Keys stored inline, values stored separately

## Remaining Work

### Structures to Migrate (from `blob/`)

#### High Priority (Actually Used)
- `state.rs` - Complex U8State structure (used in keyval_nfa)
- `automaton.rs` - Automaton structure (used in keyval_nfa, configmaton)
- `keyval_state.rs` - KeyValState (used in keyval_runner, keyval_nfa)

#### Dependencies for Above
- `vecmap.rs` - VecMap (used by U8State)
- `arrmap.rs` - ArrMap (used by U8State)
- `hashmap.rs` - BlobHashMap (used by U8State)

#### Lower Priority (Exported but unused)
- `assoc_list.rs`
- `flagellum.rs`
- `listmap.rs`

### Final Steps
- Migrate critical structures (state, automaton, keyval_state) and their dependencies
- Update all usage sites to use new Ancha system
- Remove old `blob` module
- Update documentation

## Key Files

- `/home/paly/hobby/configmaton/configmaton/src/ancha.rs` - Main module
- `/home/paly/hobby/configmaton/ANCHA_SYSTEM.md` - System documentation
- `/home/paly/hobby/configmaton/BLOB_REFACTORING_PLAN.md` - Original plan
