use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use crate::config_extractor::api_config::ApiConfig;
use crate::data_updater::DataUpdater;
use crate::gcal_updater::GcalUpdater;
use crate::kitty_updater::KittyUpdater;
use crate::screen_service::screen_service_server::ScreenService;
use crate::screen_service::{
    ScreenContentReply, ScreenContentRequest, ScreenHashReply, ScreenHashRequest, Time,
};
use crate::transport_updater::TransportUpdater;
use chrono::Timelike;
use log::{debug, error, warn};
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
        // Start the updaters in dummy mode, to avoid spamming the server if we got something wrong
        self.start_kitty_updates(crate::kitty_updater::KittyUpdateMode::Dummy);
        self.start_gcal_updates(crate::gcal_updater::GcalUpdateMode::Dummy);
        self.start_transport_updates(crate::transport_updater::TransportUpdateMode::Dummy);
    }

    fn start_kitty_updates(&self, update_mode: crate::kitty_updater::KittyUpdateMode) {
        let config_copy = self.config.clone();
        let container = Arc::clone(&self.screen_content_container);
        tokio::spawn(async move {
            let mut kitty_updater = KittyUpdater::new(update_mode, &config_copy)
                .expect("Error creating the Kitty updater");
            loop {
                kitty_updater.update(&container).await;
                tokio::time::sleep_until(kitty_updater.get_next_update_time()).await;
            }
        });
    }

    fn start_gcal_updates(&self, update_mode: crate::gcal_updater::GcalUpdateMode) {
        let config_copy = self.config.clone();
        let container = Arc::clone(&self.screen_content_container);
        tokio::spawn(async move {
            let mut gcal_updater = GcalUpdater::new(update_mode, &config_copy)
                .expect("Error creating the gcal updater");
            loop {
                gcal_updater.update(&container).await;
                tokio::time::sleep_until(gcal_updater.get_next_update_time()).await;
            }
        });
    }

    fn start_transport_updates(&self, update_mode: crate::transport_updater::TransportUpdateMode) {
        let config_copy = self.config.clone();
        let container = Arc::clone(&self.screen_content_container);
        tokio::spawn(async move {
            let mut transport_updater = TransportUpdater::new(update_mode, &config_copy).expect("Error creating the transport updater");
            loop {
                transport_updater.update(&container).await;
                tokio::time::sleep_until(transport_updater.get_next_update_time()).await;
            }
        });
    }

    // Computes the hash of the content proto **after updating its time field**
    fn get_hash<'a>(
        &'a self, content: &'a Arc<Mutex<ScreenContentReply>>,
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
            // Update the brightness according to now
            content.brightness = self.get_brightness(now.hour()).unwrap_or(1.0);
            // Serialize the latest proto into our bytes buffer
            content.encode(&mut buf)?;
        }

        // Hash the proto bytes
        buf.hash(&mut hasher);
        Ok(hasher.finish())
    }

    fn get_brightness(&self, hour: u32) -> Option<f32> {
        let brightness_map = &self.config.server.as_ref()?.brightness_map;
        get_brightness_impl(brightness_map, hour).or_else(|| {
            warn!("Couldn't find a brightness from the config map for hour {}", hour);
            None
        })
    }
}

fn get_brightness_impl(brightness_map: &HashMap<u32, f32>, hour: u32) -> Option<f32> {
    let (mut best_hour, mut best_brightness) = (None, None);
    for (h, b) in brightness_map {
        if h <= &hour && h > &best_hour.unwrap_or(u32::MIN) {
            best_hour = Some(*h);
            best_brightness = Some(*b);
        }
    }
    best_brightness
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
        let reply = match self.get_hash(&self.screen_content_container) {
            Ok(hash) => ScreenHashReply { hash },
            Err(e) => {
                error!("Error computing hash: {:#?}", e);
                ScreenHashReply { hash: 0 }
            }
        };
        Ok(Response::new(reply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_brightness() {
        let map = HashMap::from([
            (0, 0.0),
            (2, 0.5),
            (3, 0.8),
            (12, 1.0),
        ]);

        assert_eq!(get_brightness_impl(&map, 0), Some(0.0));
        assert_eq!(get_brightness_impl(&map, 1), Some(0.0));
        assert_eq!(get_brightness_impl(&map, 2), Some(0.5));
        assert_eq!(get_brightness_impl(&map, 5), Some(0.8));
        assert_eq!(get_brightness_impl(&map, 11), Some(0.8));
        assert_eq!(get_brightness_impl(&map, 12), Some(1.0));
    }
}
