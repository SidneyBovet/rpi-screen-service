/// Example showing some basic usage of the C++ library.
mod config_extractor;

pub mod screen_service {
    tonic::include_proto!("screen_service"); // The string specified here must match the proto package name
}

use crate::config_extractor::cli;
use chrono::{DateTime, Datelike, Local, Timelike};
use config_extractor::api_config::ApiConfig;
use config_extractor::extract_config;
use embedded_graphics::{
    mono_font::{ascii::FONT_4X6, ascii::FONT_5X7, ascii::FONT_9X15_BOLD, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    text::Text,
};
use log::{debug, error, info, warn};
use rpi_led_matrix::{LedCanvas, LedMatrix, LedMatrixOptions, LedRuntimeOptions};
use screen_service::{
    screen_service_client::ScreenServiceClient, ScreenContentReply, ScreenContentRequest,
    ScreenHashRequest,
};
use tonic::transport::Channel;

// Styles used by the drawing operations.
fn clock_style(b: f32) -> MonoTextStyle<'static, Rgb888> {
    MonoTextStyle::new(
        &FONT_9X15_BOLD,
        Rgb888::new(
            (f32::from(0xff as u8) * b) as u8,
            (f32::from(0xff as u8) * b) as u8,
            (f32::from(0xff as u8) * b) as u8,
        ),
    )
}
fn debt_style(b: f32) -> MonoTextStyle<'static, Rgb888> {
    MonoTextStyle::new(
        &FONT_5X7,
        Rgb888::new(
            (f32::from(0xcd as u8) * b) as u8,
            (f32::from(0xcd as u8) * b) as u8,
            (f32::from(0xf1 as u8) * b) as u8,
        ),
    )
}
fn bus_style(b: f32) -> MonoTextStyle<'static, Rgb888> {
    MonoTextStyle::new(
        &FONT_5X7,
        Rgb888::new(
            (f32::from(0xff as u8) * b) as u8,
            (f32::from(0xe6 as u8) * b) as u8,
            (f32::from(0x89 as u8) * b) as u8,
        ),
    )
}
fn cal_style(b: f32) -> MonoTextStyle<'static, Rgb888> {
    MonoTextStyle::new(
        &FONT_4X6,
        Rgb888::new(
            (f32::from(0xd4 as u8) * b) as u8,
            (f32::from(0xfd as u8) * b) as u8,
            (f32::from(0xc7 as u8) * b) as u8,
        ),
    )
}
fn err_style(b: f32) -> MonoTextStyle<'static, Rgb888> {
    MonoTextStyle::new(
        &FONT_4X6,
        Rgb888::new(
            (f32::from(0xff as u8) * b) as u8,
            (f32::from(0x00 as u8) * b) as u8,
            (f32::from(0x00 as u8) * b) as u8,
        ),
    )
}

