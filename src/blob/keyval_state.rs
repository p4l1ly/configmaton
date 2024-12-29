use super::{bdd::Bdd, list::List, sediment::Sediment, state::U8State, tupellum::Tupellum, vec::BlobVec};

pub struct LeafOrigin {
    pub states: Vec<usize>,
    pub get_olds: Vec<Vec<u8>>,
    pub exts: Vec<Vec<u8>>,
}

pub type LeafMeta<'a> = Tupellum<'a, Sediment<'a, BlobVec<'a, u8>>, Sediment<'a, BlobVec<'a, u8>>>;
pub type Leaf<'a> = Tupellum<'a, BlobVec<'a, *const KeyValState<'a>>, LeafMeta<'a>>;
pub type Finals<'a> = Bdd<'a, usize, Leaf<'a>>;
pub type InitsAndFinals<'a> = Tupellum<'a, BlobVec<'a, *const U8State<'a>>, Finals<'a>>;

#[repr(C)]
pub struct KeyValState<'a> {
    pub keyvals: List<'a, Tupellum<'a, BlobVec<'a, u8>, InitsAndFinals<'a>>>,
}
