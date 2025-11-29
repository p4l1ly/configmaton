use std::{fs::File, io::Read};

use clap;
use clap::Parser;
use configmaton::{
    blob::{keyval_state::LeafOrigin, state::build::U8BuildConfig},
    keyval_nfa::{Cmd, Msg, Parser as AutParser},
};

#[derive(Parser)]
struct Args {
    #[clap(short, long)]
    output: Option<String>,

    #[clap(long)]
    dot: Option<String>,
}

pub struct BuildConfig;
impl U8BuildConfig for BuildConfig {
    fn guard_size_keep(&self) -> u32 {
        10
    }
    fn hashmap_cap_power_fn(&self, _len: usize) -> usize {
        3
    }
    fn dense_guard_count(&self) -> usize {
        15
    }
}

pub fn json_to_automaton_matchrun(
    json: &str,
) -> Result<(Msg, AutParser, LeafOrigin), serde_json::Error> {
    let config: Vec<Cmd> = serde_json::from_str(json)?;
    let (parser, init) = AutParser::parse(config);
    let msg = Msg::serialize(&parser, &init, &BuildConfig);
    Ok((msg, parser, init))
}

fn main() {
    // take stdin, run json_to_automaton_matchrun, and, depending on arguments, store the msg
    // or AutParser::to_dot

    let args = Args::parse();
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).unwrap();
    let (msg, parser, init) = json_to_automaton_matchrun(&buf).unwrap();

    if let Some(output) = args.output {
        let slice = unsafe { std::slice::from_raw_parts(msg.data, msg.data_len()) };
        std::fs::write(output, slice).unwrap();
    }

    if let Some(dot) = args.dot {
        let file = File::create(dot).unwrap();
        parser.to_dot(&init, file);
    }
}
