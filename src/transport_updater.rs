use crate::config_extractor::api_config::TransportConfig;
use crate::dummy_client::screen_service::departure::DestinationEnum;
use crate::screen_service::{Departure, ScreenContentReply};
use crate::{config_extractor::api_config, data_updater::DataUpdater};
use chrono::{Datelike, NaiveDateTime, Timelike};
use log::{debug, error, info, warn};
use prost_types::Timestamp;
use quick_xml::events::{BytesText, Event};
use quick_xml::Reader;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;

// In case we have no next departure after an update, this is the time after which we want to be run again.
const DEFAULT_UPDATE_PERIOD: Duration = Duration::from_secs(600);

#[derive(Debug)]
// We switch from one to the other for manual testing, but it's actually fine to keep both.
#[allow(dead_code)]
pub enum TransportUpdateMode {
    Dummy,
    Real,
}

#[derive(Debug)]
pub struct TransportUpdater {
    update_mode: TransportUpdateMode,
    client: Client,
    config: TransportConfig,
    transport_update_period: Duration,
}

#[tonic::async_trait]
impl DataUpdater for TransportUpdater {
    fn get_period(&self) -> Duration {
        match self.update_mode {
            TransportUpdateMode::Dummy => Duration::from_secs(19),
            TransportUpdateMode::Real => self.transport_update_period,
        }
    }

    async fn update(&mut self, screen_content: &Arc<Mutex<ScreenContentReply>>) {
        info!("Updating {:?} transport", self.update_mode);
        let destinations;
        match self.update_mode {
            TransportUpdateMode::Dummy => {
                let now = chrono::offset::Local::now();
                let st: std::time::SystemTime = now.try_into().unwrap();
                destinations = vec![Departure {
                    destination_enum: DestinationEnum::Flon.into(),
                    departure_time: Some(prost_types::Timestamp::from(st)),
                }]
            }
            TransportUpdateMode::Real => {
                destinations = match self.get_departures().await {
                    Ok(d) => d,
                    Err(e) => {
                        error!("Error getting next departures: {}", e);
                        vec![]
                    }
                }
            }
        };
        match screen_content.lock() {
            Ok(mut content) => {
                content.bus_departures = destinations;
            }
            Err(e) => error!("Poisoned lock when writing debts: {}", e),
        };
    }
}

impl TransportUpdater {
    pub fn new(
        update_mode: TransportUpdateMode,
        config: &api_config::ApiConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let transport_config = config.transport.as_ref().ok_or("No transport config")?;
        // This will get set after each update to match the next departure
        Ok(TransportUpdater {
            update_mode,
            client: Client::new(),
            config: transport_config.to_owned(),
            transport_update_period: DEFAULT_UPDATE_PERIOD,
        })
    }

    async fn get_departures(&self) -> Result<Vec<Departure>, Box<dyn std::error::Error>> {
        // TODO (using the quick-xml Writer)
        // - use now() to set time
        // - config's stop as starting point (do we need the name?)
        // - hardcode other options from https://opentransportdata.swiss/explorer/?api=ojp
        let api_url = &self.config.url;
        let api_key = &self.config.api_key;
        let body = self.client.get(api_url).send().await?.text().await?;

        let mut departures = extract_departures(&body, &self.config)
            .inspect_err(|e| error!("Error parsing next departures: {:?}", e))?;
        // TODO: update transport_update_period so we update a few seconds after the earliest arrival in our departures
        departures.sort_by_key(|departure| {
            departure
                .departure_time
                .map_or(i64::MAX, |departure| departure.seconds)
        });
        // TODO: make the update interface's update method mutable, so we can change things in here
        let todo = self.get_duration_to_next_departure(departures.first(), Duration::from_secs(1));
        info!("Next departure is {:?}, will update again in {} seconds", departures.first(), todo.as_secs());
        Ok(departures)
    }

