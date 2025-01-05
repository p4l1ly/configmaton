use hashbrown::HashMap;
use hashbrown::HashSet;
use std::io::Write;
use std::fmt;

use serde::de::{MapAccess, Visitor, Deserialize, Deserializer, Error, Unexpected};
use serde_json;
use serde_json::Value;

use crate::ast;
use crate::blob::bdd::BddOrigin;
use crate::blob::keyval_state::LeafOrigin;
use crate::blob::keyval_state::StateOrigin;
use crate::blob::keyval_state::TranOrigin;
use crate::char_enfa;
use crate::char_nfa;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StateIx (pub usize);
#[derive(Debug, Clone, Copy)]
pub struct DfaIx (pub usize);
#[derive(Debug, Clone, Copy)]
pub struct DfaStateIx (pub usize);

pub fn join_leaves<I: Iterator<Item=LeafOrigin>>(targets: I) -> LeafOrigin {
    let mut states = HashSet::new();
    let mut get_olds = HashSet::new();
    let mut exts = HashSet::new();
    for target in targets {
        states.extend(target.states.into_iter());
        get_olds.extend(target.get_olds.into_iter());
        exts.extend(target.exts.into_iter());
    }
    LeafOrigin {
        exts: exts.into_iter().collect(),
        get_olds: get_olds.into_iter().collect(),
        states: states.into_iter().collect(),
    }
}

fn bytes_as_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b|
        if b.is_ascii_graphic()
            { char::from(*b).to_string() }
        else
            { format!("\\{}", b) }
    ).collect()
}

fn fmte(exts: &Vec<Vec<u8>>, get_olds: &Vec<Vec<u8>>) -> String {
    exts.iter().map(|ext| bytes_as_string(ext)).chain(
        get_olds.iter().map(|old| format!("GetOld({})", bytes_as_string(old)))
    ).collect::<Vec<_>>().join(", ").replace("\\", "\\\\").replace("\"", "\\\"")
}

pub fn to_dot
    <F: FnMut(String)>
    (bdd: &BddOrigin<usize, LeafOrigin>, bix: &mut usize, tix: &mut usize, write: &mut F)
    -> String
{
    let mut visited = HashMap::new();
    match bdd {
        BddOrigin::Leaf(target) => {
            let me = format!("t{}", tix);
            write(format!("  t{} [ shape=\"square\" ]\n", tix));
            write(format!("  e{} [ shape=\"diamond\" ]\n", tix));
            write(format!("  t{} -> e{} [label=\"{}\"]\n",
                tix, tix, fmte(&target.exts, &target.get_olds)));
            for state in target.states.iter()
                { write(format!("  e{} -> q{}\n", tix, state)); }
            *tix += 1;
            me
        }
        _ => {
            let dtag = bdd.get_var();
            let pos = unsafe { bdd.get_pos() };
            let neg = unsafe { bdd.get_neg() };
            let me = format!("b{}", bix);
            write(format!("  {} [ shape=\"diamond\", label=\"{}\" ]\n", me, dtag));
            *bix += 1;
            let pos = visited.entry(pos as *const _)
                .or_insert_with(|| to_dot(pos, bix, tix, write));
            write(format!("  {} -> {} [ color=green{} ]\n", me, pos,
                if bdd.owns_pos() { ", penwidth=2" } else { "" }));
            let neg = visited.entry(neg as *const _)
                .or_insert_with(|| to_dot(neg, bix, tix, write));
            write(format!("  {} -> {} [ color=red{} ]\n", me, neg,
                if bdd.owns_neg() { ", penwidth=2" } else { "" }));
            me
        }
    }
}

pub struct Parser {
    pub states: Vec<StateOrigin>,
    pub nfa: char_nfa::Nfa,
    pub regexes: HashMap<String, (DfaStateIx, DfaIx)>,
}

impl Parser {
    pub fn parse(cmds: Vec<Cmd>) -> (Self, LeafOrigin) {
        let mut parser = Parser {
            states: vec![],
            nfa: char_nfa::Nfa::new(),
            regexes: HashMap::new(),
        };
        let init = parser.parse_parallel(cmds);

        (parser, init)
    }

    fn parse_parallel(&mut self, cmds: Vec<Cmd>) -> LeafOrigin {
        let targets = cmds.into_iter().map(|cmd| match cmd {
            Cmd::Match(match_) => self.parse_match(match_),
            _ => unimplemented!(),
        });
        join_leaves(targets)
    }

