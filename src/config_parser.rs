use std::collections::HashMap;

use serde_json;
use serde_json::Value;
use crate::automaton::{Explicit, Pattern};

pub mod dfa;
pub mod guards;
pub mod ast;
pub mod nfa;

use ast::Ast;

#[derive(Debug)]
enum Ext {
    GetOld(String),
    Ext(Value),
}

#[derive(Debug, Clone, Copy)]
struct TargetIx (usize);
impl TargetIx {
    fn target_offset(&self, offset: usize) -> TargetIx {
        TargetIx(self.0 + offset)
    }
}

#[derive(Debug, Clone, Copy)]
struct StateIx (usize);
#[derive(Debug, Clone, Copy)]
struct DfaIx (usize);

#[derive(Debug)]
struct Target {
    exts: Vec<Ext>,
    states: Vec<StateIx>,
}

#[derive(Debug)]
struct State {
    explicit_transitions: Vec<(Explicit, TargetIx)>,
    pattern_transitions: Vec<(Pattern, TargetIx)>,
}

struct Parser {
    states: Vec<State>,
    targets: Vec<Target>,
    dfas: Vec<dfa::Dfa>,
    regexes: HashMap<String, DfaIx>,
    min_guard_cardinality: u32,
}

impl Parser {
    fn parse(cmd: Value, min_guard_cardinality: u32) -> (Self, Target) {
        let mut parser = Parser {
            states: vec![],
            targets: vec![],
            dfas: vec![],
            regexes: HashMap::new(),
            min_guard_cardinality,
        };
        let init = parser.parse_cmd(cmd);

        (parser, init)
    }

