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
