//! Lab TLS: a small CA + server cert under GOBLIND_HOME/tls/.
//! Production later swaps these for Let’s Encrypt; the client still verifies.

use super::paths;
use crate::error::Error;
use crate::fsutil;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SANS: &[&str] = &["localhost", "127.0.0.1", "mail.vanguardaautomovel.com"];

pub fn ca_file() -> PathBuf {
    paths::tls_dir().join("ca.pem")
}

pub fn cert_file() -> PathBuf {
    paths::tls_dir().join("cert.pem")
}

pub fn key_file() -> PathBuf {
    paths::tls_dir().join("key.pem")
}

pub fn ensure_certs() -> Result<(), Error> {
    paths::ensure_layout()?;
    if ca_file().is_file() && cert_file().is_file() && key_file().is_file() {
        return Ok(());
    }
    generate_lab_certs()
}

pub fn server_config() -> Result<Arc<rustls::ServerConfig>, Error> {
    crate::tls::install_crypto();
    ensure_certs()?;
    let certs = load_certs(&cert_file())?;
    if certs.is_empty() {
        return Err(Error::TlsPolicy(format!(
            "no certificates in {}",
            cert_file().display()
        )));
    }
    let key = load_key(&key_file())?;
    let cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| Error::TlsPolicy(format!("server cert: {e}")))?;
    Ok(Arc::new(cfg))
}

fn generate_lab_certs() -> Result<(), Error> {
    let mut ca_params = CertificateParams::default();
    ca_params.distinguished_name = DistinguishedName::new();
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "goblind lab CA");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let ca_key = KeyPair::generate().map_err(rcgen_err)?;
    let ca_cert = ca_params.self_signed(&ca_key).map_err(rcgen_err)?;

    let mut ee = CertificateParams::new(SANS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
        .map_err(rcgen_err)?;
    ee.distinguished_name = DistinguishedName::new();
    ee.distinguished_name
        .push(DnType::CommonName, "mail.vanguardaautomovel.com");
    ee.is_ca = IsCa::ExplicitNoCa;
    ee.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    ee.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    ee.use_authority_key_identifier_extension = true;
    let ee_key = KeyPair::generate().map_err(rcgen_err)?;
    let ee_cert = ee
        .signed_by(&ee_key, &ca_cert, &ca_key)
        .map_err(rcgen_err)?;

    fsutil::write_private(&ca_file(), ca_cert.pem().as_bytes())?;
    fsutil::write_private(&cert_file(), ee_cert.pem().as_bytes())?;
    fsutil::write_private(&key_file(), ee_key.serialize_pem().as_bytes())?;
    Ok(())
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, Error> {
    let mut r = BufReader::new(File::open(path)?);
    rustls_pemfile::certs(&mut r)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::TlsPolicy(format!("read {}: {e}", path.display())))
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>, Error> {
    let mut r = BufReader::new(File::open(path)?);
    rustls_pemfile::private_key(&mut r)
        .map_err(|e| Error::TlsPolicy(format!("read {}: {e}", path.display())))?
        .ok_or_else(|| Error::TlsPolicy(format!("no private key in {}", path.display())))
}

fn rcgen_err(e: rcgen::Error) -> Error {
    Error::TlsPolicy(format!("tls cert: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::paths::with_goblind_home;

    #[test]
    fn ensure_certs_writes_pems_once() {
        crate::tls::install_crypto();
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            ensure_certs().unwrap();
            for p in [ca_file(), cert_file(), key_file()] {
                assert!(p.is_file(), "{}", p.display());
                let text = std::fs::read_to_string(&p).unwrap();
                assert!(text.contains("BEGIN"), "{text}");
            }
            let ca = std::fs::read(ca_file()).unwrap();
            let cert = std::fs::read(cert_file()).unwrap();
            let key = std::fs::read(key_file()).unwrap();
            assert!(ca.starts_with(b"-----BEGIN CERTIFICATE-----"));
            assert!(cert.starts_with(b"-----BEGIN CERTIFICATE-----"));
            assert!(ca != cert);
            let cfg = server_config().unwrap();
            drop(cfg);
            ensure_certs().unwrap();
            assert_eq!(std::fs::read(ca_file()).unwrap(), ca);
            assert_eq!(std::fs::read(cert_file()).unwrap(), cert);
            assert_eq!(std::fs::read(key_file()).unwrap(), key);
            #[cfg(unix)]
            {
                assert_eq!(crate::fsutil::file_mode(&key_file()).unwrap(), Some(0o600));
            }
        });
    }

    #[test]
    fn client_with_extra_ca_handshakes() {
        crate::tls::install_crypto();
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            ensure_certs().unwrap();
            let ca = ca_file();
            crate::tls::with_extra_ca_env(Some(&ca), || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    use tokio::net::{TcpListener, TcpStream};
                    use tokio_rustls::TlsAcceptor;
                    let cfg = server_config().unwrap();
                    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let port = l.local_addr().unwrap().port();
                    let acceptor = TlsAcceptor::from(cfg);
                    tokio::spawn(async move {
                        let (s, _) = l.accept().await.unwrap();
                        let mut t = acceptor.accept(s).await.unwrap();
                        t.write_all(b"ok\n").await.unwrap();
                    });
                    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                    let mut t = crate::tls::wrap_tls("localhost", tcp).await.unwrap();
                    let mut buf = [0u8; 3];
                    t.read_exact(&mut buf).await.unwrap();
                    assert_eq!(&buf, b"ok\n");
                });
            });
        });
    }
}
