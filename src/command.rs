use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::ptr;

use serde_json::Value;

pub trait CommandTarget {
    fn custom_fn(&mut self, value: &Value);
    fn set(&mut self, key: &str, value: Value);
    fn get(&self, key: &str) -> Option<Value>;
}

pub trait Command<T> {
    fn execute_post(
        &self,
        commands: &mut HashSet<CommandBox<T>>,
        new_commands: &mut HashSet<CommandBox<T>>,
        configmaton: &mut T,
    );

    fn execute(
        &self,
        _key: &str,
        _value: &Value,
        commands: &mut HashSet<CommandBox<T>>,
        new_commands: &mut HashSet<CommandBox<T>>,
        configmaton: &mut T,
    ) {
        self.execute_post(commands, new_commands, configmaton);
    }
}

pub struct CommandBox<T> {
    pub inner: *const dyn Command<T>,
}

impl<T> Clone for CommandBox<T> {
    fn clone(&self) -> Self {
        CommandBox { inner: self.inner }
    }
}

impl<T> Copy for CommandBox<T> {}

impl<T> PartialEq for CommandBox<T> {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.inner, other.inner)
    }
}

impl<T> Eq for CommandBox<T> {}

impl<T> Hash for CommandBox<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        ptr::hash(self.inner, state);
    }
}
