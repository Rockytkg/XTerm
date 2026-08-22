use std::{
    ffi::{c_char, c_int, c_uchar, c_void, CStr},
    ptr::NonNull,
};

const EVENT_DATA: c_int = 0;
const EVENT_SEND: c_int = 1;
const EVENT_IAC: c_int = 2;
const EVENT_WILL: c_int = 3;
const EVENT_WONT: c_int = 4;
const EVENT_DO: c_int = 5;
const EVENT_DONT: c_int = 6;
const EVENT_SUBNEGOTIATION: c_int = 7;
const EVENT_WARNING: c_int = 8;
const EVENT_ERROR: c_int = 9;

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

#[repr(C)]
struct NativeTelnet {
    _private: [u8; 0],
}

type NativeCallback = unsafe extern "C" fn(
    *mut c_void,
    c_int,
    c_uchar,
    c_uchar,
    *const c_char,
    usize,
    c_int,
    *const c_char,
);

#[repr(C)]
struct NativeContext {
    callback: Option<NativeCallback>,
    user_data: *mut c_void,
}

extern "C" {
    fn xterm_telnet_init(
        context: *mut NativeContext,
        callback: Option<NativeCallback>,
        user_data: *mut c_void,
    ) -> *mut NativeTelnet;
    fn xterm_telnet_free(telnet: *mut NativeTelnet);
    fn xterm_telnet_recv(telnet: *mut NativeTelnet, data: *const c_char, size: usize);
    fn xterm_telnet_negotiate(telnet: *mut NativeTelnet, command: c_uchar, option: c_uchar);
    fn xterm_telnet_send_text(telnet: *mut NativeTelnet, data: *const c_char, size: usize);
    fn xterm_telnet_subnegotiation(
        telnet: *mut NativeTelnet,
        option: c_uchar,
        data: *const c_char,
        size: usize,
    );
}

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

#[derive(Default)]
struct EventSink {
    events: Vec<EngineEvent>,
}

pub(super) struct TelnetEngine {
    native: NonNull<NativeTelnet>,
    _native_context: Box<NativeContext>,
    sink: Box<EventSink>,
    local_enabled: [bool; 256],
    remote_enabled: [bool; 256],
}

impl TelnetEngine {
    pub(super) fn new() -> Result<Self, String> {
        let mut sink = Box::<EventSink>::default();
        let mut native_context = Box::new(NativeContext {
            callback: None,
            user_data: std::ptr::null_mut(),
        });
        let native = unsafe {
            xterm_telnet_init(
                native_context.as_mut(),
                Some(native_callback),
                sink.as_mut() as *mut EventSink as *mut c_void,
            )
        };
        let native = NonNull::new(native)
            .ok_or_else(|| "libtelnet failed to allocate protocol state".to_string())?;
        Ok(Self {
            native,
            _native_context: native_context,
            sink,
            local_enabled: [false; 256],
            remote_enabled: [false; 256],
        })
    }

    pub(super) fn receive(&mut self, data: &[u8]) -> Vec<EngineEvent> {
        unsafe {
            xterm_telnet_recv(
                self.native.as_ptr(),
                data.as_ptr().cast::<c_char>(),
                data.len(),
            );
        }
        self.take_events()
    }

    pub(super) fn negotiate(&mut self, command: u8, option: u8) -> Vec<EngineEvent> {
        unsafe { xterm_telnet_negotiate(self.native.as_ptr(), command, option) };
        self.take_events()
    }

    pub(super) fn send_terminal_input(&mut self, data: &[u8]) -> Vec<EngineEvent> {
        let normalized = if self.local_enabled[BINARY as usize] {
            data.to_vec()
        } else {
            normalize_terminal_input(data)
        };
        unsafe {
            xterm_telnet_send_text(
                self.native.as_ptr(),
                normalized.as_ptr().cast::<c_char>(),
                normalized.len(),
            );
        }
        self.take_events()
    }

    pub(super) fn subnegotiation(&mut self, option: u8, data: &[u8]) -> Vec<EngineEvent> {
        unsafe {
            xterm_telnet_subnegotiation(
                self.native.as_ptr(),
                option,
                data.as_ptr().cast::<c_char>(),
                data.len(),
            );
        }
        self.take_events()
    }

    pub(super) fn local_enabled(&self, option: u8) -> bool {
        self.local_enabled[option as usize]
    }

