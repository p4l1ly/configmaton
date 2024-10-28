use regex_syntax::ast;

#[derive(Debug, PartialEq)]
pub enum Ast {
    Range(u8, u8),
    Alternation(Box<Ast>, Box<Ast>),
    Concatenation(Box<Ast>, Box<Ast>),
    Repetition(Box<Ast>),
    Epsilon,
}

pub fn parse_regex(regex: &str) -> Ast {
    let ast = ast::parse::Parser::new().parse(regex).unwrap();
    parse_ext_ast(&ast)
}

fn parse_ext_ast(ext: &ast::Ast) -> Ast {
    match ext {
        ast::Ast::Literal(lit) => { let c = lit.c as u8; Ast::Range(c, c) },
        ast::Ast::Dot(_) => { Ast::Range(0, 255) },
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
        ast::Ast::Empty(_) => Ast::Epsilon,
        _ => {
            panic!("invalid regex command {}", ext);
        }
    }
}

fn parse_ext_class_set_item(item: &ast::ClassSetItem) -> Ast {
    match item {
        ast::ClassSetItem::Range(range) => {
            Ast::Range(range.start.c as u8, range.end.c as u8)
        },
        ast::ClassSetItem::Literal(c) => {
            let c = c.c as u8;
            Ast::Range(c, c)
        },
        ast::ClassSetItem::Union(union) => {
            let mut result = parse_ext_class_set_item(&union.items[0]);
            for child in union.items[1..].iter() {
                result = Ast::Alternation(
                    Box::new(result),
                    Box::new(parse_ext_class_set_item(child))
                );
            }
            result
        },
        _ => {
            panic!("invalid regex command");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_regex() {
        let ast = parse_regex("a");
        assert_eq!(ast, Ast::Range(b'a', b'a'));

        let ast = parse_regex("ab");
        assert_eq!(ast, Ast::Concatenation(
            Box::new(Ast::Range(b'a', b'a')),
            Box::new(Ast::Range(b'b', b'b'))
        ));

        let ast = parse_regex("(d|ef)(g|([a-dA-D]b)*|bc)");
        assert_eq!(ast, Ast::Concatenation(
            Box::new(Ast::Alternation(
                Box::new(Ast::Range(b'd', b'd')),
                Box::new(Ast::Concatenation(
                    Box::new(Ast::Range(b'e', b'e')),
                    Box::new(Ast::Range(b'f', b'f'))
                )),
            )),
            Box::new(Ast::Alternation(
                Box::new(Ast::Alternation(
                    Box::new(Ast::Range(b'g', b'g')),
                    Box::new(Ast::Repetition(
                        Box::new(Ast::Concatenation(
                            Box::new(Ast::Alternation(
                                Box::new(Ast::Range(b'a', b'd')),
                                Box::new(Ast::Range(b'A', b'D')),
                            )),
                            Box::new(Ast::Range(b'b', b'b')),
                        )),
                    )),
                )),
                Box::new(Ast::Concatenation(
                    Box::new(Ast::Range(b'b', b'b')),
                    Box::new(Ast::Range(b'c', b'c')),
                )),
            )),
        ));

        let ast = parse_regex("");
        assert_eq!(ast, Ast::Epsilon);
    }
}
