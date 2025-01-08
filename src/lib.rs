use onion::Onion;

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
pub mod onion;

pub struct Configmaton<'a> {
    onion: Onion<'a>,

}


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
