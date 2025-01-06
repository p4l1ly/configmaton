use indexmap::IndexSet;

use crate::{blob::{align_up_ptr, automaton::{Automaton, InitsAndStates}, get_behind_struct, keyval_state::{Bytes, KeyValState}, sediment::Sediment, tupellum::Tupellum, vec::BlobVec}, keyval_runner::Runner};

pub struct Simulation<'a> {
    keyval_runner: Runner<'a>,
    pub exts: IndexSet<&'a [u8]>,
    getolds: IndexSet<&'a [u8]>,
}

impl<'a> Simulation<'a> {
    pub fn new<F: Fn(&[u8]) -> Option<&[u8]>>(
        aut1: &Automaton<'a>,
        db: F,
    ) -> Self {
        let mut getolds = IndexSet::new();
        let mut exts = IndexSet::new();
        let mut behind = unsafe { get_behind_struct(aut1) };
        unsafe { aut1.a.each(|getold| {
            getolds.insert(getold.as_ref());
            behind = getold.behind();
            behind
        }) };
        let aut2: &Tupellum<'a, Sediment<'a, Bytes<'a>>, InitsAndStates<'a>> =
            unsafe { &*align_up_ptr(behind) };
        let mut behind = unsafe { get_behind_struct(aut2) };
        unsafe { aut2.a.each(|ext| {
            exts.insert(ext.as_ref());
            behind = ext.behind();
            behind
        }) };
        let initial_states: &BlobVec<*const KeyValState<'a>> = unsafe { &*align_up_ptr(behind) };
        let mut runner = Simulation {
            keyval_runner: unsafe { Runner::new(initial_states.as_ref().iter().map(|x| &**x )) },
            exts,
            getolds,
        };
        runner.finish_read(db);
        runner
    }

    pub fn read<F: Fn(&[u8]) -> Option<&[u8]>>(&mut self, key: &[u8], val: &[u8], db: F) {
        unsafe {
            self.keyval_runner.read(key, val,
                |getold| { self.getolds.insert(getold); },
                |ext| { self.exts.insert(ext); }
            )
        };
        self.finish_read(db)
    }

    fn finish_read<F: Fn(&[u8]) -> Option<&[u8]>>(&mut self, db: F) {
        while let Some(oldkey) = self.getolds.pop() {
            if let Some(oldval) = db(&oldkey) {
                unsafe {
                    self.keyval_runner.read(oldkey, oldval,
                        |getold| { self.getolds.insert(getold); },
                        |ext| { self.exts.insert(ext); }
                    );
                }
            }
        }
    }
}
