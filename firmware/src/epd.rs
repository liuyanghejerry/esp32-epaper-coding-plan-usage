//! Waveshare 1.54" 4-colour (G) e-paper driver (200x200, 2bpp).
//!
//! Ported command-for-command from Waveshare's reference `EPD_1in54g.c`
//! (repo: waveshareteam/ESP32-S3-ePaper-1.54G) — keep the register sequence
//! in sync with it if the panel misbehaves.

use core::convert::Infallible;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::raw::{RawData, RawU2},
    pixelcolor::PixelColor,
    prelude::Pixel,
};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiBus;
use esp_hal::delay::Delay;

pub const WIDTH: usize = 200;
pub const HEIGHT: usize = 200;
/// 2 bits per pixel, 4 pixels per byte, row-major.
pub const BUF_LEN: usize = WIDTH / 4 * HEIGHT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black = 0x0,
    White = 0x1,
    Yellow = 0x2,
    Red = 0x3,
}

impl From<Color> for u8 {
    fn from(c: Color) -> u8 {
        c as u8
    }
}

impl PixelColor for Color {
    type Raw = RawU2;
}

impl From<RawU2> for Color {
    fn from(raw: RawU2) -> Self {
        match raw.into_inner() {
            0 => Color::Black,
            1 => Color::White,
            2 => Color::Yellow,
            _ => Color::Red,
        }
    }
}

/// Packed 2bpp framebuffer, usable as an embedded-graphics DrawTarget.
pub struct FrameBuffer {
    pub buf: [u8; BUF_LEN],
}

impl FrameBuffer {
    pub fn new() -> Self {
        Self { buf: [0x55; BUF_LEN] } // all White
    }

    pub fn fill(&mut self, color: Color) {
        self.buf.fill((color as u8) * 0x55);
    }

    pub fn set(&mut self, x: usize, y: usize, color: Color) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }
        let idx = y * (WIDTH / 4) + x / 4;
        let shift = 6 - 2 * (x % 4);
        self.buf[idx] = (self.buf[idx] & !(0x3 << shift)) | ((color as u8) << shift);
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl DrawTarget for FrameBuffer {
    type Color = Color;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            let (x, y) = (coord.x as usize, coord.y as usize);
            self.set(x, y, color);
        }
        Ok(())
    }
}

/// Driver over any SPI bus + control GPIOs (CS/DC/RST active as documented).
pub struct Epd<SPI, CS, DC, RST, BUSY> {
    spi: SPI,
    cs: CS,
    dc: DC,
    rst: RST,
    busy: BUSY,
}

impl<SPI, CS, DC, RST, BUSY> Epd<SPI, CS, DC, RST, BUSY>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    pub fn new(spi: SPI, cs: CS, dc: DC, rst: RST, busy: BUSY) -> Self {
        Self { spi, cs, dc, rst, busy }
    }

    fn cmd(&mut self, reg: u8) {
        let _ = self.dc.set_low();
        let _ = self.cs.set_low();
        let _ = self.spi.write(&[reg]);
        let _ = self.cs.set_high();
    }

    fn data(&mut self, bytes: &[u8]) {
        let _ = self.dc.set_high();
        let _ = self.cs.set_low();
        let _ = self.spi.write(bytes);
        let _ = self.cs.set_high();
    }

    fn data1(&mut self, b: u8) {
        self.data(&[b]);
    }

    /// Hardware reset, timing per the shipped epaper_port.c (20 ms low pulse).
    pub fn reset(&mut self) {
        let delay = Delay::new();
        let _ = self.rst.set_high();
        delay.delay_millis(200);
        let _ = self.rst.set_low();
        delay.delay_millis(20);
        let _ = self.rst.set_high();
        delay.delay_millis(200);
    }

    /// Raw BUSY level (true = HIGH = idle, per the reference driver).
    pub fn busy_is_high(&mut self) -> bool {
        self.busy.is_high().unwrap_or(false)
    }

    /// BUSY idles HIGH, pulled LOW while the panel works.
    /// Polls every 10 ms like `epaper_readbusyh`, but with a timeout so a
    /// dead panel can't hang the boot. Returns true when it saw HIGH.
    fn wait_busy(&mut self, max_ms: u32) -> bool {
        let delay = Delay::new();
        let mut waited: u32 = 0;
        while !self.busy_is_high() {
            if waited >= max_ms {
                return false;
            }
            delay.delay_millis(10);
            waited += 10;
        }
        true
    }

    /// Full (non-fast) initialisation, register-for-register from the reference.
    pub fn init(&mut self) {
        self.reset();

        self.cmd(0x4D);
        self.data1(0x78);

        self.cmd(0x00); // PSR
        self.data1(0x0F);
        self.data1(0x29);

        self.cmd(0x06); // BTST_P
        for v in [0x0D, 0x12, 0x30, 0x20, 0x19, 0x2A, 0x22] {
            self.data1(v);
        }

        self.cmd(0x50); // CDI
        self.data1(0x37);

        self.cmd(0x61); // TRES 200x200
        self.data1((WIDTH / 256) as u8);
        self.data1((WIDTH % 256) as u8);
        self.data1((HEIGHT / 256) as u8);
        self.data1((HEIGHT % 256) as u8);

        self.cmd(0xE9);
        self.data1(0x01);

        self.cmd(0x30);
        self.data1(0x08);

        self.cmd(0x04); // power on
        if !self.wait_busy(5_000) {
            log::warn!("busy timeout after power-on (0x04) — panel not responding?");
        }
    }

    /// Push a full framebuffer (command 0x10) and refresh (0x12).
    /// A full refresh takes ~20 s. Returns true when BUSY released in time.
    pub fn display(&mut self, fb: &[u8; BUF_LEN]) -> bool {
        self.cmd(0x10);
        self.data(fb);

        self.cmd(0x12);
        self.data1(0x00);
        self.wait_busy(40_000)
    }

    /// Power off + deep sleep the panel; the image stays on screen.
    pub fn sleep(&mut self) {
        self.cmd(0x02); // power off
        self.data1(0x00);
        let _ = self.wait_busy(5_000);
        self.cmd(0x07); // deep sleep
        self.data1(0xA5);
    }
}
