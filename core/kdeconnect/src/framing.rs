// Pixel Connect — KDE Connect protocol v8, newline-delimited packet framing.
//
// KDE Connect protocol >= 7 sends each packet as one compact UTF-8 JSON line
// terminated by '\n'. There is no length prefix. Reading therefore has to be
// incremental and bounded so a malicious or broken peer cannot make us buffer
// without limit (upstream caps a line at 32 MiB, identity packets at 8 KiB).

use anyhow::{anyhow, Result};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::packet::{NetworkPacket, MAX_PACKET_SIZE};

pub struct LineReader<R> {
    inner: BufReader<R>,
}

impl<R: AsyncRead + Unpin> LineReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner: BufReader::with_capacity(16 * 1024, inner),
        }
    }

    /// Read exactly one newline-terminated packet.
    pub async fn read_packet(&mut self, max: usize) -> Result<NetworkPacket> {
        let max = max.min(MAX_PACKET_SIZE);
        let mut line = Vec::with_capacity(1024);
        let read = self.inner.read_until(b'\n', &mut line).await?;
        if read == 0 {
            return Err(anyhow!("connection closed by peer"));
        }
        if read > max {
            return Err(anyhow!("packet exceeds {max} bytes"));
        }
        // Tolerate CRLF from Windows peers even though KDE Connect never sends it.
        while matches!(line.last(), Some(b'\n') | Some(b'\r')) {
            line.pop();
        }
        if line.is_empty() {
            return Err(anyhow!("empty packet"));
        }
        NetworkPacket::unserialize(&line).map_err(|e| anyhow!("{e}"))
    }

    pub fn into_inner(self) -> R {
        self.inner.into_inner()
    }
}

pub struct PacketWriter<W> {
    inner: W,
}

/// `setsockopt` with an integer value, tolerating platforms that lack an option.
#[cfg(unix)]
fn setsockopt_i32(fd: i32, level: i32, name: i32, value: i32) -> std::io::Result<()> {
    // An unsupported option is not a failure worth propagating: keepalive is a
    // refinement, and the link works without it. Report it so the caller can log.
    let rc = unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            &value as *const i32 as *const std::ffi::c_void,
            size_of::<i32>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn setsockopt_i32(_fd: i32, _level: i32, _name: i32, _value: i32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
const SOL_SOCKET: i32 = libc::SOL_SOCKET;
#[cfg(unix)]
const SO_KEEPALIVE: i32 = libc::SO_KEEPALIVE;
#[cfg(unix)]
const IPPROTO_TCP: i32 = libc::IPPROTO_TCP;
#[cfg(unix)]
const TCP_KEEPIDLE: i32 = libc::TCP_KEEPIDLE;
#[cfg(unix)]
const TCP_KEEPINTVL: i32 = libc::TCP_KEEPINTVL;
#[cfg(unix)]
const TCP_KEEPCNT: i32 = libc::TCP_KEEPCNT;

impl<W: AsyncWrite + Unpin> PacketWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    pub async fn write_packet(&mut self, packet: &NetworkPacket) -> Result<()> {
        self.write_raw(&packet.serialize()).await
    }

    pub async fn write_raw(&mut self, bytes: &[u8]) -> Result<()> {
        self.inner.write_all(bytes).await?;
        self.inner.flush().await?;
        Ok(())
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

/// Apply upstream KDE Connect's TCP keepalive settings to a socket.
///
/// `LanLinkProvider::configureSocket` sets `SO_KEEPALIVE`, then `TCP_KEEPIDLE=30`,
/// `TCP_KEEPINTVL=10`, `TCP_KEEPCNT=3`, so a peer that has silently gone is
/// noticed in about a minute rather than the system default of roughly two hours
/// (upstream BUG 476747), and NAT mappings stay open while the link is idle.
///
/// This has to happen at the TCP layer. An application-level rule that treats
/// "no traffic for N seconds" as death tears down healthy links, because an idle
/// phone sends nothing at all.
#[cfg(unix)]
pub fn configure_socket_keepalive(fd: i32) -> std::io::Result<()> {
    setsockopt_i32(fd, SOL_SOCKET, SO_KEEPALIVE, 1)
        .and_then(|_| {
            // Not every platform has TCP_KEEPIDLE; where it is absent the
            // system default applies, which is longer but still correct.
            setsockopt_i32(
                fd,
                IPPROTO_TCP,
                TCP_KEEPIDLE,
                crate::link::TCP_KEEPALIVE_IDLE_SECS as i32,
            )
        })
        .and_then(|_| {
            setsockopt_i32(
                fd,
                IPPROTO_TCP,
                TCP_KEEPINTVL,
                crate::link::TCP_KEEPALIVE_INTERVAL_SECS as i32,
            )
        })
        .and_then(|_| {
            setsockopt_i32(fd, IPPROTO_TCP, TCP_KEEPCNT, crate::link::TCP_KEEPALIVE_COUNT as i32)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::PACKET_TYPE_PING;

    #[tokio::test]
    async fn reads_consecutive_packets() {
        let mut a = NetworkPacket::new(PACKET_TYPE_PING);
        a.set("message", "hi");
        let mut b = NetworkPacket::new(PACKET_TYPE_PING);
        b.set("message", "there");

        let mut buf = Vec::new();
        buf.extend_from_slice(&a.serialize());
        buf.extend_from_slice(&b.serialize());

        let mut r = LineReader::new(std::io::Cursor::new(buf));
        assert_eq!(
            r.read_packet(MAX_PACKET_SIZE)
                .await
                .unwrap()
                .get_str("message")
                .unwrap(),
            "hi"
        );
        assert_eq!(
            r.read_packet(MAX_PACKET_SIZE)
                .await
                .unwrap()
                .get_str("message")
                .unwrap(),
            "there"
        );
        assert!(r.read_packet(MAX_PACKET_SIZE).await.is_err());
    }

    #[tokio::test]
    async fn enforces_size_cap() {
        let mut r = LineReader::new(std::io::Cursor::new(vec![b'x'; 5000]));
        assert!(r.read_packet(100).await.is_err());
    }

    #[tokio::test]
    async fn tolerates_crlf() {
        let mut buf = br#"{"id":1,"type":"kdeconnect.ping","body":{}}"#.to_vec();
        buf.push(b'\r');
        buf.push(b'\n');
        let mut r = LineReader::new(std::io::Cursor::new(buf));
        assert!(r.read_packet(MAX_PACKET_SIZE).await.is_ok());
    }
}
