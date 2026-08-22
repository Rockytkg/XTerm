pub(crate) mod commands;
mod crypto;
mod models;
mod platform_key;
mod records;

pub use models::{CredentialCleanupResult, CredentialMetadata, CredentialSecret, CredentialUsage};
pub(crate) use platform_key::keyring_entry;
pub(crate) use records::credential_metadata_by_id_in_store;
pub use records::credential_secret_by_id;

use models::{CredentialCreateInput, CredentialRecord, CredentialUpdateInput};
