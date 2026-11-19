use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::ServerConfig;
use serde::Serialize;
use time::OffsetDateTime;

use crate::error::AppError;
use crate::routing::engine::RuleEngine;

const SUBJECT: &str = "Local Endpoint Router Dev CA";

pub struct CertificateAuthority {
    cert_path: PathBuf,
    cert_der: CertificateDer<'static>,
    fingerprint: String,
    subject: String,
    issuer: Mutex<Option<Issuer<'static, KeyPair>>>,
    leaves: Mutex<HashMap<String, Arc<CertifiedKey>>>,
    unavailable: Mutex<Option<String>>,
    trust_error: Mutex<Option<String>>,
    trust_command_succeeded: Mutex<bool>,
}

impl fmt::Debug for CertificateAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CertificateAuthority")
            .field("fingerprint", &self.fingerprint)
            .field("subject", &self.subject)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaStatus {
    pub available: bool,
    pub subject: String,
    pub fingerprint: String,
    pub path: String,
    pub trust_error: Option<String>,
    pub trust_command_succeeded: bool,
    pub trusted: bool,
}

impl CertificateAuthority {
    pub fn open(dir: &Path) -> Arc<Self> {
        match load_or_create(dir) {
            Ok(authority) => authority,
            Err(err) => Arc::new(Self {
                cert_path: dir.join("dev-ca.pem"),
                cert_der: CertificateDer::from(Vec::new()),
                fingerprint: String::new(),
                subject: SUBJECT.into(),
                issuer: Mutex::new(None),
                leaves: Mutex::new(HashMap::new()),
                unavailable: Mutex::new(Some(err.to_string())),
                trust_error: Mutex::new(None),
                trust_command_succeeded: Mutex::new(false),
            }),
        }
    }

    pub fn status(&self) -> CaStatus {
        let available = self.unavailable().is_none();
        CaStatus {
            available,
            subject: self.subject.clone(),
            fingerprint: self.fingerprint.clone(),
            path: self.cert_path.display().to_string(),
            trust_error: self.trust_error.lock().expect("ca trust").clone(),
            trust_command_succeeded: *self.trust_command_succeeded.lock().expect("ca trust"),
            trusted: available && crate::tls::trust::certificate_trusted(&self.cert_path, &self.fingerprint),
        }
    }

    pub fn unavailable(&self) -> Option<String> {
        self.unavailable.lock().expect("ca").clone()
    }

    pub fn cert_der(&self) -> CertificateDer<'static> {
        self.cert_der.clone()
    }

    pub fn server_config(self: &Arc<Self>, engine: Arc<RuleEngine>, port: u16) -> Arc<ServerConfig> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(LeafResolver {
                ca: self.clone(),
                engine,
                port,
            }));
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Arc::new(config)
    }

    pub fn trust(&self) -> Result<(), AppError> {
        if let Some(err) = self.unavailable() {
            return Err(AppError::message(err));
        }
        match crate::tls::trust::trust_cert(&self.cert_path) {
            Ok(()) => {
                *self.trust_command_succeeded.lock().expect("ca trust") = true;
                *self.trust_error.lock().expect("ca trust") = None;
                Ok(())
            }
            Err(err) => {
                *self.trust_command_succeeded.lock().expect("ca trust") = false;
                *self.trust_error.lock().expect("ca trust") = Some(err.to_string());
                Err(err)
            }
        }
    }

    fn leaf(&self, sni: &str) -> Result<Arc<CertifiedKey>, AppError> {
        let sni = sni.trim_end_matches('.').to_ascii_lowercase();
        if let Some(cached) = self.leaves.lock().expect("leaves").get(&sni) {
            return Ok(cached.clone());
        }
        let guard = self.issuer.lock().expect("issuer");
        let issuer = guard.as_ref().ok_or_else(|| AppError::message("the local CA is not available"))?;
        let certified = mint_leaf(issuer, &sni, &self.cert_der)?;
        self.leaves.lock().expect("leaves").insert(sni, certified.clone());
        Ok(certified)
    }
}

struct LeafResolver {
    ca: Arc<CertificateAuthority>,
    engine: Arc<RuleEngine>,
    port: u16,
}

impl fmt::Debug for LeafResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LeafResolver")
    }
}

impl ResolvesServerCert for LeafResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let sni = client_hello.server_name()?.trim_end_matches('.').to_ascii_lowercase();
        if !self.engine.host_mapped(&sni, self.port) {
            tracing::debug!(sni, port = self.port, "refusing a leaf for an unmapped SNI");
            return None;
        }
        match self.ca.leaf(&sni) {
            Ok(key) => Some(key),
            Err(err) => {
                tracing::debug!(error = %err, sni, "could not mint a leaf certificate");
                None
            }
        }
    }
}

