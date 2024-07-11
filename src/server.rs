mod config_extractor;
mod my_screen_service;
mod kitty_updater;
mod dummy_client;

use screen_service::screen_service_server::ScreenServiceServer;
use crate::dummy_client::maybe_dummy_client;
use tonic::transport::Server;
use log::info;

pub mod screen_service {
    tonic::include_proto!("screen_service"); // The string specified here must match the proto package name
}

fn logging_setup() -> () {
    log4rs::init_file("log4rs_config.yml", Default::default()).unwrap();
    info!("Server started");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging_setup();

    let matches = config_extractor::cli().get_matches();
    let config = config_extractor::extract_config(&matches).expect("Error reading config");
    info!("Config loaded: {:#?}", config);

    maybe_dummy_client(matches.get_flag("dummy_client"), &config);

    let server_config = config.server.as_ref().expect("No server config found");
    let address = format!("{}:{}", server_config.address, server_config.port).parse()?;

    let screen_service = my_screen_service::MyScreenService::new(&config);
    Server::builder()
        .add_service(ScreenServiceServer::new(screen_service))
        .serve(address)
        .await?;

    Ok(())
}

// TODO:
// - migrate to actual project
//   - move everything to a new project
//   - find a way to have led matrix only for the Rpi client
//   - make another client that just prints the proto on hash change
// - play with google_calendar crate to read stuff
// - Query stop info, see https://opentransportdata.swiss/en/cookbook/open-journey-planner-ojp/
//   - Timonet ID: 8588845
//   - Get enough results that we have next to Flon, and next to Renens (could be 32 or 54)
//   - Check out how to POST, and parse XML
