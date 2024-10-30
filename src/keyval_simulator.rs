use indexmap::IndexSet;

use serde_json::Value;

use crate::keyval_runner::{Runner, TranListener, AnyStateLock};

#[derive(Clone, Debug)]
pub enum Trigger<'a> {
    Old(&'a str),
    Ext(&'a Value),
}

#[derive(Clone, Debug)]
pub struct Triggers<'a>(pub Box<[Trigger<'a>]>);

#[derive(Clone, Debug)]
pub struct KeyValState<'a> {
    pub result: Vec<&'a Value>,
    pub old_queries: IndexSet<&'a str>,
}

impl KeyValState<'_> {
    pub fn new() -> Self {
        KeyValState { result: vec![], old_queries: IndexSet::new() }
    }
}

impl<'a> TranListener<Triggers<'a>> for KeyValState<'a> {
    fn trigger(&mut self, trigger: &Triggers<'a>) {
        for trigger in trigger.0.iter() {
            match trigger {
                Trigger::Old(key) => {
                    self.old_queries.insert(key);
                }
                Trigger::Ext(value) => {
                    self.result.push(value);
                }
            }
        }
    }
}

pub struct Simulator<'a> {
    runner: Runner<'a, Triggers<'a>>
}

impl<'a> Simulator<'a> {
    pub fn new<'b, I: IntoIterator<Item = &'b AnyStateLock<'a, Triggers<'a>>>>
    (initial_states: I) -> Self
    where 'a: 'b
    {
        Simulator { runner: Runner::new(initial_states) }
    }
}

impl<'a> Simulator<'a> {
    pub fn read<'b, F: Fn(&str) -> Option<&'b str>>
        (&'b mut self, key: &'b str, val: &'b str, olds: F)
        -> Vec<&'a Value>
    {
        let mut tl = KeyValState { result: vec![], old_queries: IndexSet::new() };
        self.runner.read(key, val, &mut tl);
        self.finish_read(tl, olds)
    }

    pub fn finish_read<'b, F: Fn(&str) -> Option<&'b str>>
        (&mut self, mut tl: KeyValState<'a>, olds: F)
        -> Vec<&'a Value>
    {
        while let Some(oldkey) = tl.old_queries.pop() {
            if let Some(oldval) = olds(&oldkey) {
                self.runner.read(oldkey, oldval, &mut tl);
            }
        }
        tl.result
    }
}
