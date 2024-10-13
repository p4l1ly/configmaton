use serde_json;
use serde_json::Value;
use crate::automaton::{Explicit, Pattern};

mod dfa;
mod guards;
mod ast;
mod nfa;

use ast::Ast;

#[derive(Debug)]
enum Ext {
    Query(String),
    Ext(Value),
}

#[derive(Debug)]
struct Target {
    exts: Vec<Ext>,
    states: Vec<usize>,
}

struct State {
    explicit_transitions: Vec<(Explicit, usize)>,
    pattern_transitions: Vec<(Pattern, usize)>,
}

struct Parser {
    states: Vec<State>,
    targets: Vec<Target>,
}

impl Parser {
    fn parse(cmd: Value) -> (Self, Target) {
        let mut parser = Parser {
            states: vec![],
            targets: vec![],
        };
        let init = parser.parse_cmd(cmd);
        (parser, init)
    }

    fn parse_match(
        &mut self,
        guards: Vec<Value>,
        exts: Vec<Value>,
        mut then: Target
    ) -> (usize, String) {
        then.exts.extend(exts.into_iter().map(|ext| Ext::Ext(ext)));

        let keyvals: Vec<_> = guards.into_iter().map(parse_guard).collect();

        unimplemented!()
    }

    fn parse_cmd(&mut self, cmd: Value) -> Target {
        match cmd {
            Value::Array(mut cmd) => {
                match cmd[0].as_str() {
                    Some("fork") => {
                        let mut result = self.parse_cmd(cmd.pop().unwrap());
                        for child in cmd.drain(1..) {
                            let child_target = self.parse_cmd(child);
                            result.states.extend(child_target.states);
                            result.exts.extend(child_target.exts);
                        }
                        result
                    },
                    Some("match") => {
                        let nexts = if cmd.len() == 4 {
                            self.parse_cmd(cmd.pop().unwrap())
                        } else { Target{ exts: vec![], states: vec![]} };
                        let exts = cmd.pop().unwrap();
                        let guards = cmd.pop().unwrap();
                        match (exts, guards) {
                            (Value::Array(exts), Value::Array(guards)) => {
                                let (state, query) = self.parse_match(guards, exts, nexts);
                                Target{ exts: vec![Ext::Query(query)], states: vec![state] }
                            },
                            _ => {
                                panic!("expected array");
                            }
                        }
                    },
                    _ => {
                        panic!("unknown command");
                    }
                }
            }
            _ => {
                panic!("expected array");
            }
        }
    }
}


fn parse_guard(guard: Value) -> (String, dfa::Dfa) {
    match guard {
        Value::Array(mut guards) => {
            if guards.len() != 2 {
                panic!("Invalid guard, expected array of two strings (key, regex)");
            }
            let value = guards.pop().unwrap();
            let key = guards.pop().unwrap();
            match (key, value) {
                (Value::String(key), Value::String(regex)) => {
                    (key, dfa::Dfa::from_nfa(nfa::Nfa::from_ast(ast::parse_regex(&regex))))
                },
                _ => {
                    panic!("expected string");
                }
            }
        },
        _ => {
            panic!("Invalid guard, expected array");
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::BufReader;

    #[test]
    fn config_to_nfa() {
        // read and parse file tests/config.json
        let file = File::open("tests/config.json").expect("file not found");
        let reader = BufReader::new(file);
        let config: Value = serde_json::from_reader(reader).unwrap();

        let (parser, init) = Parser::parse(config);
        println!("~~~ {:?} ~~~> {:?}", init.exts, init.states);

        for (i, state) in parser.states.into_iter().enumerate() {
            for (guard, target) in state.explicit_transitions {
                println!("{} --- {:?} ---> {:?};", i, guard, target);
            }
            for (guard, target) in state.pattern_transitions {
                println!("{} --- {:?} ---> {:?};", i, guard, target);
            }
        }

        for (i, target) in parser.targets.into_iter().enumerate() {
            println!("{} ~~~ {:?} ~~~> {:?};", i, target.exts, target.states);
        }

        // assert!(false);
    }
}