fn load_or_create(dir: &Path) -> Result<Arc<CertificateAuthority>, AppError> {
    std::fs::create_dir_all(dir).map_err(|err| AppError::io(dir, err))?;
    let cert_path = dir.join("dev-ca.pem");
    let key_path = dir.join("dev-ca.key.pem");
    if !cert_path.exists() || !key_path.exists() {
        generate(&cert_path, &key_path)?;
    }
    let cert_pem = std::fs::read_to_string(&cert_path).map_err(|err| AppError::io(&cert_path, err))?;
    let key_pem = std::fs::read_to_string(&key_path).map_err(|err| AppError::io(&key_path, err))?;
    let cert_der = first_cert(&cert_pem)?;
    let fingerprint = fingerprint(&cert_der);
    let key = KeyPair::from_pem(&key_pem).map_err(|err| AppError::message(format!("could not read the CA key: {err}")))?;
    let issuer = Issuer::from_ca_cert_pem(&cert_pem, key)
        .map_err(|err| AppError::message(format!("could not read the CA certificate: {err}")))?;
    Ok(Arc::new(CertificateAuthority {
        cert_path,
        cert_der,
        fingerprint,
        subject: SUBJECT.into(),
        issuer: Mutex::new(Some(issuer)),
        leaves: Mutex::new(HashMap::new()),
        unavailable: Mutex::new(None),
        trust_error: Mutex::new(None),
        trust_command_succeeded: Mutex::new(false),
    }))
}

fn generate(cert_path: &Path, key_path: &Path) -> Result<(), AppError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let key = KeyPair::generate().map_err(|err| AppError::message(format!("could not generate the CA key: {err}")))?;
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, SUBJECT);
    name.push(DnType::OrganizationName, "Local Endpoint Router");
    params.distinguished_name = name;
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let now = OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(3650);
    let cert = params
        .self_signed(&key)
        .map_err(|err| AppError::message(format!("could not sign the CA certificate: {err}")))?;
    write_secret(key_path, &key.serialize_pem())?;
    write_secret(cert_path, &cert.pem())?;
    Ok(())
}

fn mint_leaf(issuer: &Issuer<'static, KeyPair>, sni: &str, ca_der: &CertificateDer<'static>) -> Result<Arc<CertifiedKey>, AppError> {
    let key = KeyPair::generate().map_err(|err| AppError::message(format!("could not generate a leaf key: {err}")))?;
    let mut params = CertificateParams::new(vec![sni.to_string()])
        .map_err(|err| AppError::message(format!("could not name the leaf for {sni}: {err}")))?;
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, sni);
    params.distinguished_name = name;
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let now = OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(90);
    let cert = params
        .signed_by(&key, issuer)
        .map_err(|err| AppError::message(format!("could not sign the leaf for {sni}: {err}")))?;
    let provider = rustls::crypto::ring::default_provider();
    let _ = provider.clone().install_default();
    let chain = vec![CertificateDer::from(cert.der().to_vec()), ca_der.clone()];
    let private = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    let certified = CertifiedKey::from_der(chain, private, &provider)
        .map_err(|err| AppError::message(format!("could not use the leaf for {sni}: {err}")))?;
    Ok(Arc::new(certified))
}

fn first_cert(pem: &str) -> Result<CertificateDer<'static>, AppError> {
    rustls_pemfile::certs(&mut pem.as_bytes())
        .next()
        .ok_or_else(|| AppError::message("the CA file has no certificate"))?
        .map_err(|err| AppError::message(format!("could not parse the CA certificate: {err}")))
}

fn fingerprint(der: &CertificateDer<'_>) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, der.as_ref());
    hex::encode(digest.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_ca_stays_pending_until_the_user_trusts_it() {
        let dir = std::env::temp_dir().join(format!("ler-ca-status-{}", uuid::Uuid::new_v4()));
        let ca = CertificateAuthority::open(&dir);
        let status = ca.status();
        assert!(status.available);
        assert!(!status.fingerprint.is_empty());
        assert!(!status.trusted);
        assert!(!status.trust_command_succeeded);
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn write_secret(path: &Path, contents: &str) -> Result<(), AppError> {
    if path.exists() {
        return Ok(());
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|err| AppError::io(path, err))?;
    file.write_all(contents.as_bytes()).map_err(|err| AppError::io(path, err))?;
    Ok(())
}

