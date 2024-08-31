mod config_extractor;
mod data_updater;
mod dummy_client;
mod gcal_updater;
mod kitty_updater;
mod my_screen_service;
mod transport_updater;

use log::debug;
use screen_service::screen_service_server::ScreenServiceServer;
use tonic::transport::Server;

pub mod screen_service {
    tonic::include_proto!("screen_service"); // The string specified here must match the proto package name
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = config_extractor::cli().get_matches();
    config_extractor::init_logging(&matches).expect("Error setting up logging");
    let config = config_extractor::extract_config(&matches).expect("Error reading config");
    debug!("Config loaded: {:#?}", config);

    // Start a one-shot dummy client if we got the cli flag
    if matches.get_flag("dummy_client") {
        crate::dummy_client::start(crate::dummy_client::ClientMode::OneShot, &config);
    }

    // Create the service, and tell it to start the content updates
    let mut screen_service = my_screen_service::MyScreenService::new(&config);
    screen_service.start_backgound_updates();

    // Start the actual serving, always from localhost ('[::1]' or '127.0.0.1' or '0.0.0.0')
    // (The address in the config is for clients)
    let server_config = config.server.as_ref().expect("No server config found");
    let address = format!("0.0.0.0:{}", server_config.port)
        .parse()
        .expect("Couldn't parse the config port to an address");
    Server::builder()
        .add_service(ScreenServiceServer::new(screen_service))
        .serve(address)
        .await
        .expect("Error while starting or executing the server");

    Ok(())
}
