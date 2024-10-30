use std::collections::{HashMap, HashSet};
use std::mem::MaybeUninit;
use std::ptr::addr_of_mut;
use serde_json::Value;

pub mod lock;
pub mod command;
pub mod commands;
pub mod keyval_runner;
pub mod keyval_simulator;
pub mod guards;
pub mod dfa;
pub mod nfa;
pub mod ast;
pub mod config_parser;
pub mod char_runner;
pub mod borrow_lock;

// pub mod parser_to_simulator;

use crate::lock::{LockSelector, Lock};
use crate::command::{CommandBox, CommandTarget};

pub struct Onion<S: LockSelector> {
    parent: Option<S::Lock<Onion<S>>>,
    data: HashMap<String, Value>,
}

impl<S: LockSelector> Onion<S>
{
    pub fn new() -> Self {
        Onion {
            parent: None,
            data: HashMap::new(),
        }
    }

    pub fn share(self) -> S::Lock<Self> {
        <S::Lock<Self>>::from(self)
    }

    pub fn new_level(parent: S::Lock<Self>) -> Self {
        Onion {
            parent: Some(parent.clone()),
            data: HashMap::new(),
        }
    }

    pub fn get_rec(&self, key: &str) -> Option<Value> {
        if let Some(value) = self.data.get(key) {
            return Some(value.clone());
        }
        let parent_ref = self.parent.as_ref()?;
        let parent_onion = parent_ref.borrow();
        parent_onion.get_rec(key)
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        if let Some(value) = self.data.get(key) {
            return Some(value.clone());
        }

        let mut parent = self.parent.clone();
        loop {
            let parent_ref = parent.take()?;
            let parent_onion = parent_ref.borrow();
            if let Some(value) = parent_onion.data.get(key) {
                return Some(value.clone());
            }
            parent = parent_onion.parent.clone();
        }
    }

    pub fn set(&mut self, key: String, value: Value) {
        self.data.insert(key, value);
    }

    pub fn iter(&self) -> OnionIter<S> {
        OnionIter::new(self)
    }
}

pub struct OnionIter<S: LockSelector> {
    parent: Option<S::Lock<Onion<S>>>,
    visited: HashSet<String>,
    items: Vec<(String, Value)>,
}

impl <S: LockSelector> OnionIter<S> {
    pub fn new(onion: &Onion<S>) -> Self {
        OnionIter {
            parent: onion.parent.clone(),
            visited: HashSet::new(),
            items: onion.data.iter().map(|(key, val)| (key.clone(), val.clone())).collect(),
        }
    }
}

impl <S: LockSelector> Iterator for OnionIter<S>
{
    type Item = (String, Value);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            while let Some((key, value)) = self.items.pop() {
                if self.visited.insert(key.clone()) {
                    return Some((key, value));
                }
            }

            let parent_ref = self.parent.take()?;
            let parent_onion = parent_ref.borrow();

            self.items.extend(parent_onion.data.iter().map(|(key, val)| (key.clone(), val.clone())));
            self.parent = parent_onion.parent.clone();
        }
    }
}


pub trait Command {}

pub struct Configmaton<S: LockSelector> {
    onion: S::Lock<Onion<S>>,
    commands: HashSet<CommandBox<Self>>,
    stepping: bool,
}

impl <S: LockSelector> CommandTarget for Configmaton<S> {
    fn custom_fn(&mut self, value: &Value) {
        println!("CustomFn {}", value);
    }

    fn set(&mut self, key: &str, value: Value) {
        if self.stepping {
            self.onion.borrow_mut().set(key.to_string(), value);
        } else {
            self.stepping = true;

            let commands1 = self.on_read(unsafe { std::ptr::read(&self.commands) }, key, &value);
            let commands2 = self.post_read(commands1);
            self.onion.borrow_mut().set(key.to_string(), value);
            unsafe { std::ptr::write(&mut self.commands, commands2) }
            self.stepping = false;
        }
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.onion.borrow().get(key)
    }
}

impl <S: LockSelector> Configmaton<S> {
    pub fn new(commands: HashSet<CommandBox<Self>>) -> Self {
        let mut uresult: MaybeUninit<Configmaton<S>> = MaybeUninit::uninit();
        let presult = uresult.as_mut_ptr();
        unsafe { addr_of_mut!((*presult).onion).write(Onion::<S>::new().share()) };
        unsafe { addr_of_mut!((*presult).stepping).write(true) };
        let commands1 = unsafe { (*presult).post_read(commands) };
        unsafe { addr_of_mut!((*presult).commands).write(commands1) };
        let mut result = unsafe { uresult.assume_init() };
        result.stepping = false;
        result
    }

    pub fn new_level(self) -> Self {
        Configmaton {
            onion: Onion::<S>::new_level(self.onion).share(),
            commands: self.commands.clone(),
            stepping: false,
        }
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.onion.borrow().get(key)
    }

    fn post_read(&mut self, mut commands: HashSet<CommandBox<Self>>)
        -> HashSet<CommandBox<Self>>
    {
        let mut visited = HashSet::new();
        let mut new_commands = HashSet::new();

        while let Some(state) = commands.iter().next().cloned() {
            commands.remove(&state);
            if !visited.insert(state) {
                continue;
            }
            unsafe { &*state.inner }.execute_post(&mut commands, &mut new_commands, self);
        }

        return new_commands;
    }

    fn on_read(&mut self, mut commands: HashSet<CommandBox<Self>>, key: &str, value: &Value)
        -> HashSet<CommandBox<Self>>
    {
        let mut visited = HashSet::new();
        let mut new_commands = HashSet::new();

        while let Some(state) = commands.iter().next().cloned() {
            commands.remove(&state);
            if !visited.insert(state) {
                continue;
            }
            unsafe { &*state.inner }.execute(key, value, &mut commands, &mut new_commands, self);
        }

        return new_commands;
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Number;
    use crate::lock::RcRefCellSelector;

    #[test]
    fn onion_works() {
        let mut onion1 = Onion::<RcRefCellSelector>::new();
        onion1.set("a".to_string(), Value::Number(Number::from(1)));
        onion1.set("b".to_string(), Value::Number(Number::from(2)));
        onion1.set("a".to_string(), Value::Number(Number::from(3)));
        assert_eq!(onion1.get("a"), Some(Value::Number(Number::from(3))));
        assert_eq!(onion1.get("b"), Some(Value::Number(Number::from(2))));
        assert_eq!(onion1.get("c"), None);

        let onion1 = onion1.share();
        let mut onion2 = Onion::<RcRefCellSelector>::new_level(onion1.clone());
        onion2.set("b".to_string(), Value::Number(Number::from(4)));
        onion2.set("c".to_string(), Value::Number(Number::from(5)));
        assert_eq!(onion2.get("a"), Some(Value::Number(Number::from(3))));
        assert_eq!(onion2.get("b"), Some(Value::Number(Number::from(4))));
        assert_eq!(onion2.get("c"), Some(Value::Number(Number::from(5))));
        assert_eq!(onion2.get("d"), None);

        assert_eq!(onion1.borrow().get("a"), Some(Value::Number(Number::from(3))));
        assert_eq!(onion1.borrow().get("b"), Some(Value::Number(Number::from(2))));
        assert_eq!(onion1.borrow().get("c"), None);

        onion1.borrow_mut().set("a".to_string(), Value::Number(Number::from(6)));
        assert_eq!(onion1.borrow().get("a"), Some(Value::Number(Number::from(6))));
        assert_eq!(onion2.get("a"), Some(Value::Number(Number::from(6))));
    }
}
