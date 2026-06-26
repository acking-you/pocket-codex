//! TLS termination: plain (none) or static cert files. The same layer fronts
//! both the HTTP API and the broker.
//!
//! For a self-contained Let's Encrypt deploy, obtain certs out of band (certbot)
//! and point `tls_cert`/`tls_key` at the live PEMs; the backend reloads them on
//! restart.

use std::sync::Arc;

use anyhow::Context as _;
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer},
    ServerConfig,
};
use tokio_rustls::TlsAcceptor;

use crate::config::{ServerConfig as Cfg, TlsMode};

/// How accepted TCP connections are wrapped before use.
#[derive(Clone)]
pub enum TlsKind {
    /// Plain TCP, no TLS.
    Plain,
    /// Static certificate from PEM files.
    Static(TlsAcceptor),
}

/// Build the TLS layer from config.
pub fn build_tls(cfg: &Cfg) -> anyhow::Result<TlsKind> {
    match cfg.tls_mode {
        TlsMode::Plain => Ok(TlsKind::Plain),
        TlsMode::Files => {
            let cert = cfg
                .tls_cert
                .as_deref()
                .context("tls_mode = \"files\" requires tls_cert")?;
            let key = cfg
                .tls_key
                .as_deref()
                .context("tls_mode = \"files\" requires tls_key")?;
            Ok(TlsKind::Static(static_acceptor(cert, key)?))
        }
    }
}

fn static_acceptor(cert_path: &str, key_path: &str) -> anyhow::Result<TlsAcceptor> {
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(load_certs(cert_path)?, load_key(key_path)?)
        .context("building TLS server config")?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

fn load_certs(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let data = std::fs::read(path).with_context(|| format!("reading cert {path}"))?;
    let certs = rustls_pemfile::certs(&mut &data[..]).collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(!certs.is_empty(), "no certificates in {path}");
    Ok(certs)
}

fn load_key(path: &str) -> anyhow::Result<PrivateKeyDer<'static>> {
    let data = std::fs::read(path).with_context(|| format!("reading key {path}"))?;
    rustls_pemfile::private_key(&mut &data[..])?.with_context(|| format!("no private key in {path}"))
}
