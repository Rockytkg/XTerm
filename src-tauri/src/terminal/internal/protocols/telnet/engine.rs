use flate2::{Decompress, FlushDecompress, Status};

const IAC: u8 = 255;
const SB: u8 = 250;
const SE: u8 = 240;

pub(super) const WILL: u8 = 251;
pub(super) const WONT: u8 = 252;
pub(super) const DO: u8 = 253;
pub(super) const DONT: u8 = 254;

pub(super) const BINARY: u8 = 0;
pub(super) const ECHO: u8 = 1;
pub(super) const SGA: u8 = 3;
pub(super) const TTYPE: u8 = 24;
pub(super) const NAWS: u8 = 31;
pub(super) const NEW_ENVIRON: u8 = 39;
pub(super) const ENVIRON: u8 = 36;
pub(super) const STATUS: u8 = 5;
/// RFC 859 subnegotiation command byte; owned here so `status_payload` and
/// the session layer cannot drift apart.
pub(super) const STATUS_IS: u8 = 0;
pub(super) const COMPRESS2: u8 = 86;
const MCCP: u8 = 85;

const MAX_SUBNEGOTIATION_SIZE: usize = 16 * 1024;
const MAX_COMPRESSED_INPUT_SIZE: usize = 64 * 1024;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum EngineEvent {
    Data(Vec<u8>),
    Send(Vec<u8>),
    Iac(u8),
    Negotiation { command: u8, option: u8 },
    Subnegotiation { option: u8, data: Vec<u8> },
    Warning(String),
    Error(String),
}

#[derive(Clone, Copy, Default)]
struct OptionState {
    us: QState,
    him: QState,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum QState {
    #[default]
    No,
    Yes,
    WantNo,
    WantYes,
    WantNoOpposite,
    WantYesOpposite,
}

#[derive(Clone, Copy)]
enum ParseState {
    Data,
    Iac,
    Will,
    Wont,
    Do,
    Dont,
    Subnegotiation,
    SubnegotiationData,
    SubnegotiationIac,
    SubnegotiationDiscard,
    SubnegotiationDiscardIac,
    MccpV1Marker,
    MccpV1MarkerIac,
}

pub(super) struct TelnetEngine {
    state: ParseState,
    subnegotiation_option: u8,
    subnegotiation: Vec<u8>,
    options: [OptionState; 256],
    decompressor: Option<Decompress>,
    compressed_input: Vec<u8>,
}

impl TelnetEngine {
    pub(super) fn new() -> Self {
        Self {
            state: ParseState::Data,
            subnegotiation_option: 0,
            subnegotiation: Vec::with_capacity(512),
            options: [OptionState::default(); 256],
            decompressor: None,
            compressed_input: Vec::with_capacity(1024),
        }
    }

    pub(super) fn receive(&mut self, data: &[u8]) -> Vec<EngineEvent> {
        let mut events = Vec::with_capacity((data.len() / 8).min(128) + 1);
        let mut pending_data = Vec::with_capacity(data.len().min(1024));
        self.receive_uncompressed(data, &mut events, &mut pending_data, true);
        // `pending_data` only accumulates in `ParseState::Data` and is
        // flushed eagerly at every IAC, so an unconditional flush is safe.
        flush_data(&mut events, &mut pending_data);
        events
    }

