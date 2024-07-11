mod config_extractor;
mod kitty_updater;

use chrono::Timelike;
use config_extractor::api_config::ApiConfig;
use screen_service::screen_service_server::{ScreenService, ScreenServiceServer};
use screen_service::{ScreenContentReply, ScreenContentRequest};
use tonic::{transport::Server, Request, Response, Status};

pub mod screen_service {
    tonic::include_proto!("screen_service"); // The string specified here must match the proto package name
}

#[derive(Debug, Default)]
pub struct MyScreenService {
    config: config_extractor::api_config::ApiConfig,
}

#[tonic::async_trait]
impl ScreenService for MyScreenService {
    async fn get_screen_content(
        &self,
        request: Request<ScreenContentRequest>, // Accept request of type ScreenContentRequest
    ) -> Result<Response<ScreenContentReply>, Status> {
        // Return an instance of type ScreenContentReply
        println!("Got a request: {:?}", request);

        let now = chrono::offset::Local::now();

        // TODO: move this into a periodic update on another thread
        let debts = match kitty_updater::get_debts(&self.config).await {
            Ok(kitty) => kitty, // TODO: here we'd set our shared copy of the content reply (and reset our error bit)
            Err(_) => vec![], // TODO: and here we set our error bit (then the code will set the reply's bit if any of the bits are set) + some logging
        };

        let reply = ScreenContentReply {
            now: Some(screen_service::Time {
                hours: now.hour(),
                minutes: now.minute(),
            }),
            brightness: 1.0,
            kitty_debts: debts,
            bud_departures: vec![],
            next_upcoming_event: None,
            error: false,
        };

        Ok(Response::new(reply))
    }
}

fn maybe_dummy_client(start_dummy: bool, api_config: &ApiConfig) {
    if start_dummy {
        let config_copy = api_config.clone();
        tokio::spawn(async move {
            println!("Starting dummy client...");
            // No need to sleep here since we'll get CPU time only after the server started.
            let server_config = config_copy.server.expect("No server config found");
            let address: tonic::transport::Endpoint =
                format!("http://{}:{}", server_config.address, server_config.port)
                    .parse()
                    .expect("couldn't parse server config into an address");
            let mut client =
                screen_service::screen_service_client::ScreenServiceClient::connect(address)
                    .await
                    .expect("couldn't start dummy client");
            let request = tonic::Request::new(ScreenContentRequest {});
            let response = client
                .get_screen_content(request)
                .await
                .expect("couldn't get server reply");

            println!("\nResponse to dummy client: {:?}", response);
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = config_extractor::cli().get_matches();
    let config = config_extractor::extract_config(&matches).expect("Error reading config");

    println!("Config loaded: {:#?}", config);

    maybe_dummy_client(matches.get_flag("dummy_client"), &config);

    let server_config = config.server.as_ref().expect("No server config found");
    let address = format!("{}:{}", server_config.address, server_config.port).parse()?;

    let screen_service = MyScreenService {
        config: config.clone(),
    };

    Server::builder()
        .add_service(ScreenServiceServer::new(screen_service))
        .serve(address)
        .await?;

    Ok(())
}

// TODO:
// - migrate to actual project
//   - (after having learned about tokyo, concurrency, async, etc.)
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
