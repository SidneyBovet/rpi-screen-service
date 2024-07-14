pub mod screen_service {
    tonic::include_proto!("screen_service"); // The string specified here must match the proto package name
}

use prost::Message;
use prost_types::Timestamp;
use screen_service::{departure::{self}, CalendarEvent, Departure, KittyDebt, ScreenContentReply, Time};
use std::{
    hash::{Hash, Hasher},
    sync::{Arc, Mutex}, time::SystemTime,
};
use chrono::Timelike;

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn hash(content: &Arc<Mutex<ScreenContentReply>>) -> Result<u64, Box<dyn std::error::Error + '_>> {
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

fn get_dummy_proto() -> ScreenContentReply {
    let now = Some(Time {
        hours: 13,
        minutes: 37
    });
    let kitty_debts = vec![
        KittyDebt {
            who: "asdf".into(),
            how_much: 123.45,
            whom: "qwert".into(),
        },
        KittyDebt {
            who: "zxcv".into(),
            how_much: 987.65,
            whom: "uiop".into(),
        },
    ];
    let bus_departures = vec![
        Departure {destination: departure::Destination::Flon.into(), departure_time: Some(Timestamp::from(SystemTime::now()))},
        Departure {destination: departure::Destination::Renens.into(), departure_time: Some(Timestamp::from(SystemTime::now()))},
    ];
    let next_upcoming_event = Some(CalendarEvent {
        event_title: "This is a rather long event title".into(),
        event_start: Some(Timestamp::from(SystemTime::now())),
    });
    ScreenContentReply {
        now,
        brightness: 0.9876,
        kitty_debts,
        bus_departures,
        next_upcoming_event,
        error: true,
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    let content = Arc::new(Mutex::new(get_dummy_proto()));
    c.bench_function("fib 20", |b| b.iter(|| hash(black_box(&content))));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
