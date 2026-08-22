use crate::{state::AppState, terminal::internal::core::HostKeyDeleteRequest};

pub(super) fn save_host_key(
    state: &AppState,
    connection_id: &str,
    fingerprint: &str,
) -> Result<(), String> {
    let store = state.store();
    store.upsert_host_key(connection_id, fingerprint)?;
    log::info!(target: "ssh.host_keys", "trusted SSH host key for connection {connection_id}");
    Ok(())
}

/// Removes a saved SSH host-key trust record for a connection profile.
///
/// The frontend calls this when a saved SSH connection profile is deleted so
/// stale trust is not kept after the user removes that session configuration.
#[tauri::command]
pub(crate) fn ssh_host_key_delete(
    state: tauri::State<'_, AppState>,
    request: HostKeyDeleteRequest,
) -> Result<(), String> {
    let connection_id = request.connection_id.trim();
    if connection_id.is_empty() {
        return Ok(());
    }
    delete_host_key(state.inner(), connection_id)
}

pub(super) fn delete_host_key(state: &AppState, connection_id: &str) -> Result<(), String> {
    state.remove_temporary_host_key_for_connection(connection_id);
    let store = state.store();
    if !store.delete_host_key(connection_id)? {
        log::info!(target: "ssh.host_keys", "no trusted SSH host key to remove for connection {connection_id}");
        return Ok(());
    }
    log::info!(target: "ssh.host_keys", "removed trusted SSH host key for connection {connection_id}");
    Ok(())
}
