use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;
use tauri::ipc::Channel;

use crate::{
    state::SessionRuntimeState,
    terminal::{api::dto::TerminalSessionChannelPayload, internal::TerminalSession},
};

pub(crate) struct TerminalRuntimeRegistry {
    pub(crate) sessions: Mutex<HashMap<String, TerminalSession>>,
    pub(crate) monitor_tasks: Mutex<HashMap<String, Arc<()>>>,
    pub(crate) session_connections: Mutex<HashMap<String, String>>,
    pub(crate) session_runtime: Mutex<HashMap<String, SessionRuntimeState>>,
    pub(crate) connection_sessions: Mutex<HashMap<String, Vec<String>>>,
    pub(crate) output_subscriptions:
        Mutex<HashMap<String, HashMap<u64, TerminalOutputSubscription>>>,
}

impl TerminalRuntimeRegistry {
    pub(crate) fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            monitor_tasks: Mutex::new(HashMap::new()),
            session_connections: Mutex::new(HashMap::new()),
            session_runtime: Mutex::new(HashMap::new()),
            connection_sessions: Mutex::new(HashMap::new()),
            output_subscriptions: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn bind_session_connection(&self, session_id: &str, connection_id: &str) {
        self.session_connections
            .lock()
            .insert(session_id.to_string(), connection_id.to_string());
        let mut connection_sessions = self.connection_sessions.lock();
        let sessions = connection_sessions
            .entry(connection_id.to_string())
            .or_default();
        if !sessions.iter().any(|candidate| candidate == session_id) {
            sessions.push(session_id.to_string());
        }
    }

    pub(crate) fn unbind_session_connection(&self, session_id: &str) -> Option<String> {
        let connection_id = self.session_connections.lock().remove(session_id);
        if let Some(connection_id) = &connection_id {
            let mut connection_sessions = self.connection_sessions.lock();
            if let Some(sessions) = connection_sessions.get_mut(connection_id) {
                sessions.retain(|candidate| candidate != session_id);
                if sessions.is_empty() {
                    connection_sessions.remove(connection_id);
                }
            }
        }
        self.session_runtime.lock().remove(session_id);
        connection_id
    }

    pub(crate) fn session_ids_for_connection(&self, connection_id: &str) -> Vec<String> {
        self.connection_sessions
            .lock()
            .get(connection_id)
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Clone)]
pub(crate) struct TerminalOutputSubscription {
    pub(crate) session_id: String,
    pub(crate) channel_id: Option<u64>,
    pub(crate) channel: Channel<TerminalSessionChannelPayload>,
}

impl TerminalOutputSubscription {
    pub(crate) fn accepts(&self, payload: &TerminalSessionChannelPayload) -> bool {
        Self::accepts_session_channel(&self.session_id, self.channel_id, payload)
    }

    pub(crate) fn accepts_session_channel(
        subscription_session_id: &str,
        subscription_channel_id: Option<u64>,
        payload: &TerminalSessionChannelPayload,
    ) -> bool {
        if payload.session_id() != subscription_session_id {
            return false;
        }
        subscription_channel_id == Some(payload.channel_id())
    }

    pub(crate) fn send(&self, payload: TerminalSessionChannelPayload) -> Result<(), String> {
        self.channel
            .send(payload)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{TerminalOutputSubscription, TerminalRuntimeRegistry};
    use crate::terminal::api::dto::TerminalSessionChannelPayload;

    fn terminal_text_payload(session_id: &str, channel_id: u64) -> TerminalSessionChannelPayload {
        TerminalSessionChannelPayload::Text {
            connection_id: "connection-1".to_string(),
            session_id: session_id.to_string(),
            channel_id,
            data: String::new(),
            encoding: "utf-8".to_string(),
            start_offset: 0,
            end_offset: 0,
        }
    }

    #[test]
    fn terminal_output_data_requires_matching_channel() {
        let payload = terminal_text_payload("session-1", 7);

        assert!(TerminalOutputSubscription::accepts_session_channel(
            "session-1",
            Some(7),
            &payload
        ));
        assert!(!TerminalOutputSubscription::accepts_session_channel(
            "session-1",
            Some(8),
            &payload
        ));
        assert!(!TerminalOutputSubscription::accepts_session_channel(
            "session-1",
            None,
            &payload
        ));
        assert!(!TerminalOutputSubscription::accepts_session_channel(
            "session-2",
            Some(7),
            &payload
        ));
    }

    #[test]
    fn one_connection_can_own_multiple_independent_sessions() {
        let registry = TerminalRuntimeRegistry::new();

        registry.bind_session_connection("session-1", "connection-1");
        registry.bind_session_connection("session-2", "connection-1");

        assert_eq!(
            registry.session_ids_for_connection("connection-1"),
            vec!["session-1", "session-2"]
        );
        assert_eq!(
            registry.unbind_session_connection("session-2").as_deref(),
            Some("connection-1")
        );
        assert_eq!(
            registry.session_ids_for_connection("connection-1"),
            vec!["session-1"]
        );
    }
}
