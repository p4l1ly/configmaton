use hashbrown::HashMap;
use hashbrown::HashSet;
use std::io::Write;
use std::fmt;

use serde::de::{MapAccess, Visitor, Deserialize, Deserializer, Error, Unexpected};
use serde_json;
use serde_json::Value;

use crate::ast;
use crate::nfa;
use crate::dfa;

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Ext {
    GetOld(String),
    Ext(Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StateIx (pub usize);
#[derive(Debug, Clone, Copy)]
pub struct DfaIx (pub usize);
#[derive(Debug, Clone, Copy)]
pub struct DfaStateIx (pub usize);

#[derive(Debug)]
pub struct Target {
    pub exts: Vec<Ext>,
    pub states: Vec<StateIx>,
}

impl Target {
    pub fn join<'a, I: Iterator<Item=Self>>(targets: I) -> Self {
        let mut exts = HashSet::new();
        let mut states = HashSet::new();
        for target in targets {
            exts.extend(target.exts.into_iter());
            states.extend(target.states.into_iter());
        }
        Target {
            exts: exts.into_iter().collect(),
            states: states.into_iter().collect(),
        }
    }
}

#[derive(Debug)]
pub struct Bdd {
    pub leaves: Vec<Target>,
    // (dfa_ix, pos, neg) where pos and neg are primarily indices to leaves and if they are bigger
    // than the number of leaves, they are indices to nodes.
    pub nodes: Vec<(DfaIx, usize, usize)>,
}

#[derive(Debug)]
pub struct Tran {
    key: String,
    dfa_inits: Vec<DfaStateIx>,
    bdd: Bdd,
}

#[derive(Debug)]
pub struct State {
    pub transitions: Vec<Tran>,
}

pub struct Parser {
    pub states: Vec<State>,
    pub dfa: dfa::Dfa,
    pub regexes: HashMap<String, (DfaStateIx, DfaIx)>,
}

impl Parser {
    pub fn parse(cmds: Vec<Cmd>) -> (Self, Target) {
        let mut parser = Parser {
            states: vec![],
            dfa: dfa::Dfa::new(),
            regexes: HashMap::new(),
        };
        let init = parser.parse_parallel(cmds);

        (parser, init)
    }

    fn parse_parallel(&mut self, cmds: Vec<Cmd>) -> Target {
        let targets = cmds.into_iter().map(|cmd| match cmd {
            Cmd::Match(match_) => self.parse_match(match_),
            _ => unimplemented!(),
        });
        Target::join(targets)
    }

    fn parse_match(
        &mut self,
        match_: Match,
    ) -> Target {
        let mut then = self.parse_parallel(match_.then);
        then.exts.extend(match_.run.into_iter().map(|ext| Ext::Ext(ext)));

        if match_.when.is_empty() { return then; }

        let dfa_ixs = match_.when.iter().map(|(_, regex)| {
            let dfa_ix = self.regexes.len();
            *self.regexes.entry(regex.clone()).or_insert_with(|| {
                let dfa_state_ix = self.dfa.states.len();
                self.dfa.add_nfa(nfa::Nfa::from_ast(ast::parse_regex(&regex)), dfa_ix);
                (DfaStateIx(dfa_state_ix), DfaIx(dfa_ix))
            })
        }).collect::<Vec<_>>();

        let guard_count = match_.when.len();
        for ((key, _), (dfa_state_ix, dfa_ix)) in
            match_.when[..guard_count - 1].into_iter().zip(dfa_ixs.iter()).rev()
        {
            let state_ix = self.states.len();
            let else_ = Target { exts: vec![], states: vec![StateIx(state_ix + guard_count)] };
            self.states.push(State { transitions: vec![Tran {
                key: key.clone(),
                dfa_inits: vec![*dfa_state_ix],
                bdd: Bdd {
                    leaves: vec![then, else_],
                    nodes: vec![(*dfa_ix, 0, 1)],
                },
            }]});
            then = Target {
                exts: vec![Ext::GetOld(key.clone())],
                states: vec![StateIx(state_ix)],
            };
        }

        for ((key, _), (dfa_state_ix, dfa_ix)) in
            match_.when[..guard_count].into_iter().zip(dfa_ixs.iter()).rev()
        {
            let state_ix = self.states.len();
            let else_ = Target { exts: vec![], states: vec![StateIx(state_ix)] };
            self.states.push(State { transitions: vec![Tran {
                key: key.clone(),
                dfa_inits: vec![*dfa_state_ix],
                bdd: Bdd {
                    leaves: vec![then, else_],
                    nodes: vec![(*dfa_ix, 0, 1)],
                },
            }]});

            then = Target {
                exts: vec![Ext::GetOld(key.clone())],
                states: vec![StateIx(state_ix)],
            };
        }

        then
    }

    pub fn to_dot<W: Write>(&self, init: &Target, mut writer: W) {
        writer.write_all(b"digraph G {\n").unwrap();

        let mut write = |x: String| writer.write_all(x.as_bytes()).unwrap();

        for i in 0..self.states.len() {
            write(format!("  q{}\n", i));
        }

        let fmte = |exts: &Vec<Ext>| -> String {
            exts.iter().map(|ext| match ext {
                Ext::GetOld(s) => format!("GetOld({})", s),
                Ext::Ext(v) => format!("{:?}", v),
            }).collect::<Vec<_>>().join(", ").replace("\\", "\\\\").replace("\"", "\\\"")
        };

        // println!("~~~ {:?} ~~~> {:?}", init.exts, init.states);
        write("  ti [ shape=\"square\" ]\n".to_owned());
        write("  ei [ shape=\"diamond\" ]\n".to_owned());

        write(format!("  ti -> ei [label=\"{}\"]\n", fmte(&init.exts)));
        for state in init.states.iter() {
            write(format!("  ei -> q{}\n", state.0));
        }

        {
            let mut tix = 0;
            let mut gix = 0;
            let mut bix = 0;
            for (qix, state) in self.states.iter().enumerate() {
                for tran in state.transitions.iter() {
                    write(format!("  g{} [ shape=\"diamond\" ]\n", gix));
                    write(format!("  q{} -> g{} [label=\"{}\"]\n", qix, gix, tran.key));

                    for dix in tran.dfa_inits.iter() {
                        write(format!("  g{} -> d{} [color=\"blue\"]\n", gix, dix.0));
                    }

                    let tix0 = tix;

                    for target in tran.bdd.leaves.iter() {
                        write(format!("  t{} [ shape=\"square\" ]\n", tix));
                        write(format!("  e{} [ shape=\"diamond\" ]\n", tix));
                        write(format!("  t{} -> e{} [label=\"{}\"]\n",
                            tix, tix, fmte(&target.exts)));
                        for state in target.states.iter()
                            { write(format!("  e{} -> q{}\n", tix, state.0)); }
                        tix += 1;
                    }

                    let llen = tran.bdd.leaves.len();
                    let bix0 = bix;

                    for (dtag, pos, neg) in tran.bdd.nodes.iter() {
                        write(format!("  b{} [ shape=\"diamond\", label=\"{}\" ]\n", bix, dtag.0));
                        if *pos < llen {
                            write(format!("  b{} -> t{} [ color=green ]\n", bix, tix0 + *pos));
                        } else {
                            write(format!("  b{} -> b{} [ color=green ]\n", bix, bix0 + *pos - llen));
                        }
                        if *neg < llen {
                            write(format!("  b{} -> t{} [ color=red ]\n", bix, tix0 + *neg));
                        } else {
                            write(format!("  b{} -> b{} [ color=red ]\n", bix, bix0 + *neg - llen));
                        }
                        bix += 1;
                    }

                    if tran.bdd.nodes.is_empty() {
                        write(format!("  g{} -> t{}\n", gix, tix - 1));
                    } else {
                        write(format!("  g{} -> b{}\n", gix, bix - 1));
                    }

                    gix += 1;
                }
            }
        }

        for (dix, state) in self.dfa.states.iter().enumerate() {
            write(format!("  d{} [label=\"d{}", dix, dix));
            for tag in state.tags.0.iter() { write(format!(" {}", tag)); }
            write("\"]\n".to_owned());

            for (guard, state) in state.transitions.iter() {
                write(format!("  d{} -> d{} [label=\"{:?}\"]\n", dix, state, guard));
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
        parser.to_dot(&init, std::io::BufWriter::new(file));
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
        parser.to_dot(&init, std::io::BufWriter::new(file));
    }
}
