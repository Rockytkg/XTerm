use std::fmt::Debug;

use ssh_encoding::{Decode, Encode};

#[doc(hidden)]
pub trait EncodedExt {
    fn encoded(&self) -> ssh_key::Result<Vec<u8>>;
}

impl<E: Encode> EncodedExt for E {
    fn encoded(&self) -> ssh_key::Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.encode(&mut buf)?;
        Ok(buf)
    }
}

mod name_list {
    use std::ops::Deref;

    use super::*;
    const MAX_NAME_LIST_ENTRIES: usize = 1024;
    const MAX_NAME_LIST_BYTES: usize = 16 * 1024;

    pub struct NameList(pub Vec<String>);

    impl Debug for NameList {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.fmt(f)
        }
    }

    impl Deref for NameList {
        type Target = [String];

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl NameList {
        pub fn as_encoded_string(&self) -> String {
            self.0.join(",")
        }

        pub fn from_encoded_string(value: &str) -> Result<Self, ssh_encoding::Error> {
            // PATCH(xterm): RFC 4251 §5 要求 name 非空且为 US-ASCII，但旧设备常在
            // name-list 尾部多写一个逗号或填入垃圾字节。条目只用于算法匹配，垃圾
            // 条目永远不会被选中——参照 OpenSSH match.c 的 match_list（strsep 循环
            // 以 *p != '\0' 为条件，静默跳过空条目），这里跳过而非报错。
            // 条目数量上限保留（GHSA-4r3c-5hpg-58qr 的 DoS 防护）。
            let mut list = Vec::new();
            for name in value.split(',') {
                if name.is_empty() || !name.is_ascii() {
                    continue;
                }
                if list.len() >= MAX_NAME_LIST_ENTRIES {
                    return Err(ssh_encoding::Error::Length);
                }
                list.push(name.into());
            }
            Ok(Self(list))
        }
    }

    impl Encode for NameList {
        fn encoded_len(&self) -> Result<usize, ssh_encoding::Error> {
            self.as_encoded_string().encoded_len()
        }

        fn encode(
            &self,
            writer: &mut impl ssh_encoding::Writer,
        ) -> Result<(), ssh_encoding::Error> {
            self.as_encoded_string().encode(writer)
        }
    }

    impl Decode for NameList {
        fn decode(reader: &mut impl ssh_encoding::Reader) -> Result<Self, ssh_encoding::Error> {
            // PATCH(xterm): 不再要求整个 name-list 是合法 UTF-8——读取原始字节后
            // 有损转换，非法条目在 from_encoded_string 中被过滤；整体长度上限保留。
            reader.read_prefixed(|reader| {
                let len = reader.remaining_len();
                if len > MAX_NAME_LIST_BYTES {
                    return Err(ssh_encoding::Error::Length);
                }
                let mut buf = vec![0; len];
                reader.read(&mut buf)?;
                reader.ensure_finished()?;
                Self::from_encoded_string(&String::from_utf8_lossy(&buf))
            })
        }

        type Error = ssh_encoding::Error;
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn empty_name_list_is_valid() {
            // RFC 4251 §5 permits a zero-name list. Servers that only
            // offer AEAD ciphers (e.g. hssh) send empty MAC name-lists.
            let nl = NameList::from_encoded_string("").unwrap();
            assert!(nl.0.is_empty());
        }

        #[test]
        fn name_list_round_trip() {
            let nl = NameList::from_encoded_string("a,b,c").unwrap();
            assert_eq!(nl.0, vec!["a", "b", "c"]);
            assert_eq!(nl.as_encoded_string(), "a,b,c");
        }

        #[test]
        fn name_list_skips_empty_entry() {
            // PATCH(xterm): 空条目（",,"、尾部逗号）按 OpenSSH match_list 语义跳过。
            let nl = NameList::from_encoded_string("a,,b,").unwrap();
            assert_eq!(nl.0, vec!["a", "b"]);
        }

        #[test]
        fn name_list_skips_non_ascii_entry() {
            // PATCH(xterm): 非 ASCII 条目（如 GBK 字节）同样跳过而非报错。
            let nl = NameList::from_encoded_string("a,中,b").unwrap();
            assert_eq!(nl.0, vec!["a", "b"]);
        }
    }
}

pub use name_list::NameList;

pub(crate) mod macros {
    #[allow(clippy::crate_in_macro_def)]
    macro_rules! map_err {
        ($result:expr) => {
            $result.map_err(|e| crate::Error::from(e))
        };
    }

