use super::{
    crypto::{decrypt_secret, encrypt_secret},
    CredentialCreateInput, CredentialMetadata, CredentialRecord, CredentialSecret,
    CredentialUpdateInput,
};
use crate::{
    state::AppState,
    storage::{Store, StoredCredential, StoredCredentialRecord},
};

pub fn credential_metadata(state: &AppState) -> Result<Vec<CredentialMetadata>, String> {
    let store = state.store();
    Ok(store
        .credentials()?
        .into_iter()
        .map(metadata_from_stored_record)
        .collect())
}

pub(crate) fn credential_metadata_by_id_in_store(
    store: &Store,
    credential_id: &str,
) -> Result<Option<CredentialMetadata>, String> {
    Ok(store
        .credential_by_id(credential_id)?
        .map(|stored| metadata_from_stored(credential_id, stored)))
}

pub fn credential_secret_by_id(
    state: &AppState,
    credential_id: &str,
) -> Result<Option<CredentialSecret>, String> {
    let store = state.store();
    credential_secret_by_id_in_store(&store, credential_id)
}

pub(crate) fn credential_secret_by_id_in_store(
    store: &Store,
    credential_id: &str,
) -> Result<Option<CredentialSecret>, String> {
    Ok(
        credential_record_by_id_in_store(store, credential_id)?.map(|record| CredentialSecret {
            cred_type: record.cred_type,
            password: record.password,
            private_key: record.private_key,
            passphrase: record.passphrase,
        }),
    )
}

pub(super) fn credential_record_by_id(
    state: &AppState,
    credential_id: &str,
) -> Result<Option<CredentialRecord>, String> {
    let store = state.store();
    credential_record_by_id_in_store(&store, credential_id)
}

pub(super) fn credential_record_by_id_in_store(
    store: &Store,
    credential_id: &str,
) -> Result<Option<CredentialRecord>, String> {
    store
        .credential_by_id(credential_id)?
        .map(|stored| CredentialRecord::try_from_stored(credential_id, stored))
        .transpose()
}

pub(super) fn normalize_new_record(
    input: CredentialCreateInput,
) -> Result<CredentialRecord, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("credential name is required".to_string());
    }

    match input.cred_type.as_str() {
        "password" => {
            let password = input
                .password
                .ok_or_else(|| "credential password is required".to_string())?;
            let mut record = CredentialRecord {
                id: String::new(),
                cred_type: "password".to_string(),
                name: name.to_string(),
                password: Some(password),
                private_key: None,
                passphrase: None,
            };
            refresh_record_id(&mut record);
            Ok(record)
        }
        "key" => {
            let private_key = input.private_key.unwrap_or_default();
            if private_key.trim().is_empty() {
                return Err("credential private key is required".to_string());
            }
            let mut record = CredentialRecord {
                id: String::new(),
                cred_type: "key".to_string(),
                name: name.to_string(),
                password: None,
                private_key: Some(private_key),
                passphrase: input.passphrase.filter(|v| !v.trim().is_empty()),
            };
            refresh_record_id(&mut record);
            Ok(record)
        }
        _ => Err("unsupported credential type".to_string()),
    }
}

pub(super) fn update_record(
    record: &mut CredentialRecord,
    input: CredentialUpdateInput,
) -> Result<(), String> {
    let input = input.base;
    let name = input.name.trim();
    let previous_type = record.cred_type.clone();
    let previous_password = record.password.clone();
    let previous_private_key = record.private_key.clone();
    let previous_passphrase = record.passphrase.clone();
    if name.is_empty() {
        return Err("credential name is required".to_string());
    }

    match input.cred_type.as_str() {
        "password" => {
            let password = input
                .password
                .filter(|v| !v.trim().is_empty())
                .or_else(|| {
                    (previous_type == "password")
                        .then_some(previous_password)
                        .flatten()
                })
                .ok_or_else(|| "credential password is required".to_string())?;
            record.name = name.to_string();
            record.cred_type = "password".to_string();
            record.password = Some(password);
            record.private_key = None;
            record.passphrase = None;
        }
        "key" => {
            let private_key = input
                .private_key
                .filter(|v| !v.trim().is_empty())
                .or_else(|| {
                    (previous_type == "key")
                        .then_some(previous_private_key)
                        .flatten()
                })
                .ok_or_else(|| "credential private key is required".to_string())?;
            record.name = name.to_string();
            record.cred_type = "key".to_string();
            record.password = None;
            record.private_key = Some(private_key);
            record.passphrase = input
                .passphrase
                .filter(|v| !v.trim().is_empty())
                .or_else(|| {
                    (previous_type == "key")
                        .then_some(previous_passphrase)
                        .flatten()
                });
        }
        _ => return Err("unsupported credential type".to_string()),
    }

    Ok(())
}

pub(super) fn metadata_for(record: &CredentialRecord) -> CredentialMetadata {
    CredentialMetadata {
        id: record.id.clone(),
        cred_type: record.cred_type.clone(),
        name: record.name.clone(),
    }
}

fn metadata_from_stored_record(record: StoredCredentialRecord) -> CredentialMetadata {
    metadata_from_stored(&record.id, record.credential)
}

fn metadata_from_stored(id: &str, stored: StoredCredential) -> CredentialMetadata {
    CredentialMetadata {
        id: id.to_string(),
        cred_type: stored.cred_type,
        name: stored.name,
    }
}

pub(super) fn refresh_record_id(record: &mut CredentialRecord) {
    record.id = credential_id(record);
}

fn credential_id(record: &CredentialRecord) -> String {
    let raw = [
        record.cred_type.as_str(),
        record.name.as_str(),
        record.password.as_deref().unwrap_or(""),
        record.private_key.as_deref().unwrap_or(""),
        record.passphrase.as_deref().unwrap_or(""),
    ]
    .join("\0");
    format!("{:x}", md5::compute(raw.as_bytes()))
}

impl StoredCredential {
    pub(super) fn try_from_record(record: &CredentialRecord) -> Result<Self, String> {
        Ok(Self {
            position: 0,
            cred_type: record.cred_type.clone(),
            name: record.name.clone(),
            password: record.password.as_deref().map(encrypt_secret).transpose()?,
            private_key: record
                .private_key
                .as_deref()
                .map(encrypt_secret)
                .transpose()?,
            passphrase: record
                .passphrase
                .as_deref()
                .map(encrypt_secret)
                .transpose()?,
        })
    }
}

impl CredentialRecord {
    fn try_from_stored(id: &str, stored: StoredCredential) -> Result<Self, String> {
        Ok(Self {
            id: id.to_string(),
            cred_type: stored.cred_type,
            name: stored.name,
            password: decrypt_secret(stored.password.as_deref())?,
            private_key: decrypt_secret(stored.private_key.as_deref())?,
            passphrase: decrypt_secret(stored.passphrase.as_deref())?,
        })
    }
}