fn get_options_from_config(api_config: &ApiConfig) -> (LedMatrixOptions, LedRuntimeOptions) {
    let client_config = api_config
        .client
        .as_ref()
        .expect("At least some client options should be provided in the config");
    let config_options = client_config
        .matrix_options
        .as_ref()
        .expect("At least some matrix options should be provided in the config");
    let mut options = LedMatrixOptions::new();
    options.set_hardware_mapping(
        config_options
            .hardware_mapping
            .as_ref()
            .unwrap_or(&"Regular".to_string()),
    );
    options.set_rows(config_options.rows.unwrap_or(32));
    options.set_cols(config_options.cols.unwrap_or(32));
    options.set_chain_length(config_options.chain_length.unwrap_or(1));
    options.set_parallel(config_options.parallel.unwrap_or(1));
    options
        .set_pwm_bits(
            config_options
                .pwm_bits
                .unwrap_or(11)
                .try_into()
                .expect("PWM bits value must be 8 bits"),
        )
        .expect("Error setting PWM bits option");
    options.set_pwm_lsb_nanoseconds(config_options.pwm_lsb_nanoseconds.unwrap_or(130));
    options.set_pwm_dither_bits(config_options.pwm_dither_bits);
    options
        .set_brightness(
            config_options
                .brightness
                .unwrap_or(100)
                .try_into()
                .expect("Brightness value must be 8 bits"),
        )
        .expect("Error setting brightness option");
    options.set_scan_mode(config_options.scan_mode);
    options.set_row_addr_type(config_options.row_address_type);
    options.set_multiplexing(config_options.multiplexing);
    options.set_led_rgb_sequence(
        config_options
            .led_rgb_sequence
            .as_ref()
            .unwrap_or(&"RGB".to_string()),
    );
    options.set_pixel_mapper_config(&config_options.pixel_mapper_config);
    options.set_panel_type(&config_options.panel_type);
    options.set_hardware_pulsing(!config_options.disable_hardware_pulsing);
    options.set_refresh_rate(config_options.show_refresh_rate);
    options.set_inverse_colors(config_options.inverse_colors);
    options.set_limit_refresh(config_options.limit_refresh_rate_hz);

    let config_rt_options = client_config.runtime_options.unwrap_or_default();
    let mut rt_options = LedRuntimeOptions::new();
    rt_options.set_gpio_slowdown(config_rt_options.gpio_slowdown.unwrap_or(1));
    rt_options.set_daemon(config_rt_options.daemon);
    rt_options.set_drop_privileges(!config_rt_options.no_drop_privileges);

    info!("Options: {:?}", options);
    info!("RT Options: {:?}", rt_options);

    (options, rt_options)
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

fn print_error_bit(canvas: &mut LedCanvas) {
    Text::new(".", Point::new(0, 0), err_style(0.5))
        .draw(canvas)
        .inspect_err(|e| error!("Can't even print the error bit: {:?}\nI'm giving up.", e))
        .expect("Can't even print the error bit, I'm giving up.");
}

fn draw_content_onto_canvas(
    canvas: &mut LedCanvas,
    content: &ScreenContentReply,
) -> Result<(), Box<dyn std::error::Error>> {
    // Consider graceful handling of the expect calls below
    canvas.clear();
    let now = Local::now();

    //let time_text = "19:24";
    let time_text = format!("{}", now.format("%H:%M")); // pls help me
    Text::new(
        &time_text,
        Point::new(9, 9),
        clock_style(content.brightness),
    )
    .draw(canvas)?;

    //let debt_text = "S>B:108\nM>B:42";
    let debt_text = content
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
                        error!("No first char in debt's whom");
                        Some('?')
                    })
                    .unwrap(),
                debt.how_much as i32
            )
        })
        .collect::<Vec<String>>()
        .join("\n");
    Text::new(
        &debt_text,
        Point::new(0, 17),
        debt_style(content.brightness),
    )
    .draw(canvas)?;

    //let bus_text = "18:12'\n32: 7'";
    let bus_text = content
        .bus_departures
        .iter()
        .map(|dep| {
            let proto_ts = dep
                .departure_time
                .or_else(|| {
                    error!("Departure without a time");
                    Some(prost_types::Timestamp::date(2000, 01, 01)?)
                })
                .unwrap();
            let departure_time: DateTime<Local> = DateTime::from_timestamp(
                proto_ts.seconds,
                proto_ts.nanos.try_into().expect("Invalid TS nanos"),
            )
            .ok_or("Unable to convert departure proto TS into DateTime")?
            .into();
            let departure_minutes_from_now =
                departure_time.signed_duration_since(now).num_minutes();
            format!(
                "{}:{}'",
                dep.destination_enum()
                    .as_str_name()
                    .chars()
                    .next()
                    .or_else(|| {
                        error!("No first char in departure");
                        Some('?')
                    })
                    .unwrap(),
                departure_minutes_from_now
            )
        })
        .collect::<Vec<String>>()
        .join("\n");
    Text::new(&bus_text, Point::new(36, 17), bus_style(content.brightness)).draw(canvas)?;

    //let cal_text = "23.10: Escape game";
    if let Some(event) = &content.next_upcoming_event {
        let proto_ts = event
            .event_start
            .or_else(|| {
                error!("Event without a time");
                Some(prost_types::Timestamp::date(2000, 01, 01)?)
            })
            .unwrap();
        let event_time: DateTime<Local> = DateTime::from_timestamp(
            proto_ts.seconds,
            proto_ts.nanos.try_into().expect("Invalid TS nanos"),
        )
        .ok_or("Unable to convert event proto TS into DateTime")?
        .into();
        let cal_text = format!(
            "{}.{}: {}",
            event_time.day(),
            event_time.month(),
            event.event_title
        );
        Text::new(&cal_text, Point::new(0, 30), cal_style(content.brightness)).draw(canvas)?;
    }

    if content.error {
        print_error_bit(canvas);
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let matches = cli().get_matches();
    config_extractor::init_logging(&matches).expect("Error setting up logging");
    let api_config = extract_config(&matches).expect("Error reading config");

    let address = crate::config_extractor::get_server_address(&api_config);
    info!("address: {:?}", address);
    let mut client = ScreenServiceClient::connect(address)
        .await
        .expect("Couldn't start raspi client");

    let (options, rt_options) = get_options_from_config(&api_config);
    let matrix = LedMatrix::new(Some(options), Some(rt_options)).unwrap();
    let mut canvas = matrix.offscreen_canvas();
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
    info!("update interval: {:?}", update_interval);
    let mut interval = tokio::time::interval(update_interval);
    let mut hash: u64 = 0;
    let mut minutes: u32 = Local::now().minute();
    let mut content: ScreenContentReply;
    loop {
        interval.tick().await;
        let new_hash = make_hash_request(&mut client).await;
        if hash != new_hash || minutes != Local::now().minute() {
            debug!("new hash or minute change, querying full content");
            hash = new_hash;
            minutes = Local::now().minute();
            content = make_full_request(&mut client).await;
            debug!("full content: {:?}", &content);
            let _ = draw_content_onto_canvas(&mut canvas, &content).inspect_err(|e| {
                warn!("Error drawing things on the canvas: {}", e);
                print_error_bit(&mut canvas);
            });
            canvas = matrix.swap(canvas);
        }
    }

    // tokio::spawn(async move {
    // }).await;
}
