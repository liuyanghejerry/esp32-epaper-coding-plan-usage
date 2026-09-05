//! Kimi Code plan usage on a Waveshare ESP32-S3-ePaper-1.54G (ESP32-S3-PICO-1).
//!
//! Polls https://api.kimi.com/coding/v1/usages over Wi-Fi every 5 minutes and
//! redraws the panel only when the numbers changed (e-paper longevity). An
//! NTP-synced "HH:MM" clock (UTC+8) sits in the bottom-right corner and
//! repaints every cycle — an e-paper keeps its image when powered off, so
//! the ticking clock is the "device is alive" indicator. The bottom-left
//! corner shows a battery gauge when a lithium pack is attached, or a plug
//! glyph when the device can only be running from USB.
//!
//! Board pin map:
//!   EPD_SCLK=GPIO12  EPD_MOSI=GPIO13  EPD_CS=GPIO11  EPD_DC=GPIO10
//!   EPD_RST=GPIO9    EPD_BUSY=GPIO8   EPD_PWR=GPIO6 (panel 3V3 enable, LOW=on)
//! Logs go to the native USB-Serial/JTAG port.

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_net::{Runner, StackResources};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::{
    analog::adc::{Adc, AdcConfig, Attenuation},
    clock::CpuClock,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    interrupt::software::SoftwareInterruptControl,
    ram,
    rng::Rng,
    spi::master::{Config as SpiConfig, Spi},
    timer::timg::TimerGroup,
};
use esp_println::logger::init_logger_from_env;
use esp_radio::wifi::{
    sta::StationConfig, Config as RadioConfig, ControllerConfig, Interface, WifiController,
};
use heapless::String;
use log::{info, warn};

mod epd;
mod ntp;
mod render;
mod usage;

// Secrets are baked in at compile time by build.rs from the gitignored
// repo-root `.env` (see .env.example).
mod secrets {
    pub const WIFI_SSID: &str = env!("WIFI_SSID");
    pub const WIFI_PASSWORD: &str = env!("WIFI_PASSWORD");
    pub const KIMI_TOKEN: &str = env!("KIMI_TOKEN");
}

use epd::{Epd, FrameBuffer};
use usage::UsageView;

esp_bootloader_esp_idf::esp_app_desc!();

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

