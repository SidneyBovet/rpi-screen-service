use crate::screen_service::ScreenContentReply;
use std::sync::{Arc, Mutex};
use tokio::time::Instant;

#[tonic::async_trait]
pub trait DataUpdater {
    async fn update(&mut self, screen_content: &Arc<Mutex<ScreenContentReply>>);
    fn get_next_update_time(&self) -> Instant;
}
