mod config_extractor;
mod dummy_client;

use crate::config_extractor::{cli, extract_config};
use crate::dummy_client::maybe_dummy_client;
use log::info;
use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::config::{Appender, Config, Root};

fn logging_setup() -> () {
    let stdout = ConsoleAppender::builder().build();
    let log_config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .build(Root::builder().appender("stdout").build(LevelFilter::Info))
        .unwrap();
    log4rs::init_config(log_config).unwrap();
    info!("Client started")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging_setup();

    let matches = cli().get_matches();
    let api_config = extract_config(&matches).expect("Error reading config");

    maybe_dummy_client(true, &api_config).await?;
    Ok(())
}
