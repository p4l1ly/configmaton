#![feature(non_lifetime_binders)]

use std::collections::{HashMap, HashSet};
use serde_json::Value;

mod reflike;
use crate::reflike::{RcRefCell, Reflike};

// pub struct OnionIter<'a, L> {
//     parent: Option<L>,
//     visited: HashSet<&'a str>,
//     items: Vec<(&'a str, Value)>,
// }
// 
// impl <'a, L: Clone> OnionIter<'a, L> {
//     pub fn new(onion: &Onion<'a, L>) -> Self {
//         OnionIter {
//             parent: onion.parent.clone(),
//             visited: HashSet::new(),
//             items: onion.data.iter().map(|(key, val)| (*key, val.clone())).collect(),
//         }
//     }
// }
// 
// impl <'a, L>
// Iterator for OnionIter<'a, L>
// where
//     for<X> L: Reflike<Target<X> = Onion<'a, X>>,
// {
//     type Item = (&'a str, Value);
// 
//     fn next(&mut self) -> Option<Self::Item> {
//         loop {
//             while let Some((key, value)) = self.items.pop() {
//                 if self.visited.insert(key) {
//                     return Some((key, value));
//                 }
//             }
// 
//             let parent_ref = self.parent.take()?;
//             let parent_onion = parent_ref.borrow();
// 
//             self.items.extend(parent_onion.data.iter().map(|(key, val)| (*key, val.clone())));
//             self.parent = parent_onion.parent.clone();
//         }
//     }
// }

pub struct Onion<'a> {
    parent: Option<RcRefCell<Onion<'a>>>,
    data: HashMap<&'a str, Value>,
}

impl<'a> Onion<'a>
{
    pub fn new() -> Self {
        Onion {
            parent: None,
            data: HashMap::new(),
        }
    }

    pub fn share(self) -> RcRefCell<Self> {
        <RcRefCell<Self>>::from(self)
    }

    pub fn new_level(parent: RcRefCell<Self>) -> Self {
        Onion {
            parent: Some(parent.clone()),
            data: HashMap::new(),
        }
    }

    // pub fn get_rec(&self, key: &str) -> Option<Value> {
    //     if let Some(value) = self.data.get(key) {
    //         return Some(value.clone());
    //     }
    //     let parent_ref = self.parent.as_ref()?;
    //     let parent_onion = parent_ref.borrow();
    //     parent_onion.get_rec(key)
    // }

    // pub fn get(&self, key: &str) -> Option<Value> {
    //     if let Some(value) = self.data.get(key) {
    //         return Some(value.clone());
    //     }

    //     let mut parent = self.parent.clone();
    //     loop {
    //         let parent_ref = parent.take()?;
    //         let parent_onion = parent_ref.borrow();
    //         if let Some(value) = parent_onion.data.get(key) {
    //             return Some(value.clone());
    //         }
    //         parent = parent_onion.parent.clone();
    //     }
    // }

    // pub fn set(&mut self, key: &'a str, value: Value) {
    //     self.data.insert(key, value);
    // }
}

// pub struct Config {}
// 
// pub struct Configmaton<'a, L: 'a + Reflike<Onion<'a, L>>> {
//     onion: L,
//     configs: HashSet<*const Config>,
//     stepping: bool,
//     _phantom: std::marker::PhantomData<&'a ()>,
// }
// 
// impl <'a, L: 'a + Reflike<Onion<'a, L>>> Configmaton<'a, L> {
//     pub fn new() -> Self {
//         Configmaton {
//             onion: Onion::new().share(),
//             configs: HashSet::new(),
//             stepping: false,
//             _phantom: std::marker::PhantomData,
//         }
//     }
// 
//     pub fn new_level(self) -> Self {
//         Configmaton {
//             onion: <Onion<'a, L>>::new_level(self.onion).share(),
//             configs: self.configs.clone(),
//             stepping: false,
//             _phantom: std::marker::PhantomData,
//         }
//     }
// 
//     pub fn get(&self, key: &str) -> Option<Value> {
//         self.onion.borrow().get(key)
//     }
// 
//     pub fn set(&mut self, key: &'a str, value: Value) {
//         self.onion.borrow_mut().set(key, value);
//     }
// 
//     pub fn do_config<F>(&mut self, config: &Config, f: F)
//     where
//         F: FnOnce(&mut Self),
//     {
//         if self.configs.contains(&(config as *const Config)) {
//             return;
//         }
// 
//         self.configs.insert(config as *const Config);
//         self.stepping = true;
//         f(self);
//         self.stepping = false;
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Number;
    use crate::reflike::RcRefCell;

    #[test]
    fn recursive_onion_is_buildable() {
        let grand_parent = RcRefCell::from(Onion::new());
        let parent = RcRefCell::from(Onion::new_level(grand_parent));
    }

    // #[test]
    // fn it_works() {
    //     let mut onion1: Onion<'_, RcRefCell<_>> = Onion::new();
    //     onion1.set("a", Value::Number(Number::from(1)));
    //     onion1.set("b", Value::Number(Number::from(2)));
    //     onion1.set("a", Value::Number(Number::from(3)));
    //     assert_eq!(onion1.get("a"), Some(Value::Number(Number::from(3))));
    //     assert_eq!(onion1.get("b"), Some(Value::Number(Number::from(2))));
    //     assert_eq!(onion1.get("c"), None);

    //     // let onion1: RcRefCell<_> = onion1.share();
    //     // let mut onion2 = onion1.fork();
    //     // onion2.set("b", Value::Number(Number::from(4)));
    //     // onion2.set("c", Value::Number(Number::from(5)));
    //     // assert_eq!(onion2.get("a"), Some(&Value::Number(Number::from(3))));
    //     // assert_eq!(onion2.get("b"), Some(&Value::Number(Number::from(4))));
    //     // assert_eq!(onion2.get("c"), Some(&Value::Number(Number::from(5))));
    //     // assert_eq!(onion2.get("d"), None);

    //     // assert_eq!(onion1.get("a"), Some(&Value::Number(Number::from(3))));
    //     // assert_eq!(onion1.get("b"), Some(&Value::Number(Number::from(2))));
    //     // assert_eq!(onion1.get("c"), None);

    //     // onion1.set("a", Value::Number(Number::from(6)));
    //     // assert_eq!(onion1.get("a"), Some(&Value::Number(Number::from(6))));
    //     // assert_eq!(onion2.get("a"), Some(&Value::Number(Number::from(6))));
    // }
}
