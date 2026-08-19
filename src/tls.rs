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

/// Build a verifying client config.
///
/// If `GOBLIN_EXTRA_CA` is set to a PEM path (lab: `$GOBLIND_HOME/tls/ca.pem`), those
/// certificates are added to the root store *in addition to* webpki_roots. Verification
/// stays on; this is not an insecure skip-verify flag.
pub fn client_config() -> Result<Arc<rustls::ClientConfig>, Error> {
    install_crypto();
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    if let Some(pem) = extra_ca_pem()? {
        add_extra_roots(&mut roots, &pem)?;
    }
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// Raw PEM bytes from `GOBLIN_EXTRA_CA`, if set.
pub fn extra_ca_pem() -> Result<Option<Vec<u8>>, Error> {
    let path = match std::env::var("GOBLIN_EXTRA_CA") {
        Ok(p) if !p.trim().is_empty() => p,
        _ => return Ok(None),
    };
    let bytes = std::fs::read(&path)
        .map_err(|e| Error::TlsPolicy(format!("GOBLIN_EXTRA_CA {path}: {e}")))?;
    Ok(Some(bytes))
}

fn add_extra_roots(roots: &mut rustls::RootCertStore, pem: &[u8]) -> Result<(), Error> {
    let mut r = std::io::Cursor::new(pem);
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> = rustls_pemfile::certs(&mut r)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::TlsPolicy(format!("GOBLIN_EXTRA_CA: {e}")))?;
    if certs.is_empty() {
        return Err(Error::TlsPolicy(
            "GOBLIN_EXTRA_CA contains no certificates".into(),
        ));
    }
    for c in certs {
        roots
            .add(c)
            .map_err(|e| Error::TlsPolicy(format!("GOBLIN_EXTRA_CA: {e}")))?;
    }
    Ok(())
}

pub async fn wrap_tls(host: &str, stream: TcpStream) -> Result<TlsStream<TcpStream>, Error> {
    let name = ServerName::try_from(host.to_string())
        .map_err(|e| Error::TlsPolicy(format!("bad tls name {host}: {e}")))?;
    TlsConnector::from(client_config()?)
        .connect(name, stream)
        .await
        .map_err(|e| Error::TlsPolicy(format!("tls handshake: {e}")))
}

#[cfg(test)]
pub(crate) fn with_extra_ca_env<R>(path: Option<&std::path::Path>, f: impl FnOnce() -> R) -> R {
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let prev = std::env::var_os("GOBLIN_EXTRA_CA");
    unsafe {
        match path {
            Some(p) => std::env::set_var("GOBLIN_EXTRA_CA", p),
            None => std::env::remove_var("GOBLIN_EXTRA_CA"),
        }
    }
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    unsafe {
        match prev {
            Some(v) => std::env::set_var("GOBLIN_EXTRA_CA", v),
            None => std::env::remove_var("GOBLIN_EXTRA_CA"),
        }
    }
    drop(guard);
    match out {
        Ok(v) => v,
        Err(p) => std::panic::resume_unwind(p),
    }
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

    #[test]
    fn extra_ca_is_loaded_into_client_config() {
        install_crypto();
        let dir = tempfile::tempdir().unwrap();
        crate::server::paths::with_goblind_home(Some(dir.path()), || {
            crate::server::tlsutil::ensure_certs().unwrap();
            let ca = crate::server::tlsutil::ca_file();
            with_extra_ca_env(Some(&ca), || {
                let pem = extra_ca_pem().unwrap().expect("pem");
                assert!(pem.starts_with(b"-----BEGIN CERTIFICATE-----"));
                client_config().unwrap();
            });
        });
    }
}