    fn receive_uncompressed(
        &mut self,
        data: &[u8],
        events: &mut Vec<EngineEvent>,
        pending_data: &mut Vec<u8>,
        allow_compression: bool,
    ) {
        let mut index = 0;

        while index < data.len() {
            if allow_compression && self.decompressor.is_some() {
                flush_data(events, pending_data);
                self.receive_compressed(&data[index..], events, pending_data);
                return;
            }

            let byte = data[index];
            let mut consume = true;

            match self.state {
                ParseState::Data => {
                    if byte == IAC {
                        flush_data(events, pending_data);
                        self.state = ParseState::Iac;
                    } else {
                        pending_data.push(byte);
                    }
                }
                ParseState::Iac => match byte {
                    SB => self.state = ParseState::Subnegotiation,
                    WILL => self.state = ParseState::Will,
                    WONT => self.state = ParseState::Wont,
                    DO => self.state = ParseState::Do,
                    DONT => self.state = ParseState::Dont,
                    IAC => {
                        events.push(EngineEvent::Data(vec![IAC]));
                        self.state = ParseState::Data;
                    }
                    command => {
                        events.push(EngineEvent::Iac(command));
                        self.state = ParseState::Data;
                    }
                },
                ParseState::Will => {
                    self.negotiate_received(WILL, byte, events);
                    self.state = ParseState::Data;
                }
                ParseState::Wont => {
                    self.negotiate_received(WONT, byte, events);
                    self.state = ParseState::Data;
                }
                ParseState::Do => {
                    self.negotiate_received(DO, byte, events);
                    self.state = ParseState::Data;
                }
                ParseState::Dont => {
                    self.negotiate_received(DONT, byte, events);
                    self.state = ParseState::Data;
                }
                ParseState::Subnegotiation => {
                    self.subnegotiation_option = byte;
                    self.subnegotiation.clear();
                    self.state = ParseState::SubnegotiationData;
                }
                ParseState::SubnegotiationData => {
                    if self.subnegotiation_option == MCCP && byte == WILL {
                        // MCCP v1 used the malformed IAC SB 85 WILL SE marker.
                        self.state = ParseState::MccpV1Marker;
                    } else if byte == IAC {
                        self.state = ParseState::SubnegotiationIac;
                    } else if !self.push_subnegotiation(byte, events) {
                        self.state = ParseState::SubnegotiationDiscard;
                    }
                }
                ParseState::SubnegotiationIac => match byte {
                    SE => {
                        self.emit_subnegotiation(events);
                        self.state = ParseState::Data;
                    }
                    IAC => {
                        if self.push_subnegotiation(IAC, events) {
                            self.state = ParseState::SubnegotiationData;
                        } else {
                            self.state = ParseState::SubnegotiationDiscard;
                        }
                    }
                    command => {
                        self.warning(
                            events,
                            format!("unexpected byte after IAC inside SB: {command}"),
                        );
                        self.emit_subnegotiation(events);
                        self.state = ParseState::Iac;
                        consume = false;
                    }
                },
                ParseState::SubnegotiationDiscard => {
                    if byte == IAC {
                        self.state = ParseState::SubnegotiationDiscardIac;
                    }
                }
                ParseState::SubnegotiationDiscardIac => match byte {
                    SE => self.state = ParseState::Data,
                    IAC => self.state = ParseState::SubnegotiationDiscard,
                    command => {
                        self.warning(
                            events,
                            format!("unexpected byte after IAC in discarded SB: {command}"),
                        );
                        self.state = ParseState::Iac;
                        consume = false;
                    }
                },
                ParseState::MccpV1Marker => match byte {
                    SE => self.state = ParseState::Data,
                    IAC => self.state = ParseState::MccpV1MarkerIac,
                    _ => self.state = ParseState::SubnegotiationDiscard,
                },
                ParseState::MccpV1MarkerIac => match byte {
                    SE => self.state = ParseState::Data,
                    IAC => self.state = ParseState::MccpV1Marker,
                    command => {
                        self.warning(events, format!("invalid MCCP v1 marker suffix: {command}"));
                        self.state = ParseState::Iac;
                        consume = false;
                    }
                },
            }

            if consume {
                index += 1;
            }
        }
    }