    pub(crate) use map_err;
}

#[cfg(any(feature = "ring", feature = "aws-lc-rs"))]
pub(crate) use macros::map_err;

#[doc(hidden)]
pub fn sign_with_hash_alg(key: &PrivateKeyWithHashAlg, data: &[u8]) -> ssh_key::Result<Vec<u8>> {
    Ok(match key.key_data() {
        #[cfg(feature = "rsa")]
        ssh_key::private::KeypairData::Rsa(rsa_keypair) => {
            let ssh_key::Algorithm::Rsa { hash } = key.algorithm() else {
                unreachable!();
            };
            signature::Signer::try_sign(&(rsa_keypair, hash), data)?.encoded()?
        }
        // With the `rsa` feature off the RSA arm above is compiled out. The
        // catch-all cannot honour the negotiated hash, so it fails with
        // `Rsa { hash: None }` only after the peer has accepted the key
        // (USERAUTH_PK_OK) -- a confusing error that points at the credential,
        // not the build. Fail here instead, naming the missing feature and
        // reporting the algorithm that was actually negotiated.
        // See https://github.com/Eugeny/russh/issues/758.
        #[cfg(not(feature = "rsa"))]
        ssh_key::private::KeypairData::Rsa(_) => {
            log::error!(
                "cannot sign with an RSA key: russh was built without the `rsa` \
                 feature (enable it, e.g. `features = [\"rsa\"]`)"
            );
            return Err(ssh_key::Error::AlgorithmUnsupported {
                algorithm: key.algorithm(),
            });
        }
        keypair => signature::Signer::try_sign(keypair, data)?.encoded()?,
    })
}

mod algorithm {
    use ssh_key::{Algorithm, HashAlg};

    pub trait AlgorithmExt {
        fn hash_alg(&self) -> Option<HashAlg>;
        fn with_hash_alg(&self, hash_alg: Option<HashAlg>) -> Self;
        fn new_certificate_ext(algo: &str) -> Result<Self, ssh_key::Error>
        where
            Self: Sized;
    }

    impl AlgorithmExt for Algorithm {
        fn hash_alg(&self) -> Option<HashAlg> {
            match self {
                Algorithm::Rsa { hash } => *hash,
                _ => None,
            }
        }

        fn with_hash_alg(&self, hash_alg: Option<HashAlg>) -> Self {
            match self {
                Algorithm::Rsa { .. } => Algorithm::Rsa { hash: hash_alg },
                x => x.clone(),
            }
        }

        fn new_certificate_ext(algo: &str) -> Result<Self, ssh_key::Error> {
            match algo {
                "rsa-sha2-256-cert-v01@openssh.com" => Ok(Algorithm::Rsa {
                    hash: Some(HashAlg::Sha256),
                }),
                "rsa-sha2-512-cert-v01@openssh.com" => Ok(Algorithm::Rsa {
                    hash: Some(HashAlg::Sha512),
                }),
                x => Algorithm::new_certificate(x),
            }
        }
    }
}

#[doc(hidden)]
pub use algorithm::AlgorithmExt;

use crate::keys::key::PrivateKeyWithHashAlg;

#[cfg(all(test, not(feature = "rsa"), any(feature = "ring", feature = "aws-lc-rs")))]
mod tests {
    use std::sync::Arc;

    use ssh_key::{HashAlg, PrivateKey};

    use super::*;

