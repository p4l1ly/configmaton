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

fn convert_to_normal_state(state: &inp::State) -> OutAnyState {
    let mut explicit_trans = vec![];
    let mut pattern_trans = vec![];

    for (guard, target) in state.transitions.iter() {
        match guard {
            inp::Guard::Var(key) =>
                explicit_trans.push((out::Explicit::Var(key.clone()), *target)),
            inp::Guard::EndVar =>
                explicit_trans.push((out::Explicit::EndVar, *target)),
            inp::Guard::Guard(guard) =>
                pattern_trans.push((guard.clone(), *target)),
        }
    }

    if explicit_trans.len() + pattern_trans.len() < 15 {
        OutAnyState::Normal(out::State{
            explicit_trans: explicit_trans.into_boxed_slice(),
            pattern_trans: pattern_trans.into_boxed_slice(),
        })
    } else if pattern_trans.len() < 10 && explicit_trans.len() < 50 {
        OutAnyState::SelfHandlingSparse(out::SelfHandlingSparseState{
            explicit_trans: explicit_trans.into_iter().collect(),
            pattern_trans: pattern_trans.into_boxed_slice(),
        })
    } else {
        OutAnyState::SelfHandlingDense(out::SelfHandlingDenseState{
            char_trans: unimplemented!(),
            var_trans: unimplemented!(),
            endvar_tran: unimplemented!(),
        })
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

pub fn parser_to_simulator<'a>
    (parsaut: inp::Parser, init: inp::Target)
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

        match convert_to_normal_state(state) {
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
    use serde_json::Value;

    use super::*;

    #[test]
    fn complex() {
        // read and parse file tests/config.json
        let config: Vec<inp::Cmd> = serde_json::from_str(r#"[
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

        let (parser, init) = inp::Parser::parse(config);
        let (_holder, mut simulator, keyval_state) = parser_to_simulator(parser, init);

        let exts = simulator.finish_read(keyval_state, |_| Some("baz"));
        assert_eq!(exts, vec![serde_json::json!({"set": {"match2":"passed"}})]);
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
        let (_holder, mut simulator, keyval_state) = parser_to_simulator(parser, init);

        let exts = simulator.finish_read(keyval_state, |_| Some("baz"));
        assert_eq!(exts, Vec::<Value>::new());
    }
}
