use crate::screen_service::{CalendarEvent, ScreenContentReply};
use crate::{config_extractor::api_config, data_updater::DataUpdater};
use chrono::{Datelike, NaiveDateTime, Timelike};
use log::{debug, error, info};
use prost_types::Timestamp;
use reqwest::Client;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::time::{Duration, Instant};

#[derive(Debug)]
// We switch from one to the other for manual testing, but it's actually fine to keep both.
#[allow(dead_code)]
pub enum GcalUpdateMode {
    Dummy,
    Real,
}

#[derive(Debug)]
pub struct GcalUpdater {
    update_mode: GcalUpdateMode,
    client: Client,
    ics_url: String,
    gcal_period: Duration,
}

#[tonic::async_trait]
impl DataUpdater for GcalUpdater {
    fn get_next_update_time(&self) -> Instant {
        match self.update_mode {
            GcalUpdateMode::Dummy => Instant::now() + Duration::from_secs(29),
            GcalUpdateMode::Real => Instant::now() + self.gcal_period,
        }
    }

    async fn update(
        &mut self,
        screen_content: &Arc<Mutex<ScreenContentReply>>,
        error_bit: &Arc<AtomicBool>,
    ) {
        info!("Updating {:?} gCal", self.update_mode);
        let event;
        match self.update_mode {
            GcalUpdateMode::Dummy => {
                let now = chrono::offset::Local::now();
                event = Some(CalendarEvent {
                    event_start: Some(Timestamp::from(SystemTime::from(now))),
                    event_title: "dummy event".into(),
                });
                error_bit.store(now.second() % 10 == 0, std::sync::atomic::Ordering::Relaxed);
            }
            GcalUpdateMode::Real => {
                event = match self.get_next_event().await {
                    Ok(returned_event) => {
                        // Make sure the server knows there are no errors
                        error_bit.store(false, std::sync::atomic::Ordering::Relaxed);
                        returned_event
                    }
                    Err(e) => {
                        error!("Error getting gCal event: {}", e);
                        error_bit.store(true, std::sync::atomic::Ordering::Relaxed);
                        None
                    }
                }
            }
        }
        match screen_content.lock() {
            Ok(mut content) => content.next_upcoming_event = event,
            Err(e) => error!("Poisoned lock when writing debts: {}", e),
        };
    }
}

impl GcalUpdater {
    pub fn new(
        update_mode: GcalUpdateMode,
        config: &api_config::ApiConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let gcal_config = config.gcal.as_ref().ok_or("No gCal config")?;
        let ics_url = gcal_config.ics_url.clone();
        let gcal_period = Duration::from_secs(
            gcal_config
                .update_period
                .as_ref()
                .ok_or("no kitty update period")?
                .seconds
                .try_into()?,
        );
        Ok(GcalUpdater {
            update_mode,
            client: Client::new(),
            ics_url,
            gcal_period,
        })
    }

    async fn get_next_event(&self) -> Result<Option<CalendarEvent>, Box<dyn std::error::Error>> {
        let ics: String = self.client.get(&self.ics_url).send().await?.text().await?;
        parse_next_event(ics).map_err(|err| format!("Error parsing ics content: {:?}", err).into())
    }
}

