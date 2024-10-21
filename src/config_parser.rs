use hashbrown::HashMap;
use std::io::Write;
use std::fmt;

use serde::de::{MapAccess, Visitor, Deserialize, Deserializer, Error, Unexpected};
use serde_json;
use serde_json::Value;

pub mod dfa;
pub mod guards;
pub mod ast;
pub mod nfa;

#[derive(Debug)]
pub enum Ext {
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
pub struct Target {
    exts: Vec<Ext>,
    states: Vec<StateIx>,
}

#[derive(Debug)]
pub struct Old {
    key: String,

    // The following fields are for determinisation purposes.
    dfa: DfaIx,
    then: TargetIx,
    waiter: StateIx,
}

#[derive(Debug)]
pub enum Guard {
    Old(Old),
    New(String),
    EndVar,
    Guard(guards::Guard),
}

#[derive(Debug)]
struct State {
    transitions: Vec<(Guard, TargetIx)>,
}

pub struct Parser {
    states: Vec<State>,
    targets: Vec<Target>,
    dfas: Vec<dfa::Dfa>,
    regexes: HashMap<String, DfaIx>,
}

impl Parser {
    pub fn parse(cmds: Vec<Cmd>) -> (Self, Target) {
        let mut parser = Parser {
            states: vec![],
            targets: vec![],
            dfas: vec![],
            regexes: HashMap::new(),
        };
        let init = parser.parse_parallel(cmds);

        (parser, init)
    }

    fn parse_parallel(&mut self, cmds: Vec<Cmd>) -> Target {
        let mut result = Target { exts: vec![], states: vec![] };
        for cmd in cmds {
            match cmd {
                Cmd::Match(match_) => {
                    let result2 = self.parse_match(match_);
                    result.exts.extend(result2.exts.into_iter());
                    result.states.extend(result2.states.into_iter());
                }
                _ => unimplemented!(),
            }
        }
        result
    }

    fn parse_match(
        &mut self,
        match_: Match,
    ) -> Target {
        if match_.when.is_empty() {
            panic!("expected at least one guard");
        }

        let mut then = self.parse_parallel(match_.then);
        then.exts.extend(match_.run.into_iter().map(|ext| Ext::Ext(ext)));
        let mut then_ix = TargetIx(self.targets.len());
        self.targets.push(then);

        let dfa_ixs = match_.when.iter().map(|(key, regex)| {
            *self.regexes.entry(key.clone()).or_insert_with(|| {
                let dfa_ix = self.dfas.len();
                self.dfas.push(
                    dfa::Dfa::from_nfa(nfa::Nfa::from_ast(ast::parse_regex(&regex)))
                );
                DfaIx(dfa_ix)
            })
        }).collect::<Vec<_>>();

        let state0_ix = self.states.len();
        let target0_ix = TargetIx(self.targets.len());

        let get_checker_state_ix = |i: usize| -> StateIx { StateIx(state0_ix + i * 3) };
        let get_waiter_state_ix = |i: usize| -> StateIx { StateIx(state0_ix + i * 3 + 1) };
        let get_closer_state_ix = |i: usize| -> StateIx {
            assert!(i < match_.when.len() - 1);
            StateIx(state0_ix + i * 3 + 2)
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
            assert!(i < match_.when.len() - 1);
            if i == 0 {
                return target0_ix.target_offset(1);
            }
            target0_ix.target_offset(2 + (i - 1) * 3 + 2)
        };

        if match_.when.len() == 1 {
            self.states.push(State { transitions: vec![] });  // checker
            self.states.push(State { transitions: vec![] });  // waiter
            self.targets.push(Target {  // waiter
                exts: vec![],
                states: vec![get_waiter_state_ix(0)],
            });
        } else {
            self.states.push(State { transitions: vec![] });  // checker
            self.states.push(State { transitions: vec![] });  // waiter
            self.targets.push(Target {  // waiter
                exts: vec![],
                states: vec![get_waiter_state_ix(0)],
            });
            // closer (nonexistent for the last guard)
            self.states.push(State { transitions: vec![] });
            self.targets.push(Target {
                exts: vec![Ext::GetOld(match_.when[0].0.clone())],
                states: vec![get_closer_state_ix(0)],
            });
            for (i, (key, _)) in match_.when[..match_.when.len() - 1].iter().enumerate().skip(1) {
                self.states.push(State { transitions: vec![] });  // checker
                self.targets.push(Target {
                    exts: vec![Ext::GetOld(key.clone())],
                    states: vec![get_checker_state_ix(i)],
                });
                self.states.push(State { transitions: vec![] });  // waiter
                self.targets.push(Target {  // waiter
                    exts: vec![],
                    states: vec![get_waiter_state_ix(i)],
                });
                // closer (nonexistent for the last guard)
                self.states.push(State { transitions: vec![] });
                self.targets.push(Target {
                    exts: vec![Ext::GetOld(key.clone())],
                    states: vec![get_closer_state_ix(i)],
                });
            }
            let i = match_.when.len() - 1;
            self.states.push(State { transitions: vec![] });  // checker
            self.targets.push(Target {  // checker
                exts: vec![Ext::GetOld(match_.when[i].0.clone())],
                states: vec![get_checker_state_ix(i)],
            });
            self.states.push(State { transitions: vec![] });  // waiter
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
                self_states.push(State { transitions: vec![] }); 
                self.targets.push(Target {
                    exts: vec![],
                    states: vec![new_state_ix],
                });

                let new_state = self_states.last_mut().unwrap();
                for (guard, target) in dfa_state.transitions.iter() {
                    if *target == dfa_state_ix {
                        continue;
                    }
                    new_state.transitions.push(
                        (Guard::Guard(guard.clone()), new_target0_ix.target_offset(*target)));
                }
                if dfa_state.is_final {
                    new_state.transitions.push((Guard::EndVar, then_ix));
                } else {
                    new_state.transitions.push((Guard::EndVar, waiter_target_ix));
                }
            }
            new_target0_ix
        };

