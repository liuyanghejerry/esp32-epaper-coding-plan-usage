//! Render the usage view onto the 200x200 4-colour framebuffer.
//!
//! Layout (y in px):
//!   0..28   black header band, white "Kimi Code Usage"
//!   38      membership level (red)
//!   62      "Week" + right-aligned "used/limit (pct%)"
//!   80      weekly progress bar (red when >= 80 %, black otherwise)
//!   104     weekly reset time (UTC+8)
//!   128     rolling window label + numbers
//!   146     rolling window progress bar
//!   180     thin yellow accent line

use core::fmt::Write as _;

use embedded_graphics::{
    mono_font::{
        ascii::{FONT_10X20, FONT_9X15},
        MonoTextStyle,
    },
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use heapless::String;

use crate::epd::{Color, FrameBuffer};
use crate::usage::UsageView;

fn pct(used: u32, limit: u32) -> u32 {
    if limit == 0 {
        0
    } else {
        (used * 100 / limit).min(100)
    }
}

fn bar(fb: &mut FrameBuffer, y: i32, used: u32, limit: u32) {
    const X: i32 = 10;
    const W: u32 = 180;
    const H: u32 = 14;
    // outline
    Rectangle::new(Point::new(X, y), Size::new(W, H))
        .into_styled(PrimitiveStyle::with_stroke(Color::Black, 1))
        .draw(fb)
        .ok();
    // fill
    let p = pct(used, limit);
    let fill_w = ((W - 2) * p / 100).max(if p > 0 { 2 } else { 0 });
    let color = if p >= 80 { Color::Red } else { Color::Black };
    if fill_w > 0 {
        Rectangle::new(Point::new(X + 1, y + 1), Size::new(fill_w, H - 2))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(fb)
            .ok();
    }
}

/// Right-aligned 9x15 text (9 px per cell).
fn text_right(fb: &mut FrameBuffer, s: &str, y: i32, color: Color) {
    let w = s.len() as i32 * 9;
    Text::new(s, Point::new(190 - w, y), MonoTextStyle::new(&FONT_9X15, color))
        .draw(fb)
        .ok();
}

fn text(fb: &mut FrameBuffer, s: &str, x: i32, y: i32, color: Color, big: bool) {
    if big {
        Text::new(s, Point::new(x, y), MonoTextStyle::new(&FONT_10X20, color))
            .draw(fb)
            .ok();
    } else {
        Text::new(s, Point::new(x, y), MonoTextStyle::new(&FONT_9X15, color))
            .draw(fb)
            .ok();
    }
}

pub fn render(fb: &mut FrameBuffer, u: &UsageView) {
    fb.fill(Color::White);

    // header band
    Rectangle::new(Point::new(0, 0), Size::new(200, 28))
        .into_styled(PrimitiveStyle::with_fill(Color::Black))
        .draw(fb)
        .ok();
    text(fb, "Kimi Code Usage", 8, 4, Color::White, true);

    let mut s: String<32> = String::new();

    // membership level
    s.clear();
    let _ = write!(s, "{} plan", u.level);
    text(fb, &s, 10, 38, Color::Red, false);

    // weekly quota
    s.clear();
    let _ = write!(
        s,
        "{}/{} ({}%)",
        u.week_used,
        u.week_limit,
        pct(u.week_used, u.week_limit)
    );
    text(fb, "Week", 10, 62, Color::Black, false);
    text_right(fb, &s, 62, Color::Black);
    bar(fb, 80, u.week_used, u.week_limit);

    s.clear();
    let _ = write!(s, "reset {}", u.week_reset);
    text(fb, &s, 10, 104, Color::Black, false);

    // rolling window
    let hours = u.win_minutes / 60;
    s.clear();
    if u.win_minutes >= 60 && u.win_minutes % 60 == 0 {
        let _ = write!(s, "{}h window", hours);
    } else {
        let _ = write!(s, "{}min window", u.win_minutes);
    }
    text(fb, &s, 10, 128, Color::Black, false);
    s.clear();
    let _ = write!(
        s,
        "{}/{} ({}%)",
        u.win_used,
        u.win_limit,
        pct(u.win_used, u.win_limit)
    );
    text_right(fb, &s, 128, Color::Black);
    bar(fb, 146, u.win_used, u.win_limit);

    // yellow accent line
    Rectangle::new(Point::new(10, 182), Size::new(180, 3))
        .into_styled(PrimitiveStyle::with_fill(Color::Yellow))
        .draw(fb)
        .ok();
}

pub fn render_error(fb: &mut FrameBuffer, line1: &str, line2: &str) {
    fb.fill(Color::White);
    Rectangle::new(Point::new(0, 0), Size::new(200, 28))
        .into_styled(PrimitiveStyle::with_fill(Color::Red))
        .draw(fb)
        .ok();
    text(fb, "Kimi Code Usage", 8, 4, Color::White, true);
    text(fb, line1, 10, 80, Color::Red, false);
    text(fb, line2, 10, 104, Color::Black, false);
}
