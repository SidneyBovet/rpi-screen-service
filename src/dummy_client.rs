use crate::config_extractor::api_config::ApiConfig;
use log::info;
use screen_service::{screen_service_client::ScreenServiceClient, ScreenHashRequest};
use screen_service::{ScreenContentReply, ScreenContentRequest};
use tokio::task::JoinHandle;
use tonic::transport::{Channel, Endpoint};

pub mod screen_service {
    tonic::include_proto!("screen_service"); // The string specified here must match the proto package name
}

#[derive(Debug)]
pub enum ClientMode {
    OneShot,
    // This is only used for the cli client, so the server compilation complains that we never use it.
    #[allow(dead_code)]
    HashQuery,
}

pub fn start(mode: ClientMode, api_config: &ApiConfig) -> JoinHandle<()> {
    info!("Dummy client spawned and running in mode: {:#?}", mode);
    match mode {
        ClientMode::OneShot => start_one_shot(api_config),
        ClientMode::HashQuery => start_hash_queries(api_config),
    }
}

fn start_one_shot(api_config: &ApiConfig) -> JoinHandle<()> {
    let address = get_server_address(api_config);
    tokio::spawn(async move {
        // Let the server start up
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let mut client = ScreenServiceClient::connect(address)
            .await
            .expect("Couldn't start dummy client");

        info!("Sending /GetScreenHash");
        let hash = make_hash_request(&mut client).await;
        info!("Hash: {}", hash);

        info!("Sending /GetScreenContent");
        let content = make_full_request(&mut client).await;
        info!("{:#?}", content);
    })
}

fn start_hash_queries(api_config: &ApiConfig) -> JoinHandle<()> {
    let address = get_server_address(api_config);
    tokio::spawn(async move {
        // Let the server start up
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let mut client = ScreenServiceClient::connect(address)
            .await
            .expect("Couldn't start dummy client");
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut hash: u64 = 0;
        loop {
            interval.tick().await;
            let new_hash = make_hash_request(&mut client).await;
            if new_hash != hash {
                hash = new_hash;
                info!("Content changed, sending /GetScreenContent");
                let content = make_full_request(&mut client).await;
                content_pretty_print(content);
            }
        }
    })
}

fn content_pretty_print(content: ScreenContentReply) {
    // TODO: maybe handle all the unwraps
    // TODO: timestamps to local time
    // TODO: consider removing the time field, since we need now() anyway (or find a way around it)
    info!("------------------");
    info!("[b:{}]", content.brightness);
    info!("[e:{}]", content.error);
    if let Some(time) = content.now {
        info!("{}:{}", time.hours, time.minutes);
    }
    if !content.kitty_debts.is_empty() {
        let debts = content
            .kitty_debts
            .iter()
            .map(|debt| {
                format!(
                    "{}>{}:{}",
                    debt.who.chars().next().unwrap(),
                    debt.whom.chars().next().unwrap(),
                    debt.how_much as i32
                )
            })
            .collect::<Vec<String>>()
            .join(" - ");
        info!("{}", debts);
    }
    if !content.bus_departures.is_empty() {
        let departures = content
            .bus_departures
            .iter()
            .map(|dep| {
                format!(
                    "{}:{}",
                    dep.destination_enum().as_str_name().chars().next().unwrap(),
                    dep.departure_time.unwrap().seconds
                )
            })
            .collect::<Vec<String>>()
            .join(" - ");
        info!("{}", departures);
    }
    if let Some(event) = content.next_upcoming_event {
        info!("{}-{}", event.event_start.unwrap().seconds, event.event_title);
    }
}

fn get_server_address(api_config: &ApiConfig) -> Endpoint {
    let server_config = api_config.server.as_ref().expect("No server config found");
    format!("http://{}:{}", server_config.address, server_config.port)
        .parse()
        .expect("Couldn't parse server config into an address")
}

async fn make_hash_request(client: &mut ScreenServiceClient<Channel>) -> u64 {
    let request = tonic::Request::new(ScreenHashRequest {});
    client
        .get_screen_hash(request)
        .await
        .expect("Full screen content request failed")
        .get_ref()
        .hash
}

async fn make_full_request(client: &mut ScreenServiceClient<Channel>) -> ScreenContentReply {
    let request = tonic::Request::new(ScreenContentRequest {});
    client
        .get_screen_content(request)
        .await
        .expect("Full screen content request failed")
        .get_ref()
        .clone()
}
