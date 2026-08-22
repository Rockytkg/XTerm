use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRecord {
    pub(super) id: String,
    pub(super) cred_type: String,
    pub(super) name: String,
    #[serde(default)]
    pub(super) password: Option<String>,
    #[serde(default)]
    pub(super) private_key: Option<String>,
    #[serde(default)]
    pub(super) passphrase: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialMetadata {
    pub(super) id: String,
    pub(super) cred_type: String,
    pub(super) name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialUsage {
    pub credential_id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub relation: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialCleanupResult {
    pub deleted_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CredentialSecret {
    pub(super) cred_type: String,
    pub(super) password: Option<String>,
    pub(super) private_key: Option<String>,
    pub(super) passphrase: Option<String>,
}

impl CredentialMetadata {
    pub fn cred_type(&self) -> &str {
        &self.cred_type
    }
}

impl CredentialSecret {
    pub fn cred_type(&self) -> &str {
        &self.cred_type
    }

    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    pub fn private_key(&self) -> Option<&str> {
        self.private_key.as_deref()
    }

    pub fn passphrase(&self) -> Option<&str> {
        self.passphrase.as_deref()
    }
}

/// Create payload for a credential. `CredentialUpdateInput` reuses these
/// fields verbatim and adds the `id` of the record being replaced.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialCreateInput {
    pub(super) cred_type: String,
    pub(super) name: String,
    pub(super) password: Option<String>,
    pub(super) private_key: Option<String>,
    pub(super) passphrase: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialUpdateInput {
    pub(super) id: String,
    #[serde(flatten)]
    pub(super) base: CredentialCreateInput,
}
