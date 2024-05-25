mod config_extractor;

use crate::config_extractor::config_extractor::extract_config;
use clap::{Arg, Command};

fn cli() -> Command {
    Command::new("API tester")
        .about("Testing API stuff (and args parsing + proto, really)")
        //.bin_name("api_tester")
        .arg_required_else_help(true)
        .arg(
            //arg!(--"config" "c" <PATH>)
            Arg::new("path")
                .short('c')
                .long("config")
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .help("Path to a JSON config file with API codes"),
        )
}

fn main() {
    let matches = cli().get_matches();
    let config = extract_config(&matches).expect("Error reading config");
    println!("Config loaded: {:#?}", config);
}
