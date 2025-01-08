use std::mem::MaybeUninit;
use std::ptr::addr_of_mut;
use hashbrown::{HashMap, HashSet};

pub mod lock;
pub mod command;
pub mod commands;
pub mod keyval_runner;
pub mod keyval_simulator;
pub mod guards;
pub mod char_nfa;
pub mod char_enfa;
pub mod ast;
pub mod keyval_nfa;
pub mod char_runner;
pub mod borrow_lock;
pub mod blob;
pub mod holder;

use crate::lock::LockSelector;

pub struct Onion<'a, S: LockSelector + 'a> {
    parent: S::MaybeLock<'a, Self>,
    children: S::Holder<'a, Self>,
    data: HashMap<&'a [u8], &'a [u8]>,
}

impl<'a, S: LockSelector + 'a> Onion<'a, S>
{
    pub fn new() -> Self {
        Onion {
            parent: S::empty(),
            children: S::new_holder(),
            data: HashMap::new(),
        }
    }

    pub fn make_child(slf: S::Lock<'a, Self>) -> S::Lock<'a, Self> {
        let child = Onion {
            parent: S::to_maybe_lock(&slf),
            children: S::new_holder(),
            data: HashMap::new(),
        };
        let children = &mut S::borrow_mut(&slf).children;
        S::add_holder(children, child)
    }

    // pub fn get_rec(&self, key: &[u8]) -> Option<&'a [u8]> {
    //     if let Some(value) = self.data.get(key) {
    //         return Some(value);
    //     }
    //     let parent_onion = S::borrow(&self.parent)?;
    //     parent_onion.get_rec(key)
    // }

    // pub fn get(&self, key: &[u8]) -> Option<&'a [u8]> {
    //     if let Some(value) = self.data.get(key) {
    //         return Some(value);
    //     }

    //     let mut parent = &self.parent;
    //     let mut parent_onion = S::borrow(parent)?;
    //     loop {
    //         if let Some(value) = parent_onion.data.get(key) {
    //             return Some(value);
    //         }
    //         parent = &parent_onion.parent;
    //         parent_onion = S::borrow(parent)?;
    //     }
    // }

    pub fn set(&mut self, key: &'a [u8], value: &'a [u8]) {
        self.data.insert(key, value);
    }

    // pub fn iter(&self) -> OnionIter<S> {
    //     OnionIter::new(self)
    // }
}

// pub struct OnionIter<S: LockSelector> {
//     parent: S::Lock<Onion<S>>,
//     children: Vec<S::Holder<Onion<S>>>,
//     visited: HashSet<String>,
//     items: Vec<(String, Value)>,
// }
// 
// impl <S: LockSelector> OnionIter<S> {
//     pub fn new(onion: &Onion<S>) -> Self {
//         OnionIter {
//             parent: onion.parent.clone(),
//             visited: HashSet::new(),
//             items: onion.data.iter().map(|(key, val)| (key.clone(), val.clone())).collect(),
//         }
//     }
// }

// impl <S: LockSelector> Iterator for OnionIter<S>
// {
//     type Item = (String, Value);
// 
//     fn next(&mut self) -> Option<Self::Item> {
//         loop {
//             while let Some((key, value)) = self.items.pop() {
//                 if self.visited.insert(key.clone()) {
//                     return Some((key, value));
//                 }
//             }
// 
//             let parent_ref = self.parent.take()?;
//             let parent_onion = parent_ref.borrow();
// 
//             self.items.extend(parent_onion.data.iter().map(|(key, val)| (key.clone(), val.clone())));
//             self.parent = parent_onion.parent.clone();
//         }
//     }
// }

// pub struct Configmaton<'a, S: LockSelector> {
//     onion: S::Lock<'a, Onion<'a, S>>,
// }


// #[cfg(test)]
// mod tests {
//     use super::*;
//     use serde_json::Number;
//     use crate::lock::RcRefCellSelector;
// 
//     #[test]
//     fn onion_works() {
//         let mut onion1 = Onion::<RcRefCellSelector>::new();
//         onion1.set("a".to_string(), Value::Number(Number::from(1)));
//         onion1.set("b".to_string(), Value::Number(Number::from(2)));
//         onion1.set("a".to_string(), Value::Number(Number::from(3)));
//         assert_eq!(onion1.get("a"), Some(Value::Number(Number::from(3))));
//         assert_eq!(onion1.get("b"), Some(Value::Number(Number::from(2))));
//         assert_eq!(onion1.get("c"), None);
// 
//         let onion1 = onion1.share();
//         let mut onion2 = Onion::<RcRefCellSelector>::new_level(onion1.clone());
//         onion2.set("b".to_string(), Value::Number(Number::from(4)));
//         onion2.set("c".to_string(), Value::Number(Number::from(5)));
//         assert_eq!(onion2.get("a"), Some(Value::Number(Number::from(3))));
//         assert_eq!(onion2.get("b"), Some(Value::Number(Number::from(4))));
//         assert_eq!(onion2.get("c"), Some(Value::Number(Number::from(5))));
//         assert_eq!(onion2.get("d"), None);
// 
//         assert_eq!(onion1.borrow().get("a"), Some(Value::Number(Number::from(3))));
//         assert_eq!(onion1.borrow().get("b"), Some(Value::Number(Number::from(2))));
//         assert_eq!(onion1.borrow().get("c"), None);
// 
//         onion1.borrow_mut().set("a".to_string(), Value::Number(Number::from(6)));
//         assert_eq!(onion1.borrow().get("a"), Some(Value::Number(Number::from(6))));
//         assert_eq!(onion2.get("a"), Some(Value::Number(Number::from(6))));
//     }
// }