// Note: this function assumes that the ics passed used the following gCal options:
// ?futureevents=true&orderby=starttime&sortorder=ascending
// This means that the first event in the body is the next upcoming event, so we can just look that up.
fn parse_next_event(ics: String) -> Result<Option<CalendarEvent>, Box<dyn std::error::Error>> {
    let mut lines = ics.lines();

    // None of the ical parsing crates out there do a good job, so let's just do it manually.
    let mut first_upcoming_event: Option<CalendarEvent> = None;
    while let Some(line) = lines.next() {
        if line == "END:VEVENT" {
            debug!("Fund end of first event");
            break;
        } else if let Some(title) = line.strip_prefix("SUMMARY:") {
            debug!("Found event title: {}", title);
            first_upcoming_event
                .get_or_insert(CalendarEvent::default())
                .event_title = title.to_string();
        } else if let Some(ts) = line.strip_prefix("DTSTART:") {
            debug!("Parsing ICS timestamp: {:#?}", ts);
            // Consider exporting this in a helper module (see also transport updater)
            let rust_ts = NaiveDateTime::parse_from_str(ts, "%Y%m%dT%H%M%SZ")
                .map_err(|e| format!("ICS timestamp parsing error {:?} parsing '{}'", e, ts))?;
            debug!("Parsed timestamp: {:#?}", rust_ts);
            let time = Timestamp::date_time(
                rust_ts.year().try_into().unwrap(),
                rust_ts.month().try_into().unwrap(),
                rust_ts.day().try_into().unwrap(),
                rust_ts.hour().try_into().unwrap(),
                rust_ts.minute().try_into().unwrap(),
                rust_ts.second().try_into().unwrap(),
            )
            .map_err(|e| format!("Proto timestamp creation error: {:?}", e))?;
            first_upcoming_event
                .get_or_insert(CalendarEvent::default())
                .event_start = Some(time);
        }
    }

    Ok(first_upcoming_event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_event() {
        let ics = "BEGIN:VCALENDAR
PRODID:-//Google Inc//Google Calendar 70.9054//EN
VERSION:2.0
CALSCALE:GREGORIAN
METHOD:PUBLISH
X-WR-CALNAME:Crissier Palace
X-WR-TIMEZONE:Europe/Zurich
BEGIN:VEVENT
DTSTART:20240720T110000Z
DTEND:20240720T120000Z
DTSTAMP:20240714T150338Z
UID:3cil81id3a6r6l6ccm9tbdcvj9@google.com
CREATED:20240714T130605Z
LAST-MODIFIED:20240714T130605Z
SEQUENCE:0
STATUS:CONFIRMED
SUMMARY:Test event
TRANSP:OPAQUE
END:VEVENT
BEGIN:VEVENT
DTSTART:20240720T130000Z
DTEND:20240720T140000Z
DTSTAMP:20240714T150338Z
UID:26n0p7c8jee803d3j08b41krme@google.com
CREATED:20240714T132024Z
LAST-MODIFIED:20240714T132024Z
SEQUENCE:0
STATUS:CONFIRMED
SUMMARY:Test next event
TRANSP:OPAQUE
END:VEVENT
END:VCALENDAR
"
        .into();
        let parsed = parse_next_event(ics).unwrap();
        let expected = Some(CalendarEvent {
            event_start: Some(Timestamp {
                // 2024-07-20 11:00 UTC
                seconds: 1721473200,
                nanos: 0,
            }),
            event_title: "Test event".into(),
        });
        assert_eq!(parsed, expected);
    }

    #[test]
    fn returns_default_on_no_event() {
        let ics = "BEGIN:VCALENDAR
PRODID:-//Google Inc//Google Calendar 70.9054//EN
VERSION:2.0
CALSCALE:GREGORIAN
METHOD:PUBLISH
X-WR-CALNAME:Crissier Palace
X-WR-TIMEZONE:Europe/Zurich
END:VCALENDAR
"
        .into();
        let parsed = parse_next_event(ics).unwrap();
        assert_eq!(parsed, None);
    }

    #[test]
    fn returns_default_on_empty_ics() {
        let ics = "".into();
        let parsed = parse_next_event(ics).unwrap();
        assert_eq!(parsed, None);
    }

    #[test]
    fn doesnt_panic_on_malformed_ics() {
        let ics = "\\
not?an
<h1>ICS</h1>
]});"
            .into();
        let parsed = parse_next_event(ics).unwrap();
        assert_eq!(parsed, None);
    }

    #[test]
    fn doesnt_panic_on_malformed_time() {
        let ics = "
DTSTART:20240732T110000Z
DTEND:20240720T120000Z
DTSTAMP:20240714T150338Z
UID:3cil81id3a6r6l6ccm9tbdcvj9@google.com
CREATED:20240714T130605Z
LAST-MODIFIED:20240714T130605Z
SEQUENCE:0
STATUS:CONFIRMED
SUMMARY:Test event
TRANSP:OPAQUE
"
        .into();
        let result = parse_next_event(ics);
        let err = result.err().unwrap();
        assert!(err.to_string().contains("timestamp parsing error"));
    }
}
