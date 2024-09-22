use std::collections::HashSet;
use serde_json::Value;

use crate::command::{Command, CommandBox, CommandTarget};

pub struct Fork<T> {
    pub branches: Box<[CommandBox<T>]>,
}

impl<T> Command<T> for Fork<T> {
    fn execute_post(
        &self,
        _commands: &mut HashSet<CommandBox<T>>,
        new_commands: &mut HashSet<CommandBox<T>>,
        _configmaton: &mut T,
    ) {
        for branch in self.branches.iter().copied() {
            new_commands.insert(branch);
        }
    }
}

pub struct End {}

impl<T> Command<T> for End {
    fn execute_post(
        &self,
        _commands: &mut HashSet<CommandBox<T>>,
        _new_commands: &mut HashSet<CommandBox<T>>,
        _configmaton: &mut T,
    ) {
    }
}

pub struct CustomFn<T> {
    pub value: Value,
    pub continuation: CommandBox<T>,
}

impl<T: CommandTarget> Command<T> for CustomFn<T> {
    fn execute_post(
        &self,
        commands: &mut HashSet<CommandBox<T>>,
        _new_commands: &mut HashSet<CommandBox<T>>,
        configmaton: &mut T,
    ) {
        configmaton.custom_fn(&self.value);
        commands.insert(self.continuation);
    }
}

pub struct Skip<T> {
    pub continuation: CommandBox<T>,
}

impl<T: 'static> Command<T> for Skip<T> {
    fn execute_post(
        &self,
        _commands: &mut HashSet<CommandBox<T>>,
        new_commands: &mut HashSet<CommandBox<T>>,
        _configmaton: &mut T,
    ) {
        new_commands.insert(CommandBox { inner: self });
    }
}

pub struct Set<T> {
    pub keyvals: Box<[(String, Value)]>,
    pub continuation: CommandBox<T>,
}

impl<T: CommandTarget> Command<T> for Set<T> {
    fn execute_post(
        &self,
        commands: &mut HashSet<CommandBox<T>>,
        _new_commands: &mut HashSet<CommandBox<T>>,
        configmaton: &mut T,
    ) {
        for (key, value) in self.keyvals.iter() {
            configmaton.set(key.as_str(), value.clone());
        }
        commands.insert(self.continuation);
    }
}

pub struct SetMax<T> {
    pub keyvals: Box<[(String, serde_json::Number)]>,
    pub continuation: CommandBox<T>,
}

impl<T: CommandTarget> Command<T> for SetMax<T> {
    fn execute_post(
        &self,
        commands: &mut HashSet<CommandBox<T>>,
        _new_commands: &mut HashSet<CommandBox<T>>,
        configmaton: &mut T,
    ) {
        for (key, value) in self.keyvals.iter() {
            if let Some(old_value) = configmaton.get(key.as_str()) {
                let new_value = match old_value {
                    Value::Number(n) => {
                        if let (Some(n), Some(value)) = (n.as_i64(), value.as_i64()) {
                            serde_json::Number::from(n.max(value))
                        } else {
                            let (n, value) = (n.as_f64().unwrap(), value.as_f64().unwrap());
                            serde_json::Number::from_f64(n.max(value)).unwrap()
                        }
                    }
                    _ => panic!("Invalid value type"),
                };
                configmaton.set(key.as_str(), Value::Number(new_value));
            } else {
                configmaton.set(key.as_str(), Value::Number(value.clone()));
            }
        }
        commands.insert(self.continuation);
    }
}

pub struct SetMin<T> {
    pub keyvals: Box<[(String, serde_json::Number)]>,
    pub continuation: CommandBox<T>,
}

impl<T: CommandTarget> Command<T> for SetMin<T> {
    fn execute_post(
        &self,
        commands: &mut HashSet<CommandBox<T>>,
        _new_commands: &mut HashSet<CommandBox<T>>,
        configmaton: &mut T,
    ) {
        for (key, value) in self.keyvals.iter() {
            if let Some(old_value) = configmaton.get(key.as_str()) {
                let new_value = match old_value {
                    Value::Number(n) => {
                        if let (Some(n), Some(value)) = (n.as_i64(), value.as_i64()) {
                            serde_json::Number::from(n.min(value))
                        } else {
                            let (n, value) = (n.as_f64().unwrap(), value.as_f64().unwrap());
                            serde_json::Number::from_f64(n.min(value)).unwrap()
                        }
                    }
                    _ => panic!("Invalid value type"),
                };
                configmaton.set(key.as_str(), Value::Number(new_value));
            } else {
                configmaton.set(key.as_str(), Value::Number(value.clone()));
            }
        }
        commands.insert(self.continuation);
    }
}
