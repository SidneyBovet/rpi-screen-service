use crate::screen_service::ScreenContentReply;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;

#[tonic::async_trait]
pub trait DataUpdater {
    async fn update(&mut self, screen_content: &Arc<Mutex<ScreenContentReply>>);
    fn get_period(&self) -> Duration;
}