    fn receive_compressed(
        &mut self,
        data: &[u8],
        events: &mut Vec<EngineEvent>,
        pending_data: &mut Vec<u8>,
    ) {
        self.compressed_input.extend_from_slice(data);
        if self.compressed_input.len() > MAX_COMPRESSED_INPUT_SIZE {
            events.push(EngineEvent::Error(
                "MCCP2 compressed input buffer limit reached".to_string(),
            ));
            self.compressed_input.clear();
            self.decompressor = None;
            return;
        }
        let mut output = [0_u8; 8192];

        while !self.compressed_input.is_empty() {
            let Some(decoder) = self.decompressor.as_mut() else {
                let remaining = std::mem::take(&mut self.compressed_input);
                self.receive_uncompressed(&remaining, events, pending_data, true);
                return;
            };
            let input_before = decoder.total_in();
            let output_before = decoder.total_out();
            let result =
                decoder.decompress(&self.compressed_input, &mut output, FlushDecompress::None);
            let consumed = (decoder.total_in() - input_before) as usize;
            let produced = (decoder.total_out() - output_before) as usize;

            if consumed > 0 {
                self.compressed_input.drain(..consumed);
            }

            if produced > 0 {
                self.receive_uncompressed(&output[..produced], events, pending_data, false);
            }

            match result {
                Ok(Status::StreamEnd) => {
                    self.decompressor = None;
                    if !self.compressed_input.is_empty() {
                        let remaining = std::mem::take(&mut self.compressed_input);
                        self.receive_uncompressed(&remaining, events, pending_data, true);
                    }
                    return;
                }
                Ok(Status::Ok) | Ok(Status::BufError) => {
                    if consumed == 0 && produced == 0 {
                        // A compressed frame may end between any two TCP reads.
                        // Retain the unconsumed prefix until the next read.
                        return;
                    }
                }
                Err(error) => {
                    events.push(EngineEvent::Error(format!(
                        "MCCP2 decompression failed: {error}"
                    )));
                    self.decompressor = None;
                    self.compressed_input.clear();
                    return;
                }
            }
        }
    }

    pub(super) fn negotiate(&mut self, command: u8, option: u8) -> Vec<EngineEvent> {
        let mut events = Vec::new();
        match command {
            WILL => match self.options[option as usize].us {
                QState::No => {
                    self.options[option as usize].us = QState::WantYes;
                    self.send_negotiate(WILL, option, &mut events);
                }
                QState::WantNo => {
                    self.options[option as usize].us = QState::WantNoOpposite;
                }
                QState::WantYesOpposite => {
                    self.options[option as usize].us = QState::WantYes;
                }
                QState::Yes | QState::WantYes | QState::WantNoOpposite => {}
            },
            WONT => match self.options[option as usize].us {
                QState::Yes => {
                    self.options[option as usize].us = QState::WantNo;
                    self.send_negotiate(WONT, option, &mut events);
                }
                QState::WantYes => {
                    self.options[option as usize].us = QState::WantYesOpposite;
                }
                QState::WantYesOpposite => {
                    self.options[option as usize].us = QState::WantNo;
                    self.send_negotiate(WONT, option, &mut events);
                }
                QState::No | QState::WantNo | QState::WantNoOpposite => {}
            },
            DO => match self.options[option as usize].him {
                QState::No => {
                    self.options[option as usize].him = QState::WantYes;
                    self.send_negotiate(DO, option, &mut events);
                }
                QState::WantNo => {
                    self.options[option as usize].him = QState::WantNoOpposite;
                }
                QState::WantYesOpposite => {
                    self.options[option as usize].him = QState::WantYes;
                }
                QState::Yes | QState::WantYes | QState::WantNoOpposite => {}
            },
            DONT => match self.options[option as usize].him {
                QState::Yes => {
                    self.options[option as usize].him = QState::WantNo;
                    self.send_negotiate(DONT, option, &mut events);
                }
                QState::WantYes => {
                    self.options[option as usize].him = QState::WantYesOpposite;
                }
                QState::WantYesOpposite => {
                    self.options[option as usize].him = QState::WantNo;
                    self.send_negotiate(DONT, option, &mut events);
                }
                QState::No | QState::WantNo | QState::WantNoOpposite => {}
            },
            _ => {}
        }
        events
    }

    pub(super) fn send_terminal_input(&mut self, data: &[u8]) -> Vec<EngineEvent> {
        let input = if self.local_enabled(BINARY) {
            data.to_vec()
        } else {
            normalize_terminal_input(data)
        };
        let mut events = Vec::new();
        self.send_text(&input, &mut events);
        events
    }

