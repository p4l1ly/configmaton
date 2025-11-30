//! Automaton type aliases for the Ancha system.
//!
//! This module defines the top-level automaton structure as a composition
//! of the keyval_state and state types.

use super::keyval_state::{Bytes, KeyValState};
use super::state::U8State;
use ancha::sediment::AnchaSediment;
use ancha::tupellum::Tupellum;
use ancha::vec::AnchaVec;

/// Sediment containing KeyValState and U8State instances.
pub type States<'a> =
    Tupellum<'a, AnchaSediment<'a, KeyValState<'a>>, AnchaSediment<'a, U8State<'a>>>;

/// Vector of KeyValState pointers with the States structure.
pub type InitsAndStates<'a> = Tupellum<'a, AnchaVec<'a, *const KeyValState<'a>>, States<'a>>;

/// Extensions (Exts) and the automaton core.
pub type ExtsAndAut<'a> = Tupellum<
    'a,
    AnchaSediment<'a, Bytes<'a>>, // Exts
    InitsAndStates<'a>,
>;

/// Complete automaton structure: GetOlds + ExtsAndAut.
pub type Automaton<'a> = Tupellum<
    'a,
    AnchaSediment<'a, Bytes<'a>>, // GetOlds
    ExtsAndAut<'a>,
>;
