//! Kimi Code plan usage client.
//!
//! Mirrors what the CLI's `/usage` does (see the `fetchManagedUsage` helper in
//! the CLI bundle):
//!
//! ```text
//! GET https://api.kimi.com/coding/v1/usages
//! Authorization: Bearer <token>     Accept: application/json
//! ```
//!
//! Numbers in the response are strings (`"limit": "100"`). We only extract
//! what fits on the 200x200 panel:
//!
//! ```json
//! {
//!   "user":  {"membership": {"level": "LEVEL_ADVANCED"}},
//!   "usage": {"limit": "100", "used": "92", "resetTime": "2026-09-05T15:59:23Z"},
//!   "limits": [{"window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
//!               "detail": {"limit": "100", "used": "23",
//!                          "resetTime": "2026-09-03T15:59:23Z", ...}}]
//! }
//!
//! `user`/`membership` 被渲染器忽略（界面已不显示等级行），解析时保留字段即可。
//! ```

use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    Stack,
};
use heapless::{String, Vec};
use log::warn;
use reqwless::{
    client::{HttpClient, TlsConfig, TlsVerify},
    request::{Method, RequestBuilder},
};

use crate::secrets;

pub const USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";

/// Buffers for one HTTPS request at a time. Created once in `main` — the TLS
/// record buffer alone is 16 KiB, way too big for a task stack.
pub struct Buffers {
    pub tcp: TcpClientState<1, 4096, 4096>,
    pub tls_read: [u8; 16640],
    pub tls_write: [u8; 4096],
    pub http: [u8; 8192],
}

impl Buffers {
    pub fn new() -> Self {
        Self {
            tcp: TcpClientState::new(),
            tls_read: [0; 16640],
            tls_write: [0; 4096],
            http: [0; 8192],
        }
    }
}

/// What the panel shows. Owned, comparable — the panel is only refreshed when
/// this changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageView {
    /// Membership level, e.g. "ADVANCED" ("LEVEL_" prefix stripped).
    pub level: String<16>,
    /// Weekly quota.
    pub week_used: u32,
    pub week_limit: u32,
    /// Weekly reset time rendered as "MM-DD HH:mm" in UTC+8.
    pub week_reset: String<16>,
    /// Rolling rate-limit window (e.g. 300 = 5h). 0 if the account has none.
    pub win_minutes: u32,
    pub win_used: u32,
    pub win_limit: u32,
    /// Rolling window reset time rendered as "MM-DD HH:mm" in UTC+8.
    pub win_reset: String<16>,
}

#[derive(Debug)]
pub enum FetchError {
    Net,
    Http(u16),
    Body,
    Parse,
    BadData,
}

pub async fn fetch_usage(
    stack: Stack<'_>,
    bufs: &mut Buffers,
    seed: u64,
) -> Result<UsageView, FetchError> {
    let tcp = TcpClient::new(stack, &mut bufs.tcp);
    let dns = DnsSocket::new(stack);
    // NOTE: no certificate verification — fine for a read-only quota display
    // on a trusted LAN, but the token would be visible to a MITM.
    let tls = TlsConfig::new(seed, &mut bufs.tls_read, &mut bufs.tls_write, TlsVerify::None);
    let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);

    let mut auth: String<768> = String::new();
    core::fmt::Write::write_fmt(&mut auth, format_args!("Bearer {}", secrets::KIMI_TOKEN))
        .map_err(|_| FetchError::BadData)?;

    let handle = client
        .request(Method::GET, USAGE_URL)
        .await
        .map_err(|e| {
            warn!("usage: connect/request failed: {:?}", e);
            FetchError::Net
        })?;

    let headers = [
        ("Authorization", auth.as_str()),
        ("Accept", "application/json"),
        ("Connection", "close"),
    ];
    let mut request = handle.headers(&headers);
    let response = request
        .send(&mut bufs.http)
        .await
        .map_err(|e| {
            warn!("usage: send/response failed: {:?}", e);
            FetchError::Net
        })?;

    if !response.status.is_successful() {
        return Err(FetchError::Http(response.status.0));
    }

    let body = response
        .body()
        .read_to_end()
        .await
        .map_err(|_| FetchError::Body)?;
    let text = core::str::from_utf8(body).map_err(|_| FetchError::Parse)?;
    parse(text)
}

// ---- response parsing (serde-json-core, zero alloc) ----