/// One usage query every 5 minutes; the wait is split into 30s heartbeats.
const QUERY_INTERVAL_SECS: u64 = 300;

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    init_logger_from_env();
    info!("epaper-usage booting");

    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    esp_alloc::heap_allocator!(#[ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 96 * 1024);

    // GPIO6 = EPD3V3_EN: e-paper 3.3 V rail switch, ACTIVE-LOW (high-side
    // P-MOSFET). Left floating, the panel is unpowered — the TCON still
    // answers SPI via parasitic power, but the ink never moves.
    let _epd_pwr = Output::new(p.GPIO6, Level::Low, OutputConfig::default());

    let timg0 = TimerGroup::new(p.TIMG0);
    let sw_int = SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    // ---- Wi-Fi ----
    let station_config = RadioConfig::Station(
        StationConfig::default()
            .with_ssid(secrets::WIFI_SSID)
            .with_password(secrets::WIFI_PASSWORD.into()),
    );
    let wifi_interface = Interface::station();
    let controller = WifiController::new(
        p.WIFI,
        ControllerConfig::default().with_initial_config(station_config),
    )
    .unwrap();

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        embassy_net::Config::dhcpv4(Default::default()),
        mk_static!(StackResources<4>, StackResources::<4>::new()),
        seed,
    );

    spawner.spawn(connection(controller).unwrap());
    spawner.spawn(net_task(runner).unwrap());

    // ---- e-paper ----
    let rst = Output::new(p.GPIO9, Level::Low, OutputConfig::default());
    let dc = Output::new(p.GPIO10, Level::Low, OutputConfig::default());
    let cs = Output::new(p.GPIO11, Level::High, OutputConfig::default());
    let busy = Input::new(p.GPIO8, InputConfig::default().with_pull(Pull::Up));
    // NOTE: keep the default SPI clock (1 MHz). On esp-hal 1.1.2/ESP32-S3,
    // ANY explicit `.with_frequency(...)` (tested 4 MHz and 20 MHz) leaves
    // SCLK dead — the panel never sees a command, BUSY never leaves idle,
    // and every transfer reports success. CPU clock (80 vs 240 MHz) is
    // unrelated; bisected on hardware. 10 KB framebuffer @1 MHz = 80 ms,
    // so the speed is irrelevant anyway.
    let spi = Spi::new(p.SPI2, SpiConfig::default())
        .unwrap()
        .with_sck(p.GPIO12)
        .with_mosi(p.GPIO13);
    let mut epd = Epd::new(spi, cs, dc, rst, busy);
    let mut fb = FrameBuffer::new();
    let bufs = mk_static!(usage::Buffers, usage::Buffers::new());

    // Battery sense: GPIO4 samples VBAT through a 200K/200K divider (Waveshare
    // board spec), so real VBAT ≈ reading × 2. 11 dB attenuation ⇒ ~3.1 V
    // full scale on ADC1, hence the ×6200/4095 estimate below. (The battery
    // power latch GPIO17/BAT_Control is deliberately left untouched — driving
    // it HIGH would keep the pack powering the board after USB is unplugged.)
    let mut adc1_config = AdcConfig::new();
    let mut bat_pin = adc1_config.enable_pin(p.GPIO4, Attenuation::_11dB);
    let mut adc1 = Adc::new(p.ADC1, adc1_config);

    info!("waiting for wifi...");
    if embassy_time::with_timeout(Duration::from_secs(30), stack.wait_config_up())
        .await
        .is_ok()
    {
        info!("got ip: {:?}", stack.config_v4().map(|c| c.address));
    } else {
        warn!("wifi not up within 30s — will retry queries anyway");
    }

    let mut last: Option<UsageView> = None;
    let mut shown_clock: Option<String<5>> = None;
    let mut last_power: Option<render::PowerState> = None;
    // Unix epoch seconds at the last NTP sync + the sync instant; between
    // syncs "now" = base + monotonic elapsed.
    let mut ntp_at: Option<(u64, embassy_time::Instant)> = None;
    let mut failures: u32 = 0;
    let mut error_shown = false;
    let mut tick: u64 = 0;

    loop {
        // ---- wall clock ----
        if stack.is_config_up() {
            match ntp::fetch_epoch(stack).await {
                Ok(epoch) => {
                    info!("ntp synced, epoch={}s", epoch);
                    ntp_at = Some((epoch, embassy_time::Instant::now()));
                }
                Err(e) => warn!("ntp sync failed: {:?} — clock keeps drifting", e),
            }
        }
        let clock: String<5> = match ntp_at {
            Some((base, at)) => ntp::fmt_hhmm(base + at.elapsed().as_secs()),
            None => {
                let mut s: String<5> = String::new();
                let _ = s.push_str("--:--");
                s
            }
        };
        let clock_changed = shown_clock.as_ref() != Some(&clock);

        // ---- battery / power indicator state ----
        // GPIO4 reads VBAT/2; raw ≈ mv/2 × 4095/3100. A real cell sits in
        // raw ≈ 1900..3100 (VBAT 2.9–4.7 V); the full-scale 4095 seen with
        // an empty battery connector means "no battery ⇒ USB-powered".
        let bat_raw = loop {
            match adc1.read_oneshot(&mut bat_pin) {
                Ok(v) => break v,
                Err(nb::Error::WouldBlock) => {}
                Err(nb::Error::Other(_)) => break 0,
            }
        };
        let bat_mv = bat_raw as u32 * 6200 / 4095;
        let power = if (1900..=3100).contains(&bat_raw) {
            let pct = (((bat_mv as i32) - 3300) * 100 / 900).clamp(0, 100) as u32;
            render::PowerState::Battery { pct }
        } else {
            render::PowerState::Usb
        };
        let power_changed = last_power.as_ref() != Some(&power);

        if !stack.is_config_up() {
            warn!("offline, retrying in 30s");
            if last.is_none() {
                failures += 1;
                if failures >= 3 && (!error_shown || clock_changed || power_changed) {
                    info!("painting error page: wifi offline, clock {}", clock);
                    render::render_error(&mut fb, "wifi offline", "check SSID / password");
                    render::draw_clock(&mut fb, &clock);
                    render::draw_power(&mut fb, &power);
                    epd.init().await;
                    let ok = epd.display(&fb.buf).await;
                    epd.sleep().await;
                    info!("error page painted, display ok={}", ok);
                    error_shown = true;
                    shown_clock = Some(clock.clone());
                    last_power = Some(power);
                }
            }
            tick += 1;
            Timer::after(Duration::from_secs(30)).await;
            continue;
        }

        match usage::fetch_usage(stack, bufs, seed ^ tick).await {
            Ok(view) => {
                failures = 0;
                error_shown = false;
                if last.as_ref() != Some(&view) || clock_changed || power_changed {
                    info!(
                        "refreshing panel: clock {}, week {}/{}",
                        clock, view.week_used, view.week_limit
                    );
                    render::render(&mut fb, &view);
                    render::draw_clock(&mut fb, &clock);
                    render::draw_power(&mut fb, &power);
                    epd.init().await;
                    let ok = epd.display(&fb.buf).await;
                    epd.sleep().await;
                    info!("refresh completed={}", ok);
                    last = Some(view);
                    shown_clock = Some(clock.clone());
                    last_power = Some(power);
                } else {
                    info!("usage unchanged, panel untouched");
                }
            }
            Err(e) => {
                failures += 1;
                warn!("usage query failed (#{}): {:?}", failures, e);
                if last.is_none() && failures >= 3 && (!error_shown || clock_changed || power_changed) {
                    info!("painting error page: query failed, clock {}", clock);
                    render::render_error(&mut fb, "query failed", "check wifi/token");
                    render::draw_clock(&mut fb, &clock);
                    render::draw_power(&mut fb, &power);
                    epd.init().await;
                    let ok = epd.display(&fb.buf).await;
                    epd.sleep().await;
                    info!("error page painted, display ok={}", ok);
                    error_shown = true;
                    shown_clock = Some(clock.clone());
                    last_power = Some(power);
                }
            }
        }
        tick += 1;
        // Heartbeat: 30s slices instead of one long sleep, so an attached
        // serial monitor always shows signs of life.
        let slices = QUERY_INTERVAL_SECS / 30;
        for i in 1..=slices {
            Timer::after(Duration::from_secs(30)).await;
            // Include the last query outcome so late-attaching serial
            // monitors (USB buffering drops early boot logs) still see state.
            match &last {
                Some(v) => info!(
                    "heartbeat, next query in {}s, clock {}, week {}/{}, bat ~{}mV (raw {})",
                    (slices - i) * 30,
                    shown_clock.as_deref().unwrap_or("--:--"),
                    v.week_used,
                    v.week_limit,
                    bat_mv,
                    bat_raw
                ),
                None => info!(
                    "heartbeat, next query in {}s, clock {}, no data (failures={}), bat ~{}mV (raw {})",
                    (slices - i) * 30,
                    shown_clock.as_deref().unwrap_or("--:--"),
                    failures,
                    bat_mv,
                    bat_raw
                ),
            }
        }
    }
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    info!("wifi connection task started");
    loop {
        match controller.connect_async().await {
            Ok(info) => {
                info!("wifi connected: {:?}", info);
                let info = controller.wait_for_disconnect_async().await.ok();
                info!("wifi disconnected: {:?}", info);
            }
            Err(e) => {
                warn!("wifi connect failed: {:?}", e);
                Timer::after(Duration::from_secs(5)).await;
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) {
    runner.run().await
}
