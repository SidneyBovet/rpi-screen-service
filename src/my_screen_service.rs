use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use crate::config_extractor::api_config::ApiConfig;
use crate::kitty_updater::KittyUpdater;
use crate::screen_service::screen_service_server::ScreenService;
use crate::screen_service::{ScreenContentReply, ScreenContentRequest, Time};
use chrono::Timelike;
use log::{info, warn};
use prost::Message;
use tonic::{Request, Response, Status};

#[tonic::async_trait]
trait DataUpdater {
    async fn update(&self, screen_content: &Arc<Mutex<ScreenContentReply>>);
    fn get_period(&self) -> tokio::time::Duration;
}

// TODO: this is dumb, there has to be a better way of keeping up with time.
struct TimeUpdater {}

#[tonic::async_trait]
impl DataUpdater for TimeUpdater {
    async fn update(&self, screen_content: &Arc<Mutex<ScreenContentReply>>) {
        info!("Dummy Time update...");
        let now = chrono::offset::Local::now();
        match screen_content.lock() {
            Ok(mut content) => {
                content.now = Some(Time {
                    hours: now.hour(),
                    minutes: now.minute(),
                });
            }
            Err(e) => warn!("Poisoned lock when writing time: {}", e),
        }
    }

    fn get_period(&self) -> tokio::time::Duration {
        tokio::time::Duration::from_secs(60)
    }
}

struct DummyKittyUpdater {}

#[tonic::async_trait]
impl DataUpdater for DummyKittyUpdater {
    async fn update(&self, screen_content: &Arc<Mutex<ScreenContentReply>>) {
        info!("Dummy Kitty update...");
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let now = chrono::offset::Local::now();
        let dummy_kitty_debt = crate::screen_service::KittyDebt {
            who: "foo".into(),
            how_much: f32::try_from(u16::try_from(now.second()).unwrap()).unwrap(),
            whom: "bar".into(),
        };
        match screen_content.lock() {
            Ok(mut content) => content.kitty_debts = vec![dummy_kitty_debt],
            Err(e) => warn!("Poisoned lock when writing Kitty: {}", e),
        }
    }

    fn get_period(&self) -> tokio::time::Duration {
        tokio::time::Duration::from_secs(30)
    }
}

pub struct MyScreenService {
    config: ApiConfig, // Do not remove, actually useful for creating the kitty updater
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
        self.start_clock_updates();
        self.start_dummy_kitty_updates();
    }

    fn start_clock_updates(&self) {
        let container = Arc::clone(&self.screen_content_container);
        tokio::spawn(async move {
            //let container = containter.clone();
            let time_updater = TimeUpdater {};
            let mut interval = tokio::time::interval(time_updater.get_period());
            loop {
                interval.tick().await;
                time_updater.update(&container).await;
            }
        });
    }

    fn start_dummy_kitty_updates(&self) {
        let container = Arc::clone(&self.screen_content_container);
        tokio::spawn(async move {
            //let container = containter.clone();
            let kitty_updater = DummyKittyUpdater {};
            let mut interval = tokio::time::interval(kitty_updater.get_period());
            loop {
                interval.tick().await;
                // TODO: we probably want the updater to be able to let us know about errors (i.e. update() should return a Result<(), _>)
                kitty_updater.update(&container).await;
            }
        });
    }

    fn start_kitty_updates(self) {
        let container = Arc::clone(&self.screen_content_container);
        // TODO: move the bulk of this into the actual kitty file, deriving the updater trait
        let kitty_period = tokio::time::Duration::from_secs(
            self.config
                .kitty
                .as_ref()
                .expect("no kitty config")
                .update_period
                .as_ref()
                .expect("no kitty update period")
                .seconds
                .try_into()
                .expect("invalid kitty update period"),
        );
        tokio::spawn(async move {
            let kitty_updater =
                KittyUpdater::new(&self.config).expect("Error creating the Kitty updater");
            // TODO: create interval with kitty's new get_preiod
            loop {
                // TODO: call new update on the kitty
                // TODO: tick the interval
            }
        });
    }

    // TODO: benchmark this
    // if fast enough we can simply update the time here and get rid of the time updater
    // if not we need to compute the hash after each overwrite, store it
    //   and we need to figure out a way to have decent time precision
    fn get_hash(&self) -> Result<u64, Box<dyn std::error::Error + '_>> {
        let mut hasher = std::hash::DefaultHasher::new();
        let mut buf = prost::bytes::BytesMut::new();

        let content = self.screen_content_container.lock()?;
        content.encode(&mut buf)?;
        drop(content);

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
        info!("Serving /GetScreenContent");

        // Try to lock and clone our screen content to return it
        let reply: ScreenContentReply = match self.screen_content_container.lock() {
            Ok(content) => content.clone(),
            Err(e) => {
                warn!("Poisoned lock when reading content for serving: {}", e);
                let mut reply = ScreenContentReply::default();
                reply.error = true;
                reply
            }
        };

        Ok(Response::new(reply))
    }
}
