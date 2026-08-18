//! Shared TLS policy and rustls client config. No insecure flag.

use crate::error::Error;
use rustls::pki_types::ServerName;
use std::sync::{Arc, Once};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

static INSTALL: Once = Once::new();

pub fn install_crypto() {
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    Implicit,
    StartTls,
}

pub fn smtp_mode(port: u16) -> Result<TlsMode, Error> {
    match port {
        465 => Ok(TlsMode::Implicit),
        587 => Ok(TlsMode::StartTls),
        25 => Err(Error::TlsPolicy(
            "smtp port 25 is refused; use 465 (implicit TLS) or 587 (STARTTLS)".into(),
        )),
        other => Err(Error::TlsPolicy(format!(
            "smtp port {other} is refused; use 465 (implicit TLS) or 587 (STARTTLS)"
        ))),
    }
}

pub fn imap_mode(port: u16) -> Result<TlsMode, Error> {
    match port {
        993 => Ok(TlsMode::Implicit),
        143 => Ok(TlsMode::StartTls),
        other => Err(Error::TlsPolicy(format!(
            "imap port {other} is refused; use 993 (implicit TLS) or 143 (STARTTLS)"
        ))),
    }
}

pub fn client_config() -> Arc<rustls::ClientConfig> {
    install_crypto();
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(cfg)
}

pub async fn wrap_tls(host: &str, stream: TcpStream) -> Result<TlsStream<TcpStream>, Error> {
    let name = ServerName::try_from(host.to_string())
        .map_err(|e| Error::TlsPolicy(format!("bad tls name {host}: {e}")))?;
    TlsConnector::from(client_config())
        .connect(name, stream)
        .await
        .map_err(|e| Error::TlsPolicy(format!("tls handshake: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smtp_refuses_cleartext() {
        match smtp_mode(25) {
            Err(Error::TlsPolicy(s)) => assert!(s.contains("25"), "{s}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(smtp_mode(465).unwrap(), TlsMode::Implicit);
        assert_eq!(smtp_mode(587).unwrap(), TlsMode::StartTls);
    }

    #[test]
    fn imap_refuses_non_tls_ports() {
        match imap_mode(143) {
            Ok(TlsMode::StartTls) => {}
            other => panic!("{other:?}"),
        }
        match imap_mode(80) {
            Err(Error::TlsPolicy(_)) => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(imap_mode(993).unwrap(), TlsMode::Implicit);
    }
}
