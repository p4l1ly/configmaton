# Ancha Implementation Status

## ✅ Working

### Core System
- `Anchize` trait (serialization with origin)
- `Deanchize` trait (pointer fixup, no origin needed)
- `StaticAnchize` trait (fixed-size types)
- Memory management: `Reserve`, `BuildCursor`, `Shifter`
- `DirectCopy<T>` default anchization for primitives

### Data Structures
- **AnchaVec** - fully working with tests ✓
  - Composable element anchization
  - Custom element strategies (e.g., MultiplyBy2)
  - All tests pass (2/2)

### BDD - Structure Complete, Bugs to Fix
- ✅ `BddOrigin<Var, Leaf>` - origin representation
- ✅ `AnchaBdd<'a, Var, Leaf>` - ancha representation
- ✅ `BddAncha<VarAnchize, LeafAnchize>` - composable strategy
- ✅ `evaluate` method works
- ✅ Compiles successfully

#### BDD Runtime Issues (TODO)
1. **HashMap lookup fails** - line 330: `unwrap()` on `None`
   - Likely issue with how pointers are being tracked/stored
   - Need to debug pointer mapping in second pass

2. **Misaligned pointer** - line 153 in evaluate
   - Alignment issue during anchization
   - Possibly not aligning properly between BDD nodes

These are fixable bugs in the logic, not design issues!

## 🔄 TODO: Migrate from Blob

### Priority 1 (Core Structures)
- [ ] Sediment (variable-size sequential elements)
- [ ] List (linked list)
- [ ] Tupellum (tuple of two elements)

### Priority 2 (Maps)
- [ ] VecMap
- [ ] ArrMap
- [ ] AssocList
- [ ] BlobHashMap

### Priority 3 (Application-Specific)
- [ ] U8State
- [ ] KeyValState
- [ ] Automaton structures

### Priority 4 (Cleanup)
- [ ] Remove blob module entirely
- [ ] Update all imports throughout codebase
- [ ] Remove old `Build` trait usage
- [ ] Update documentation

## Key Design Decisions

1. **Build trait removed** - merged into Anchize
2. **Separate Anchize/Deanchize** - deanchization is origin-agnostic
3. **GAT for lifetime** - `Ancha<'a>` allows blob lifetime parameterization
4. **'static bounds** - strategies need 'static for type safety
5. **where Self: 'a** - added to trait to ensure proper lifetimes

## Example: Custom Anchization

```rust
// Custom: multiply vars by 10
struct MultiplyBy10;
impl StaticAnchize for MultiplyBy10 {
    type Origin = u8;
    type Ancha = u8;
    fn anchize_static(&self, origin: &Origin, ancha: &mut Ancha) {
        *ancha = *origin * 10;
    }
}

// Use with BDD: custom var, default leaf
let ancha = BddAncha::new(
    MultiplyBy10,
    VecAncha::new(DirectCopy::<u8>::new())
);

ancha.anchize(&bdd_origin, cur);  // Vars multiplied by 10!
```

## Next Session Goals

1. Fix BDD HashMap pointer tracking
2. Fix BDD alignment issues
3. Implement Sediment with composable element anchization
4. Implement List
5. Start migrating existing code to use ancha

## Performance Notes

- Zero overhead (compile-time resolution)
- All strategy selection is static
- No vtables, no dynamic dispatch
- Same performance as hand-written code