#[derive(serde::Deserialize)]
struct RawResponse<'a> {
    #[serde(borrow)]
    user: Option<RawUser<'a>>,
    #[serde(borrow)]
    usage: Option<RawQuota<'a>>,
    #[serde(borrow)]
    limits: Vec<RawWindowed<'a>, 4>,
}

#[derive(serde::Deserialize)]
struct RawUser<'a> {
    #[serde(borrow)]
    membership: Option<RawMembership<'a>>,
}

#[derive(serde::Deserialize)]
struct RawMembership<'a> {
    level: Option<&'a str>,
}

#[derive(serde::Deserialize)]
struct RawQuota<'a> {
    limit: Option<&'a str>,
    used: Option<&'a str>,
    #[serde(rename = "resetTime")]
    reset_time: Option<&'a str>,
}

#[derive(serde::Deserialize)]
struct RawWindowed<'a> {
    window: Option<RawWindow<'a>>,
    #[serde(borrow)]
    detail: Option<RawQuota<'a>>,
}

#[derive(serde::Deserialize)]
struct RawWindow<'a> {
    duration: Option<u32>,
    #[serde(rename = "timeUnit")]
    time_unit: Option<&'a str>,
}

fn num(s: Option<&str>) -> Option<u32> {
    s?.parse().ok()
}

fn parse(text: &str) -> Result<UsageView, FetchError> {
    let (raw, _): (RawResponse, usize) =
        serde_json_core::from_str(text).map_err(|_| FetchError::Parse)?;

    let week = raw.usage.ok_or(FetchError::BadData)?;
    let level = raw
        .user
        .and_then(|u| u.membership)
        .and_then(|m| m.level)
        .unwrap_or("")
        .strip_prefix("LEVEL_")
        .unwrap_or("");

    let mut level_s: String<16> = String::new();
    let _ = level_s.push_str(level);

    let mut view = UsageView {
        level: level_s,
        week_used: num(week.used).ok_or(FetchError::BadData)?,
        week_limit: num(week.limit).ok_or(FetchError::BadData)?,
        week_reset: fmt_reset_utc8(week.reset_time.unwrap_or("")),
        win_minutes: 0,
        win_used: 0,
        win_limit: 0,
        win_reset: String::new(),
    };

    if let Some(w) = raw.limits.into_iter().next() {
        if let (Some(win), Some(detail)) = (w.window, w.detail) {
            let minutes = match win.time_unit.unwrap_or("") {
                "TIME_UNIT_MINUTE" => win.duration.unwrap_or(0),
                "TIME_UNIT_HOUR" => win.duration.unwrap_or(0) * 60,
                _ => 0,
            };
            view.win_minutes = minutes;
            view.win_used = num(detail.used).unwrap_or(0);
            view.win_limit = num(detail.limit).unwrap_or(0);
            view.win_reset = fmt_reset_utc8(detail.reset_time.unwrap_or(""));
        }
    }

    Ok(view)
}

/// "2026-09-05T15:59:23.059441Z" (UTC) -> "09-05 23:59" (UTC+8).
/// Returns an empty string when the input doesn't look like an ISO timestamp.
fn fmt_reset_utc8(iso: &str) -> String<16> {
    let mut out: String<16> = String::new();
    let b = iso.as_bytes();
    if b.len() < 16 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return out;
    }
    let d2 = |i: usize| -> Option<u32> {
        let (hi, lo) = (b[i].wrapping_sub(b'0'), b[i + 1].wrapping_sub(b'0'));
        if hi > 9 || lo > 9 {
            None
        } else {
            Some(hi as u32 * 10 + lo as u32)
        }
    };
    let (Some(y4), Some(mo), Some(mut d), Some(mut h), Some(mi)) = (
        d2(0).map(|a| a * 100 + d2(2).unwrap_or(0)),
        d2(5),
        d2(8),
        d2(11),
        d2(14),
    ) else {
        return out;
    };
    let y = 2000 + y4; // good enough until 2100

    h += 8; // UTC -> UTC+8
    if h >= 24 {
        h -= 24;
        d += 1;
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let dim = match mo {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            _ => 28,
        };
        if d > dim {
            d = 1; // month (and possibly year) rolls over; only shown as MM-DD
        }
    }

    let _ = core::fmt::Write::write_fmt(
        &mut out,
        format_args!("{:02}-{:02} {:02}:{:02}", mo, d, h, mi),
    );
    out
}
