use super::{keyval_state::{Bytes, KeyValState}, sediment::Sediment, state::U8State, tupellum::Tupellum, vec::BlobVec};

pub type States<'a> = Tupellum<'a, Sediment<'a, KeyValState<'a>>, Sediment<'a, U8State<'a>>>;
pub type InitsAndStates<'a> = Tupellum<'a, BlobVec<'a, *const KeyValState<'a>>, States<'a>>;
pub type ExtsAndAut<'a> = 
    Tupellum<'a,
        Sediment<'a, Bytes<'a>>,  // Exts
        InitsAndStates<'a>
    >;
pub type Automaton<'a> = Tupellum<'a,
    Sediment<'a, Bytes<'a>>,  // GetOlds
    ExtsAndAut<'a>
>;