    fn get_duration_to_next_departure(
        &self,
        next_departure: Option<&Departure>,
        offset: Duration,
    ) -> Duration {
        let Some(departure_sec) = next_departure
            .map(|d| d.departure_time)
            .flatten()
            .map(|ts| ts.seconds)
        else {
            return DEFAULT_UPDATE_PERIOD;
        };
        // This is not super nice, we just do it at seconds precision. But hey, it should work, no?
        let Ok(departure_sec) = u64::try_from(departure_sec) else {
            warn!(
                "Couldn't cast the departure seconds from i64 to u64: {}",
                departure_sec
            );
            return DEFAULT_UPDATE_PERIOD;
        };
        let now_sec = u64::from(chrono::offset::Local::now().second());

        Duration::from_secs(departure_sec - now_sec) + offset
    }
}

#[derive(Debug, Default)]
struct DepartureBuilder {
    departure_time: Option<Timestamp>,
    dest_id: Option<u32>,
}

fn extract_departures(
    body: &str,
    config: &TransportConfig,
) -> Result<Vec<Departure>, Box<dyn std::error::Error>> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);

    let mut departures = HashMap::<i32, Departure>::default();
    let mut departure = DepartureBuilder::default();
    // The `Reader` does not implement `Iterator` because it outputs borrowed data (`Cow`s)
    loop {
        match reader.read_event() {
            Err(e) => {
                error!("Error at position {}: {:?}", reader.error_position(), e);
                break;
            }
            // Exit the loop when reaching end of file
            Ok(Event::Eof) => break,
            // Handle the start of interesting tags
            Ok(Event::Start(e)) => {
                match e.name().as_ref() {
                    b"ojp:StopEventResult" => debug!("Found stop event..."),
                    b"ojp:TimetabledTime" => {
                        let text = reader.read_event();
                        debug_print(&text, "Departure time");
                        match text {
                            Ok(Event::Text(t)) => {
                                let time = get_time(&t)?;
                                departure.departure_time = Some(time);
                            }
                            other => {
                                error!(
                                    "Expected text type after 'TimetabledTime', got {:?}",
                                    other
                                );
                            }
                        };
                    }
                    b"ojp:DestinationStopPointRef" => {
                        let text = reader.read_event();
                        debug_print(&text, "Destination ID");
                        match text {
                            Ok(Event::Text(t)) => {
                                let Ok(text) = t.unescape().inspect_err(|e| {
                                    error!("Couldn't unescape stop point ID: {}", e)
                                }) else {
                                    break;
                                };
                                debug!("  Towards: {:?}", text);
                                let Ok(dest_id) = text.parse::<u32>().inspect_err(|e| {
                                    error!("Couldn't parse stop point ID into a number: {}", e)
                                }) else {
                                    break;
                                };
                                departure.dest_id = Some(dest_id);
                            }
                            other => {
                                error!(
                                    "Expected text type after 'DestinationStopPointRef', got {:?}",
                                    other
                                );
                            }
                        };
                    }
                    b"ojp:PublishedLineName" => {
                        // Just for debug purposes, to see the destination name
                        let _inner = reader.read_event();
                        let text = reader.read_event();
                        debug_print(&text, "Line is");
                    }
                    b"ojp:DestinationText" => {
                        // Just for debug purposes, to see the destination name
                        let _inner = reader.read_event();
                        let text = reader.read_event();
                        debug_print(&text, "Destination name");
                    }
                    // We don't care about the start of other tags
                    _ => (),
                }
            }
            // Handle the end of interesting tags
            Ok(Event::End(e)) => {
                match e.name().as_ref() {
                    b"ojp:StopEventResult" => {
                        debug!("Found event end, inspecting constructed departure");
                        match &departure {
                            DepartureBuilder {
                                departure_time: Some(depart_ts),
                                dest_id: Some(dest_id),
                            } => {
                                debug!("Found a full event: {:?}", &departure);
                                for dest in &config.destination_points {
                                    debug!("Checking {:?} for matches", dest);
                                    if dest.stops.iter().any(|stop| stop == dest_id) {
                                        let Some(actual_enum) =
                                            DestinationEnum::from_str_name(&dest.destination_name)
                                        else {
                                            error!("Configured destination name didn't match a destination enum: '{}'", &dest.destination_name);
                                            break;
                                        };
                                        let new_departure = Departure {
                                            departure_time: Some(*depart_ts),
                                            destination_enum: actual_enum.into(),
                                        };
                                        debug!("Considering {:?} for insertion", new_departure);
                                        match departures.get(&actual_enum.into()) {
                                            None => {
                                                // No candidate for this destination yet, let's record ours
                                                departures
                                                    .insert(actual_enum.into(), new_departure);
                                            }
                                            Some(existing_departure) => {
                                                // Let's see if ours departs earlier
                                                let existing_seconds = match existing_departure
                                                    .departure_time
                                                {
                                                    Some(t) => t.seconds,
                                                    None => {
                                                        warn!("Existing departure doesn't have a timestamp");
                                                        break;
                                                    }
                                                };
                                                let new_seconds = match new_departure.departure_time
                                                {
                                                    Some(t) => t.seconds,
                                                    None => {
                                                        warn!("New departure doesn't have a timestamp");
                                                        break;
                                                    }
                                                };
                                                if existing_seconds > new_seconds {
                                                    departures
                                                        .insert(actual_enum.into(), new_departure);
                                                }
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                            misconstructed => {
                                error!(
                                    "Constructed departure didn't contain all its fields: {:?}",
                                    misconstructed
                                );
                            }
                        }
                    }
                    // We don't care about the end of other tags
                    _ => (),
                }
            }
            // We don't care about other event types
            _ => (),
        }
    }
    // Return whatever we collected so far (may be empty, let the caller deal with that)
    Ok(departures.iter().map(|entry| *entry.1).collect())
}

fn debug_print(text: &Result<Event, quick_xml::Error>, prefix: &str) -> () {
    match text {
        Ok(Event::Text(t)) => {
            if let Ok(text) = t.unescape() {
                debug!("  {}: {}", prefix, text);
            }
        }
        other => {
            info!(
                "Expected text type for debug print of '{}', got {:?}",
                prefix, other
            );
        }
    };
}

fn get_time(text: &BytesText) -> Result<Timestamp, Box<dyn std::error::Error>> {
    let time = text.unescape().unwrap();
    debug!("  Parsing OJP timestamp: {:#?}", time);
    // Consider exporting this in a helper module (see also gcal updater)
    let rust_ts = NaiveDateTime::parse_from_str(&time, "%Y-%m-%dT%H:%M:%SZ")
        .map_err(|e| format!("ICS timestamp parsing error {:?} parsing '{}'", e, time))?;
    debug!("  Parsed timestamp: {:#?}", rust_ts);
    let time = Timestamp::date_time(
        rust_ts.year().try_into().unwrap(),
        rust_ts.month().try_into().unwrap(),
        rust_ts.day().try_into().unwrap(),
        rust_ts.hour().try_into().unwrap(),
        rust_ts.minute().try_into().unwrap(),
        rust_ts.second().try_into().unwrap(),
    )
    .map_err(|e| format!("Proto timestamp creation error: {:?}", e))?;
    Ok(time)
}

#[cfg(test)]
mod tests {
    use std::vec;

    use api_config::transport_config::DestinationPoints;

    use super::*;

    #[test]
    fn extracts_departures() {
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<siri:OJP xmlns:siri="http://www.siri.org.uk/siri" xmlns:ojp="http://www.vdv.de/ojp" version="1.0">
    <siri:OJPResponse>
        <siri:ServiceDelivery>
            <ojp:OJPStopEventDelivery>
                <ojp:CalcTime>44</ojp:CalcTime>
                <ojp:StopEventResult>
                    <ojp:StopEvent>
                        <ojp:ThisCall>
                            <ojp:CallAtStop>
                                <siri:StopPointRef>123</siri:StopPointRef>
                                <ojp:StopPointName>
                                    <ojp:Text xml:lang="de">The stop</ojp:Text>
                                </ojp:StopPointName>
                                <ojp:ServiceDeparture>
                                    <ojp:TimetabledTime>2024-07-23T11:02:00Z</ojp:TimetabledTime>
                                </ojp:ServiceDeparture>
                                <ojp:Order>14</ojp:Order>
                            </ojp:CallAtStop>
                        </ojp:ThisCall>
                        <ojp:Service>
                            <ojp:OperatingDayRef>2024-07-23</ojp:OperatingDayRef>
                            <ojp:PublishedLineName>
                                <ojp:Text xml:lang="de">4</ojp:Text>
                            </ojp:PublishedLineName>
                            <ojp:OriginStopPointRef>42427</ojp:OriginStopPointRef>
                            <ojp:OriginText>
                                <ojp:Text xml:lang="de">Some origin</ojp:Text>
                            </ojp:OriginText>
                            <ojp:DestinationStopPointRef>345</ojp:DestinationStopPointRef>
                            <ojp:DestinationText>
                                <ojp:Text xml:lang="de">Renens 1</ojp:Text>
                            </ojp:DestinationText>
                        </ojp:Service>
                    </ojp:StopEvent>
                </ojp:StopEventResult>
                <ojp:StopEventResult>
                    <ojp:StopEvent>
                        <ojp:ThisCall>
                            <ojp:CallAtStop>
                                <siri:StopPointRef>123</siri:StopPointRef>
                                <ojp:StopPointName>
                                    <ojp:Text xml:lang="de">The stop</ojp:Text>
                                </ojp:StopPointName>
                                <ojp:ServiceDeparture>
                                    <ojp:TimetabledTime>2024-07-23T11:04:00Z</ojp:TimetabledTime>
                                </ojp:ServiceDeparture>
                                <ojp:Order>1</ojp:Order>
                            </ojp:CallAtStop>
                        </ojp:ThisCall>
                        <ojp:Service>
                            <ojp:OperatingDayRef>2024-07-23</ojp:OperatingDayRef>
                            <ojp:PublishedLineName>
                                <ojp:Text xml:lang="de">8</ojp:Text>
                            </ojp:PublishedLineName>
                            <ojp:OriginStopPointRef>123</ojp:OriginStopPointRef>
                            <ojp:OriginText>
                                <ojp:Text xml:lang="de">Some origin</ojp:Text>
                            </ojp:OriginText>
                            <ojp:DestinationStopPointRef>456</ojp:DestinationStopPointRef>
                            <ojp:DestinationText>
                                <ojp:Text xml:lang="de">Flon</ojp:Text>
                            </ojp:DestinationText>
                        </ojp:Service>
                    </ojp:StopEvent>
                </ojp:StopEventResult>
                <ojp:StopEventResult>
                    <ojp:StopEvent>
                        <ojp:ThisCall>
                            <ojp:CallAtStop>
                                <siri:StopPointRef>123</siri:StopPointRef>
                                <ojp:StopPointName>
                                    <ojp:Text xml:lang="de">The stop</ojp:Text>
                                </ojp:StopPointName>
                                <ojp:ServiceDeparture>
                                    <ojp:TimetabledTime>2024-07-23T11:14:00Z</ojp:TimetabledTime>
                                </ojp:ServiceDeparture>
                                <ojp:Order>1</ojp:Order>
                            </ojp:CallAtStop>
                        </ojp:ThisCall>
                        <ojp:Service>
                            <ojp:OperatingDayRef>2024-07-23</ojp:OperatingDayRef>
                            <ojp:PublishedLineName>
                                <ojp:Text xml:lang="de">8</ojp:Text>
                            </ojp:PublishedLineName>
                            <ojp:OriginStopPointRef>123</ojp:OriginStopPointRef>
                            <ojp:OriginText>
                                <ojp:Text xml:lang="de">Some origin</ojp:Text>
                            </ojp:OriginText>
                            <ojp:DestinationStopPointRef>456</ojp:DestinationStopPointRef>
                            <ojp:DestinationText>
                                <ojp:Text xml:lang="de">Flon</ojp:Text>
                            </ojp:DestinationText>
                        </ojp:Service>
                    </ojp:StopEvent>
                </ojp:StopEventResult>
                <ojp:StopEventResult>
                    <ojp:StopEvent>
                        <ojp:ThisCall>
                            <ojp:CallAtStop>
                                <siri:StopPointRef>123</siri:StopPointRef>
                                <ojp:StopPointName>
                                    <ojp:Text xml:lang="de">The stop</ojp:Text>
                                </ojp:StopPointName>
                                <ojp:ServiceDeparture>
                                    <ojp:TimetabledTime>2024-07-23T11:15:00Z</ojp:TimetabledTime>
                                </ojp:ServiceDeparture>
                                <ojp:Order>4</ojp:Order>
                            </ojp:CallAtStop>
                        </ojp:ThisCall>
                        <ojp:Service>
                            <ojp:OperatingDayRef>2024-07-23</ojp:OperatingDayRef>
                            <ojp:PublishedLineName>
                                <ojp:Text xml:lang="de">2</ojp:Text>
                            </ojp:PublishedLineName>
                            <ojp:OriginStopPointRef>42420</ojp:OriginStopPointRef>
                            <ojp:OriginText>
                                <ojp:Text xml:lang="de">Some origin</ojp:Text>
                            </ojp:OriginText>
                            <ojp:DestinationStopPointRef>234</ojp:DestinationStopPointRef>
                            <ojp:DestinationText>
                                <ojp:Text xml:lang="de">Renens 2</ojp:Text>
                            </ojp:DestinationText>
                        </ojp:Service>
                    </ojp:StopEvent>
                </ojp:StopEventResult>
                <ojp:StopEventResult>
                    <ojp:StopEvent>
                        <ojp:ThisCall>
                            <ojp:CallAtStop>
                                <siri:StopPointRef>123</siri:StopPointRef>
                                <ojp:StopPointName>
                                    <ojp:Text xml:lang="de">The stop</ojp:Text>
                                </ojp:StopPointName>
                                <ojp:ServiceDeparture>
                                    <ojp:TimetabledTime>2024-07-23T11:15:00Z</ojp:TimetabledTime>
                                </ojp:ServiceDeparture>
                                <ojp:Order>8</ojp:Order>
                            </ojp:CallAtStop>
                        </ojp:ThisCall>
                        <ojp:Service>
                            <ojp:OperatingDayRef>2024-07-23</ojp:OperatingDayRef>
                            <ojp:PublishedLineName>
                                <ojp:Text xml:lang="de">2</ojp:Text>
                            </ojp:PublishedLineName>
                            <ojp:OriginStopPointRef>234</ojp:OriginStopPointRef>
                            <ojp:OriginText>
                                <ojp:Text xml:lang="de">Some origin</ojp:Text>
                            </ojp:OriginText>
                            <ojp:DestinationStopPointRef>42428</ojp:DestinationStopPointRef>
                            <ojp:DestinationText>
                                <ojp:Text xml:lang="de">Wrong dest</ojp:Text>
                            </ojp:DestinationText>
                        </ojp:Service>
                    </ojp:StopEvent>
                </ojp:StopEventResult>
            </ojp:OJPStopEventDelivery>
        </siri:ServiceDelivery>
    </siri:OJPResponse>
</siri:OJP>
"#;
        let config = TransportConfig {
            url: "".into(),
            api_key: "".into(),
            stop_id: 123,
            destination_points: vec![
                DestinationPoints {
                    stops: vec![234, 345],
                    destination_name: DestinationEnum::Renens.as_str_name().into(),
                },
                DestinationPoints {
                    stops: vec![456],
                    destination_name: DestinationEnum::Flon.as_str_name().into(),
                },
            ],
        };
        let departures = extract_departures(&body, &config).expect("should succeed");
        assert_eq!(departures.len(), 2);
    }

    #[test]
    fn doesnt_panic_on_empty_response() {
        let body = "";
        let config = TransportConfig::default();
        let departures = extract_departures(&body, &config).expect("should succeed");
        assert_eq!(departures.len(), 0);
    }

    #[test]
    fn enum_string_check() {
        assert_eq!(DestinationEnum::Flon.as_str_name(), "FLON");
    }
}