    pub(super) fn subnegotiation(&mut self, option: u8, data: &[u8]) -> Vec<EngineEvent> {
        let mut frame =
            Vec::with_capacity(data.len() + 5 + data.iter().filter(|&&b| b == IAC).count());
        frame.extend_from_slice(&[IAC, SB, option]);
        append_escaped_data(&mut frame, data);
        frame.extend_from_slice(&[IAC, SE]);
        vec![EngineEvent::Send(frame)]
    }

    pub(super) fn local_enabled(&self, option: u8) -> bool {
        self.options[option as usize].us == QState::Yes
    }

    pub(super) fn status_payload(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(32);
        payload.push(STATUS_IS);
        for (option, state) in self.options.iter().enumerate() {
            let option = option as u8;
            if state.us != QState::No {
                payload.extend_from_slice(&[
                    if state.us == QState::Yes { WILL } else { WONT },
                    option,
                ]);
            }
            if state.him != QState::No {
                payload
                    .extend_from_slice(&[if state.him == QState::Yes { DO } else { DONT }, option]);
            }
        }
        payload
    }

    fn negotiate_received(&mut self, command: u8, option: u8, events: &mut Vec<EngineEvent>) {
        let state = self.options[option as usize];
        match command {
            WILL => match state.him {
                QState::No => {
                    if supports_remote(option) {
                        self.options[option as usize].him = QState::Yes;
                        self.send_negotiate(DO, option, events);
                        events.push(EngineEvent::Negotiation {
                            command: WILL,
                            option,
                        });
                    } else {
                        self.send_negotiate(DONT, option, events);
                    }
                }
                QState::WantNo => {
                    self.options[option as usize].him = QState::No;
                    events.push(EngineEvent::Negotiation {
                        command: WONT,
                        option,
                    });
                    self.warning(events, "DONT answered by WILL".to_string());
                }
                QState::WantNoOpposite => {
                    self.options[option as usize].him = QState::Yes;
                    self.warning(events, "DONT answered by WILL".to_string());
                }
                QState::WantYes => {
                    self.options[option as usize].him = QState::Yes;
                    events.push(EngineEvent::Negotiation {
                        command: WILL,
                        option,
                    });
                }
                QState::WantYesOpposite => {
                    self.options[option as usize].him = QState::WantNo;
                    self.send_negotiate(DONT, option, events);
                    events.push(EngineEvent::Negotiation {
                        command: WILL,
                        option,
                    });
                }
                QState::Yes => {}
            },
            WONT => match state.him {
                QState::Yes => {
                    self.options[option as usize].him = QState::No;
                    self.send_negotiate(DONT, option, events);
                    events.push(EngineEvent::Negotiation {
                        command: WONT,
                        option,
                    });
                }
                QState::WantNo => {
                    self.options[option as usize].him = QState::No;
                    events.push(EngineEvent::Negotiation {
                        command: WONT,
                        option,
                    });
                }
                QState::WantNoOpposite => {
                    self.options[option as usize].him = QState::WantYes;
                    self.send_negotiate(DO, option, events);
                    events.push(EngineEvent::Negotiation {
                        command: WONT,
                        option,
                    });
                }
                QState::WantYes | QState::WantYesOpposite => {
                    self.options[option as usize].him = QState::No;
                }
                QState::No => {}
            },
            DO => match state.us {
                QState::No => {
                    if supports_local(option) {
                        self.options[option as usize].us = QState::Yes;
                        self.send_negotiate(WILL, option, events);
                        events.push(EngineEvent::Negotiation {
                            command: DO,
                            option,
                        });
                    } else {
                        self.send_negotiate(WONT, option, events);
                    }
                }
                QState::WantNo => {
                    self.options[option as usize].us = QState::No;
                    events.push(EngineEvent::Negotiation {
                        command: DONT,
                        option,
                    });
                    self.warning(events, "WONT answered by DO".to_string());
                }
                QState::WantNoOpposite => {
                    self.options[option as usize].us = QState::Yes;
                    self.warning(events, "WONT answered by DO".to_string());
                }
                QState::WantYes => {
                    self.options[option as usize].us = QState::Yes;
                    events.push(EngineEvent::Negotiation {
                        command: DO,
                        option,
                    });
                }
                QState::WantYesOpposite => {
                    self.options[option as usize].us = QState::WantNo;
                    self.send_negotiate(WONT, option, events);
                    events.push(EngineEvent::Negotiation {
                        command: DO,
                        option,
                    });
                }
                QState::Yes => {}
            },
            DONT => match state.us {
                QState::Yes => {
                    self.options[option as usize].us = QState::No;
                    self.send_negotiate(WONT, option, events);
                    events.push(EngineEvent::Negotiation {
                        command: DONT,
                        option,
                    });
                }
                QState::WantNo => {
                    self.options[option as usize].us = QState::No;
                    events.push(EngineEvent::Negotiation {
                        command: DONT,
                        option,
                    });
                }
                QState::WantNoOpposite => {
                    self.options[option as usize].us = QState::WantYes;
                    self.send_negotiate(WILL, option, events);
                    events.push(EngineEvent::Negotiation {
                        command: DONT,
                        option,
                    });
                }
                QState::WantYes | QState::WantYesOpposite => {
                    self.options[option as usize].us = QState::No;
                }
                QState::No => {}
            },
            _ => {}
        }
    }

