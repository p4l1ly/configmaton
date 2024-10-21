use indexmap::IndexSet;


use serde_json::Value;

use crate::{automaton::{Explicit, Listeners, TranListener}, lock::LockSelector};

pub enum Trigger {
    Old(String),
    Ext(Value),
}

pub struct KeyValSimulator<S: LockSelector> {
    listeners: Listeners<S, Trigger>
}

pub struct MyTranListener {
    result: Vec<Value>,
    old_queries: IndexSet<String>,
}

impl TranListener<Trigger> for MyTranListener {
    fn trigger(&mut self, trigger: &Trigger) {
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

impl<S: LockSelector> KeyValSimulator<S> {
    pub fn read<'a, F: Fn(&str) -> &'a str>
        (&'a mut self, key: String, val: &'a str, olds: F) -> Vec<Value>
    {
        let mut tl = MyTranListener { result: vec![], old_queries: IndexSet::new() };
        self.listeners.read(Explicit::NewVar(key), &mut tl);
        for c in val.chars() {
            self.listeners.read(Explicit::Char(c as u8), &mut tl);
        }
        while let Some(oldkey) = tl.old_queries.pop() {
            let oldval = olds(&oldkey);
            self.listeners.read(Explicit::OldVar(oldkey), &mut tl);
            for c in oldval.chars() {
                self.listeners.read(Explicit::Char(c as u8), &mut tl);
            }
        }
        tl.result
    }
}
