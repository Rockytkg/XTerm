use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use super::{
    records::{
        credential_metadata, credential_record_by_id, metadata_for, normalize_new_record,
        refresh_record_id, update_record,
    },
    CredentialCleanupResult, CredentialCreateInput, CredentialMetadata, CredentialUpdateInput,
    CredentialUsage,
};
use crate::{logging, state::AppState, storage::StoredCredential};

/// Lists saved credential metadata without returning passwords, private keys, or
/// key passphrases to the frontend.
#[tauri::command]
pub(crate) fn credentials_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CredentialMetadata>, String> {
    let credentials = credential_metadata(&state)?;
    logging::event("credentials.commands", "credentials.list")
        .field("count", credentials.len())
        .debug();
    Ok(credentials)
}

#[tauri::command]
pub(crate) fn credentials_usages(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CredentialUsage>, String> {
    let usages = state
        .store()
        .credential_usages()?
        .into_iter()
        .map(|usage| CredentialUsage {
            credential_id: usage.credential_id,
            connection_id: usage.connection_id,
            connection_name: usage.connection_name,
            relation: usage.relation,
        })
        .collect::<Vec<_>>();
    logging::event("credentials.commands", "credentials.usages")
        .field("count", usages.len())
        .debug();
    Ok(usages)
}

#[tauri::command]
pub(crate) fn credentials_reorder(
    state: tauri::State<'_, AppState>,
    order: Vec<String>,
) -> Result<(), String> {
    state.store().reorder_credentials(&order)?;
    logging::event("credentials.commands", "credentials.reorder")
        .field("count", order.len())
        .info();
    Ok(())
}

#[tauri::command]
pub(crate) async fn credentials_delete_unused(
    state: tauri::State<'_, AppState>,
) -> Result<CredentialCleanupResult, String> {
    let deleted_ids = state
        .inner()
        .run_store_blocking(|store| store.delete_unused_credentials())
        .await?;
    logging::event("credentials.commands", "credentials.delete_unused")
        .field("count", deleted_ids.len())
        .warn();
    Ok(CredentialCleanupResult { deleted_ids })
}

#[tauri::command]
pub(crate) async fn credentials_clear_references(
    state: tauri::State<'_, AppState>,
    credential_id: String,
) -> Result<usize, String> {
    let credential_id_for_store = credential_id.clone();
    let cleared = state
        .inner()
        .run_store_blocking(move |store| {
            store.clear_credential_references(&credential_id_for_store)
        })
        .await?;
    logging::event("credentials.commands", "credentials.clear_references")
        .field("credential_id", &credential_id)
        .field("count", cleared)
        .warn();
    Ok(cleared)
}

/// Creates a credential record and stores the secret material inside the
/// encrypted Rust-owned document. The frontend supplies secrets only at creation
/// time; subsequent reads expose metadata only.
#[tauri::command]
pub(crate) fn credentials_create(
    state: tauri::State<'_, AppState>,
    credential: CredentialCreateInput,
) -> Result<CredentialMetadata, String> {
    logging::event("credentials.commands", "credentials.create.start")
        .field("name", &credential.name)
        .info();
    let record = normalize_new_record(credential)?;
    let metadata = metadata_for(&record);
    let stored = StoredCredential::try_from_record(&record)?;
    state.store().insert_credential(&record.id, &stored)?;
    logging::event("credentials.commands", "credentials.create.success")
        .field("credential_id", &metadata.id)
        .field("credential_type", metadata.cred_type())
        .info();
    Ok(metadata)
}

/// Updates credential metadata and optionally replaces secret material.
#[tauri::command]
pub(crate) fn credentials_update(
    state: tauri::State<'_, AppState>,
    credential: CredentialUpdateInput,
) -> Result<CredentialMetadata, String> {
    logging::event("credentials.commands", "credentials.update.start")
        .field("credential_id", &credential.id)
        .info();
    let mut record = credential_record_by_id(state.inner(), &credential.id)?
        .ok_or_else(|| "credential not found".to_string())?;
    let previous_id = record.id.clone();
    update_record(&mut record, credential)?;
    refresh_record_id(&mut record);
    let metadata = metadata_for(&record);
    let stored = StoredCredential::try_from_record(&record)?;
    state
        .store()
        .update_credential(&previous_id, &record.id, &stored)?;
    logging::event("credentials.commands", "credentials.update.success")
        .field("credential_id", &metadata.id)
        .field("credential_type", metadata.cred_type())
        .info();
    Ok(metadata)
}

/// Deletes one credential after checking the current workspace connections.
#[tauri::command]
pub(crate) async fn credentials_delete(
    state: tauri::State<'_, AppState>,
    credential_id: String,
) -> Result<(), String> {
    let credential_id_for_store = credential_id.clone();
    state
        .inner()
        .run_store_blocking(move |store| {
            let used_by = store.credential_connection_names(&credential_id_for_store)?;

            if !used_by.is_empty() {
                return Err(format!("credential is used by: {}", used_by.join(", ")));
            }

            if !store.delete_credential(&credential_id_for_store)? {
                return Err("credential not found".to_string());
            }
            Ok(())
        })
        .await?;
    logging::event("credentials.commands", "credentials.delete")
        .field("credential_id", &credential_id)
        .warn();
    Ok(())
}

/// Opens an OS-native file picker and reads one SSH private key file.
#[tauri::command]
pub(crate) async fn credentials_choose_private_key(
    app: AppHandle,
    title: Option<String>,
) -> Result<Option<String>, String> {
    logging::event("credentials.commands", "credentials.choose_private_key")
        .maybe_field("title", title.clone())
        .debug();
    let title = title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Choose SSH private key");
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title(title).pick_file(move |path| {
        let _ = sender.send(path);
    });
    let Some(path) = receiver
        .await
        .map_err(|_| "private key picker closed before returning a result".to_string())?
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| format!("failed to resolve selected private key path: {error}"))?;
    tokio::fs::read_to_string(&path)
        .await
        .map(Some)
        .map_err(|error| {
            format!(
                "failed to read private key file '{}': {error}",
                path.to_string_lossy()
            )
        })
}
