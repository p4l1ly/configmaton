use hashbrown::HashMap;
use hashbrown::HashSet;
use std::io::Write;
use std::fmt;

use serde::de::{MapAccess, Visitor, Deserialize, Deserializer, Error, Unexpected};
use serde_json;
use serde_json::Value;

use crate::ast;
use crate::blob::align_up_mut_ptr;
use crate::blob::automaton::Automaton;
use crate::blob::automaton::ExtsAndAut;
use crate::blob::automaton::InitsAndStates;
use crate::blob::automaton::States;
use crate::blob::bdd::BddOrigin;
use crate::blob::keyval_state::KeyValState;
use crate::blob::keyval_state::LeafOrigin;
use crate::blob::keyval_state::StateOrigin;
use crate::blob::keyval_state::TranOrigin;
use crate::blob::keyval_state::Bytes;
use crate::blob::sediment::Sediment;
use crate::blob::state::build::U8BuildConfig;
use crate::blob::state::U8State;
use crate::blob::state::U8StatePrepared;
use crate::blob::vec::BlobVec;
use crate::blob::BuildCursor;
use crate::blob::Reserve;
use crate::blob::Shifter;
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
    // Fields for goto/label support
    labels: HashMap<String, LeafOrigin>,
    label_definitions: HashMap<String, Cmd>, // Store label bodies for later parsing
    parsing_labels: HashSet<String>, // Track which labels are currently being parsed
}

impl Parser {
    pub fn parse(cmds: Vec<Cmd>) -> (Self, LeafOrigin) {
        let mut parser = Parser {
            states: vec![],
            nfa: char_nfa::Nfa::new(),
            regexes: HashMap::new(),
            labels: HashMap::new(),
            label_definitions: HashMap::new(),
            parsing_labels: HashSet::new(),
        };
        
        // First pass: collect all label definitions without parsing them
        parser.collect_label_definitions(&cmds);
        
        // Second pass: parse all labels now that we know what exists
        for (label_name, label_body) in parser.label_definitions.clone() {
            if !parser.labels.contains_key(&label_name) {
                parser.parse_label(&label_name, &label_body);
            }
        }
        
        // Third pass: parse the main structure now that all labels are resolved
        let init = parser.parse_parallel(cmds);

        (parser, init)
    }
    
    fn collect_label_definitions(&mut self, cmds: &[Cmd]) {
        for cmd in cmds {
            self.collect_labels_from_cmd(cmd);
        }
    }
    
    fn collect_labels_from_cmd(&mut self, cmd: &Cmd) {
        match cmd {
            Cmd::Label(label, body) => {
                // Store the label definition for later parsing
                self.label_definitions.insert(label.clone(), (**body).clone());
                // Also recursively collect labels from within the body
                self.collect_labels_from_cmd(body);
            }
            Cmd::Match(match_) => {
                // Collect labels from the 'then' clause
                self.collect_label_definitions(&match_.then);
            }
            Cmd::Goto(_) => {
                // Nothing to collect from gotos
            }
        }
    }

    fn parse_parallel(&mut self, cmds: Vec<Cmd>) -> LeafOrigin {
        let targets = cmds.into_iter().map(|cmd| self.parse_cmd(&cmd));
        join_leaves(targets)
    }

    fn parse_cmd(&mut self, cmd: &Cmd) -> LeafOrigin {
        match cmd {
            Cmd::Match(match_) => self.parse_match(match_.clone()),
            Cmd::Label(_, body) => self.parse_cmd(body), // Return the body's result
            Cmd::Goto(target) => {
                // Try to get the label, parsing it if necessary
                if let Some(target_leaf) = self.labels.get(target).cloned() {
                    target_leaf
                } else if let Some(label_body) = self.label_definitions.get(target).cloned() {
                    self.parse_label(target, &label_body)
                } else {
                    panic!("Unresolved goto: label '{}' not found", target);
                }
            }
        }
    }

