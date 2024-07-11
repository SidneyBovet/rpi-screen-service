use crate::config_extractor::api_config::ApiConfig;
use crate::kitty_updater::KittyUpdater;
use crate::screen_service::screen_service_server::ScreenService;
use crate::screen_service::{ScreenContentReply, ScreenContentRequest, Time};
use chrono::Timelike;
use tonic::{Request, Response, Status};

#[derive(Debug)]
pub struct MyScreenService {
    //config: ApiConfig,
    kitty_updater: KittyUpdater,
}

impl MyScreenService {
    pub fn new(config: &ApiConfig) -> Self {
        MyScreenService {
            kitty_updater: KittyUpdater::new(&config).expect("Error creating the Kitty updater"),
        }
    }
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
        let debts = match self.kitty_updater.get_debts().await {
            Ok(kitty) => kitty, // TODO: here we'd set our shared copy of the content reply (and reset our error bit)
            Err(e) => {
                println!("Error while parsing Kitty: {}", e);
                vec![] // TODO: and here we set our error bit (then the code will set the reply's bit if any of the bits are set) + some logging
            }
        };

        let reply = ScreenContentReply {
            now: Some(Time {
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
