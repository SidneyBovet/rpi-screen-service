use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use crate::config_extractor::api_config::ApiConfig;
use crate::data_updater::DataUpdater;
use crate::kitty_updater::KittyUpdater;
use crate::screen_service::screen_service_server::ScreenService;
use crate::screen_service::{
    ScreenContentReply, ScreenContentRequest, ScreenHashReply, ScreenHashRequest, Time,
};
use chrono::Timelike;
use log::{error, debug};
use prost::Message;
use tonic::{Request, Response, Status};

pub struct MyScreenService {
    config: ApiConfig,
    screen_content_container: Arc<Mutex<ScreenContentReply>>,
}

impl MyScreenService {
    pub fn new(config: &ApiConfig) -> Self {
        let screen_content_container = Arc::new(Mutex::new(ScreenContentReply::default()));
        MyScreenService {
            config: config.clone(),
            screen_content_container,
        }
    }

    pub fn start_backgound_updates(&self) {
        // Start the Kitty updater in dummy mode, to avoid spamming the server if we got something wrong
        self.start_kitty_updates(crate::kitty_updater::KittyUpdateMode::Dummy);
    }

    fn start_kitty_updates(&self, update_mode: crate::kitty_updater::KittyUpdateMode) {
        let config_copy = self.config.clone();
        let container = Arc::clone(&self.screen_content_container);
        tokio::spawn(async move {
            let kitty_updater = KittyUpdater::new(update_mode, &config_copy)
                .expect("Error creating the Kitty updater");
            let mut interval = tokio::time::interval(kitty_updater.get_period());
            loop {
                interval.tick().await;
                kitty_updater.update(&container).await;
            }
        });
    }

    // Computes the hash of the content proto **after updating its time field**
    fn get_hash(
        content: &Arc<Mutex<ScreenContentReply>>,
    ) -> Result<u64, Box<dyn std::error::Error + '_>> {
        let mut hasher = std::hash::DefaultHasher::new();
        let mut buf = prost::bytes::BytesMut::new();

        {
            let mut content = content.lock()?;
            // Update the time, in case minutes have changed since our last hash
            let now = chrono::offset::Local::now();
            content.now = Some(Time {
                hours: now.hour(),
                minutes: now.minute(),
            });
            content.encode(&mut buf)?;
        }

        // Hash the proto bytes
        buf.hash(&mut hasher);
        Ok(hasher.finish())
    }
}

#[tonic::async_trait]
impl ScreenService for MyScreenService {
    // Handles the /GetScreenContent RPC
    async fn get_screen_content(
        &self,
        _request: Request<ScreenContentRequest>,
    ) -> Result<Response<ScreenContentReply>, Status> {
        debug!("Serving /GetScreenContent");
        // Try to lock and clone our screen content to return it
        let reply: ScreenContentReply = match self.screen_content_container.lock() {
            Ok(content) => content.clone(),
            Err(e) => {
                error!("Poisoned lock when reading content for serving: {}", e);
                let mut reply = ScreenContentReply::default();
                reply.error = true;
                reply
            }
        };

        Ok(Response::new(reply))
    }

    async fn get_screen_hash(
        &self,
        _request: Request<ScreenHashRequest>,
    ) -> Result<Response<ScreenHashReply>, Status> {
        debug!("Serving /GetScreenHash");
        let reply = match MyScreenService::get_hash(&self.screen_content_container) {
            Ok(hash) => ScreenHashReply { hash },
            Err(e) => {
                error!("Error computing hash: {:#?}", e);
                ScreenHashReply { hash: 0 }
            }
        };
        Ok(Response::new(reply))
    }
}