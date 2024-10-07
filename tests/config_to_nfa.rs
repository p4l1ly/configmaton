use regex_syntax::ast;
use std::fs::File;
use std::io::BufReader;
use serde_json;
use serde_json::Value;
use configmaton::automaton::{Explicit, Pattern, NumCondition};

#[derive(Debug)]
enum Ext {
    Query(String),
    Ext(Value),
}

#[derive(Debug)]
struct Target {
    exts: Vec<Ext>,
    states: Vec<usize>,
}

struct State {
    explicit_transitions: Vec<(Explicit, usize)>,
    pattern_transitions: Vec<(Pattern, usize)>,
}

struct Parser {
    states: Vec<State>,
    targets: Vec<Target>,
}

impl Parser {
    fn parse(cmd: Value) -> (Self, Target) {
        let mut parser = Parser {
            states: vec![],
            targets: vec![],
        };
        let init = parser.parse_cmd(cmd);
        (parser, init)
    }

    fn parse_match(
        &mut self,
        guards: Vec<Value>,
        exts: Vec<Value>,
        mut then: Target
    ) -> (usize, String) {
        then.exts.extend(exts.into_iter().map(|ext| Ext::Ext(ext)));

        let keyvals: Vec<_> = guards.into_iter().map(parse_guard).collect();

        unimplemented!()
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
                                let (state, query) = self.parse_match(guards, exts, nexts);
                                Target{ exts: vec![Ext::Query(query)], states: vec![state] }
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

enum Ast {
    Pattern(Pattern),
    Explicit(Explicit),
    Alternation(Box<Ast>, Box<Ast>),
    Concatenation(Box<Ast>, Box<Ast>),
    Repetition(Box<Ast>),
}

fn parse_ext_class_set_item(item: &ast::ClassSetItem) -> Ast {
    match item {
        ast::ClassSetItem::Range(range) => {
            Ast::Pattern(Pattern::CharRange(range.start.c, range.end.c))
        },
        ast::ClassSetItem::Literal(c) => {
            Ast::Explicit(Explicit::Char(c.c))
        },
        ast::ClassSetItem::Union(union) => {
            let mut result = parse_ext_class_set_item(&union.items[0]);
            for child in union.items[1..].iter() {
                result = Ast::Alternation(Box::new(result), Box::new(parse_ext_class_set_item(child)));
            }
            result
        },
        _ => {
            panic!("invalid regex command");
        }
    }
}

fn parse_ext_ast(ext: &ast::Ast) -> Ast {
    match ext {
        ast::Ast::Literal(lit) => { Ast::Explicit(Explicit::Char(lit.c)) },
        ast::Ast::Concat(x) => {
            let mut result = parse_ext_ast(&x.asts[0]);
            for child in x.asts[1..].iter() {
                result = Ast::Concatenation(Box::new(result), Box::new(parse_ext_ast(child)));
            }
            result
        },
        ast::Ast::Alternation(x) => {
            let mut result = parse_ext_ast(&x.asts[0]);
            for child in x.asts[1..].iter() {
                result = Ast::Alternation(Box::new(result), Box::new(parse_ext_ast(child)));
            }
            result
        },
        ast::Ast::Repetition(a) => {
            Ast::Repetition(Box::new(parse_ext_ast(&a.ast)))
        },
        ast::Ast::Group(a) => {
            parse_ext_ast(&a.ast)
        },
        ast::Ast::ClassBracketed(x) => {
            if x.negated {
                panic!("negated class bracketed not supported");
            }
            match &x.kind {
                ast::ClassSet::Item(item) => {
                    parse_ext_class_set_item(item)
                },
                _ => {
                    panic!("invalid regex command");
                }
            }
        }

        _ => {
            panic!("invalid regex command");
        }
    }
}

fn parse_regex_string(regex: String) -> Ast {
    parse_ext_ast(&ast::parse::Parser::new().parse(r"ab(.[e-z])*|c").unwrap())
}

fn parse_array_regex(mut regex: Vec<Value>) -> Ast {
    match regex[0].as_str() {
        Some("Null") => { Ast::Explicit(Explicit::Null) },
        Some("Bool0") => { Ast::Explicit(Explicit::Bool(false)) },
        Some("Bool1") => { Ast::Explicit(Explicit::Bool(true)) },
        Some("[") => { Ast::Explicit(Explicit::ListStart) },
        Some("{") => { Ast::Explicit(Explicit::DictStart) },
        Some(";") => { Ast::Explicit(Explicit::End) },
        Some("<") => {
            match regex.pop().unwrap() {
                Value::Number(n) => {
                    Ast::Pattern(Pattern::NumCondition(NumCondition::Less(n)))
                },
                _ => { panic!("expected number"); }
            }
        },
        Some(">") => {
            match regex.pop().unwrap() {
                Value::Number(n) => {
                    Ast::Pattern(Pattern::NumCondition(NumCondition::Greater(n)))
                },
                _ => { panic!("expected number"); }
            }
        },
        Some("==") => {
            match regex.pop().unwrap() {
                Value::Number(n) => {
                    Ast::Pattern(Pattern::NumCondition(NumCondition::Eq(n)))
                },
                _ => { panic!("expected number"); }
            }
        },
        Some("!=") => {
            match regex.pop().unwrap() {
                Value::Number(n) => {
                    Ast::Pattern(Pattern::NumCondition(NumCondition::NotEq(n)))
                },
                _ => { panic!("expected number"); }
            }
        },
        Some("<=") => {
            match regex.pop().unwrap() {
                Value::Number(n) => {
                    Ast::Pattern(Pattern::NumCondition(NumCondition::LessEq(n)))
                },
                _ => { panic!("expected number"); }
            }
        },
        Some(">=") => {
            match regex.pop().unwrap() {
                Value::Number(n) => {
                    Ast::Pattern(Pattern::NumCondition(NumCondition::GreaterEq(n)))
                },
                _ => { panic!("expected number"); }
            }
        },
        Some("In") => {
            match (regex.pop().unwrap(), regex.pop().unwrap()) {
                (Value::Number(end), Value::Number(start)) => {
                    Ast::Pattern(Pattern::NumCondition(NumCondition::In(start, end)))
                },
                _ => { panic!("expected number"); }
            }
        },
        Some("NotIn") => {
            match (regex.pop().unwrap(), regex.pop().unwrap()) {
                (Value::Number(end), Value::Number(start)) => {
                    Ast::Pattern(Pattern::NumCondition(NumCondition::NotIn(start, end)))
                },
                _ => { panic!("expected number"); }
            }
        },
        Some("or") => {
            let mut result = parse_regex(regex.pop().unwrap());
            for r in regex.drain(1..).rev() {
                result = Ast::Alternation(Box::new(parse_regex(r)), Box::new(result));
            }
            result
        },
        Some("concat") => {
            let mut result = parse_regex(regex.pop().unwrap());
            for r in regex.drain(1..).rev() {
                result = Ast::Concatenation(Box::new(parse_regex(r)), Box::new(result));
            }
            result
        },
        Some("repeat") => {
            Ast::Repetition(Box::new(parse_regex(regex.pop().unwrap())))
        },
        _ => {
            panic!("invalid regex command");
        }
    }
}

fn parse_regex(regex: Value) -> Ast {
    match regex {
        Value::String(regex) => {
            Ast::Concatenation(
                Box::new(Ast::Explicit(Explicit::StringStart)),
                Box::new(Ast::Concatenation(
                    Box::new(parse_regex_string(regex)),
                    Box::new(Ast::Explicit(Explicit::End)),
                )),
            )
        },
        Value::Array(regex) => {
            parse_array_regex(regex)
        }
        _ => {
            panic!("expected string");
        }
    }
}

fn parse_guard(guard: Value) -> (String, Ast) {
    match guard {
        Value::Array(mut guards) => {
            assert!(guards.len() == 2);
            let value = guards.pop().unwrap();
            let key = guards.pop().unwrap();
            match key {
                Value::String(key) => {
                    (key, parse_regex(value))
                },
                _ => {
                    panic!("expected string");
                }
            }
        },
        _ => {
            panic!("expected array");
        }
    }
}


#[test]
fn config_to_nfa() {
    // read and parse file tests/config.json
    dbg!(ast::parse::Parser::new().parse(r"ab(.[e-z0])*|c").unwrap());

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
