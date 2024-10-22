use indexmap::IndexSet;

use serde_json::Value;

use crate::{automaton::{Explicit, Listeners, TranListener, AnyStateLock}, lock::LockSelector};

#[derive(Clone, Debug)]
pub enum Trigger {
    Old(String),
    Ext(Value),
}

#[derive(Clone, Debug)]
pub struct Triggers(pub Box<[Trigger]>);

pub struct KeyValState {
    pub result: Vec<Value>,
    pub old_queries: IndexSet<String>,
}

impl KeyValState {
    pub fn new() -> Self {
        KeyValState { result: vec![], old_queries: IndexSet::new() }
    }
}

impl TranListener<Triggers> for KeyValState {
    fn trigger(&mut self, trigger: &Triggers) {
        for trigger in trigger.0.iter() {
            match trigger {
                Trigger::Old(key) => {
                    self.old_queries.insert(key.clone());
                }
                Trigger::Ext(value) => {
                    self.result.push(value.clone());
                }
            }
        }
    }
}


pub trait Database {
    fn read(&self, key: String) -> Option<String>;
}

pub struct KeyValSimulator<S: LockSelector> {
    listeners: Listeners<S, Triggers>
}

impl<S: LockSelector> KeyValSimulator<S> {
    pub fn new<I: IntoIterator<Item = AnyStateLock<S, Triggers>>>(initial_states: I) -> Self {
        KeyValSimulator { listeners: Listeners::new(initial_states) }
    }
}

impl<S: LockSelector> KeyValSimulator<S> {
    pub fn read<'a, F: Fn(&str) -> Option<&'a str>>
        (&'a mut self, key: String, val: &'a str, olds: F)
        -> Vec<Value>
    {
        let mut tl = KeyValState { result: vec![], old_queries: IndexSet::new() };
        self.listeners.read(Explicit::Var(key), &mut tl);
        for c in val.chars() {
            self.listeners.read(Explicit::Char(c as u8), &mut tl);
        }
        self.finish_read(tl, olds)
    }

    pub fn finish_read<'a, F: Fn(&str) -> Option<&'a str>>
        (&mut self, mut tl: KeyValState, olds: F)
        -> Vec<Value>
    {
        while let Some(oldkey) = tl.old_queries.pop() {
            if let Some(oldval) = olds(&oldkey) {
                self.listeners.read(Explicit::Var(oldkey), &mut tl);
                for c in oldval.chars() {
                    self.listeners.read(Explicit::Char(c as u8), &mut tl);
                }
            }
        }
        tl.result
    }
}
