//! Ancha-based serialization structures specific to configmaton.
//!
//! This module contains structures that use the ancha serialization system
//! but are specific to configmaton's domain (automaton states, etc.).

pub mod automaton;
pub mod keyval_state;
pub mod state;

/// Wrapper to make UnsafeIterator safe to use in standard iterator chains.
pub struct FakeSafeIterator<T: ancha::UnsafeIterator>(pub T);

impl<T: ancha::UnsafeIterator> Iterator for FakeSafeIterator<T> {
    type Item = T::Item;
    fn next(&mut self) -> Option<Self::Item> {
        unsafe { self.0.next() }
    }
}
