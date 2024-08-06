use std::collections::{HashMap, HashSet};
use std::mem::MaybeUninit;
use std::ptr::addr_of_mut;
use serde_json::Value;

mod lock;
mod command;
pub mod commands;

use crate::lock::{LockSelector, Lock};
use crate::command::{CommandBox, CommandTarget};

pub struct Onion<'a, S: LockSelector> {
    parent: Option<S::Lock<Onion<'a, S>>>,
    data: HashMap<&'a str, Value>,
}

impl<'a, S: LockSelector> Onion<'a, S>
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

    pub fn set(&mut self, key: &'a str, value: Value) {
        self.data.insert(key, value);
    }

    pub fn iter(&self) -> OnionIter<'a, S> {
        OnionIter::new(self)
    }
}

pub struct OnionIter<'a, S: LockSelector> {
    parent: Option<S::Lock<Onion<'a, S>>>,
    visited: HashSet<&'a str>,
    items: Vec<(&'a str, Value)>,
}

impl <'a, S: LockSelector> OnionIter<'a, S> {
    pub fn new(onion: &Onion<'a, S>) -> Self {
        OnionIter {
            parent: onion.parent.clone(),
            visited: HashSet::new(),
            items: onion.data.iter().map(|(key, val)| (*key, val.clone())).collect(),
        }
    }
}

impl <'a, S: LockSelector> Iterator for OnionIter<'a, S>
{
    type Item = (&'a str, Value);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            while let Some((key, value)) = self.items.pop() {
                if self.visited.insert(key) {
                    return Some((key, value));
                }
            }

            let parent_ref = self.parent.take()?;
            let parent_onion = parent_ref.borrow();

            self.items.extend(parent_onion.data.iter().map(|(key, val)| (*key, val.clone())));
            self.parent = parent_onion.parent.clone();
        }
    }
}


pub trait Command {}

pub struct Configmaton<'a, S: LockSelector> {
    onion: S::Lock<Onion<'a, S>>,
    commands: HashSet<CommandBox<'a, Self>>,
    stepping: bool,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl <'a, S: LockSelector> CommandTarget for Configmaton<'a, S> {}

impl <'a, S: LockSelector> Configmaton<'a, S> {
    pub fn new(commands: HashSet<CommandBox<'a, Self>>) -> Self {
        let mut uresult: MaybeUninit<Configmaton<'a, S>> = MaybeUninit::uninit();
        let presult = uresult.as_mut_ptr();
        unsafe { addr_of_mut!((*presult).onion).write(Onion::<'a, S>::new().share()) };
        unsafe { addr_of_mut!((*presult).stepping).write(true) };
        let commands1 = unsafe { (*presult).post_read(commands) };
        unsafe { addr_of_mut!((*presult).commands).write(commands1) };
        let mut result = unsafe { uresult.assume_init() };
        result.stepping = false;
        result
    }

    pub fn new_level(self) -> Self {
        Configmaton {
            onion: Onion::<'a, S>::new_level(self.onion).share(),
            commands: self.commands.clone(),
            stepping: false,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.onion.borrow().get(key)
    }

    pub fn set(&mut self, key: &'a str, value: Value) {
        if self.stepping {
            self.onion.borrow_mut().set(key, value);
        } else {
            self.stepping = true;

            let commands1 = self.on_read(unsafe { std::ptr::read(&self.commands) }, key, &value);
            let commands2 = self.post_read(commands1);
            self.onion.borrow_mut().set(key, value);
            unsafe { std::ptr::write(&mut self.commands, commands2) }
            self.stepping = false;
        }
    }

    fn post_read(&mut self, mut commands: HashSet<CommandBox<'a, Self>>)
        -> HashSet<CommandBox<'a, Self>>
    {
        let mut visited = HashSet::new();
        let mut new_commands = HashSet::new();

        while let Some(state) = commands.iter().next().cloned() {
            commands.remove(&state);
            if !visited.insert(state.clone()) {
                continue;
            }
            state.inner.execute_post(&mut commands, &mut new_commands, self);
        }

        return new_commands;
    }

    fn on_read(&mut self, mut commands: HashSet<CommandBox<'a, Self>>, key: &str, value: &Value)
        -> HashSet<CommandBox<'a, Self>>
    {
        let mut visited = HashSet::new();
        let mut new_commands = HashSet::new();

        while let Some(state) = commands.iter().next().cloned() {
            commands.remove(&state);
            if !visited.insert(state.clone()) {
                continue;
            }
            state.inner.execute(key, value, &mut commands, &mut new_commands, self);
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
    fn it_works() {
        let mut onion1 = Onion::<'_, RcRefCellSelector>::new();
        onion1.set("a", Value::Number(Number::from(1)));
        onion1.set("b", Value::Number(Number::from(2)));
        onion1.set("a", Value::Number(Number::from(3)));
        assert_eq!(onion1.get("a"), Some(Value::Number(Number::from(3))));
        assert_eq!(onion1.get("b"), Some(Value::Number(Number::from(2))));
        assert_eq!(onion1.get("c"), None);

        let onion1 = onion1.share();
        let mut onion2 = Onion::<'_, RcRefCellSelector>::new_level(onion1.clone());
        onion2.set("b", Value::Number(Number::from(4)));
        onion2.set("c", Value::Number(Number::from(5)));
        assert_eq!(onion2.get("a"), Some(Value::Number(Number::from(3))));
        assert_eq!(onion2.get("b"), Some(Value::Number(Number::from(4))));
        assert_eq!(onion2.get("c"), Some(Value::Number(Number::from(5))));
        assert_eq!(onion2.get("d"), None);

        assert_eq!(onion1.borrow().get("a"), Some(Value::Number(Number::from(3))));
        assert_eq!(onion1.borrow().get("b"), Some(Value::Number(Number::from(2))));
        assert_eq!(onion1.borrow().get("c"), None);

        onion1.borrow_mut().set("a", Value::Number(Number::from(6)));
        assert_eq!(onion1.borrow().get("a"), Some(Value::Number(Number::from(6))));
        assert_eq!(onion2.get("a"), Some(Value::Number(Number::from(6))));
    }
}
