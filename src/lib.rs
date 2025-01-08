use blob::automaton::Automaton;
use keyval_simulator::Simulation;
use onion::{Locker, Onion};

pub mod lock;
pub mod command;
pub mod commands;
pub mod keyval_runner;
pub mod keyval_simulator;
pub mod guards;
pub mod char_nfa;
pub mod char_enfa;
pub mod ast;
pub mod keyval_nfa;
pub mod char_runner;
pub mod borrow_lock;
pub mod blob;
pub mod holder;
pub mod onion;

pub struct Configmaton<'a, L: Locker> {
    onion: Onion<'a, L, Self>,
    simulation: Simulation<'a>,
}

impl<'a, L: Locker> Configmaton<'a, L> {
    pub fn new(automaton: &Automaton<'a>) -> Self {
        Configmaton {
            onion: Onion::new(),
            simulation: Simulation::new(automaton, |_| None),
        }
    }

    pub fn make_child(&mut self) -> *mut Self {
        self.onion.make_child(|onion| Configmaton {
            onion,
            simulation: self.simulation.clone(),
        })
    }

    pub fn set(&mut self, key: &'a [u8], value: &'a [u8]) {
        self.onion.set(key, value);
        self.simulation.read(key, value, |key| { self.onion.get(key) });

        for child in self.onion.iter_children() {
            let child = unsafe {&mut *child};
            child.simulation.read(key, value, |key| { child.onion.get(key) });
        }
    }

    pub fn get(&self, key: &[u8]) {
        self.onion.get(key);
    }

    pub fn pop_command(&mut self) -> Option<&'a [u8]> {
        self.simulation.exts.pop()
    }

    pub fn handle_commands<F: FnMut(&mut Self, &'a [u8])>(&mut self, f: &mut F) {
        while let Some(command) = self.pop_command() {
            f(self, command);
        }
    }

    pub fn set_and_handle<F: FnMut(&mut Self, &'a [u8])>
        (&mut self, key: &'a [u8], value: &'a [u8], f: &mut F)
    {
        self.set(key, value);
        self.handle_commands(f);

        for child in self.onion.iter_children() {
            let child = unsafe {&mut *child};
            child.handle_commands(f);
        }
    }

    pub fn clear_children(&mut self) {
        self.onion.clear_children();
    }
}

#[cfg(test)]
mod tests {
    use blob::tests::TestU8BuildConfig;
    use keyval_nfa::{Cmd, Msg, Parser};
    use onion::ThreadUnsafeLocker;

    use super::*;

    macro_rules! handle {
        ($cmds:expr, $react:expr) => {
            |configmaton: &mut Configmaton<ThreadUnsafeLocker>, command: &[u8]| {
                $cmds.push(command);
                match command {
                    b"m2" => {
                        configmaton.set(b"qux", $react);
                    },
                    _ => {},
                }
            }
        };
    }

    #[test]
    fn it_works() {
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
        let file = std::fs::File::create("/tmp/test_configmaton.dot").unwrap();
        parser.to_dot(&init, std::io::BufWriter::new(file));

        let outmsg = Msg::serialize(parser, init, &TestU8BuildConfig);
        let inmsg = unsafe {
            Msg::read(|buf| buf.copy_from(outmsg.data, outmsg.data_len()), outmsg.data_len()) };
        let aut = inmsg.get_automaton();
        let mut cmds: Vec<&[u8]> = Vec::new();
        let mut configmaton = Configmaton::new(aut);

        configmaton.set_and_handle(b"qux", b"no!", &mut handle!(cmds, b"arrgh"));
        assert!(cmds.is_empty());

        {
            let configmaton2 = unsafe { &mut *configmaton.make_child() };
            let configmaton3 = unsafe { &mut *configmaton.make_child() };
            let configmaton4 = unsafe { &mut *configmaton.make_child() };

            configmaton2.set_and_handle(b"foo", b"bar", &mut handle!(cmds, b"arrgh"));
            assert!(cmds.drain(..).collect::<Vec<_>>().is_empty());

            configmaton3.set_and_handle(b"foo", b"baz", &mut handle!(cmds, b"arrgh"));
            assert_eq!(cmds.drain(..).collect::<Vec<_>>(), vec![b"m2", b"m3"]);

            configmaton2.set_and_handle(b"qux", b"ahoy", &mut handle!(cmds, b"arrgh"));
            assert_eq!(cmds.drain(..).collect::<Vec<_>>(), vec![b"m1"]);
            configmaton2.set_and_handle(b"qux", b"ahoy", &mut handle!(cmds, b"arrgh"));
            assert!(cmds.drain(..).collect::<Vec<_>>().is_empty());

            configmaton3.set_and_handle(b"qux", b"arrgh", &mut handle!(cmds, b"arrgh"));
            assert!(cmds.drain(..).collect::<Vec<_>>().is_empty());
            configmaton3.set_and_handle(b"qux", b"ahoy", &mut handle!(cmds, b"arrgh"));
            assert_eq!(cmds.drain(..).collect::<Vec<_>>(), vec![b"m4"]);

            configmaton4.set_and_handle(b"foo", b"baz", &mut handle!(cmds, b""));
            assert_eq!(cmds.drain(..).collect::<Vec<_>>(), vec![b"m2"]);
            configmaton4.set_and_handle(b"qux", b"ahoy", &mut handle!(cmds, b"arrgh"));
            let mut cmds_now = cmds.drain(..).collect::<Vec<_>>();
            cmds_now.sort();
            assert_eq!(cmds_now, vec![b"m3", b"m4"]);
        }

        // The following interface is quite messy and unsafe, without a real use case.
        // Changes made to the parent configmaton should be reflected in the children.
        configmaton.clear_children();

        {
            let configmaton2 = unsafe { &mut *configmaton.make_child() };

            configmaton2.set_and_handle(b"foo", b"bar", &mut handle!(cmds, b"arrgh"));
            assert!(cmds.drain(..).collect::<Vec<_>>().is_empty());

            // This invokes the child's m1, as it already has foo: bar.
            configmaton.set_and_handle(b"qux", b"arrgh", &mut handle!(cmds, b"arrgh"));
            assert_eq!(cmds.drain(..).collect::<Vec<_>>(), vec![b"m1"]);

            // This finally invokes the parent's m1.
            configmaton.set_and_handle(b"foo", b"bar", &mut handle!(cmds, b"arrgh"));
            assert_eq!(cmds.drain(..).collect::<Vec<_>>(), vec![b"m1"]);
        }
    }
}
