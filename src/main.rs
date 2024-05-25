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

    // TODO:
    // - migrate to actual project
    //   - add gRPC layer here
    //   - move everything to a new project
    //   - find a way to have led matrix only for the Rpi client
    //   - make another client that just prints the proto on hash change
    // - play with google_calendar crate to read stuff
    // - implement kitty parser
    //   - Kitty URL: https://www.kittysplit.com/number-three/NjCvUvs50prTrXsKaY352sJ9amQppQbm-2?view_as_creator=true
    //   - See kitty_manager::update_debts in \\unraid.home\backups\Programming\led-panel\led-panel\display_content_managers.cpp
    // - Query stop info, see https://opentransportdata.swiss/en/cookbook/open-journey-planner-ojp/
    //   - Timonet ID: 8588845
    //   - Get enough results that we have next to Flon, and next to Renens (could be 32 or 54)
    //   - Check out how to POST, and parse XML
}
