//! Render the usage view onto the 200x200 4-colour framebuffer.
//!
//! Layout (y in px, text = 9x15 monospace):
//!   0..28   black header band, white "Kimi Code Usage" at y=13 (flush with the
//!           band's bottom edge — the panel's top rows are physically clipped)
//!   47      "Week" + right-aligned "used/limit (pct%)"
//!   62      weekly progress bar (red when >= 80 %, black otherwise)
//!   86      "reset MM-DD HH:mm" (weekly reset time, UTC+8)
//!   121     "5h" (or "Nmin") + right-aligned rolling-window numbers
//!   136     rolling window progress bar
//!   160     "reset MM-DD HH:mm" (window reset time, UTC+8)

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

fn text(fb: &mut FrameBuffer, s: &str, x: i32, y: i32, color: Color) {
    Text::new(s, Point::new(x, y), MonoTextStyle::new(&FONT_9X15, color))
        .draw(fb)
        .ok();
}

pub fn render(fb: &mut FrameBuffer, u: &UsageView) {
    fb.fill(Color::White);

    // header band — unchanged 28px tall; title sits flush against its bottom
    // edge (y=13): the panel's top rows are physically clipped, so the title
    // must stay as low as possible without leaving the band.
    Rectangle::new(Point::new(0, 0), Size::new(200, 28))
        .into_styled(PrimitiveStyle::with_fill(Color::Black))
        .draw(fb)
        .ok();
    text(fb, "Kimi Code Usage", 10, 13, Color::White);

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
    text(fb, "Week", 10, 47, Color::Black);
    text_right(fb, &s, 47, Color::Black);
    bar(fb, 62, u.week_used, u.week_limit);

    s.clear();
    let _ = write!(s, "reset {}", u.week_reset);
    text(fb, &s, 10, 86, Color::Black);

    // rolling window
    s.clear();
    if u.win_minutes >= 60 && u.win_minutes % 60 == 0 {
        let _ = write!(s, "{}h", u.win_minutes / 60);
    } else {
        let _ = write!(s, "{}min", u.win_minutes);
    }
    text(fb, &s, 10, 121, Color::Black);
    s.clear();
    let _ = write!(
        s,
        "{}/{} ({}%)",
        u.win_used,
        u.win_limit,
        pct(u.win_used, u.win_limit)
    );
    text_right(fb, &s, 121, Color::Black);
    bar(fb, 136, u.win_used, u.win_limit);

    s.clear();
    let _ = write!(s, "reset {}", u.win_reset);
    text(fb, &s, 10, 160, Color::Black);
}

pub fn render_error(fb: &mut FrameBuffer, line1: &str, line2: &str) {
    fb.fill(Color::White);
    Rectangle::new(Point::new(0, 0), Size::new(200, 28))
        .into_styled(PrimitiveStyle::with_fill(Color::Red))
        .draw(fb)
        .ok();
    text(fb, "Kimi Code Usage", 10, 13, Color::White);
    text(fb, line1, 10, 92, Color::Red);
    text(fb, line2, 10, 116, Color::Black);
}
