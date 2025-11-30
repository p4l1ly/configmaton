# Ancha: The New Serialization System

## Name Origin

"Ancha" comes from "anchor" and the Slovak name "Anča".

## Core Philosophy

**Serialization strategies are composable objects with customizable defaults.**

## Key Changes from Old System

### 1. Removed `Build` Trait

The old `Build` trait just linked blob types to origins:
```rust
trait Build {
    type Origin;
}
```

This is now part of the `Anchize` trait, which is more explicit about the relationship.

### 2. Split Serialization and Deserialization

**Old**: Combined in one trait/method
**New**: Separate traits

- `Anchize`: Origin → Ancha (serialization)
- `Deanchize`: Pointer fixup (no origin needed!)

### 3. Terminology

| Old | New |
|-----|-----|
| Blob | Ancha |
| Target | Ancha |
| serialize | anchize |
| deserialize | deanchize |
| Build trait | (removed) |

### 4. Implementation Location

**Old**: Helper module (ser.rs) with examples
**New**: Each data structure implements its own anchization in its module

## The Type System

### For Fixed-Size Types (Primitives)

```rust
trait StaticAnchize {
    type Origin;
    type Ancha;  // No lifetime!
    fn anchize_static(&self, origin: &Origin, ancha: &mut Ancha);
}
```

### For Variable-Size Types (Containers)

```rust
trait Anchize {
    type Origin;
    type Ancha<'a>: Sized;  // Generic over blob lifetime!
    fn reserve(&self, origin: &Origin, sz: &mut Reserve) -> usize;
    unsafe fn anchize<'a, After>(&self, origin: &Origin, cur: BuildCursor<Ancha<'a>>) -> BuildCursor<After>;
}

trait Deanchize {
    type Ancha<'a>: Sized;
    unsafe fn deanchize<'a, After>(&self, cur: BuildCursor<Ancha<'a>>) -> BuildCursor<After>;
}
```

## Example: AnchaVec with Custom Element Anchization

```rust
// Custom anchization: multiply by 2
struct MultiplyBy2;
impl StaticAnchize for MultiplyBy2 {
    type Origin = usize;
    type Ancha = usize;
    fn anchize_static(&self, origin: &Origin, ancha: &mut Ancha) {
        *ancha = *origin * 2;
    }
}

// Create vector anchization with custom element strategy
let ancha = VecAncha::new(MultiplyBy2);

// Anchize vec![1,2,3]
ancha.anchize(&vec![1,2,3], cur);

// Result: [2,4,6] - elements multiplied!
```

## Implementation Status

### ✅ Completed

- Core `Anchize` and `Deanchize` traits
- `StaticAnchize` for fixed-size types
- `DirectCopy<T>` - default for primitives
- `AnchaVec<X>` - fully migrated with tests
- Memory management utilities (Reserve, BuildCursor, Shifter)

### 🔄 In Progress

- BDD with composable var/leaf anchization
- Sediment, List, Tupellum
- Other data structures

### 📋 TODO

- Migrate all blob structures to ancha
- Remove old blob module
- Update all uses throughout codebase
- Add more examples

## Advantages

1. **Multiple Origins**: Same ancha type, different origin transformations
2. **Customizable Defaults**: Override just the parts you need
3. **Composition**: Strategies contain strategies
4. **External Context**: Pass dictionaries, mappings, etc.
5. **Clean Separation**: Anchization (needs origin) vs Deanchization (no origin)

## Next Steps

1. Implement `BddAncha<VarAnchize, LeafAnchize>` to demonstrate composition
2. Migrate Sediment, List, Tupellum
3. Update existing code to use ancha
4. Remove blob module