    fn parse_label(&mut self, label_name: &str, label_body: &Cmd) -> LeafOrigin {
        // Check for cycles
        if self.parsing_labels.contains(label_name) {
            // We're in a cycle - create a placeholder that will be resolved later
            // For now, just return an empty LeafOrigin to break the cycle
            return LeafOrigin {
                states: vec![],
                get_olds: vec![],
                exts: vec![],
            };
        }
        
        // Check if already parsed
        if let Some(existing) = self.labels.get(label_name).cloned() {
            return existing;
        }
        
        // Mark as being parsed
        self.parsing_labels.insert(label_name.to_string());
        
        // Parse the label body
        let result = self.parse_cmd(label_body);
        
        // Store the result
        self.labels.insert(label_name.to_string(), result.clone());
        
        // Remove from parsing set
        self.parsing_labels.remove(label_name);
        
        result
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

#[derive(Debug, Clone)]
pub enum Cmd {
    Match(Match),
    Label(String, Box<Cmd>),  // No support yet.
    Goto(String),  // No support yet.
}

#[derive(Debug, Clone, serde::Deserialize)]
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
        let mut goto = None;
        let mut label = None;
        let mut body = None;
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
                "goto" => {
                    if goto.is_some() {
                        return Err(Error::duplicate_field("goto"));
                    }
                    goto = Some(map.next_value()?);
                }
                "label" => {
                    if label.is_some() {
                        return Err(Error::duplicate_field("label"));
                    }
                    label = Some(map.next_value()?);
                }
                "body" => {
                    if body.is_some() {
                        return Err(Error::duplicate_field("body"));
                    }
                    body = Some(map.next_value()?);
                }
                _ => {
                    return Err(
                        Error::unknown_field(
                            key, &["when", "run", "then", "label", "body", "goto"]
                        )
                    );
                }
            }
        }
        match (when, goto, label) {
            (Some(when), None, None) => {
                let run = run.unwrap_or_else(|| vec![]).into_iter().map(|s| s.into_bytes()).collect();
                let then = then.unwrap_or_else(|| vec![]);
                Ok(Cmd::Match(Match { when, run, then }))
            }
            (None, Some(goto), None) => {
                Ok(Cmd::Goto(goto))
            }
            (None, None, Some(label)) => {
                let body = body.ok_or_else(|| Error::missing_field("body"))?;
                Ok(Cmd::Label(label, body))
            }
            _ => Err(Error::custom("exactly one of when/label/goto expected"))
        }
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


pub struct Msg {
    owner: Box<[u8]>,
    pub data: *const u8,
}

// This is safe because we guarantee that `data` always points into `owner`.
unsafe impl Send for Msg {}

impl Msg {
    pub fn data_len(&self) -> usize {
        self.owner.len() - size_of::<usize>()
    }

    pub unsafe fn read<R: FnOnce(*mut u8)>(ext_read: R, len: usize) -> Msg {
        let mut buff = vec![0; len + size_of::<usize>()].into_boxed_slice();
        let buf = align_up_mut_ptr::<u8, u128>(buff.as_mut_ptr()) as *mut u8;
        ext_read(buf);
        Msg::deserialize(buf);
        Msg { owner: buff, data: buf }
    }

    pub fn get_automaton<'a>(&'a self) -> &'a Automaton<'a> {
        unsafe { &*(self.data as *const Automaton<'a>) }
    }

    pub unsafe fn deserialize<'a>(buf: *mut u8) {
        let cur = BuildCursor::new(buf);
        let shifter = Shifter(cur.buf);
        let _: BuildCursor<()> = unsafe {
            Automaton::deserialize(cur,
                |cur| Sediment::<Bytes>::deserialize(cur,
                    |cur| Bytes::deserialize(cur, |_| ())),
                |cur| ExtsAndAut::deserialize(cur,
                    |cur| Sediment::<Bytes>::deserialize(cur,
                        |cur| Bytes::deserialize(cur, |_| ())),
                    |cur| InitsAndStates::deserialize(cur,
                        |cur| BlobVec::<*const KeyValState>::deserialize(cur,
                            |x| { shifter.shift(x); }),
                        |cur| States::deserialize(cur,
                            |cur| Sediment::<KeyValState>::deserialize(cur,
                                |cur| KeyValState::deserialize(cur)),
                            |cur| Sediment::<U8State>::deserialize(cur,
                                |cur| U8State::deserialize(cur)),
                        )
                    )
                )
            )
        };
    }

    pub fn serialize<Cfg: U8BuildConfig>(parser: &Parser, init: &LeafOrigin, cfg: &Cfg) -> Msg {
        let u8states = parser.nfa.states.iter()
            .map(|q| U8StatePrepared::prepare(q, cfg)).collect::<Vec<_>>();
        let mut sz = Reserve(0);
        let mut u8qs = Vec::<usize>::new();
        let mut kvqs = Vec::<usize>::new();
        let mut origin = (
            &init.get_olds,
            (
                &init.exts,
                (
                    vec![0; init.states.len()],
                    (
                        &parser.states,
                        &u8states,
                    )
                )
            )
        );

        Automaton::reserve(&origin, &mut sz,
            |getolds, sz| {Sediment::<Bytes>::reserve(getolds, sz,
                |getold, sz| {Bytes::reserve(getold, sz);} );},
            |exts_and_aut, sz| {ExtsAndAut::reserve(exts_and_aut, sz,
                |exts, sz| {Sediment::<Bytes>::reserve(exts, sz,
                    |ext, sz| {Bytes::reserve(ext, sz);} );},
                |inits_and_states, sz| {InitsAndStates::reserve(inits_and_states, sz,
                    |inits, sz| { BlobVec::<*const KeyValState>::reserve(inits, sz); },
                    |states, sz| {States::reserve(states, sz,
                        |orig_kvqs, sz| {Sediment::<KeyValState>::reserve(orig_kvqs, sz,
                            |kvq, sz| { kvqs.push(KeyValState::reserve(kvq, sz)) } );},
                        |orig_u8qs, sz| {Sediment::<U8State>::reserve(orig_u8qs, sz,
                            |u8q, sz| { u8qs.push(U8State::reserve(u8q, sz)) } );},
                    );}
                );}
            );}
        );

        for (target, source) in origin.1.1.0.iter_mut().zip(init.states.iter()) {
            *target = kvqs[*source];
        }

        let mut buff = vec![0; sz.0 + size_of::<usize>()].into_boxed_slice();
        let buf = align_up_mut_ptr::<u8, u128>(buff.as_mut_ptr()) as *mut u8;
        let cur = BuildCursor::new(buf);
        let _: BuildCursor<()> = unsafe {
            Automaton::serialize(&origin, cur,
                |getolds, cur| Sediment::<Bytes>::serialize(getolds, cur,
                    |getold, cur| Bytes::serialize(getold, cur, |x, y| { *y = *x; })),
                |exts_and_aut, cur| ExtsAndAut::serialize(exts_and_aut, cur,
                    |exts, cur| Sediment::<Bytes>::serialize(exts, cur,
                        |ext, cur| Bytes::serialize(ext, cur, |x, y| { *y = *x; })),
                    |inits_and_states, cur| InitsAndStates::serialize(inits_and_states, cur,
                        |inits, cur| BlobVec::<*const KeyValState>::serialize(inits, cur,
                            |x, y| { *y = *x as *const KeyValState; }),
                        |states, cur| States::serialize(states, cur,
                            |orig_kvqs, cur| Sediment::<KeyValState>::serialize(orig_kvqs, cur,
                                |kvq, cur| KeyValState::serialize(kvq, cur, &u8qs, &kvqs)),
                            |orig_u8qs, cur| Sediment::<U8State>::serialize(orig_u8qs, cur,
                                |u8q, cur| U8State::serialize(u8q, cur, &u8qs)),
                        )
                    )
                )
            )
        };

        Msg { owner: buff, data: buf }
    }
}