    fn parse_match(
        &mut self,
        match_: Match,
    ) -> LeafOrigin {
        let mut then = self.parse_parallel(match_.then);
        then.exts.extend(match_.run);

        if match_.when.is_empty() { return then; }

        let dfa_ixs = match_.when.iter().map(|(_, regex)| {
            let dfa_ix = self.regexes.len();
            *self.regexes.entry(regex.clone()).or_insert_with(|| {
                let dfa_state_ix = self.nfa.states.len();
                self.nfa.add_nfa(char_enfa::Nfa::from_ast(ast::parse_regex(&regex)), dfa_ix);
                (DfaStateIx(dfa_state_ix), DfaIx(dfa_ix))
            })
        }).collect::<Vec<_>>();

        let guard_count = match_.when.len();
        for ((key, _), (dfa_state_ix, dfa_ix)) in
            match_.when[..guard_count - 1].into_iter().zip(dfa_ixs.iter()).rev()
        {
            let state_ix = self.states.len();
            let else_ = LeafOrigin {
                exts: vec![], get_olds: vec![], states: vec![state_ix + guard_count]
            };
            self.states.push(StateOrigin { transitions: vec![TranOrigin {
                key: key.clone().into_bytes(),
                dfa_inits: vec![dfa_state_ix.0],
                bdd: BddOrigin::NodeBothOwned {
                    var: dfa_ix.0,
                    pos: Box::new(BddOrigin::Leaf(then)),
                    neg: Box::new(BddOrigin::Leaf(else_)),
                }
            }]});
            then = LeafOrigin {
                exts: vec![],
                get_olds: vec![key.clone().into_bytes()],
                states: vec![state_ix],
            };
        }

        for ((key, _), (dfa_state_ix, dfa_ix)) in
            match_.when[..guard_count].into_iter().zip(dfa_ixs.iter()).rev()
        {
            let state_ix = self.states.len();
            let else_ = LeafOrigin
                { exts: vec![], get_olds: vec![], states: vec![state_ix] };
            self.states.push(StateOrigin { transitions: vec![TranOrigin {
                key: key.clone().into_bytes(),
                dfa_inits: vec![dfa_state_ix.0],
                bdd: BddOrigin::NodeBothOwned {
                    var: dfa_ix.0,
                    pos: Box::new(BddOrigin::Leaf(then)),
                    neg: Box::new(BddOrigin::Leaf(else_))
                }
            }]});

            then = LeafOrigin {
                exts: vec![],
                get_olds: vec![key.clone().into_bytes()],
                states: vec![state_ix],
            };
        }

        then
    }

    pub fn to_dot<W: Write>(&self, init: &LeafOrigin, mut writer: W) {
        writer.write_all(b"digraph G {\n").unwrap();

        let mut write = |x: String| writer.write_all(x.as_bytes()).unwrap();

        for i in 0..self.states.len() {
            write(format!("  q{}\n", i));
        }

        // println!("~~~ {:?} ~~~> {:?}", init.exts, init.states);
        write("  ti [ shape=\"square\" ]\n".to_owned());
        write("  ei [ shape=\"diamond\" ]\n".to_owned());

        write(format!("  ti -> ei [label=\"{}\"]\n", fmte(&init.exts, &init.get_olds)));
        for state in init.states.iter() {
            write(format!("  ei -> q{}\n", state));
        }

        {
            let mut tix = 0;
            let mut gix = 0;
            let mut bix = 0;
            for (qix, state) in self.states.iter().enumerate() {
                for tran in state.transitions.iter() {
                    write(format!("  g{} [ shape=\"diamond\" ]\n", gix));
                    write(format!("  q{} -> g{} [label=\"{}\"]\n",
                        qix, gix, bytes_as_string(&tran.key)));

                    for dix in tran.dfa_inits.iter() {
                        write(format!("  g{} -> d{} [color=\"blue\"]\n", gix, dix));
                    }

                    let root = to_dot(&tran.bdd, &mut bix, &mut tix, &mut write);

                    write(format!("  g{} -> {}\n", gix, root));

                    gix += 1;
                }
            }
        }

        for (dix, state) in self.nfa.states.iter().enumerate() {
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
    run: Vec<Vec<u8>>,
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
        let mut run: Option<Vec<String>> = None;
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
        let run = run.unwrap_or_else(|| vec![]).into_iter().map(|s| s.into_bytes()).collect();
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
                "run": [ "m1" ]
            },
            {
                "when": { "foo": "baz" },
                "run": [ "m2" ],
                "then": [
                    {
                        "when": { "qux": "a.*" },
                        "run": [ "m3" ]
                    },
                    {
                        "when": { "qux": "ahoy" },
                        "run": [ "m4" ]
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
