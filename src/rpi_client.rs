/// Example showing some basic usage of the C++ library.
mod config_extractor;

use crate::config_extractor::cli;
//use clap::{arg, crate_version, App};
use embedded_graphics::{
    mono_font::{ascii::FONT_4X6 ,ascii::FONT_5X7, ascii::FONT_9X15_BOLD, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    text::Text,
};
use rpi_led_matrix::{args, LedMatrix};

const DELAY: std::time::Duration = std::time::Duration::from_secs(30);

fn main() {
    //logging_setup();

    let matches = cli().get_matches();
    // TODO: make options and rt_options from config.json instead
    let (options, rt_options) = args::matrix_options_from_args(&matches);

    let matrix = LedMatrix::new(Some(options), Some(rt_options)).unwrap();
    let mut canvas = matrix.offscreen_canvas();

    // Create styles used by the drawing operations.
    let clock_style = MonoTextStyle::new(&FONT_9X15_BOLD, Rgb888::new(0xff, 0xff, 0xff));
    let debt_style = MonoTextStyle::new(&FONT_5X7, Rgb888::new(0xcd, 0xcd, 0xf1));
    let bus_style = MonoTextStyle::new(&FONT_5X7, Rgb888::new(0xff, 0xe6, 0x89));
    let cal_style = MonoTextStyle::new(&FONT_4X6, Rgb888::new(0xd4, 0xfd, 0xc7));
    //let text_4_6 = MonoTextStyle::new(&FONT_4X6, Rgb888::new(0xff, 0xff, 0xff));
    //let text_6_9 = MonoTextStyle::new(&FONT_6X9, Rgb888::new(0xff, 0xff, 0xff));

    // Draw centered text.
    let time_text = "19:24";
    Text::new(time_text, Point::new(9, 9), clock_style)
        .draw(&mut canvas)
        .unwrap();
    let debt_text = "S>B:108\nM>B:42";
    Text::new(debt_text, Point::new(0, 17), debt_style)
        .draw(&mut canvas)
        .unwrap();
    let bus_text = "18:12'\n32: 7'";
    Text::new(bus_text, Point::new(36, 17), bus_style)
        .draw(&mut canvas)
        .unwrap();
    let cal_text = "23.10: Escape game";
    Text::new(cal_text, Point::new(0, 30), cal_style)
        .draw(&mut canvas)
        .unwrap();

    // Draw the thing
    // could also do canvas = matrix.swap to get the new offscreen canvas
    let _ = matrix.swap(canvas);
    std::thread::sleep(DELAY);
}
