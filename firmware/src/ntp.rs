//! Minimal SNTP client + wall-clock formatting.
//!
//! The ESP32-S3 has no battery-backed RTC — every boot starts at t=0, and an
//! e-paper keeps its last image when powered off, so a static usage view
//! can't tell "running" from "dead". One small NTP query per poll cycle
//! (5 min) keeps a "HH:MM" clock in the panel's bottom-right corner; between
//! syncs the embassy monotonic timer carries the time (ppm-level drift is
//! irrelevant at minute granularity). Displayed timezone is UTC+8, matching
//! the reset times from `usage::fmt_reset_utc8`.

use embassy_net::{
    dns::DnsQueryType,
    udp::{PacketMetadata, UdpSocket},
    IpEndpoint, Stack,
};
use embassy_time::{with_timeout, Duration};
use heapless::String;
use log::warn;

/// NTP servers: a China-local one first, the global pool as fallback.
const NTP_HOSTS: [&str; 2] = ["ntp.aliyun.com", "pool.ntp.org"];
const NTP_PORT: u16 = 123;
/// Seconds between the NTP epoch (1900) and the Unix epoch (1970).
const NTP_UNIX_DELTA: u64 = 2_208_988_800;
/// Display timezone, same as `usage::fmt_reset_utc8`.
const TZ_OFFSET_SECS: u64 = 8 * 3600;

#[derive(Debug)]
pub enum NtpError {
    Net,
    Timeout,
    BadReply,
}

/// One SNTPv4 client query; returns Unix epoch seconds for "now".
pub async fn fetch_epoch(stack: Stack<'_>) -> Result<u64, NtpError> {
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx_buf = [0u8; 128];
    let mut tx_buf = [0u8; 48];
    let mut socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buf,
        &mut tx_meta,
        &mut tx_buf,
    );
    socket.bind(45001).map_err(|_| NtpError::Net)?;

    // LI=0, VN=4, Mode=3 (client); the rest of the 48-byte request is zeros.
    let mut req = [0u8; 48];
    req[0] = 0x1b;

    for host in NTP_HOSTS {
        let ips = match stack.dns_query(host, DnsQueryType::A).await {
            Ok(ips) if !ips.is_empty() => ips,
            _ => {
                warn!("ntp: dns lookup failed for {host}");
                continue;
            }
        };
        let remote = IpEndpoint::new(ips[0], NTP_PORT);

        let query = async {
            socket.send_to(&req, remote).await.map_err(|e| {
                warn!("ntp: send to {host} failed: {:?}", e);
                NtpError::Net
            })?;
            let mut rx = [0u8; 128];
            let (n, _) = with_timeout(Duration::from_secs(4), socket.recv_from(&mut rx))
                .await
                .map_err(|_| {
                    warn!("ntp: reply timeout from {host}");
                    NtpError::Timeout
                })?
                .map_err(|e| {
                    warn!("ntp: recv from {host} failed: {:?}", e);
                    NtpError::Net
                })?;
            parse_reply(&rx[..n]).ok_or(NtpError::BadReply)
        };
        match with_timeout(Duration::from_secs(8), query).await {
            Ok(Ok(epoch)) => return Ok(epoch),
            Ok(Err(e)) => warn!("ntp: {host}: {e:?}"),
            Err(_) => warn!("ntp: {host}: attempt timeout"),
        }
    }
    Err(NtpError::Net)
}

/// 48-byte SNTP reply; bytes 40..44 = Transmit Timestamp (seconds, big-endian,
/// 1900-based). Mode must be "server".
fn parse_reply(pkt: &[u8]) -> Option<u64> {
    if pkt.len() < 48 || pkt[0] & 0x07 != 0x04 {
        return None;
    }
    let secs = u32::from_be_bytes([pkt[40], pkt[41], pkt[42], pkt[43]]) as u64;
    let epoch = secs.checked_sub(NTP_UNIX_DELTA)?;
    // Sanity window: 2024-01-01 ..= 2099-01-01, so a garbage reply can't set
    // the clock to 1970.
    (1_704_067_200..=4_082_244_480).contains(&epoch).then_some(epoch)
}

/// Unix epoch seconds -> "HH:MM" in UTC+8.
pub fn fmt_hhmm(epoch: u64) -> String<5> {
    let secs_of_day = (epoch + TZ_OFFSET_SECS) % 86_400;
    let (h, m) = (secs_of_day / 3600, secs_of_day % 3600 / 60);
    let mut s: String<5> = String::new();
    let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{:02}:{:02}", h, m));
    s
}