    // return state_ix and key of the first checker (for construction of a new target).
    fn parse_match(
        &mut self,
        guards: Vec<Value>,
        exts: Vec<Value>,
        mut then: Target
    ) -> Target {
        if guards.len() == 0 {
            panic!("expected at least one guard");
        }

        then.exts.extend(exts.into_iter().map(|ext| Ext::Ext(ext)));
        let mut then_ix = TargetIx(self.targets.len());
        self.targets.push(then);

        let keyvals: Vec<_> = guards.into_iter().map(parse_guard).collect();

        let dfa_ixs = keyvals.iter().map(|(key, regex)| {
            *self.regexes.entry(key.clone()).or_insert_with(|| {
                let dfa_ix = self.dfas.len();
                self.dfas.push(
                    dfa::Dfa::from_nfa(nfa::Nfa::from_ast(ast::parse_regex(&regex)))
                );
                DfaIx(dfa_ix)
            })
        }).collect::<Vec<_>>();

        let state0_ix = self.states.len();
        let target0_ix = TargetIx(self.states.len());

        let get_checker_state_ix = |i: usize| -> StateIx { StateIx(state0_ix + i * 3) };
        let get_waiter_state_ix = |i: usize| -> StateIx { StateIx(state0_ix + i * 3 + 1) };
        let get_closer_state_ix = |i: usize| -> StateIx {
            assert!(i < keyvals.len() - 1);
            StateIx(state0_ix + i * 3 + 1)
        };
        let get_checker_target_ix = |i: usize| -> TargetIx {
            assert!(i != 0);
            target0_ix.target_offset(2 + (i - 1) * 3)
        };
        let get_waiter_target_ix = |i: usize| -> TargetIx {
            if i == 0 {
                return target0_ix;
            }
            target0_ix.target_offset(2 + (i - 1) * 3 + 1)
        };
        let get_closer_target_ix = |i: usize| -> TargetIx {
            assert!(i < keyvals.len() - 1);
            if i == 0 {
                return target0_ix.target_offset(1);
            }
            target0_ix.target_offset(2 + (i - 1) * 3 + 2)
        };

        if keyvals.len() == 1 {
            self.states.push(State {  // checker
                explicit_transitions: vec![],
                pattern_transitions: vec![],
            });
            self.states.push(State {  // waiter
                explicit_transitions: vec![],
                pattern_transitions: vec![],
            });
            self.targets.push(Target {  // waiter
                exts: vec![],
                states: vec![get_waiter_state_ix(0)],
            });
        } else {
            self.states.push(State {  // checker
                explicit_transitions: vec![],
                pattern_transitions: vec![],
            });
            self.states.push(State {  // waiter
                explicit_transitions: vec![],
                pattern_transitions: vec![],
            });
            self.targets.push(Target {  // waiter
                exts: vec![],
                states: vec![get_waiter_state_ix(0)],
            });
            self.states.push(State {  // closer (nonexistent for the last guard)
                explicit_transitions: vec![],
                pattern_transitions: vec![],
            });
            self.targets.push(Target {  // closer (nonexistent for the last guard)
                exts: vec![Ext::GetOld(keyvals[0].0.clone())],
                states: vec![get_closer_state_ix(0)],
            });
            for (i, (key, _)) in keyvals[..keyvals.len() - 1].iter().enumerate().skip(1) {
                self.states.push(State {  // checker
                    explicit_transitions: vec![],
                    pattern_transitions: vec![],
                });
                self.targets.push(Target {
                    exts: vec![Ext::GetOld(key.clone())],
                    states: vec![get_checker_state_ix(i)],
                });
                self.states.push(State {  // waiter
                    explicit_transitions: vec![],
                    pattern_transitions: vec![],
                });
                self.targets.push(Target {  // waiter
                    exts: vec![],
                    states: vec![get_waiter_state_ix(i)],
                });
                self.states.push(State {  // closer (nonexistent for the last guard)
                    explicit_transitions: vec![],
                    pattern_transitions: vec![],
                });
                self.targets.push(Target {  // closer (nonexistent for the last guard)
                    exts: vec![Ext::GetOld(key.clone())],
                    states: vec![get_closer_state_ix(i)],
                });
            }
            let i = keyvals.len() - 1;
            self.states.push(State {  // checker
                explicit_transitions: vec![],
                pattern_transitions: vec![],
            });
            self.targets.push(Target {  // checker
                exts: vec![],
                states: vec![get_checker_state_ix(i)],
            });
            self.states.push(State {  // waiter
                explicit_transitions: vec![],
                pattern_transitions: vec![],
            });
            self.targets.push(Target {  // waiter
                exts: vec![],
                states: vec![get_waiter_state_ix(i)],
            });
        }

        let mut copy_dfa = |
            self_states: &mut Vec<State>,
            dfa_ix: DfaIx,
            then_ix: TargetIx,
            waiter_target_ix: TargetIx
        | -> TargetIx {
            let new_target0_ix = TargetIx(self_states.len());
            for (dfa_state_ix, dfa_state) in self.dfas[dfa_ix.0].states.iter().enumerate() {
                let new_state_ix = StateIx(self_states.len());
                self_states.push(State {
                    explicit_transitions: vec![],
                    pattern_transitions: vec![],
                });
                self.targets.push(Target {
                    exts: vec![],
                    states: vec![new_state_ix],
                });

                let new_state = self_states.last_mut().unwrap();
                for (guard, target) in dfa_state.transitions.iter() {
                    if *target == dfa_state_ix {
                        continue;
                    }
                    if guard.size() < self.min_guard_cardinality {
                        panic!("not implemented");
                    } else {
                        new_state.pattern_transitions.push(
                            (guard.clone(), new_target0_ix.target_offset(*target)));
                    }
                }
                if dfa_state.is_final {
                    new_state.explicit_transitions.push((Explicit::EndVar, then_ix));
                } else {
                    new_state.explicit_transitions.push((Explicit::EndVar, waiter_target_ix));
                }
            }
            new_target0_ix
        };

        for (i, ((key, _), dfa_ix)) in
            keyvals[..keyvals.len() - 1].into_iter().zip(dfa_ixs.iter()).enumerate().rev()
        {
            let dfa_target0_ix = copy_dfa(
                &mut self.states, *dfa_ix, then_ix, get_waiter_target_ix(i));

            self.states[get_closer_state_ix(i).0].explicit_transitions.push(
                (Explicit::OldVar(key.clone()), dfa_target0_ix));

            if i != 0 {
                then_ix = get_closer_target_ix(i);
            }
        }

        for (i, ((key, _), dfa_ix)) in
            keyvals[..keyvals.len()].into_iter().zip(dfa_ixs.iter()).enumerate().rev()
        {
            let dfa_target0_ix = copy_dfa(
                &mut self.states, *dfa_ix, then_ix, get_waiter_target_ix(i));

            self.states[get_checker_state_ix(i).0].explicit_transitions.push(
                (Explicit::OldVar(key.clone()), dfa_target0_ix));

            self.states[get_waiter_state_ix(i).0].explicit_transitions.push(
                (Explicit::NewVar(key.clone()), dfa_target0_ix));

            if i != 0 {
                then_ix = get_checker_target_ix(i);
            }
        }

        Target{ exts: vec![Ext::GetOld(keyvals[0].0.clone())], states: vec![StateIx(state0_ix)] }
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
                                self.parse_match(guards, exts, nexts)
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


fn parse_guard(guard: Value) -> (String, String) {
    match guard {
        Value::Array(mut guards) => {
            if guards.len() != 2 {
                panic!("Invalid guard, expected array of two strings (key, regex)");
            }
            let value = guards.pop().unwrap();
            let key = guards.pop().unwrap();
            match (key, value) {
                (Value::String(key), Value::String(regex)) => {
                    (key, regex)
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