#[cfg(test)]
mod tests {
    use indexmap::IndexSet;

    use crate::{blob::tests::TestU8BuildConfig, keyval_simulator::Simulation};

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

        let outmsg = Msg::serialize(&parser, &init, &TestU8BuildConfig);
        let inmsg = unsafe {
            Msg::read(|buf| buf.copy_from(outmsg.data, outmsg.data_len()), outmsg.data_len()) };
        let aut = inmsg.get_automaton();
        let mut sim = Simulation::new(aut, |_| None);

        assert!(sim.exts.is_empty());
        sim.read(b"foo", b"a", |x| match x { b"foo" => Some(b"a"), _ => None });
        assert!(sim.exts.is_empty());
        sim.read(b"foo", b"b", |x| match x { b"foo" => Some(b"b"), _ => None });
        assert!(sim.exts.is_empty());
        sim.read(b"bar", b"b",
            |x| match x { b"foo" => Some(b"b"), b"bar" => Some(b"b"), _ => None });
        assert!(sim.exts.is_empty());
        sim.read(b"foo", b"a",
            |x| match x { b"foo" => Some(b"a"), b"bar" => Some(b"b"), _ => None });
        let ext = b"you win";
        let mut exts = IndexSet::new();
        exts.insert(ext.as_slice());
        assert_eq!(&sim.exts, &exts);
    }

    #[test]
    fn config_to_automaton_simplest() {
        // read and parse file tests/config.json
        let config: Vec<Cmd> = serde_json::from_str(r#"[{"when": {"foo": "a"}, "run": ["bar"]}]"#).unwrap();

        let (parser, init) = Parser::parse(config);

        // The output automaton is for now only for visual checking.
        let file = std::fs::File::create("/tmp/test_simplest.dot").unwrap();
        parser.to_dot(&init, std::io::BufWriter::new(file));
    }

    #[test]
    fn test_goto_label_comprehensive() {
        let config: Vec<Cmd> = serde_json::from_str(r#"[
            {
                "when": { "test_type": "basic" },
                "then": [
                    {
                        "label": "basic_target",
                        "body": {
                            "when": { "action": "process" },
                            "run": [ "basic_processed" ]
                        }
                    }
                ]
            },
            {
                "when": { "test_type": "basic_goto" },
                "then": [
                    { "goto": "basic_target" }
                ]
            },
            {
                "when": { "test_type": "forward_ref" },
                "then": [
                    { "goto": "forward_target" }
                ]
            },
            {
                "when": { "test_type": "cycle_a" },
                "then": [
                    {
                        "label": "cycle_a",
                        "body": {
                            "when": { "step": "1" },
                            "run": [ "cycle_a_step" ],
                            "then": [
                                { "goto": "cycle_b" }
                            ]
                        }
                    }
                ]
            },
            {
                "when": { "test_type": "cycle_b" },
                "then": [
                    {
                        "label": "cycle_b", 
                        "body": {
                            "when": { "step": "2" },
                            "run": [ "cycle_b_step" ],
                            "then": [
                                { "goto": "cycle_a" }
                            ]
                        }
                    }
                ]
            },
            {
                "when": { "test_type": "nested" },
                "then": [
                    {
                        "when": { "level": "outer" },
                        "run": [ "outer_action" ],
                        "then": [
                            {
                                "label": "nested_target",
                                "body": {
                                    "when": { "level": "inner" },
                                    "run": [ "inner_action" ]
                                }
                            }
                        ]
                    }
                ]
            },
            {
                "when": { "test_type": "nested_goto" },
                "then": [
                    { "goto": "nested_target" }
                ]
            },
            {
                "label": "forward_target",
                "body": {
                    "when": { "action": "forward" },
                    "run": [ "forward_processed" ]
                }
            }
        ]"#).unwrap();

        let (parser, init) = Parser::parse(config);

        // Verify parsing succeeded
        assert!(parser.states.len() > 0);
        assert!(init.states.len() > 0);
        assert_eq!(parser.labels.len(), 5); // Should have 5 labels

        // Test serialization/deserialization
        let outmsg = Msg::serialize(&parser, &init, &TestU8BuildConfig);
        let inmsg = unsafe {
            Msg::read(|buf| buf.copy_from(outmsg.data, outmsg.data_len()), outmsg.data_len()) 
        };
        let aut = inmsg.get_automaton();
        
        // Test basic goto functionality
        let mut sim = Simulation::new(aut, |_| None);
        sim.read(b"test_type", b"basic_goto", |x| match x { b"test_type" => Some(b"basic_goto"), _ => None });
        sim.read(b"action", b"process", |x| match x { 
            b"test_type" => Some(b"basic_goto"), 
            b"action" => Some(b"process"), 
            _ => None 
        });
        let mut expected_exts = IndexSet::new();
        expected_exts.insert(b"basic_processed".as_slice());
        assert_eq!(&sim.exts, &expected_exts);

        // Test forward reference
        let mut sim = Simulation::new(aut, |_| None);
        sim.read(b"test_type", b"forward_ref", |x| match x { b"test_type" => Some(b"forward_ref"), _ => None });
        sim.read(b"action", b"forward", |x| match x { 
            b"test_type" => Some(b"forward_ref"), 
            b"action" => Some(b"forward"), 
            _ => None 
        });
        expected_exts.clear();
        expected_exts.insert(b"forward_processed".as_slice());
        assert_eq!(&sim.exts, &expected_exts);

        // Test cycle A -> B
        let mut sim = Simulation::new(aut, |_| None);
        sim.read(b"test_type", b"cycle_a", |x| match x { b"test_type" => Some(b"cycle_a"), _ => None });
        sim.read(b"step", b"1", |x| match x { 
            b"test_type" => Some(b"cycle_a"), 
            b"step" => Some(b"1"), 
            _ => None 
        });
        expected_exts.clear();
        expected_exts.insert(b"cycle_a_step".as_slice());
        assert_eq!(&sim.exts, &expected_exts);

        // Test nested label access
        let mut sim = Simulation::new(aut, |_| None);
        sim.read(b"test_type", b"nested_goto", |x| match x { b"test_type" => Some(b"nested_goto"), _ => None });
        sim.read(b"level", b"inner", |x| match x { 
            b"test_type" => Some(b"nested_goto"), 
            b"level" => Some(b"inner"), 
            _ => None 
        });
        expected_exts.clear();
        expected_exts.insert(b"inner_action".as_slice());
        assert_eq!(&sim.exts, &expected_exts);

        // Generate dot file for visual inspection
        let file = std::fs::File::create("/tmp/test_goto_comprehensive.dot").unwrap();
        parser.to_dot(&init, std::io::BufWriter::new(file));
    }

    #[test]
    fn goto() {
        let _config: Vec<Cmd> = serde_json::from_str(r#"[
            {
                "when": { "foo": "baz" },
                "then": [
                    {
                        "label": "hello",
                        "body": {
                            "when": {},
                            "run": [ "m1", "m2", "m3" ]
                        }
                    }
                ]
            },
            {
                "when": { "foo": "qux" },
                "then": [{"goto": "hello"}]
            }
        ]"#).unwrap();
    }
}
