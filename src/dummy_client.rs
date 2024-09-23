use crate::config_extractor::api_config::ApiConfig;
use chrono::{DateTime, Datelike, Local, Timelike};
use log::{debug, error, info};
use screen_service::{
    screen_service_client::ScreenServiceClient, ScreenContentReply, ScreenContentRequest,
    ScreenHashRequest,
};
use tokio::task::JoinHandle;
use tonic::transport::Channel;

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
    let address = crate::config_extractor::get_server_address(api_config);
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
    let update_interval = tokio::time::Duration::from_secs(
        api_config
            .client
            .as_ref()
            .expect("No client config")
            .update_period
            .expect("No client update period")
            .seconds
            .try_into()
            .expect("Invalid client update period"),
    );
    debug!("update interval: {:?}", update_interval);
    let address = crate::config_extractor::get_server_address(api_config);
    debug!("address: {:?}", address);
    tokio::spawn(async move {
        // Let the server start up
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let mut client = ScreenServiceClient::connect(address)
            .await
            .expect("Couldn't start dummy client");
        let mut interval = tokio::time::interval(update_interval);
        let mut hash: u64 = 0;
        let mut minutes: u32 = Local::now().minute();
        loop {
            interval.tick().await;
            let new_hash = make_hash_request(&mut client).await;
            if hash != new_hash || minutes != Local::now().minute() {
                hash = new_hash;
                minutes = Local::now().minute();
                let content = make_full_request(&mut client).await;
                content_pretty_print(content).expect("Couldn't pretty print");
            }
        }
    })
}

fn content_pretty_print(content: ScreenContentReply) -> Result<(), Box<dyn std::error::Error>> {
    // TODO: handle all the unwraps
    let now = Local::now();
    info!("------------------");
    info!("[b:{}]", content.brightness);
    info!("[e:{}]", content.error);
    // On the real client this will be updated every minute, not with incoming messages
    // (otherwise we'd need to wait for e.g. a bus departure to have the minutes change)
    info!("{}", now.format("%H:%M"));
    if !content.kitty_debts.is_empty() {
        let debts = content
            .kitty_debts
            .iter()
            .map(|debt| {
                format!(
                    "{}>{}:{}",
                    debt.who
                        .chars()
                        .next()
                        .or_else(|| {
                            error!("No first char in debt's who");
                            Some('?')
                        })
                        .unwrap(),
                    debt.whom
                        .chars()
                        .next()
                        .or_else(|| {
                            error!("No first char in debt's who");
                            Some('?')
                        })
                        .unwrap(),
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
                let proto_ts = dep
                    .departure_time
                    .or_else(|| {
                        error!("Departure without a time");
                        Some(
                            prost_types::Timestamp::date(2000, 01, 01)
                                .expect("Can't even make a hardcoded proto"),
                        )
                    })
                    .unwrap();
                let departure_time: DateTime<Local> = DateTime::from_timestamp(
                    proto_ts.seconds,
                    proto_ts.nanos.try_into().expect("Invalid TS nanos"),
                )
                // We can't use `?` here because the function (we're in the lambda) doesn't return a Result
                .expect("Unable to convert departure proto TS into DateTime")
                .into();
                let departure_minutes_from_now =
                    departure_time.signed_duration_since(now).num_minutes();
                format!(
                    "{}:{}'",
                    dep.destination_enum().as_str_name().chars().next().unwrap(),
                    departure_minutes_from_now
                )
            })
            .collect::<Vec<String>>()
            .join(" - ");
        info!("{}", departures);
    }
    if let Some(event) = content.next_upcoming_event {
        let proto_ts = event
            .event_start
            .or_else(|| {
                error!("Event without a time");
                Some(
                    prost_types::Timestamp::date(2000, 01, 01)
                        .expect("Can't even make a hardcoded proto"),
                )
            })
            .unwrap();
        let event_time: DateTime<Local> = DateTime::from_timestamp(
            proto_ts.seconds,
            proto_ts.nanos.try_into().expect("Invalid TS nanos"),
        )
        .ok_or("Unable to convert event proto TS into DateTime")?
        .into();
        info!(
            "{}.{}-{}",
            event_time.day(),
            event_time.month(),
            event.event_title
        );
    }

    Ok(())
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
