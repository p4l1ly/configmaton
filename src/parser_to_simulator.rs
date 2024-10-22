use std::mem::{MaybeUninit, transmute};

use crate::config_parser as inp;
use crate::config_parser::TargetIx;
use crate::automaton as out;
use crate::keyval_simulator as outsim;
use crate::lock::{LockSuper, LockSelector, Lock as _};

type TT = outsim::Triggers;
type Lock<S, X> = <S as LockSelector>::Lock<X>;
type StateLock<S> = Lock<S, out::State<out::Tran<S, TT>>>;
type SelfHandlingSparseStateLock<S> = Lock<S, out::SelfHandlingSparseState<out::Tran<S, TT>>>;
type SelfHandlingDenseStateLock<S> = Lock<S, out::SelfHandlingDenseState<out::Tran<S, TT>>>;
type TranBodyLock<S> = Lock<S, out::TranBody<S, TT>>;

type Holder<S, X> = <Lock<S, X> as LockSuper>::Holder;
type StateHolder<S> = Holder<S, out::State<out::Tran<S, TT>>>;
type SelfHandlingSparseStateHolder<S> = Holder<S, out::SelfHandlingSparseState<out::Tran<S, TT>>>;
type SelfHandlingDenseStateHolder<S> = Holder<S, out::SelfHandlingDenseState<out::Tran<S, TT>>>;
type TranBodyHolder<S> = Holder<S, out::TranBody<S, TT>>;


pub struct AutHolder<S: LockSelector> {
    pub normal_states: Box<[StateHolder<S>]>,
    pub self_handling_sparse_states: Box<[SelfHandlingSparseStateHolder<S>]>,
    pub self_handling_dense_states: Box<[SelfHandlingDenseStateHolder<S>]>,
    pub shared_trans: Box<[TranBodyHolder<S>]>,
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

pub fn worth_sharing_target(target: &inp::Target, refcount: usize) -> bool {
    refcount >= 2 && (!target.exts.is_empty() || target.states.len() > 2)
}

pub fn vec_with_fn<T>(f: impl Fn() -> T, len: usize) -> Vec<T> {
    let mut v = Vec::with_capacity(len);
    for _ in 0..len {
        v.push(f());
    }
    v
}


pub fn uninit_vec<X, Y>(v: &Vec<X>) -> Vec<MaybeUninit<Y>> {
    vec_with_fn(MaybeUninit::uninit, v.len())
}

pub fn uninit_ptr<X>(x: &mut MaybeUninit<X>) -> *mut X {
    x.as_mut_ptr()
}

pub fn clone_tran<S: LockSelector>(tran: &out::Tran<S, TT>) -> out::Tran<S, TT> {
    match tran {
        out::Tran::Owned(out::TranBody{ right_states, tran_trigger }) =>
            out::Tran::Owned(out::TranBody{
                right_states: right_states.clone(),
                tran_trigger: tran_trigger.clone(),
            }),
        out::Tran::Shared(body) => out::Tran::Shared(body.clone()),
    }
}

pub fn parser_to_simulator<S: LockSelector>
    (parsaut: inp::Parser, init: inp::Target)
    -> (AutHolder<S>, outsim::KeyValSimulator<S>, outsim::KeyValState)
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

    let mut holder = unsafe {
        let mut normal_states: Vec<MaybeUninit<StateHolder<S>>> =
            uninit_vec(&normal_states0);
        let mut self_handling_sparse_states: Vec<MaybeUninit<SelfHandlingSparseStateHolder<S>>> =
            uninit_vec(&self_handling_sparse_states0);
        let mut self_handling_dense_states: Vec<MaybeUninit<SelfHandlingDenseStateHolder<S>>> =
            uninit_vec(&self_handling_dense_states0);

        let mut shared_trans: Vec<TranBodyHolder<S>> = vec![];
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
                                    <StateLock<S>>::refer(&mut *normal_states[ix].as_mut_ptr())),
                                OutStateIx::SelfHandlingSparse(ix) => out::AnyStateLock::Sparse(
                                    <SelfHandlingSparseStateLock<S>>::refer(
                                        &mut *self_handling_sparse_states[ix].as_mut_ptr())),
                                OutStateIx::SelfHandlingDense(ix) => out::AnyStateLock::Dense(
                                    <SelfHandlingDenseStateLock<S>>::refer(
                                        &mut *self_handling_dense_states[ix].as_mut_ptr())),
                            }
                        });
                        out::Succ(
                            match any_state_locks.next() { Some(x) => x, None => out::AnyStateLock::None }, 
                            any_state_locks.collect::<Vec<_>>().into_boxed_slice(),
                        )
                    }
                };
                if worth_sharing_target(target, refcount) {
                    let ix = shared_trans.len();
                    shared_trans.push(<TranBodyLock<S>>::new(tranbody));
                    out::Tran::Shared(<TranBodyLock<S>>::refer(&mut shared_trans[ix]))
                } else {
                    out::Tran::Owned(tranbody)
                }
            }
        ).collect::<Vec<_>>();

        for (outq, inq) in normal_states.iter_mut().zip(normal_states0.into_iter()) {
            outq.write(<StateLock<S>>::new(inq.map(|tran_ix| clone_tran(&targetmap[tran_ix.0]))));
        }

        for (outq, inq) in
            self_handling_sparse_states.iter_mut()
            .zip(self_handling_sparse_states0.into_iter())
        {
            outq.write(<SelfHandlingSparseStateLock<S>>::new(
                inq.map(|tran_ix| clone_tran(&targetmap[tran_ix.0]))));
        }

        for (outq, inq) in
            self_handling_dense_states.iter_mut()
            .zip(self_handling_dense_states0.into_iter())
        {
            outq.write(<SelfHandlingDenseStateLock<S>>::new(
                inq.map(|tran_ix| clone_tran(&targetmap[tran_ix.0]))));
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

    let init_states = init.states.iter().map(|state| {
        match parseaut_statemap[state.0] {
            OutStateIx::Normal(ix) => out::AnyStateLock::Normal(
                <StateLock<S>>::refer(&mut holder.normal_states[ix])),
            OutStateIx::SelfHandlingSparse(ix) => out::AnyStateLock::Sparse(
                <SelfHandlingSparseStateLock<S>>::refer(&mut holder.self_handling_sparse_states[ix])),
            OutStateIx::SelfHandlingDense(ix) => out::AnyStateLock::Dense(
                <SelfHandlingDenseStateLock<S>>::refer(&mut holder.self_handling_dense_states[ix])),
        }
    });

    let simulator = outsim::KeyValSimulator::new(init_states);
    (holder, simulator, keyval_state)
}


#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::lock::RcRefCellSelector;

    #[test]
    fn config_to_automaton_complex() {
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
        let (_holder, mut simulator, keyval_state) = parser_to_simulator::<RcRefCellSelector>(parser, init);

        // let exts = simulator.finish_read(keyval_state, |_| Some("baz"));
        // assert_eq!(exts, Vec::<Value>::new());
    }

    #[test]
    fn config_to_automaton_simple() {
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

        // The output automaton is for now only for visual checking.
        let file = std::fs::File::create("/tmp/test_simple.dot").unwrap();
        parser.to_dot(init, std::io::BufWriter::new(file));
    }
}