        for (i, ((key, _), dfa_ix)) in
            match_.when[..match_.when.len() - 1].into_iter().zip(dfa_ixs.iter()).enumerate().rev()
        {
            let dfa_target0_ix = copy_dfa(
                &mut self.states, *dfa_ix, then_ix, get_waiter_target_ix(i));

            let guard = Guard::Old(Old {
                key: key.clone(),
                dfa: *dfa_ix,
                then: then_ix,
                waiter: get_waiter_state_ix(i),
            });
            self.states[get_closer_state_ix(i).0].transitions.push((guard, dfa_target0_ix));

            then_ix = get_closer_target_ix(i);
        }

        for (i, ((key, _), dfa_ix)) in
            match_.when[..match_.when.len()].into_iter().zip(dfa_ixs.iter()).enumerate().rev()
        {
            let dfa_target0_ix = copy_dfa(
                &mut self.states, *dfa_ix, then_ix, get_waiter_target_ix(i));

            let guard = Guard::Old(Old {
                key: key.clone(),
                dfa: *dfa_ix,
                then: then_ix,
                waiter: get_waiter_state_ix(i),
            });
            self.states[get_checker_state_ix(i).0].transitions.push(
                (guard, dfa_target0_ix));

            self.states[get_waiter_state_ix(i).0].transitions.push(
                (Guard::New(key.clone()), dfa_target0_ix));

            if i != 0 {
                then_ix = get_checker_target_ix(i);
            }
        }

        Target{
            exts: vec![Ext::GetOld(match_.when[0].0.clone())],
            states: vec![StateIx(state0_ix)]
        }
    }

    pub fn to_dot<W: Write>(&self, init: Target, mut writer: W) {
        writer.write_all(b"digraph G {\n").unwrap();

        for i in 0..self.states.len() {
            writer.write_all(format!("  q{}\n", i).as_bytes()).unwrap();
        }

        for i in 0..self.targets.len() {
            writer.write_all(
                format!("  t{} [ shape=\"rect\" ]\n", i).as_bytes()).unwrap();
            writer.write_all(
                format!("  e{} [ shape=\"diamond\" ]\n", i).as_bytes()).unwrap();
        }

        // println!("~~~ {:?} ~~~> {:?}", init.exts, init.states);
        writer.write_all(b"  ti [ shape=\"rect\" ]\n").unwrap();
        writer.write_all(b"  ei [ shape=\"diamond\" ]\n").unwrap();

        let fmte = |exts: &Vec<Ext>| -> String {
            exts.iter().map(|ext| match ext {
                Ext::GetOld(s) => format!("GetOld({})", s),
                Ext::Ext(v) => format!("{:?}", v),
            }).collect::<Vec<_>>().join(", ").replace("\\", "\\\\").replace("\"", "\\\"")
        };

        let fmtg = |guard: &Guard| -> String {
            match guard {
                Guard::Old(s) => format!("Old({})", s.key),
                Guard::New(s) => format!("New({})", s),
                Guard::EndVar => "EndVar".to_string(),
                Guard::Guard(g) => format!("{:?}", g),
            }.replace("\\", "\\\\").replace("\"", "\\\"")
        };

        writer.write_all(
            format!("  ti -> ei [label=\"{}\"]\n", fmte(&init.exts)).as_bytes()).unwrap();
        for state in init.states {
            writer.write_all(format!("  ei -> q{}\n", state.0).as_bytes()).unwrap();
        }

        for (i, state) in self.states.iter().enumerate() {
            for (guard, target) in state.transitions.iter() {
                writer.write_all(
                    format!("  q{} -> t{} [label=\"{}\"]\n", i, target.0, fmtg(&guard)).as_bytes()
                ).unwrap();
            }
        }

        for (i, target) in self.targets.iter().enumerate() {
            writer.write_all(
                format!("  t{} -> e{} [label=\"{}\"]\n", i, i, fmte(&target.exts)).as_bytes()
            ).unwrap();
            for state in target.states.iter() {
                writer.write_all(format!("  e{} -> q{}\n", i, state.0).as_bytes()).unwrap();
            }
        }

        writer.write_all(b"}\n").unwrap();
    }
}

