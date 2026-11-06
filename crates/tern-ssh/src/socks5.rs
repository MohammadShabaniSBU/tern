//! Minimal SOCKS5 server side (S6-T05): no authentication, CONNECT only, IPv4/IPv6/domain.

use std::net::{Ipv4Addr, Ipv6Addr};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const REP_SUCCEEDED: u8 = 0x00;
pub const REP_GENERAL_FAILURE: u8 = 0x01;
pub const REP_HOST_UNREACHABLE: u8 = 0x04;
pub const REP_CONNECTION_REFUSED: u8 = 0x05;
pub const REP_COMMAND_NOT_SUPPORTED: u8 = 0x07;
pub const REP_ADDRESS_NOT_SUPPORTED: u8 = 0x08;

#[derive(Debug, PartialEq, Eq)]
pub enum Socks5Error {
    /// The client spoke something else (or hung up); drop the connection.
    Protocol,
    /// A well-formed request we cannot serve; reply with this code first.
    Reply(u8),
}

impl From<std::io::Error> for Socks5Error {
    fn from(_: std::io::Error) -> Self {
        Self::Protocol
    }
}

/// Negotiate the (only) no-auth method and read the CONNECT request. Returns the destination.
/// The final reply is left to the caller (see [`reply`]).
pub async fn handshake<S>(stream: &mut S) -> Result<(String, u16), Socks5Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await?;
    if head[0] != 5 {
        return Err(Socks5Error::Protocol);
    }
    let mut methods = vec![0u8; usize::from(head[1])];
    stream.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        stream.write_all(&[5, 0xff]).await?;
        return Err(Socks5Error::Protocol);
    }
    stream.write_all(&[5, 0]).await?;

    let mut req = [0u8; 4];
    stream.read_exact(&mut req).await?;
    if req[0] != 5 {
        return Err(Socks5Error::Protocol);
    }
    let host = match req[3] {
        1 => {
            let mut a = [0u8; 4];
            stream.read_exact(&mut a).await?;
            Ipv4Addr::from(a).to_string()
        }
        3 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut name = vec![0u8; usize::from(len[0])];
            stream.read_exact(&mut name).await?;
            String::from_utf8(name).map_err(|_| Socks5Error::Protocol)?
        }
        4 => {
            let mut a = [0u8; 16];
            stream.read_exact(&mut a).await?;
            Ipv6Addr::from(a).to_string()
        }
        _ => return Err(Socks5Error::Reply(REP_ADDRESS_NOT_SUPPORTED)),
    };
    let mut port = [0u8; 2];
    stream.read_exact(&mut port).await?;
    if req[1] != 1 {
        return Err(Socks5Error::Reply(REP_COMMAND_NOT_SUPPORTED));
    }
    Ok((host, u16::from_be_bytes(port)))
}

/// Send the reply with a zeroed bound address.
pub async fn reply<S: AsyncWrite + Unpin>(stream: &mut S, code: u8) -> std::io::Result<()> {
    stream.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn run(request: &[u8]) -> (Result<(String, u16), Socks5Error>, Vec<u8>) {
        let (mut client, mut server) = duplex(256);
        client.write_all(request).await.unwrap();
        client.shutdown().await.unwrap();
        let res = handshake(&mut server).await;
        drop(server);
        let mut out = Vec::new();
        client.read_to_end(&mut out).await.unwrap();
        (res, out)
    }

    #[tokio::test]
    async fn domain_connect() {
        let mut req = vec![5, 1, 0, 5, 1, 0, 3, 11];
        req.extend_from_slice(b"example.com");
        req.extend_from_slice(&443u16.to_be_bytes());
        let (res, out) = run(&req).await;
        assert_eq!(res, Ok(("example.com".into(), 443)));
        assert_eq!(out, [5, 0]);
    }

    #[tokio::test]
    async fn ipv4_and_ipv6() {
        let (res, _) = run(&[5, 1, 0, 5, 1, 0, 1, 10, 0, 0, 7, 0, 80]).await;
        assert_eq!(res, Ok(("10.0.0.7".into(), 80)));
        let mut req = vec![5, 1, 0, 5, 1, 0, 4];
        req.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        req.extend_from_slice(&22u16.to_be_bytes());
        let (res, _) = run(&req).await;
        assert_eq!(res, Ok(("::1".into(), 22)));
    }

    #[tokio::test]
    async fn rejects_missing_no_auth_method_and_wrong_version() {
        let (res, out) = run(&[5, 1, 2]).await;
        assert_eq!(res, Err(Socks5Error::Protocol));
        assert_eq!(out, [5, 0xff]);
        let (res, _) = run(&[4, 1, 0]).await;
        assert_eq!(res, Err(Socks5Error::Protocol));
    }

    #[tokio::test]
    async fn bind_command_and_unknown_address_type_get_replies() {
        let (res, _) = run(&[5, 1, 0, 5, 2, 0, 1, 1, 1, 1, 1, 0, 1]).await;
        assert_eq!(res, Err(Socks5Error::Reply(REP_COMMAND_NOT_SUPPORTED)));
        let (res, _) = run(&[5, 1, 0, 5, 1, 0, 9]).await;
        assert_eq!(res, Err(Socks5Error::Reply(REP_ADDRESS_NOT_SUPPORTED)));
    }

    #[tokio::test]
    async fn truncated_request_is_a_protocol_error() {
        let (res, _) = run(&[5, 1, 0, 5, 1, 0, 3, 11, b'e']).await;
        assert_eq!(res, Err(Socks5Error::Protocol));
    }
}
