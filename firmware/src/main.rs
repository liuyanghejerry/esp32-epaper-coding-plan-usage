//! "Hello World!" on a Waveshare ESP32-S3-ePaper-1.54G (ESP32-S3-PICO-1-N8R8).
//!
//! Board pin map (from Waveshare docs):
//!   EPD_SCLK=GPIO12  EPD_MOSI=GPIO13  EPD_CS=GPIO11  EPD_DC=GPIO10
//!   EPD_RST=GPIO9    EPD_BUSY=GPIO8   EPD_PWR=GPIO6 (panel 3V3 enable)
//! Logs go to UART0 (GPIO43/44) — visible on the board's other USB port.

#![no_std]
#![no_main]

use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, ascii::FONT_9X15, MonoTextStyle},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use esp_backtrace as _;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_println::logger::init_logger_from_env;
use log::info;

mod epd;
use epd::{Color, Epd, FrameBuffer};

#[esp_hal::main]
fn main() -> ! {
    // Small boot delay so a serial listener can attach after (re)enumeration
    // before the one-shot boot logs are printed.
    Delay::new().delay_millis(2000);
    init_logger_from_env();
    info!("epaper-hello booting on ESP32-S3");

    let p = esp_hal::init(esp_hal::Config::default());

    // GPIO6 = EPD3V3_EN: e-paper 3.3 V rail switch, ACTIVE-LOW (high-side
    // P-MOSFET). Drive low to power the panel. Left floating, the panel is
    // unpowered — the TCON still answers SPI via parasitic power and BUSY
    // toggles, but the ink never moves.
    let _epd_pwr = Output::new(p.GPIO6, Level::Low, OutputConfig::default());
    let rst = Output::new(p.GPIO9, Level::Low, OutputConfig::default());
    let dc = Output::new(p.GPIO10, Level::Low, OutputConfig::default());
    let cs = Output::new(p.GPIO11, Level::High, OutputConfig::default());
    let busy = Input::new(p.GPIO8, InputConfig::default().with_pull(Pull::Up));
    let sclk = p.GPIO12;
    let mosi = p.GPIO13;

    let spi = Spi::new(p.SPI2, SpiConfig::default())
        .unwrap()
        .with_sck(sclk)
        .with_mosi(mosi);

    let mut delay = Delay::new();

    let mut epd = Epd::new(spi, cs, dc, rst, busy);
    info!("busy idle level: high={}", epd.busy_is_high());
    epd.init();
    info!("panel initialised, busy after power-on: high={}", epd.busy_is_high());

    let mut fb = FrameBuffer::new();
    fb.fill(Color::White);

    // Red frame around the whole screen
    Rectangle::new(Point::new(4, 4), Size::new(192, 192))
        .into_styled(PrimitiveStyle::with_stroke(Color::Red, 4))
        .draw(&mut fb)
        .ok();
    Text::new("Hello World!", Point::new(40, 80), MonoTextStyle::new(&FONT_10X20, Color::Black))
        .draw(&mut fb)
        .ok();
    Text::new("Rust on ESP32-S3", Point::new(28, 115), MonoTextStyle::new(&FONT_9X15, Color::Red))
        .draw(&mut fb)
        .ok();
    // Yellow accent bar
    Rectangle::new(Point::new(36, 140), Size::new(128, 14))
        .into_styled(PrimitiveStyle::with_fill(Color::Yellow))
        .draw(&mut fb)
        .ok();
    Text::new("no_std + esp-hal", Point::new(28, 165), MonoTextStyle::new(&FONT_9X15, Color::Black))
        .draw(&mut fb)
        .ok();

    let refreshed = epd.display(&fb.buf);
    info!("refresh completed={}", refreshed);
    epd.sleep();
    info!("panel asleep — image persists, idling");

    loop {
        delay.delay_millis(1000);
    }
}
