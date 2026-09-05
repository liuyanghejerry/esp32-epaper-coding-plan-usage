//! Render the usage view onto the 200x200 4-colour framebuffer.
//!
//! All positional values come from `firmware/layout.json`, parsed by
//! `build.rs` into `OUT_DIR/layout.rs` at compile time. Edit that JSON (or
//! export it from tools/layout-editor.html) and rebuild — no code changes.

use core::fmt::Write as _;

use embedded_graphics::{
    mono_font::{ascii::FONT_9X15, MonoTextStyle},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use heapless::String;

use crate::epd::{Color, FrameBuffer};
use crate::usage::UsageView;

// Constants generated from layout.json by build.rs: BAND_H, TITLE_X/Y,
// WEEK_Y, BAR1_Y, RESET1_X/Y, WIN_Y, BAR2_Y, RESET2_X/Y, TEXT_LEFT,
// TEXT_RIGHT, BAR_X/W/H.
include!(concat!(env!("OUT_DIR"), "/layout.rs"));

fn pct(used: u32, limit: u32) -> u32 {
    if limit == 0 {
        0
    } else {
        (used * 100 / limit).min(100)
    }
}

fn bar(fb: &mut FrameBuffer, y: i32, used: u32, limit: u32) {
    // outline
    Rectangle::new(Point::new(BAR_X, y), Size::new(BAR_W, BAR_H))
        .into_styled(PrimitiveStyle::with_stroke(Color::Black, 1))
        .draw(fb)
        .ok();
    // fill
    let p = pct(used, limit);
    let fill_w = ((BAR_W - 2) * p / 100).max(if p > 0 { 2 } else { 0 });
    let color = if p >= 80 { Color::Red } else { Color::Black };
    if fill_w > 0 {
        Rectangle::new(Point::new(BAR_X + 1, y + 1), Size::new(fill_w, BAR_H - 2))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(fb)
            .ok();
    }
}

/// Right-aligned 9x15 text (9 px per cell).
fn text_right(fb: &mut FrameBuffer, s: &str, y: i32, color: Color) {
    let w = s.len() as i32 * 9;
    Text::new(s, Point::new(TEXT_RIGHT - w, y), MonoTextStyle::new(&FONT_9X15, color))
        .draw(fb)
        .ok();
}

fn text(fb: &mut FrameBuffer, s: &str, x: i32, y: i32, color: Color) {
    Text::new(s, Point::new(x, y), MonoTextStyle::new(&FONT_9X15, color))
        .draw(fb)
        .ok();
}

pub fn render(fb: &mut FrameBuffer, u: &UsageView) {
    fb.fill(Color::White);

    // header band; title sits flush against its bottom edge (the panel's top
    // rows are physically clipped, so the title must stay as low as possible
    // without leaving the band).
    Rectangle::new(Point::new(0, 0), Size::new(200, BAND_H))
        .into_styled(PrimitiveStyle::with_fill(Color::Black))
        .draw(fb)
        .ok();
    text(fb, "Kimi Code Usage", TITLE_X, TITLE_Y, Color::White);

    let mut s: String<32> = String::new();

    // weekly quota
    s.clear();
    let _ = write!(
        s,
        "{}/{} ({}%)",
        u.week_used,
        u.week_limit,
        pct(u.week_used, u.week_limit)
    );
    text(fb, "Week", TEXT_LEFT, WEEK_Y, Color::Black);
    text_right(fb, &s, WEEK_Y, Color::Black);
    bar(fb, BAR1_Y, u.week_used, u.week_limit);

    s.clear();
    let _ = write!(s, "reset {}", u.week_reset);
    text(fb, &s, RESET1_X, RESET1_Y, Color::Black);

    // rolling window
    s.clear();
    if u.win_minutes >= 60 && u.win_minutes % 60 == 0 {
        let _ = write!(s, "{}h", u.win_minutes / 60);
    } else {
        let _ = write!(s, "{}min", u.win_minutes);
    }
    text(fb, &s, TEXT_LEFT, WIN_Y, Color::Black);
    s.clear();
    let _ = write!(
        s,
        "{}/{} ({}%)",
        u.win_used,
        u.win_limit,
        pct(u.win_used, u.win_limit)
    );
    text_right(fb, &s, WIN_Y, Color::Black);
    bar(fb, BAR2_Y, u.win_used, u.win_limit);

    s.clear();
    let _ = write!(s, "reset {}", u.win_reset);
    text(fb, &s, RESET2_X, RESET2_Y, Color::Black);
}

/// Right-aligned "HH:MM" wall clock (NTP-synced, UTC+8) in the bottom-right
/// corner — a live display proves the device is powered, which a static
/// e-paper image cannot.
pub fn draw_clock(fb: &mut FrameBuffer, hhmm: &str) {
    let w = hhmm.len() as i32 * 9;
    Text::new(
        hhmm,
        Point::new(CLOCK_RIGHT - w, CLOCK_Y),
        MonoTextStyle::new(&FONT_9X15, Color::Black),
    )
    .draw(fb)
    .ok();
}

/// Power indicator in the bottom-left corner. The board has no VBUS or
/// charge-status line to the MCU (ETA6098 STAT only drives the charge LED),
/// so "battery attached" (VBAT in a plausible cell range) is the proxy:
/// with a pack attached the icon shows its level, without one USB is the
/// only possible source and a plug glyph is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    Usb,
    Battery { pct: u32 },
}

pub fn draw_power(fb: &mut FrameBuffer, ps: &PowerState) {
    fn dot_rect(
        fb: &mut FrameBuffer,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        filled: bool,
    ) {
        for yy in 0..h {
            for xx in 0..w {
                if filled || xx == 0 || xx == w - 1 || yy == 0 || yy == h - 1 {
                    fb.set((x + xx) as usize, (y + yy) as usize, Color::Black);
                }
            }
        }
    }
    let (x0, y0) = (POWER_X, POWER_Y);
    match ps {
        PowerState::Usb => {
            dot_rect(fb, x0, y0 + 1, 2, 2, true); // prongs
            dot_rect(fb, x0, y0 + 7, 2, 2, true);
            dot_rect(fb, x0 + 3, y0, 8, 10, true); // body
            dot_rect(fb, x0 + 11, y0 + 4, 7, 2, true); // cord
        }
        PowerState::Battery { pct } => {
            dot_rect(fb, x0, y0, 23, 10, false); // body outline
            dot_rect(fb, x0 + 23, y0 + 3, 2, 4, true); // nub
            let fw = (*pct as i32).clamp(0, 100) * 19 / 100;
            if fw > 0 {
                dot_rect(fb, x0 + 2, y0 + 2, fw, 6, true);
            }
            let mut s: String<5> = String::new();
            let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{}%", pct));
            Text::new(
                &s,
                Point::new(x0 + 29, y0 + 9),
                MonoTextStyle::new(&FONT_9X15, Color::Black),
            )
            .draw(fb)
            .ok();
        }
    }
}

pub fn render_error(fb: &mut FrameBuffer, line1: &str, line2: &str) {
    fb.fill(Color::White);
    Rectangle::new(Point::new(0, 0), Size::new(200, BAND_H))
        .into_styled(PrimitiveStyle::with_fill(Color::Red))
        .draw(fb)
        .ok();
    text(fb, "Kimi Code Usage", TITLE_X, TITLE_Y, Color::White);
    text(fb, line1, 10, 92, Color::Red);
    text(fb, line2, 10, 116, Color::Black);
}
