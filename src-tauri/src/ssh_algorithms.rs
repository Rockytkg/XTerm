use std::borrow::Cow;

use russh::{
    cipher, kex,
    keys::{Algorithm, EcdsaCurve, HashAlg},
    mac, Preferred,
};

const MODERN_KEX: &[kex::Name] = &[
    kex::MLKEM768X25519_SHA256,
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
    kex::DH_GEX_SHA256,
    kex::DH_G18_SHA512,
    kex::DH_G17_SHA512,
    kex::DH_G16_SHA512,
    kex::DH_G15_SHA512,
    kex::DH_G14_SHA256,
    kex::EXTENSION_SUPPORT_AS_CLIENT,
    kex::EXTENSION_SUPPORT_AS_SERVER,
    kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT,
    kex::EXTENSION_OPENSSH_STRICT_KEX_AS_SERVER,
];

const LEGACY_KEX: &[kex::Name] = &[kex::DH_G14_SHA1, kex::DH_G1_SHA1, kex::DH_GEX_SHA1];

const MODERN_CIPHERS: &[cipher::Name] = &[
    cipher::CHACHA20_POLY1305,
    cipher::AES_256_GCM,
    cipher::AES_128_GCM,
    cipher::AES_256_CTR,
    cipher::AES_192_CTR,
    cipher::AES_128_CTR,
];

const LEGACY_CIPHERS: &[cipher::Name] = &[
    cipher::AES_128_CBC,
    cipher::AES_192_CBC,
    cipher::AES_256_CBC,
    cipher::TRIPLE_DES_CBC,
];

const MODERN_MACS: &[mac::Name] = &[
    mac::HMAC_SHA512_ETM,
    mac::HMAC_SHA256_ETM,
    mac::HMAC_SHA512,
    mac::HMAC_SHA256,
];

const LEGACY_MACS: &[mac::Name] = &[mac::HMAC_SHA1_ETM, mac::HMAC_SHA1];

/// SSH 终端、SFTP 子系统与内置 SFTP 服务端共用同一协商策略。
/// 显式维护安全集合，避免继承 russh 默认表中仍包含的 ssh-rsa（SHA-1）。
pub(crate) fn preferred(allow_legacy: bool) -> Preferred {
    let mut preferred = Preferred {
        kex: Cow::Borrowed(MODERN_KEX),
        key: Cow::Owned(vec![
            Algorithm::Ed25519,
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP256,
            },
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP384,
            },
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP521,
            },
            Algorithm::Rsa {
                hash: Some(HashAlg::Sha512),
            },
            Algorithm::Rsa {
                hash: Some(HashAlg::Sha256),
            },
        ]),
        cipher: Cow::Borrowed(MODERN_CIPHERS),
        mac: Cow::Borrowed(MODERN_MACS),
        ..Default::default()
    };
    if allow_legacy {
        preferred.kex.to_mut().extend_from_slice(LEGACY_KEX);
        preferred.key.to_mut().push(Algorithm::Rsa { hash: None });
        preferred.cipher.to_mut().extend_from_slice(LEGACY_CIPHERS);
        preferred.mac.to_mut().extend_from_slice(LEGACY_MACS);
    }
    preferred
}