    fn send_negotiate(&self, command: u8, option: u8, events: &mut Vec<EngineEvent>) {
        events.push(EngineEvent::Send(vec![IAC, command, option]));
    }

    fn send_text(&self, data: &[u8], events: &mut Vec<EngineEvent>) {
        if data.is_empty() {
            return;
        }

        let binary = self.local_enabled(BINARY);
        let extra = data
            .iter()
            .filter(|&&byte| byte == IAC || (!binary && matches!(byte, b'\r' | b'\n')))
            .count();
        let mut encoded = Vec::with_capacity(data.len() + extra);
        for &byte in data {
            if byte == IAC {
                encoded.extend_from_slice(&[IAC, IAC]);
            } else if !binary && byte == b'\r' {
                encoded.extend_from_slice(&[b'\r', 0]);
            } else if !binary && byte == b'\n' {
                encoded.extend_from_slice(b"\r\n");
            } else {
                encoded.push(byte);
            }
        }
        events.push(EngineEvent::Send(encoded));
    }

    fn push_subnegotiation(&mut self, byte: u8, events: &mut Vec<EngineEvent>) -> bool {
        if self.subnegotiation.len() >= MAX_SUBNEGOTIATION_SIZE {
            self.warning(
                events,
                "subnegotiation buffer size limit reached".to_string(),
            );
            return false;
        }
        self.subnegotiation.push(byte);
        true
    }

    fn emit_subnegotiation(&mut self, events: &mut Vec<EngineEvent>) {
        // Retain the allocation for the next SB frame to avoid allocator
        // churn when a peer sends many small sub-negotiations.
        let data = self.subnegotiation.clone();
        self.subnegotiation.clear();
        match self.subnegotiation_option {
            TTYPE => validate_ttype(&data, events),
            ENVIRON | NEW_ENVIRON => validate_environ(self.subnegotiation_option, &data, events),
            _ => {}
        }
        let compression_marker = self.subnegotiation_option == COMPRESS2 && data.is_empty();
        events.push(EngineEvent::Subnegotiation {
            option: self.subnegotiation_option,
            data,
        });
        if compression_marker {
            // MCCP2 carries a zlib-wrapped DEFLATE stream after IAC SB 86 IAC
            // SE. Only honour the marker when COMPRESS2 was actually
            // negotiated; otherwise the peer could silently corrupt every
            // following byte by forcing us into decompression.
            if matches!(
                self.options[COMPRESS2 as usize].him,
                QState::Yes | QState::WantYes
            ) {
                self.decompressor = Some(Decompress::new(true));
            } else {
                self.warning(
                    events,
                    "MCCP2 activation marker without prior negotiation".to_string(),
                );
            }
        }
    }