    fn take_events(&mut self) -> Vec<EngineEvent> {
        let events = std::mem::take(&mut self.sink.events);
        for event in &events {
            if let EngineEvent::Negotiation { command, option } = *event {
                match command {
                    DO => self.local_enabled[option as usize] = true,
                    DONT => self.local_enabled[option as usize] = false,
                    WILL => self.remote_enabled[option as usize] = true,
                    WONT => self.remote_enabled[option as usize] = false,
                    _ => {}
                }
            }
        }
        events
    }
}

impl Drop for TelnetEngine {
    fn drop(&mut self) {
        unsafe { xterm_telnet_free(self.native.as_ptr()) };
    }
}

unsafe extern "C" fn native_callback(
    user_data: *mut c_void,
    kind: c_int,
    command: c_uchar,
    option: c_uchar,
    buffer: *const c_char,
    size: usize,
    error_code: c_int,
    message: *const c_char,
) {
    let Some(sink) = user_data.cast::<EventSink>().as_mut() else {
        return;
    };
    let bytes = if buffer.is_null() || size == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(buffer.cast::<u8>(), size).to_vec()
    };
    let event = match kind {
        EVENT_DATA => EngineEvent::Data(bytes),
        EVENT_SEND => EngineEvent::Send(bytes),
        EVENT_IAC => EngineEvent::Iac(command),
        EVENT_WILL => EngineEvent::Negotiation {
            command: WILL,
            option,
        },
        EVENT_WONT => EngineEvent::Negotiation {
            command: WONT,
            option,
        },
        EVENT_DO => EngineEvent::Negotiation {
            command: DO,
            option,
        },
        EVENT_DONT => EngineEvent::Negotiation {
            command: DONT,
            option,
        },
        EVENT_SUBNEGOTIATION => EngineEvent::Subnegotiation {
            option,
            data: bytes,
        },
        EVENT_WARNING | EVENT_ERROR => {
            let detail = if message.is_null() {
                format!("libtelnet error {error_code}")
            } else {
                CStr::from_ptr(message).to_string_lossy().into_owned()
            };
            if kind == EVENT_WARNING {
                EngineEvent::Warning(detail)
            } else {
                EngineEvent::Error(detail)
            }
        }
        _ => return,
    };
    sink.events.push(event);
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
                // xterm emits a bare CR for Enter; libtelnet maps LF to the
                // NVT new-line sequence CR LF and maps CR to literal CR NUL.
                output.push(b'\n');
                index += 1;
            }
        }
    }
    output
}

// The native state is owned by one transport actor and is never shared.
unsafe impl Send for TelnetEngine {}

#[cfg(test)]
mod tests {
    use super::{EngineEvent, TelnetEngine, BINARY, DO, SGA, WILL};

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

    #[test]
    fn libtelnet_preserves_negotiation_across_every_fragment() {
        let mut engine = TelnetEngine::new().unwrap();
        assert!(engine.receive(&[255]).is_empty());
        assert!(engine.receive(&[WILL]).is_empty());
        let events = engine.receive(&[BINARY]);
        assert_eq!(sent(&events), [255, DO, BINARY]);
    }

    #[test]
    fn libtelnet_q_method_suppresses_duplicate_acknowledgements() {
        let mut engine = TelnetEngine::new().unwrap();
        assert_eq!(sent(&engine.negotiate(DO, SGA)), [255, DO, SGA]);
        assert!(sent(&engine.receive(&[255, WILL, SGA])).is_empty());
        assert!(sent(&engine.receive(&[255, WILL, SGA])).is_empty());
    }

    #[test]
    fn terminal_enter_is_one_nvt_newline() {
        let mut engine = TelnetEngine::new().unwrap();
        assert_eq!(sent(&engine.send_terminal_input(b"\r")), b"\r\n");
        assert_eq!(sent(&engine.send_terminal_input(b"\r\n")), b"\r\n");
    }

    #[test]
    fn binary_mode_is_directional_and_keeps_iac_escaping() {
        let mut engine = TelnetEngine::new().unwrap();
        let _ = engine.negotiate(WILL, BINARY);
        let _ = engine.receive(&[255, DO, BINARY]);
        assert_eq!(
            sent(&engine.send_terminal_input(&[b'\r', 255])),
            [b'\r', 255, 255]
        );
    }
}