    // OpenSSH-format RSA keys parse even with the `rsa` feature off (only
    // PKCS#1/#8 parsing is gated), so a caller can hold an RSA keypair the
    // signer cannot honour. It must fail loudly rather than sign as SHA-1 and
    // be rejected by the peer after USERAUTH_PK_OK.
    // See https://github.com/Eugeny/russh/issues/758.
    const RSA_OPENSSH: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAABFwAAAAdzc2gtcn\nNhAAAAAwEAAQAAAQEAzXFzBprJRnloMqy6YBEIHGSkA2HhAkXPfNqb66NW4vZRo5jyVqB9\nStvPc1mnLVdMBLLsPhs+/QNd9DP+1a2rbkHbA112mGlFjX28GeD7cJ8tbSLbO9Ovqs9Fr9\n7PTWLCgIwObWNcmedCHRZp1o61ZO7ez+YPtrl+97UPjwyvr9pJ9P5vtIFpWfUpzlUzA3OI\nlGEy3ozWAC3nz7BqeIrP6shQV92Z6YvzA4GmqzCIHGYu3LjMbykpmrgPvTbOVGk5MlV61u\n3VmpwwYAxfBYIIsW1CVFDYgoNhtz6CHrcL+yjaaMJ6XNdNv0Fkm718k3J/tr+NhaFxMGSd\nx/kLPozW+QAAA7jZ6aUF2emlBQAAAAdzc2gtcnNhAAABAQDNcXMGmslGeWgyrLpgEQgcZK\nQDYeECRc982pvro1bi9lGjmPJWoH1K289zWactV0wEsuw+Gz79A130M/7VratuQdsDXXaY\naUWNfbwZ4Ptwny1tIts706+qz0Wv3s9NYsKAjA5tY1yZ50IdFmnWjrVk7t7P5g+2uX73tQ\n+PDK+v2kn0/m+0gWlZ9SnOVTMDc4iUYTLejNYALefPsGp4is/qyFBX3Znpi/MDgaarMIgc\nZi7cuMxvKSmauA+9Ns5UaTkyVXrW7dWanDBgDF8FggixbUJUUNiCg2G3PoIetwv7KNpown\npc102/QWSbvXyTcn+2v42FoXEwZJ3H+Qs+jNb5AAAAAwEAAQAAAQAXOOu3o2d8/9w3Xi7z\nDPnNu9KOT1IP37REeLGHQT9hCFbSF0fNYvbGr3ITp96t0WBWZ6MsogfMscS9BeOYY7ktDW\nSkTLxLHhsukzff9P53DIcl/vqpGJSSyjsmVhk4tiEmRq6aztB41Rs0DoSILDl030twVKES\nFoWzmlPX56c0SVRzbbqrZr2D458iK4TxmAa2kmFJk0EicGpgZWNC8zcHKhkAblPb1PdfFE\nirphsql3yPa1InoGrp/X/uHXsT3MjDxgXwOUPsQHG2t2kLKbdPCJ1Cci3wysXpzvGfytOK\neheuvUMTcLDSw4Vf7nadT9CR2pP4QJsAbfdWza1aQt3BAAAAgG+4pVf6y26Ki/Q41mD8a6\nNv3wFNTkTgMVWXNezJ2zGhuaZgYqLu5n6GS1s0qyZzW7xfwyowajttWH4haHqddi1jYC53\nrKE0vLHsBiMlc/pyvr0QueWyAXy0OKcZ9YzS36H2A5dznpLuwnkgI/Aw7RsfgnXp2YdmpS\n+jl3L7OSDYAAAAgQD3idiLn9RDCEXBf4foi6r5j2B5h9sOIX5ixdfAlDZtcMd8eRVOFJNl\n1GpqJRqeh5QM2oV657fqHlqouSM0ooTwSHYBDpQDGV43u+YHFqGkKb3OC24sN/RejNyLDA\naP8MSCv8JhvjWnlMT9iFI+Es+1AjAov0s0T2K2L3xDbk6kqwAAAIEA1Hc8oE88mYLa+7x2\nykqMjoPb9b8J4I3oBvIUQIy+wKqe9K5s6rh3nIaaAnhOEwahOqayb0jfvkv4bc1taHBgQS\ndJ2lO2kZ+AZDZbboOD+Gi5iOnNphtCem/h2LTQ/crjcDx1QGfDOsfqtF1ggzXFJVNqJaUA\n+fC7anjMQbvHCusAAAAAAQID\n-----END OPENSSH PRIVATE KEY-----\n";

    #[test]
    fn rsa_signing_without_feature_fails_loudly() {
        let rsa = Arc::new(PrivateKey::from_openssh(RSA_OPENSSH).unwrap());
        let key = PrivateKeyWithHashAlg::new(rsa, Some(HashAlg::Sha512));
        // Fails at signing time, and names the negotiated algorithm
        // (`Rsa { hash: Some(Sha512) }`) rather than the misleading
        // `Rsa { hash: None }` the bare signer would report.
        match sign_with_hash_alg(&key, b"payload") {
            Err(ssh_key::Error::AlgorithmUnsupported {
                algorithm: ssh_key::Algorithm::Rsa { hash: Some(HashAlg::Sha512) },
            }) => {}
            other => panic!("expected AlgorithmUnsupported naming rsa-sha2-512, got {other:?}"),
        }
    }
}