    fn warning(&self, events: &mut Vec<EngineEvent>, message: String) {
        events.push(EngineEvent::Warning(message));
    }
}

fn supports_local(option: u8) -> bool {
    matches!(
        option,
        BINARY | SGA | TTYPE | NAWS | ENVIRON | NEW_ENVIRON | STATUS
    )
}

fn supports_remote(option: u8) -> bool {
    matches!(option, BINARY | ECHO | SGA | COMPRESS2)
}

fn append_escaped_data(output: &mut Vec<u8>, data: &[u8]) {
    for &byte in data {
        output.push(byte);
        if byte == IAC {
            output.push(IAC);
        }
    }
}

fn flush_data(events: &mut Vec<EngineEvent>, pending_data: &mut Vec<u8>) {
    if !pending_data.is_empty() {
        events.push(EngineEvent::Data(std::mem::take(pending_data)));
    }
}

fn validate_ttype(data: &[u8], events: &mut Vec<EngineEvent>) {
    let Some(&command) = data.first() else {
        events.push(EngineEvent::Warning(
            "incomplete TERMINAL-TYPE request".to_string(),
        ));
        return;
    };
    if !matches!(command, 0 | 1) {
        events.push(EngineEvent::Warning(
            "TERMINAL-TYPE request has invalid type".to_string(),
        ));
    }
}

fn validate_environ(option: u8, data: &[u8], events: &mut Vec<EngineEvent>) {
    let Some(&command) = data.first() else {
        return;
    };
    if !matches!(command, 0..=2) {
        events.push(EngineEvent::Warning(format!(
            "telopt {option} subneg has invalid command"
        )));
        return;
    }
    if data.len() == 1 {
        return;
    }
    if !matches!(data[1], 0 | 3) {
        events.push(EngineEvent::Warning(format!(
            "telopt {option} subneg missing variable type"
        )));
        return;
    }
    if data.last() == Some(&2) {
        events.push(EngineEvent::Warning(format!(
            "telopt {option} subneg ends with ESC"
        )));
    }
}

fn normalize_terminal_input(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] != b'\r' {
            output.push(input[index]);
            index += 1;
            continue;
        }
        match input.get(index + 1).copied() {
            Some(b'\n') => {
                output.push(b'\n');
                index += 2;
            }
            Some(0) => {
                output.push(b'\r');
                index += 2;
            }
            _ => {
                output.push(b'\n');
                index += 1;
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{EngineEvent, TelnetEngine, BINARY, COMPRESS2, DO, IAC, NAWS, SGA, WILL, WONT};

    fn sent(events: &[EngineEvent]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|event| match event {
                EngineEvent::Send(data) => Some(data.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect()
    }

    fn data(events: &[EngineEvent]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|event| match event {
                EngineEvent::Data(data) => Some(data.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect()
    }

    #[test]
    fn negotiation_survives_every_fragment() {
        let mut engine = TelnetEngine::new();
        assert!(engine.receive(&[IAC]).is_empty());
        assert!(engine.receive(&[WILL]).is_empty());
        let events = engine.receive(&[BINARY]);
        assert_eq!(sent(&events), [IAC, DO, BINARY]);
    }

    #[test]
    fn q_method_suppresses_duplicate_acknowledgements() {
        let mut engine = TelnetEngine::new();
        assert_eq!(sent(&engine.negotiate(DO, SGA)), [IAC, DO, SGA]);
        assert!(sent(&engine.receive(&[IAC, WILL, SGA])).is_empty());
        assert!(sent(&engine.receive(&[IAC, WILL, SGA])).is_empty());
    }

    #[test]
    fn terminal_enter_is_one_nvt_newline() {
        let mut engine = TelnetEngine::new();
        assert_eq!(sent(&engine.send_terminal_input(b"\r")), b"\r\n");
        assert_eq!(sent(&engine.send_terminal_input(b"\r\n")), b"\r\n");
        assert_eq!(sent(&engine.send_terminal_input(b"\n")), b"\r\n");
        assert_eq!(sent(&engine.send_terminal_input(b"\r\0")), b"\r\0");
    }

    #[test]
    fn binary_mode_is_directional_and_keeps_iac_escaping() {
        let mut engine = TelnetEngine::new();
        let _ = engine.negotiate(WILL, BINARY);
        let _ = engine.receive(&[IAC, DO, BINARY]);
        assert_eq!(
            sent(&engine.send_terminal_input(&[b'\r', IAC])),
            [b'\r', IAC, IAC]
        );
    }

    #[test]
    fn application_data_is_preserved_and_iac_is_unescaped() {
        let mut engine = TelnetEngine::new();
        let events = engine.receive(b"login: ");
        assert_eq!(data(&events), b"login: ");
        let events = engine.receive(&[IAC, IAC, b'X']);
        assert_eq!(data(&events), [IAC, b'X']);
    }

    #[test]
    fn unsupported_options_are_rejected() {
        let mut engine = TelnetEngine::new();
        assert_eq!(
            sent(&engine.receive(&[IAC, WILL, COMPRESS2])),
            [IAC, DO, COMPRESS2]
        );
        assert_eq!(
            sent(&engine.receive(&[IAC, DO, 42])),
            [IAC, super::WONT, 42]
        );
    }

    #[test]
    fn naws_subnegotiation_is_fragment_safe() {
        let mut engine = TelnetEngine::new();
        assert!(engine.receive(&[IAC, super::SB, NAWS, 0]).is_empty());
        let events = engine.receive(&[80, 0, 24, IAC]);
        assert!(events.is_empty());
        let events = engine.receive(&[super::SE]);
        assert!(matches!(
            events.as_slice(),
            [EngineEvent::Subnegotiation { option: NAWS, data }] if data == &[0, 80, 0, 24]
        ));
    }

    #[test]
    fn subnegotiation_escapes_iac_bytes() {
        let mut engine = TelnetEngine::new();
        assert_eq!(
            sent(&engine.subnegotiation(NAWS, &[1, IAC, 2])),
            [IAC, super::SB, NAWS, 1, IAC, IAC, 2, IAC, super::SE]
        );
    }

    #[test]
    fn ordinary_iac_commands_are_reported_and_not_delivered_as_data() {
        let mut engine = TelnetEngine::new();
        let events = engine.receive(&[IAC, 241]);
        assert!(matches!(events.as_slice(), [EngineEvent::Iac(241)]));
        assert!(data(&events).is_empty());
    }

    #[test]
    fn malformed_subnegotiation_is_reported_without_panicking() {
        let mut engine = TelnetEngine::new();
        let events = engine.receive(&[IAC, super::SB, super::TTYPE, 9, IAC, super::SE]);
        assert!(events.iter().any(|event| matches!(
            event,
            EngineEvent::Warning(message) if message.contains("invalid type")
        )));
    }

    #[test]
    fn oversized_subnegotiation_is_bounded() {
        let mut engine = TelnetEngine::new();
        let mut input = vec![IAC, super::SB, NAWS];
        input.extend(std::iter::repeat_n(0, super::MAX_SUBNEGOTIATION_SIZE + 1));
        let events = engine.receive(&input);
        assert!(events.iter().any(|event| matches!(
            event,
            EngineEvent::Warning(message) if message.contains("size limit")
        )));
    }

    #[test]
    fn oversized_subnegotiation_is_discarded_until_its_end_marker() {
        let mut engine = TelnetEngine::new();
        let mut input = vec![IAC, super::SB, NAWS];
        input.extend(std::iter::repeat_n(
            b'x',
            super::MAX_SUBNEGOTIATION_SIZE + 1,
        ));
        input.extend([IAC, super::SE]);
        input.extend(b"prompt> ");

        let events = engine.receive(&input);
        assert_eq!(data(&events), b"prompt> ");
    }

    #[test]
    fn malformed_mccp_v1_marker_is_consumed_without_leaking_bytes() {
        let mut engine = TelnetEngine::new();
        let events = engine.receive(&[IAC, super::SB, super::MCCP, WILL, super::SE, b'o', b'k']);

        assert_eq!(data(&events), b"ok");
        assert!(!events
            .iter()
            .any(|event| matches!(event, EngineEvent::Iac(_))));
    }

    #[test]
    fn mccp2_marker_without_negotiation_is_ignored() {
        let mut engine = TelnetEngine::new();
        let events = engine.receive(&[IAC, super::SB, COMPRESS2, IAC, super::SE, b'o', b'k']);

        assert_eq!(data(&events), b"ok");
        assert!(events.iter().any(|event| matches!(
            event,
            EngineEvent::Warning(message) if message.contains("without prior negotiation")
        )));
        assert!(!events
            .iter()
            .any(|event| matches!(event, EngineEvent::Error(_))));
    }

    #[test]
    fn mccp2_decompresses_data_after_the_activation_marker() {
        use flate2::{write::ZlibEncoder, Compression};
        use std::io::Write;

        let mut compressed = ZlibEncoder::new(Vec::new(), Compression::fast());
        compressed.write_all(b"compressed prompt> ").unwrap();
        let compressed = compressed.finish().unwrap();
        let mut input = vec![IAC, super::SB, COMPRESS2, IAC, super::SE];
        input.extend_from_slice(&compressed);

        let mut engine = TelnetEngine::new();
        let _ = engine.negotiate(DO, COMPRESS2);
        let events = engine.receive(&input);
        assert_eq!(data(&events), b"compressed prompt> ");
        assert!(!events
            .iter()
            .any(|event| matches!(event, EngineEvent::Error(_))));
    }

    #[test]
    fn mccp2_decompression_survives_compressed_input_fragments() {
        use flate2::{write::ZlibEncoder, Compression};
        use std::io::Write;

        let mut compressed = ZlibEncoder::new(Vec::new(), Compression::fast());
        compressed.write_all(b"fragmented").unwrap();
        let compressed = compressed.finish().unwrap();
        let mut engine = TelnetEngine::new();
        let _ = engine.negotiate(DO, COMPRESS2);
        let _ = engine.receive(&[IAC, super::SB, COMPRESS2, IAC, super::SE]);

        let mut events = Vec::new();
        for byte in compressed {
            events.extend(engine.receive(&[byte]));
        }
        assert_eq!(data(&events), b"fragmented");
    }

    #[test]
    fn mccp2_invalid_stream_is_reported_as_a_protocol_error() {
        let mut engine = TelnetEngine::new();
        let _ = engine.negotiate(DO, COMPRESS2);
        let events = engine.receive(&[
            IAC,
            super::SB,
            COMPRESS2,
            IAC,
            super::SE,
            0xde,
            0xad,
            0xbe,
            0xef,
        ]);
        assert!(events.iter().any(|event| matches!(
            event,
            EngineEvent::Error(message) if message.contains("MCCP2 decompression failed")
        )));
    }

    #[test]
    fn crossed_negotiation_requests_follow_rfc1143() {
        let mut engine = TelnetEngine::new();
        assert_eq!(sent(&engine.negotiate(WILL, SGA)), [IAC, WILL, SGA]);
        assert!(sent(&engine.negotiate(WONT, SGA)).is_empty());
        let events = engine.receive(&[IAC, DO, SGA]);
        assert_eq!(sent(&events), [IAC, WONT, SGA]);
        assert!(events.iter().any(|event| {
            matches!(
                event,
                EngineEvent::Negotiation {
                    command: DO,
                    option: SGA
                }
            )
        }));
    }

    #[test]
    fn arbitrary_fragmented_input_does_not_panic_or_grow_unboundedly() {
        let mut engine = TelnetEngine::new();
        let mut seed = 0x6d2b_79f5_u32;
        for _ in 0..2048 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let size = (seed as usize & 31) + 1;
            let mut chunk = Vec::with_capacity(size);
            for _ in 0..size {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                chunk.push((seed >> 24) as u8);
            }
            let _ = engine.receive(&chunk);
        }
    }
}
