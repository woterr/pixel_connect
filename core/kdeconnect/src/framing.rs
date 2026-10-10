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