/// server_hash 沿用 russh 的嵌套 Option：None 表示未宣告 server-sig-algs，
/// Some(None) 表示对端只接受 RSA-SHA1。未知扩展能力时优先尝试 RSA-SHA2。
pub(crate) fn rsa_authentication_hash(
    server_hash: Option<Option<HashAlg>>,
    allow_legacy: bool,
) -> Result<Option<HashAlg>, &'static str> {
    match server_hash {
        Some(None) if !allow_legacy => Err(
            "server only supports RSA-SHA1 public-key authentication; enable legacy SSH algorithms explicitly to use it",
        ),
        None if !allow_legacy => Ok(Some(HashAlg::Sha256)),
        value => Ok(value.flatten()),
    }
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, collections::HashSet, sync::Arc};

    use russh::{
        cipher, client, kex,
        keys::{Algorithm, HashAlg, PrivateKey, PublicKeyOrCertificate},
        mac, server, Disconnect,
    };

    use super::{
        preferred, rsa_authentication_hash, LEGACY_CIPHERS, LEGACY_KEX, LEGACY_MACS,
        MODERN_CIPHERS, MODERN_KEX, MODERN_MACS,
    };

    #[test]
    fn modern_profile_contains_secure_algorithms_only() {
        let preferred = preferred(false);
        assert_eq!(preferred.kex.as_ref(), MODERN_KEX);
        assert_eq!(preferred.cipher.as_ref(), MODERN_CIPHERS);
        assert_eq!(preferred.mac.as_ref(), MODERN_MACS);
        assert!(!preferred.key.contains(&Algorithm::Rsa { hash: None }));
        let names = all_names(&preferred);
        for algorithm in legacy_names() {
            assert!(!names.contains(&algorithm));
        }
    }

    #[test]
    fn legacy_profile_extends_the_complete_modern_profile() {
        let modern = preferred(false);
        let legacy = preferred(true);
        assert!(legacy.kex.starts_with(&modern.kex));
        assert!(legacy.key.starts_with(&modern.key));
        assert!(legacy.cipher.starts_with(&modern.cipher));
        assert!(legacy.mac.starts_with(&modern.mac));
        assert_eq!(&legacy.kex[modern.kex.len()..], LEGACY_KEX);
        assert_eq!(&legacy.cipher[modern.cipher.len()..], LEGACY_CIPHERS);
        assert_eq!(&legacy.mac[modern.mac.len()..], LEGACY_MACS);
        assert!(legacy.key.contains(&Algorithm::Rsa { hash: None }));
        let names = all_names(&legacy);
        assert_eq!(
            names.iter().collect::<HashSet<_>>().len(),
            names.len(),
            "algorithm lists must not contain duplicates"
        );
    }

    #[test]
    fn rsa_authentication_never_silently_falls_back_to_sha1() {
        assert_eq!(
            rsa_authentication_hash(None, false),
            Ok(Some(HashAlg::Sha256))
        );
        assert_eq!(
            rsa_authentication_hash(Some(Some(HashAlg::Sha512)), false),
            Ok(Some(HashAlg::Sha512))
        );
        assert!(rsa_authentication_hash(Some(None), false).is_err());
        assert_eq!(rsa_authentication_hash(None, true), Ok(None));
        assert_eq!(rsa_authentication_hash(Some(None), true), Ok(None));
    }

    fn all_names(preferred: &russh::Preferred) -> Vec<String> {
        preferred
            .kex
            .iter()
            .map(|name| name.as_ref().to_string())
            .chain(preferred.key.iter().map(ToString::to_string))
            .chain(
                preferred
                    .cipher
                    .iter()
                    .map(|name| name.as_ref().to_string()),
            )
            .chain(preferred.mac.iter().map(|name| name.as_ref().to_string()))
            .collect()
    }

    fn legacy_names() -> Vec<String> {
        LEGACY_KEX
            .iter()
            .map(|name| name.as_ref().to_string())
            .chain(LEGACY_CIPHERS.iter().map(|name| name.as_ref().to_string()))
            .chain(LEGACY_MACS.iter().map(|name| name.as_ref().to_string()))
            .collect()
    }

    #[tokio::test]
    async fn legacy_transport_requires_explicit_opt_in() {
        assert!(connect_legacy_only(false).await.is_err());

        let mut session = connect_legacy_only(true)
            .await
            .expect("legacy algorithms should negotiate after explicit opt-in");
        assert!(session
            .authenticate_password("user", "secret")
            .await
            .expect("password authentication should use the negotiated transport")
            .success());
        session
            .disconnect(Disconnect::ByApplication, "test complete", "")
            .await
            .expect("session should disconnect cleanly");
    }

    async fn connect_legacy_only(
        allow_legacy: bool,
    ) -> Result<client::Handle<TestClient>, russh::Error> {
        let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
        let mut server_preferred = preferred(false);
        server_preferred.kex = Cow::Borrowed(&[kex::DH_G14_SHA1]);
        server_preferred.cipher = Cow::Borrowed(&[cipher::AES_128_CBC]);
        server_preferred.mac = Cow::Borrowed(&[mac::HMAC_SHA1]);
        let server_config = Arc::new(server::Config {
            keys: vec![PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
                .expect("test host key should generate")],
            preferred: server_preferred,
            inactivity_timeout: None,
            ..Default::default()
        });
        tokio::spawn(async move {
            let _ = server::run_stream(server_config, server_stream, TestServer).await;
        });
        let client_config = Arc::new(client::Config {
            preferred: preferred(allow_legacy),
            ..Default::default()
        });
        client::connect_stream(client_config, client_stream, TestClient).await
    }

    struct TestServer;

    impl server::Handler for TestServer {
        type Error = russh::Error;

        async fn auth_password(
            &mut self,
            _user: &str,
            _password: &str,
        ) -> Result<server::Auth, Self::Error> {
            Ok(server::Auth::Accept)
        }
    }

    struct TestClient;

    impl client::Handler for TestClient {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            _server_public_key: &PublicKeyOrCertificate,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }
}
