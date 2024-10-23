use std::mem::{MaybeUninit, transmute};

use crate::config_parser as inp;
use crate::config_parser::TargetIx;
use crate::automaton as out;
use crate::keyval_simulator as outsim;
use crate::lock::LockSelector;

type TT = outsim::Triggers;
type State<S> = out::State<out::Tran<S, TT>>;
type SelfHandlingSparseState<S> = out::SelfHandlingSparseState<out::Tran<S, TT>>;
type SelfHandlingDenseState<S> = out::SelfHandlingDenseState<out::Tran<S, TT>>;

type UninitHolder<S, X> = <S as LockSelector>::Holder<MaybeUninit<X>>;
type Holder<S, X> = <S as LockSelector>::Holder<X>;
type StateHolder<S> = Holder<S, State<S>>;
type SelfHandlingSparseStateHolder<S> = Holder<S, SelfHandlingSparseState<S>>;
type SelfHandlingDenseStateHolder<S> = Holder<S, SelfHandlingDenseState<S>>;
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

pub fn uninit_vec<S: LockSelector, X, Y>(v: &Vec<X>) -> Vec<UninitHolder<S, Y>> {
    let len = v.len();
    let mut v = Vec::with_capacity(len);
    for _ in 0..len {
        v.push(<S as LockSelector>::new(MaybeUninit::uninit()));
    }
    v
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

    let mut normal_states =
        uninit_vec::<S, _, State<S>>(&normal_states0);

    for outq in normal_states.iter_mut() {
        <S as LockSelector>::borrow_mut_holder(outq);
    }

    let mut self_handling_sparse_states =
        uninit_vec::<S, _, SelfHandlingSparseState<S>>(&self_handling_sparse_states0);
    let mut self_handling_dense_states =
        uninit_vec::<S, _, SelfHandlingDenseState<S>>(&self_handling_dense_states0);
    let mut shared_trans: Vec<TranBodyHolder<S>> = vec![];

    let mut holder = unsafe {
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
                                    <S as LockSelector>::refer_uninit(
                                        &mut normal_states[ix])),
                                OutStateIx::SelfHandlingSparse(ix) => out::AnyStateLock::Sparse(
                                    <S as LockSelector>::refer_uninit(
                                        &mut self_handling_sparse_states[ix])),
                                OutStateIx::SelfHandlingDense(ix) => out::AnyStateLock::Dense(
                                    <S as LockSelector>::refer_uninit(
                                        &mut self_handling_dense_states[ix])),
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
                    shared_trans.push(<S as LockSelector>::new(tranbody));
                    out::Tran::Shared(<S as LockSelector>::refer(&mut shared_trans[ix]))
                } else {
                    out::Tran::Owned(tranbody)
                }
            }
        ).collect::<Vec<_>>();

        for (outq, inq) in normal_states.iter_mut().zip(normal_states0.into_iter()) {
            let inq2 = inq.map(|tran_ix| clone_tran(&targetmap[tran_ix.0]));
            <S as LockSelector>::borrow_mut_holder(outq).write(inq2);
        }

        for (outq, inq) in
            self_handling_sparse_states.iter_mut()
            .zip(self_handling_sparse_states0.into_iter())
        {
            let inq2 = inq.map(|tran_ix| clone_tran(&targetmap[tran_ix.0]));
            <S as LockSelector>::borrow_mut_holder(outq).write(inq2);
        }

        for (outq, inq) in
            self_handling_dense_states.iter_mut()
            .zip(self_handling_dense_states0.into_iter())
        {
            let inq2 = inq.map(|tran_ix| clone_tran(&targetmap[tran_ix.0]));
            <S as LockSelector>::borrow_mut_holder(outq).write(inq2);
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
                <S as LockSelector>::refer(&mut holder.normal_states[ix])),
            OutStateIx::SelfHandlingSparse(ix) => out::AnyStateLock::Sparse(
                <S as LockSelector>::refer(&mut holder.self_handling_sparse_states[ix])),
            OutStateIx::SelfHandlingDense(ix) => out::AnyStateLock::Dense(
                <S as LockSelector>::refer(&mut holder.self_handling_dense_states[ix])),
        }
    });

    let simulator = outsim::KeyValSimulator::new(init_states);
    (holder, simulator, keyval_state)
}


#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::lock::RawPtrSelector;  // WARNING RcRefCellSelector does not work with MaybeUninit.

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
        let (_holder, mut simulator, keyval_state) =
            parser_to_simulator::<RawPtrSelector>(parser, init);

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
        let (_holder, mut simulator, keyval_state) =
            parser_to_simulator::<RawPtrSelector>(parser, init);

        let exts = simulator.finish_read(keyval_state, |_| Some("baz"));
        assert_eq!(exts, Vec::<Value>::new());
    }
}
