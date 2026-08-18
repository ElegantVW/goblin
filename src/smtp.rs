//! SMTP submit over rustls. TLS required.

use crate::config::Account;
use crate::error::Error;
use crate::tls::{self, TlsMode};
use lettre::address::{Address, Envelope};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

pub fn validate(port: u16) -> Result<(), Error> {
    tls::smtp_mode(port).map(|_| ())
}

pub fn send(account: &Account, password: &str, to: &[String], rfc5322: &[u8]) -> Result<(), Error> {
    tls::install_crypto();
    validate(account.smtp.port)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::Smtp(e.to_string()))?;
    rt.block_on(send_async(account, password, to, rfc5322))
}

async fn send_async(
    account: &Account,
    password: &str,
    to: &[String],
    rfc5322: &[u8],
) -> Result<(), Error> {
    let mode = tls::smtp_mode(account.smtp.port)?;
    let host = account.smtp.host.clone();
    let tls_params = TlsParameters::builder(host.clone())
        .build_rustls()
        .map_err(|e| Error::Smtp(format!("tls params: {e}")))?;
    let creds = Credentials::new(account.smtp.user.clone(), password.to_string());

    let builder = match mode {
        TlsMode::Implicit => AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
            .map_err(|e| Error::Smtp(e.to_string()))?
            .port(account.smtp.port)
            .tls(Tls::Wrapper(tls_params)),
        TlsMode::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
            .map_err(|e| Error::Smtp(e.to_string()))?
            .port(account.smtp.port)
            .tls(Tls::Required(tls_params)),
    };
    let mailer = builder.credentials(creds).build();

    let from: Address = parse_addr(&account.from)?;
    let mut rcpts = Vec::new();
    for t in to {
        rcpts.push(parse_addr(t)?);
    }
    if rcpts.is_empty() {
        return Err(Error::Usage("no recipients".into()));
    }
    let envelope = Envelope::new(Some(from), rcpts).map_err(|e| Error::Smtp(e.to_string()))?;
    mailer
        .send_raw(&envelope, rfc5322)
        .await
        .map_err(|e| Error::Smtp(e.to_string()))?;
    Ok(())
}

fn parse_addr(s: &str) -> Result<Address, Error> {
    let s = s.trim();
    let inner = if let (Some(a), Some(b)) = (s.find('<'), s.rfind('>')) {
        &s[a + 1..b]
    } else {
        s
    };
    inner
        .parse::<Address>()
        .map_err(|e| Error::Usage(format!("bad address {s:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_refuses_port_25() {
        match validate(25) {
            Err(Error::TlsPolicy(s)) => assert!(s.contains("25"), "{s}"),
            other => panic!("{other:?}"),
        }
        assert!(validate(465).is_ok());
        assert!(validate(587).is_ok());
        assert!(validate(2525).is_err());
    }
}
