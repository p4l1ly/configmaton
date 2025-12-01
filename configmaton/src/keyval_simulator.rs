use indexmap::IndexSet;

use crate::{
    keyval_runner::Runner,
    my_ancha::{
        automaton::{Automaton, InitsAndStates},
        keyval_state::{Bytes, KeyValState},
    },
};
use ancha::{
    align_up_ptr, get_behind_struct, sediment::AnchaSediment as Sediment, tupellum::Tupellum,
    vec::AnchaVec as BlobVec,
};

#[derive(Clone)]
pub struct Simulation<'a> {
    keyval_runner: Runner<'a>,
    pub exts: IndexSet<&'a [u8]>,
    getolds: IndexSet<&'a [u8]>,
}

impl<'a> Simulation<'a> {
    pub fn new<F: Fn(&'a [u8]) -> Option<&'a [u8]>>(aut1: &Automaton<'a>, db: F) -> Self {
        let mut getolds = IndexSet::new();
        let mut exts = IndexSet::new();
        let mut behind = unsafe { get_behind_struct(aut1) };
        unsafe {
            aut1.a.each(|getold| {
                getolds.insert(getold.as_ref());
                behind = getold.behind();
                behind
            })
        };
        let aut2: &Tupellum<'a, Sediment<'a, Bytes<'a>>, InitsAndStates<'a>> =
            unsafe { &*align_up_ptr(behind) };
        let mut behind = unsafe { get_behind_struct(aut2) };
        unsafe {
            aut2.a.each(|ext| {
                exts.insert(ext.as_ref());
                behind = ext.behind();
                behind
            })
        };
        let initial_states: &BlobVec<*const KeyValState<'a>> = unsafe { &*align_up_ptr(behind) };
        let mut sim = Simulation {
            keyval_runner: unsafe { Runner::new(initial_states.as_ref().iter().map(|x| &**x)) },
            exts,
            getolds,
        };
        sim.finish_read(db);
        sim
    }

    pub fn read<F: Fn(&'a [u8]) -> Option<&'a [u8]>>(
        &mut self,
        key: &'a [u8],
        val: &'a [u8],
        db: F,
    ) {
        unsafe {
            self.keyval_runner.read(
                key,
                val,
                |getold| {
                    self.getolds.insert(getold);
                },
                |ext| {
                    self.exts.insert(ext);
                },
            );
        };
        self.finish_read(db)
    }

    fn finish_read<F: Fn(&'a [u8]) -> Option<&'a [u8]>>(&mut self, db: F) {
        while let Some(key) = self.getolds.pop() {
            if let Some(val) = db(&key) {
                unsafe {
                    self.keyval_runner.read(
                        key,
                        val,
                        |getold| {
                            self.getolds.insert(getold);
                        },
                        |ext| {
                            self.exts.insert(ext);
                        },
                    );
                }
            }
        }
    }
}
