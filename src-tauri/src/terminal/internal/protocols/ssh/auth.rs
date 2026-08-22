use crate::{
    credentials::{credential_secret_by_id, CredentialSecret},
    state::AppState,
    terminal::internal::core::ResolvedConnection,
};

pub(crate) enum SshAuth {
    Password(String),
    Key {
        private_key: String,
        passphrase: Option<String>,
    },
}

pub(super) fn resolve_ssh_auth(
    state: &AppState,
    request: &ResolvedConnection,
) -> Result<SshAuth, String> {
    // Inline credentials (from deep-link URIs) take priority over saved ones.
    if let Some(password) = request.inline_password.as_deref().filter(|v| !v.is_empty()) {
        return Ok(SshAuth::Password(password.to_string()));
    }
    if let Some(key) = request
        .inline_private_key
        .as_deref()
        .filter(|v| !v.is_empty())
    {
        return Ok(SshAuth::Key {
            private_key: key.to_string(),
            passphrase: request.inline_private_key_passphrase.clone(),
        });
    }

    // Fall back to saved credential lookup.
    let credential_id = request
        .saved_credential_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "SSH connections require a saved credential or inline password/key".to_string()
        })?;

    let secret = credential_secret_by_id(state, credential_id)?
        .ok_or_else(|| format!("selected credential '{credential_id}' does not exist"))?;
    let auth = auth_from_secret(secret)?;
    if let Some(method) = request.auth_method.as_deref() {
        match (&auth, method) {
            (SshAuth::Password(_), "password") | (SshAuth::Key { .. }, "key") => {}
            _ => {
                return Err(format!(
                    "saved credential '{credential_id}' does not match auth method '{method}'"
                ));
            }
        }
    }
    Ok(auth)
}

pub(super) fn auth_from_secret(secret: CredentialSecret) -> Result<SshAuth, String> {
    match secret.cred_type() {
        "password" => Ok(SshAuth::Password(
            secret
                .password()
                .ok_or_else(|| "saved password credential is missing its password".to_string())?
                .to_string(),
        )),
        "key" => Ok(SshAuth::Key {
            private_key: secret
                .private_key()
                .ok_or_else(|| "saved key credential is missing its private key".to_string())?
                .to_string(),
            passphrase: secret.passphrase().map(ToOwned::to_owned),
        }),
        value => Err(format!("unsupported saved credential type '{value}'")),
    }
}
