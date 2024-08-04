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
use tokio::time::{Duration, Instant};

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
    transport_next_update: Instant,
}

#[tonic::async_trait]
impl DataUpdater for TransportUpdater {
    fn get_next_update_time(&self) -> Instant {
        match self.update_mode {
            TransportUpdateMode::Dummy => Instant::now() + Duration::from_secs(21),
            TransportUpdateMode::Real => {
                if self.transport_next_update < Instant::now() {
                    warn!("Next planned update is in the past, returning the default next update (in 10 minutes)");
                    get_default_next_update_time()
                } else {
                    self.transport_next_update
                }
            }
        }
    }

    async fn update(&mut self, screen_content: &Arc<Mutex<ScreenContentReply>>) {
        info!("Updating {:?} transport", self.update_mode);
        let destinations;
        match self.update_mode {
            TransportUpdateMode::Dummy => {
                let now = chrono::offset::Local::now();
                let st: std::time::SystemTime = (now
                    + chrono::Duration::minutes(now.second().into()))
                .try_into()
                .unwrap();
                destinations = vec![Departure {
                    destination_enum: DestinationEnum::Flon.into(),
                    departure_time: Some(prost_types::Timestamp::from(st)),
                }]
            }
            TransportUpdateMode::Real => {
                destinations = match self.get_departures().await {
                    Ok(mut departures) => {
                        self.set_next_update_time(&mut departures);
                        departures
                    }
                    Err(e) => {
                        error!("Error getting next departures: {}", e);
                        self.transport_next_update = get_default_next_update_time();
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
            transport_next_update: get_default_next_update_time(),
        })
    }

    async fn get_departures(&self) -> Result<Vec<Departure>, Box<dyn std::error::Error>> {
        let api_url = &self.config.url;
        let api_key = &self.config.api_key;
        let request_body = create_ojp_request(&self.config, &chrono::Utc::now());

        let request = self
            .client
            .post(api_url)
            .header("Content-Type", "application/xml")
            .bearer_auth(api_key)
            .body(request_body);

        let response_body = request.send().await?.text().await?;

        debug!("Received transport response: {:?}", response_body);
        extract_departures(&response_body, &self.config)
    }

    fn set_next_update_time(&mut self, departures: &mut Vec<Departure>) {
        departures.sort_by_key(|departure| {
            departure
                .departure_time
                .map_or(i64::MAX, |departure| departure.seconds)
        });
        match self.get_duration_to_next_departure(departures.first(), Duration::from_secs(1)) {
            Ok(next_update) => {
                info!(
                    "Next departure is {:?}, will update again at {:?}",
                    departures.first(),
                    next_update
                );
                self.transport_next_update = next_update;
            }
            Err(e) => {
                warn!(
                    "Error getting next update time: {} -- setting default 10 minutes from now",
                    e
                );
                self.transport_next_update = get_default_next_update_time();
            }
        }
    }

    fn get_duration_to_next_departure(
        &self,
        next_departure: Option<&Departure>,
        offset: Duration,
    ) -> Result<Instant, Box<dyn std::error::Error>> {
        let departure_utc_sec = next_departure
            .map(|d| d.departure_time)
            .flatten()
            .map(|ts| ts.seconds)
            .ok_or("No next departure")?;
        let now_utc_sec = chrono::offset::Utc::now().timestamp();

        if departure_utc_sec - now_utc_sec < 0 && departure_utc_sec - now_utc_sec > -60 {
            info!("Supposed next departure is ~now; retying in one minute");
            return Ok(Instant::now() + Duration::from_secs(60));
        }

        // This is probably bogous as hell, with all the Unix TS and timezones shenanigans
        Ok(Instant::now()
            + Duration::from_secs(u64::try_from(departure_utc_sec - now_utc_sec)?)
            + offset)
    }
}

// If we don't have any upcoming departure, this default asks for us to re-run in 10 minutes
fn get_default_next_update_time() -> Instant {
    Instant::now() + Duration::from_secs(600)
}

fn create_ojp_request(config: &TransportConfig, now: &chrono::DateTime<chrono::Utc>) -> String {
    let now_utc_string = now.format("%Y-%m-%dT%H:%M:%S%.3fZ");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<OJP xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns="http://www.siri.org.uk/siri" version="1.0" xmlns:ojp="http://www.vdv.de/ojp" xsi:schemaLocation="http://www.siri.org.uk/siri ../ojp-xsd-v1.0/OJP.xsd">
    <OJPRequest>
        <ServiceRequest>
            <RequestTimestamp>{}</RequestTimestamp>
            <RequestorRef>raspi-screen-server</RequestorRef>
            <ojp:OJPStopEventRequest>
                <RequestTimestamp>{}</RequestTimestamp>
                <ojp:Location>
                    <ojp:PlaceRef>
                        <StopPlaceRef>{}</StopPlaceRef>
                        <ojp:LocationName>
                            <ojp:Text>ignored</ojp:Text>
                        </ojp:LocationName>
                    </ojp:PlaceRef>
                    <ojp:DepArrTime>{}</ojp:DepArrTime>
                </ojp:Location>
                <ojp:Params>
                    <ojp:NumberOfResults>10</ojp:NumberOfResults>
                    <ojp:StopEventType>departure</ojp:StopEventType>
                    <ojp:IncludeRealtimeData>true</ojp:IncludeRealtimeData>
                </ojp:Params>
            </ojp:OJPStopEventRequest>
        </ServiceRequest>
    </OJPRequest>
</OJP>
"#,
        now_utc_string, now_utc_string, config.stop_id, now_utc_string
    )
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
                        debug_print(&text, "Departure time (timetable)");
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
                    b"ojp:EstimatedTime" => {
                        let text = reader.read_event();
                        debug_print(&text, "Departure time (estimated)");
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
    use super::*;
    use api_config::transport_config::DestinationPoints;
    use std::{i64, vec};

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
                                    <ojp:EstimatedTime>2024-07-23T11:02:30Z</ojp:EstimatedTime>
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
                                    <ojp:EstimatedTime>2024-07-23T11:14:30Z</ojp:EstimatedTime>
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
                                    <ojp:EstimatedTime>2024-07-23T11:15:30Z</ojp:EstimatedTime>
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
                                    <ojp:EstimatedTime>2024-07-23T11:15:30Z</ojp:EstimatedTime>
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
        let mut departures = extract_departures(&body, &config).expect("should succeed");
        // Let's sort to avoid any nondeterministic flakiness
        departures.sort_by_key(|d| d.departure_time.map_or(i64::MAX, |t| t.seconds));
        assert_eq!(departures.len(), 2);
        // Make sure the code picked up the estimated departure, not the timetable one
        assert_eq!(
            departures[0]
                .departure_time
                .expect("expected a departure with a timestamp")
                .seconds,
            1721732550
        );
        // The second departure doesn't have an estimated time, check that we did fall back to the timetable time
        assert_eq!(
            departures[1]
                .departure_time
                .expect("expected a departure with a timestamp")
                .seconds,
            1721732640
        );
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

    #[test]
    fn makes_request() {
        let time = "2024-07-26T09:42:09.123Z";
        let fake_now = chrono::NaiveDateTime::parse_from_str(time, "%Y-%m-%dT%H:%M:%S%.3fZ")
            .unwrap()
            .and_utc();
        let expected_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<OJP xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns="http://www.siri.org.uk/siri" version="1.0" xmlns:ojp="http://www.vdv.de/ojp" xsi:schemaLocation="http://www.siri.org.uk/siri ../ojp-xsd-v1.0/OJP.xsd">
    <OJPRequest>
        <ServiceRequest>
            <RequestTimestamp>2024-07-26T09:42:09.123Z</RequestTimestamp>
            <RequestorRef>raspi-screen-server</RequestorRef>
            <ojp:OJPStopEventRequest>
                <RequestTimestamp>2024-07-26T09:42:09.123Z</RequestTimestamp>
                <ojp:Location>
                    <ojp:PlaceRef>
                        <StopPlaceRef>123</StopPlaceRef>
                        <ojp:LocationName>
                            <ojp:Text>ignored</ojp:Text>
                        </ojp:LocationName>
                    </ojp:PlaceRef>
                    <ojp:DepArrTime>2024-07-26T09:42:09.123Z</ojp:DepArrTime>
                </ojp:Location>
                <ojp:Params>
                    <ojp:NumberOfResults>10</ojp:NumberOfResults>
                    <ojp:StopEventType>departure</ojp:StopEventType>
                    <ojp:IncludeRealtimeData>true</ojp:IncludeRealtimeData>
                </ojp:Params>
            </ojp:OJPStopEventRequest>
        </ServiceRequest>
    </OJPRequest>
</OJP>
"#;
        let config = TransportConfig {
            url: "".into(),
            api_key: "".into(),
            stop_id: 123,
            destination_points: vec![],
        };
        assert_eq!(create_ojp_request(&config, &fake_now), expected_xml);
    }
}
