# Composable Serialization: Strategy Objects

## The Vision

Serialization should be **composable objects with customizable defaults**, not just trait methods.

### The Problem We're Solving

Original request:
> "E.g. BddOrigin has two type parameters: Var and Leaf. Let's say Var is int and Leaf is BlobVec<int>. If BddOrigin has Var that serializes to the Bdd's Var, and Leaf that serializes to Bdd's Leaf, we can just call Bdd's serialize with the default children-conversion strategy. But we can also override parts of children-conversion strategy, to e.g. multiply Var by two (artificial example), or to map them via some external dict."

### The Solution

**Serialization strategies are VALUES (objects)**, not just traits:

```rust
// Default: direct copy
let default_ser = DirectCopy::<usize>::new();

// Custom: multiply by 2
let custom_ser = MultiplyBy2;

// Compose: BlobVec with custom element serialization
let vec_ser = BlobVecSer::new(custom_ser);

// Use it!
vec_ser.serialize(&origin, cur);
```

## Architecture

### Two Kinds of Serialization

1. **StaticSerialization**: For fixed-size types that can be mutated in place
   ```rust
   trait StaticSerialization {
       type Origin;
       type Target;
       fn serialize(&self, origin: &Origin, target: &mut Target);
   }
   ```

2. **DynamicSerialization**: For variable-size types that need cursor management
   ```rust
   trait DynamicSerialization {
       type Origin;
       type Target<'a>: Build;  // Generic over blob lifetime
       fn reserve(&self, origin: &Origin, sz: &mut Reserve) -> usize;
       unsafe fn serialize<'a, After>(&self, origin: &Origin, cur: BuildCursor<Target<'a>>) -> BuildCursor<After>;
       unsafe fn deserialize<'a, After>(&self, cur: BuildCursor<Target<'a>>) -> BuildCursor<After>;
   }
   ```

### Key Innovation: Generic Associated Type (GAT)

`Target<'a>` allows the blob type to be parameterized by lifetime:
- The serialization strategy has NO lifetime (it's just logic)
- The blob HAS a lifetime (it lives in a buffer)
- `Target<'a>` bridges the gap: "For blob lifetime 'a, produce BlobVec<'a, X>"

## Example: BlobVec with Custom Serialization

```rust
// Define custom serialization: multiply by 2
struct MultiplyBy2;
impl StaticSerialization for MultiplyBy2 {
    type Origin = usize;
    type Target = usize;
    fn serialize(&self, origin: &Origin, target: &mut Target) {
        *target = *origin * 2;
    }
}

// Create BlobVec serializer with custom element strategy
let ser = BlobVecSer::new(MultiplyBy2);

// Serialize vector [1, 2, 3]
let origin = vec![1usize, 2, 3];
let mut sz = Reserve(0);
ser.reserve(&origin, &mut sz);

let mut buf = vec![0u8; sz.0];
let cur = BuildCursor::new(buf.as_mut_ptr());

unsafe {
    ser.serialize(&origin, cur.clone());
    ser.deserialize(cur);
}

// Result: [2, 4, 6] - elements multiplied by 2!
let blobvec = unsafe { &*(buf.as_ptr() as *const BlobVec<usize>) };
assert_eq!(blobvec.as_ref(), &[2, 4, 6]);
```

## Advantages

### 1. Multiple Origins for Same Blob Type

```rust
// Origin 1: Vec<usize> → BlobVec<usize> (direct)
let ser1 = BlobVecSer::new(DirectCopy::<usize>::new());

// Origin 2: Vec<usize> → BlobVec<usize> (multiplied)
let ser2 = BlobVecSer::new(MultiplyBy2);

// Origin 3: Vec<String> → BlobVec<usize> (parsed)
let ser3 = BlobVecSer::new(ParseString);
```

### 2. Customizable Defaults

```rust
// Default behavior
let default_bdd_ser = BddSerialization {
    var_ser: DirectCopy::new(),
    leaf_ser: DirectCopy::new(),
};

// Partial override: customize var serialization, keep leaf default
let custom_bdd_ser = BddSerialization {
    var_ser: MultiplyBy2,
    leaf_ser: DirectCopy::new(),
};
```

### 3. Composition Without Boilerplate

```rust
// Nest strategies
let inner_ser = DirectCopy::<u8>::new();
let outer_ser = BlobVecSer::new(inner_ser);
let nested_ser = SedimentSer::new(outer_ser);

// One line to serialize Sediment<BlobVec<u8>>!
nested_ser.serialize(&origin, cur);
```

### 4. External Context

```rust
// Map through external dictionary
struct DictMapSer<'a> {
    dict: &'a HashMap<usize, usize>,
}

impl<'a> StaticSerialization for DictMapSer<'a> {
    type Origin = usize;
    type Target = usize;
    fn serialize(&self, origin: &Origin, target: &mut Target) {
        *target = self.dict.get(origin).copied().unwrap_or(*origin);
    }
}

// Use it!
let dict = create_mapping();
let ser = BlobVecSer::new(DictMapSer { dict: &dict });
ser.serialize(&origin, cur);
```

## Implementation Status

### ✅ Completed

- `StaticSerialization` trait for fixed-size types
- `DynamicSerialization` trait with GAT for variable-size types
- `DirectCopy<T>` - default serialization for primitives
- `MultiplyBy2` - example custom serialization
- `StaticToDynamic` - adapter to lift Static → Dynamic
- `BlobVecSer<ElemSer>` - composable BlobVec serialization
- All tests passing (29/29)

### 🔄 Next Steps

1. **BDD Serialization**: Implement composable Bdd serialization
   ```rust
   struct BddSerialization<VarSer, LeafSer> {
       var_ser: VarSer,
       leaf_ser: LeafSer,
   }
   ```

2. **More Containers**: Sediment, List, Tupellum using the new approach

3. **Helper Functions**: Simplify common patterns
   ```rust
   fn direct<T>() -> BlobVecSer<DirectCopy<T>> {
       BlobVecSer::new(DirectCopy::new())
   }
   ```

4. **Documentation**: Add examples for each container

## Comparison to Previous Approaches

### Attempt 1: Trait methods (rejected)
- Problem: Can't customize - trait methods are fixed
- Example: `BlobVec::serialize` always does direct copy

### Attempt 2: Category-theoretic strategies (wrong focus)
- Problem: Solved iteration patterns, not customization
- Focus: FixedSize vs VariableSize (how to iterate)
- Missing: Ability to change WHAT happens during iteration

### Attempt 3: Composable objects (THIS APPROACH) ✓
- Solution: Serialization as first-class values
- Benefits: Multiple origins, customizable defaults, composition
- Example: `BlobVecSer::new(MultiplyBy2)` - just works!

## The Key Insight

**Serialization is a RELATIONSHIP between Origin and Target, mediated by a STRATEGY**.

The strategy is:
- A value (can be created, stored, passed)
- Composable (strategies contain strategies)
- Customizable (override just the parts you need)
- Reusable (same strategy for many serializations)

This matches the original vision: defaults with selective overrides, multiple origins for one target, external context support.