#[derive(Debug)]
pub enum Cmd {
    Match(Match),
    Label(String, Vec<Cmd>),  // No support yet.
    Goto(String),  // No support yet.
}

#[derive(Debug, serde::Deserialize)]
pub struct Match {
    when: Vec<(String, String)>,
    run: Vec<Value>,
    then: Vec<Cmd>,
}

struct CmdVisitor;

impl<'de> Visitor<'de> for CmdVisitor {
    type Value = Cmd;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a match")
    }

    fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
    where
        V: MapAccess<'de>,
    {
        let mut when = None;
        let mut run = None;
        let mut then = None;
        while let Some(key) = map.next_key()? {
            match key {
                "when" => {
                    if when.is_some() {
                        return Err(Error::duplicate_field("when"));
                    }
                    let when_map: Value = map.next_value()?;
                    match when_map {
                        Value::Object(obj) => {
                            let mut when_map = vec![];
                            for (key, value) in obj {
                                match value {
                                    Value::String(value) => when_map.push((key, value)),
                                    _ => return Err(
                                        Error::invalid_type(
                                            Unexpected::Other("match value is not a string"),
                                            &"a string (regex)"
                                        )
                                    ),
                                }
                            }
                            when = Some(when_map);
                        },
                        _ => return Err(
                            Error::invalid_type(
                                Unexpected::Other("match is not an object"),
                                &"an object of key-regex pairs"
                            )
                        ),
                    }
                }
                "run" => {
                    if run.is_some() {
                        return Err(Error::duplicate_field("run"));
                    }
                    run = Some(map.next_value()?);
                }
                "then" => {
                    if then.is_some() {
                        return Err(Error::duplicate_field("then"));
                    }
                    then = Some(map.next_value()?);
                }
                _ => {
                    return Err(Error::unknown_field(key, &["when", "run", "then"]));
                }
            }
        }
        let when = when.ok_or_else(|| Error::missing_field("when"))?;
        let run = run.unwrap_or_else(|| vec![]);
        let then = then.unwrap_or_else(|| vec![]);
        Ok(Cmd::Match(Match { when, run, then }))
    }
}

impl<'de> Deserialize<'de> for Cmd {
    fn deserialize<D>(deserializer: D) -> Result<Cmd, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(CmdVisitor)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_to_automaton_complex() {
        // read and parse file tests/config.json
        let config: Vec<Cmd> = serde_json::from_str(r#"[
            { 
                "when": {
                    "foo": "bar",
                    "qux": "a.*"
                },
                "run": [ { "set": { "match1": "passed" } } ]
            },
            {
                "when": { "foo": "baz" },
                "run": [ { "set": { "match2": "passed" } } ],
                "then": [
                    {
                        "when": { "qux": "a.*" },
                        "run": [ { "set": { "match3": "passed" } } ]
                    },
                    {
                        "when": { "qux": "ahoy" },
                        "run": [ { "set": { "match4": "passed" } } ]
                    }
                ]
            }
        ]"#).unwrap();

        let (parser, init) = Parser::parse(config);

        // The output automaton is for now only for visual checking.
        let file = std::fs::File::create("/tmp/test_complex.dot").unwrap();
        parser.to_dot(init, std::io::BufWriter::new(file));
    }

    #[test]
    fn config_to_automaton_simple() {
        // read and parse file tests/config.json
        let config: Vec<Cmd> = serde_json::from_str(r#"[
            { 
                "when": {
                    "foo": "a",
                    "bar": "b"
                },
                "run": [ "you win" ]
            }
        ]"#).unwrap();

        let (parser, init) = Parser::parse(config);

        // The output automaton is for now only for visual checking.
        let file = std::fs::File::create("/tmp/test_simple.dot").unwrap();
        parser.to_dot(init, std::io::BufWriter::new(file));
    }
}
