use std::mem::{MaybeUninit, transmute};

use crate::config_parser as inp;
use crate::config_parser::TargetIx;
use crate::automaton as out;
use crate::keyval_simulator as outsim;

type TT = outsim::Triggers;
type Tran<'a> = out::Tran<'a, TT>;
type State<'a> = out::State<Tran<'a>>;
type SelfHandlingSparseState<'a> = out::SelfHandlingSparseState<Tran<'a>>;
type SelfHandlingDenseState<'a> = out::SelfHandlingDenseState<Tran<'a>>;
type TranBody<'a> = out::TranBody<'a, TT>;


pub struct AutHolder<'a> {
    pub normal_states: Box<[State<'a>]>,
    pub self_handling_sparse_states: Box<[SelfHandlingSparseState<'a>]>,
    pub self_handling_dense_states: Box<[SelfHandlingDenseState<'a>]>,
    pub shared_trans: Box<[out::TranBody<'a, TT>]>,
}


enum OutStateIx {
    Normal(usize),
    SelfHandlingSparse(usize),
    SelfHandlingDense(usize),
}

enum OutAnyState {
    Normal(out::State<TargetIx>),
    SelfHandlingSparse(out::SelfHandlingSparseState<TargetIx>),
    SelfHandlingDense(out::SelfHandlingDenseState<TargetIx>),
}

fn convert_to_normal_state<
    const DENSE_GUARD_COUNT: usize,
    const BIG_GUARD_SIZE: u32,
    const SELFHANDLING_TRANS_COUNT: usize,
>(state: &inp::State) -> OutAnyState {
    let mut has_var = false;
    let mut has_char = false;
    let mut guard_count = 0;

    for (guard, _) in state.transitions.iter() {
        match guard {
            inp::Guard::Var(_) => {
                has_var = true;
                if has_char { panic!("Mixed Var-Chars") }
            },
            inp::Guard::EndVar => {
                has_char = true;
                if has_var { panic!("Mixed Var-Chars") }
            }
            inp::Guard::Guard(_) => {
                guard_count += 1;
                has_char = true;
                if has_var { panic!("Mixed Var-Chars") }
            }
        }
    }

    if has_char && guard_count >= DENSE_GUARD_COUNT {
        let mut trans: [Option<TargetIx>; 257] = [None; 257];
        let mut add_tran = |c: usize, target: TargetIx| {
            let x = &mut trans[c];
            match *x {
                Some(_) => panic!("Overlapping guards"),
                None => *x = Some(target),
            }
        };

        let mut guard_trans = vec![];
        for (guard, target) in state.transitions.iter() {
            match guard {
                inp::Guard::Guard(guard) => guard_trans.push((guard, *target)),
                inp::Guard::EndVar => add_tran(256, *target),
                inp::Guard::Var(_) => unreachable!(),
            }
        }

        let mut c = 0;
        loop {
            for (guard, target) in guard_trans.iter() {
                if guard.contains(c) { add_tran(c as usize, *target); }
            }
            if c == 255 { break; }
            c += 1;
        }

        OutAnyState::SelfHandlingDense(out::SelfHandlingDenseState(trans))
    }
    else {
        let mut explicit_trans = vec![];
        let mut pattern_trans = vec![];
        let mut small_guard_trans = vec![];
        for (guard, target) in state.transitions.iter() {
            match guard {
                inp::Guard::Var(key) =>
                    explicit_trans.push((out::Explicit::Var(key.clone()), *target)),
                inp::Guard::EndVar =>
                    explicit_trans.push((out::Explicit::EndVar, *target)),
                inp::Guard::Guard(guard) => {
                    if guard.size() < BIG_GUARD_SIZE {
                        small_guard_trans.push((guard, *target));
                    } else {
                        pattern_trans.push((guard.clone(), *target));
                    }
                },
            }
        }

        if !small_guard_trans.is_empty() {
            let mut c = 0;
            loop {
                for (guard, target) in small_guard_trans.iter() {
                    if guard.contains(c)
                        { explicit_trans.push((out::Explicit::Char(c), *target)); }
                }
                if c == 255 { break; }
                c += 1;
            }
        }

        if explicit_trans.len() + pattern_trans.len() < SELFHANDLING_TRANS_COUNT {
            OutAnyState::Normal(out::State{
                explicit_trans: explicit_trans.into_boxed_slice(),
                pattern_trans: pattern_trans.into_boxed_slice(),
            })
        } else {
            OutAnyState::SelfHandlingSparse(out::SelfHandlingSparseState{
                explicit_trans: explicit_trans.into_iter().collect(),
                pattern_trans: pattern_trans.into_boxed_slice(),
            })
        }
    }
}

fn worth_sharing_target(target: &inp::Target, refcount: usize) -> bool {
    refcount >= 2 && (!target.exts.is_empty() || target.states.len() > 2)
}

fn uninit_vec<X, Y>(v: &Vec<X>) -> Vec<MaybeUninit<Y>> {
    let len = v.len();
    let mut v = Vec::with_capacity(len);
    for _ in 0..len {
        v.push(MaybeUninit::uninit());
    }
    v
}

unsafe fn refer<'a, X>(x: *const X) -> out::Lock<'a, X> {
    out::Lock(transmute(x))
}

unsafe fn refer_uninit<'a, X>(x: *const MaybeUninit<X>) -> out::Lock<'a, X> {
    out::Lock(transmute(x))
}

fn clone_tran<'a>(tran: &Tran<'a>) -> Tran<'a> {
    match tran {
        out::Tran::Owned(out::TranBody{ right_states, tran_trigger }) =>
            out::Tran::Owned(out::TranBody{
                right_states: right_states.clone(),
                tran_trigger: tran_trigger.clone(),
            }),
        out::Tran::Shared(body) => out::Tran::Shared(body.clone()),
    }
}

pub fn parser_to_simulator<
    'a,
    const DENSE_GUARD_COUNT: usize,
    const BIG_GUARD_SIZE: u32,
    const SELFHANDLING_TRANS_COUNT: usize,
>(parsaut: inp::Parser, init: inp::Target)
-> (AutHolder<'a>, outsim::KeyValSimulator<'a>, outsim::KeyValState)
{
    let mut normal_states0: Vec<out::State<TargetIx>> = vec![];
    let mut self_handling_sparse_states0: Vec<out::SelfHandlingSparseState<TargetIx>> = vec![];
    let mut self_handling_dense_states0: Vec<out::SelfHandlingDenseState<TargetIx>> = vec![];

    let mut target_refcounts: Vec<usize> = vec![0; parsaut.targets.len()];
    let parseaut_statemap = parsaut.states.iter().map(|state| {
        for (_, target) in state.transitions.iter() {
            target_refcounts[target.0] += 1;
        }

        match convert_to_normal_state::<
            DENSE_GUARD_COUNT,
            BIG_GUARD_SIZE,
            SELFHANDLING_TRANS_COUNT,
        >(state) {
            OutAnyState::Normal(state) => {
                normal_states0.push(state);
                OutStateIx::Normal(normal_states0.len() - 1)
            }
            OutAnyState::SelfHandlingSparse(state) => {
                self_handling_sparse_states0.push(state);
                OutStateIx::SelfHandlingSparse(self_handling_sparse_states0.len() - 1)
            }
            OutAnyState::SelfHandlingDense(state) => {
                self_handling_dense_states0.push(state);
                OutStateIx::SelfHandlingDense(self_handling_dense_states0.len() - 1)
            }
        }
    }).collect::<Vec<_>>();

    let mut normal_states =
        uninit_vec::<_, State<'a>>(&normal_states0);
    let mut self_handling_sparse_states =
        uninit_vec::<_, SelfHandlingSparseState<'a>>(&self_handling_sparse_states0);
    let mut self_handling_dense_states =
        uninit_vec::<_, SelfHandlingDenseState<'a>>(&self_handling_dense_states0);
    let mut shared_trans: Vec<TranBody<'a>> = vec![];

    let holder = unsafe {
        let targetmap = parsaut.targets.iter().zip(target_refcounts.into_iter()).map(
            |(target, refcount)|
            {
                let tranbody = out::TranBody {
                    tran_trigger: outsim::Triggers(
                        target.exts.iter().map(|ext| {
                            match ext {
                                inp::Ext::GetOld(old) => outsim::Trigger::Old(old.clone()),
                                inp::Ext::Ext(ext) => outsim::Trigger::Ext(ext.clone()),
                            }
                        }).collect::<Vec<_>>().into_boxed_slice(),
                    ),
                    right_states: {
                        let mut any_state_locks = target.states.iter().map(|state| {
                            match parseaut_statemap[state.0] {
                                OutStateIx::Normal(ix) => out::AnyStateLock::Normal(
                                    refer_uninit(&normal_states[ix])),
                                OutStateIx::SelfHandlingSparse(ix) => out::AnyStateLock::Sparse(
                                    refer_uninit(&self_handling_sparse_states[ix])),
                                OutStateIx::SelfHandlingDense(ix) => out::AnyStateLock::Dense(
                                    refer_uninit(&self_handling_dense_states[ix])),
                            }
                        });
                        out::Succ(
                            match any_state_locks.next() {
                                Some(x) => x,
                                None => out::AnyStateLock::None
                            }, 
                            any_state_locks.collect::<Vec<_>>().into_boxed_slice(),
                        )
                    }
                };
                if worth_sharing_target(target, refcount) {
                    let ix = shared_trans.len();
                    shared_trans.push(tranbody);
                    out::Tran::Shared(refer(&shared_trans[ix]))
                } else {
                    out::Tran::Owned(tranbody)
                }
            }
        ).collect::<Vec<_>>();

        for (outq, inq) in normal_states.iter_mut().zip(normal_states0.into_iter()) {
            let inq2 = inq.map(|tran_ix| clone_tran(&targetmap[tran_ix.0]));
            outq.write(inq2);
        }

        for (outq, inq) in
            self_handling_sparse_states.iter_mut()
            .zip(self_handling_sparse_states0.into_iter())
        {
            let inq2 = inq.map(|tran_ix| clone_tran(&targetmap[tran_ix.0]));
            outq.write(inq2);
        }

        for (outq, inq) in
            self_handling_dense_states.iter_mut()
            .zip(self_handling_dense_states0.into_iter())
        {
            let inq2 = inq.map(|tran_ix| clone_tran(&targetmap[tran_ix.0]));
            outq.write(inq2);
        }

        AutHolder {
            normal_states:
                transmute::<_, Vec<_>>(normal_states).into_boxed_slice(),
            self_handling_sparse_states:
                transmute::<_, Vec<_>>(self_handling_sparse_states).into_boxed_slice(),
            self_handling_dense_states:
                transmute::<_, Vec<_>>(self_handling_dense_states).into_boxed_slice(),
            shared_trans: shared_trans.into_boxed_slice(),
        }
    };

    let mut keyval_state = outsim::KeyValState::new();

    for ext in init.exts {
        match ext {
            inp::Ext::GetOld(old) => { keyval_state.old_queries.insert(old); },
            inp::Ext::Ext(ext) => keyval_state.result.push(ext),
        }
    }

    let simulator = unsafe {
        let init_states = init.states.iter().map(|state| {
            match parseaut_statemap[state.0] {
                OutStateIx::Normal(ix) => out::AnyStateLock::Normal(
                    refer(&holder.normal_states[ix])),
                OutStateIx::SelfHandlingSparse(ix) => out::AnyStateLock::Sparse(
                    refer(&holder.self_handling_sparse_states[ix])),
                OutStateIx::SelfHandlingDense(ix) => out::AnyStateLock::Dense(
                    refer(&holder.self_handling_dense_states[ix])),
            }
        });

        outsim::KeyValSimulator::new(init_states)
    };
    (holder, simulator, keyval_state)
}


#[cfg(test)]
mod tests {
    use hashbrown::HashSet;
    use serde_json::Value;

    use super::*;

    fn jsonstr_set(s: Vec<Value>) -> HashSet<String> {
        s.into_iter().map(|x| x.as_str().unwrap().to_string()).collect()
    }

    fn str_set(s: Vec<&str>) -> HashSet<String> {
        s.into_iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn complex() {
        // read and parse file tests/config.json
        let config: Vec<inp::Cmd> = serde_json::from_str(r#"[
            {
                "when": {
                    "foo": "bar",
                    "qux": "a.*"
                },
                "run": [ "match1" ]
            },
            {
                "when": { "foo": "baz" },
                "run": [ "match2" ],
                "then": [
                    {
                        "when": { "qux": "a.*" },
                        "run": [ "match3" ]
                    },
                    {
                        "when": { "qux": "ahoy" },
                        "run": [ "match4" ]
                    }
                ]
            }
        ]"#).unwrap();

        let (parser, init) = inp::Parser::parse(config);
        let (_holder, mut simulator, keyval_state) =
            parser_to_simulator::<30, 4, 15>(parser, init);

        let exts = simulator.finish_read(keyval_state, |_| Some("baz"));
        assert_eq!(jsonstr_set(exts), str_set(vec!["match2"]));

        let exts = simulator.read("qux".to_string(), "ahoy", |_| Some("baz"));
        assert_eq!(jsonstr_set(exts), str_set(vec!["match3", "match4"]));

        let exts = simulator.read("foo".to_string(), "bar", |_| Some("baz"));
        assert_eq!(jsonstr_set(exts), str_set(vec![]));

        // qux will not be checked twice because it is the last item in the match.
        let exts = simulator.read("qux".to_string(), "ahoy", |_| Some("bar"));
        assert_eq!(jsonstr_set(exts), str_set(vec!["match1"]));
    }

    #[test]
    fn simple() {
        // read and parse file tests/config.json
        let config: Vec<inp::Cmd> = serde_json::from_str(r#"[
            { 
                "when": {
                    "foo": "a",
                    "bar": "b"
                },
                "run": [ "you win" ]
            }
        ]"#).unwrap();

        let (parser, init) = inp::Parser::parse(config);
        let (_holder, mut simulator, keyval_state) =
            parser_to_simulator::<30, 4, 15>(parser, init);

        let exts = simulator.finish_read(keyval_state, |_| None);
        assert_eq!(exts, Vec::<Value>::new());

        let exts = simulator.read("bar".to_string(), "b", |_| Some("baz"));
        assert_eq!(jsonstr_set(exts), str_set(vec![]));

        // foo will be checked twice because it is not the last item in the match.
        let exts = simulator.read("foo".to_string(), "a", |x| match x {
            "bar" => Some("b"), "foo" => Some("a"), _ => None});
        assert_eq!(jsonstr_set(exts), str_set(vec!["you win"]));
    }
}
