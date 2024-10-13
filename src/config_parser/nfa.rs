use std::collections::HashSet;

use super::ast::Ast;
use super::guards::Monoid;

#[derive(Debug)]
pub struct NfaState {
    pub transitions: Vec<((u8, u8), usize)>,
    pub epsilon_transitions: Vec<usize>
}

impl NfaState {
    fn new() -> Self {
        Self { transitions: Vec::new(), epsilon_transitions: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Cfg(pub Vec<usize>, pub bool);

impl Monoid for Cfg {
    fn empty() -> Self {
        Cfg(vec![], false)
    }

    fn append(&mut self, other: Self) {
        // union of sorted lists
        let mut i = 0;
        let mut j = 0;
        let mut result = vec![];
        while i < self.0.len() && j < other.0.len() {
            if self.0[i] < other.0[j] {
                result.push(self.0[i]);
                i += 1;
            } else {
                result.push(other.0[j]);
                j += 1;
            }
        }
        result.extend(self.0[i..].iter().cloned());
        result.extend(other.0[j..].iter().cloned());
        self.0 = result;
        self.1 |= other.1;
    }
}

#[derive(Debug)]
pub struct Nfa {
    pub states: Vec<NfaState>,
}

impl Nfa {
    pub fn from_ast(ast: Ast) -> Self {
        let mut automaton = Self {
            states: Vec::new(),
        };
        automaton.states.push(NfaState::new());
        automaton.states.push(NfaState::new());
        automaton.recur_ast(ast, 0, 1);
        automaton
    }

    fn recur_ast(&mut self, ast: Ast, qpre: usize, qsuc: usize) {
        match ast {
            Ast::Alternation(left, right) => {
                self.recur_ast(*left, qpre, qsuc);
                self.recur_ast(*right, qpre, qsuc);
            }
            Ast::Range(from, to) => {
                self.states[qpre].transitions.push(((from, to), qsuc));
            }
            Ast::Concatenation(left, right) => {
                let qmid = self.states.len();
                self.states.push(NfaState::new());
                self.recur_ast(*left, qpre, qmid);
                self.recur_ast(*right, qmid, qsuc);
            }
            Ast::Repetition(body) => {
                self.states[qpre].epsilon_transitions.push(qsuc);
                self.recur_ast(*body, qpre, qpre);
            }
            Ast::Epsilon => {
                self.states[qpre].epsilon_transitions.push(qsuc);
            }
        }
    }

    fn add_inherited(&self, q: usize, configuration: &mut HashSet<usize>) {
        if !configuration.insert(q) {
            return;
        }

        for parent in self.states[q].epsilon_transitions.iter() {
            self.add_inherited(*parent, configuration);
        }
    }

    pub fn expand_config(&self, config0: Vec<usize>) -> Cfg {
        let mut configuration = HashSet::new();
        for state in config0 {
            self.add_inherited(state, &mut configuration);
        }
        let is_final = configuration.contains(&1);

        // remove useless states that only inherit.
        configuration.retain(|x| !self.states[*x].transitions.is_empty());

        let mut configuration2: Vec<usize> = configuration.into_iter().collect();
        configuration2.sort();
        Cfg(configuration2, is_final)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ast::parse_regex;

    #[test]
    fn test_nfa() {
        let ast = parse_regex("a([bA-D]|[cB-C])*d");
        let nfa = Nfa::from_ast(ast);
        assert_eq!(nfa.states.len(), 4);
        assert_eq!(nfa.states[1].transitions, vec![]);
        assert_eq!(nfa.states[0].transitions, vec![((b'a', b'a'), 3)]);
        assert_eq!(
            nfa.states[3].transitions,
            vec![((b'b', b'b'), 3), ((b'A', b'D'), 3), ((b'c', b'c'), 3), ((b'B', b'C'), 3)]
        );
        assert_eq!(nfa.states[3].epsilon_transitions, vec![2]);
        assert_eq!(nfa.states[2].transitions, vec![((b'd', b'd'), 1)]);

        assert_eq!(nfa.expand_config(vec![0]), Cfg(vec![0], false));
        assert_eq!(nfa.expand_config(vec![1]), Cfg(vec![], true));
        assert_eq!(nfa.expand_config(vec![2]), Cfg(vec![2], false));
        assert_eq!(nfa.expand_config(vec![3]), Cfg(vec![2, 3], false));
    }
}
