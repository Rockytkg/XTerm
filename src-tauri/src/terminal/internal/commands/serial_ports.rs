use crate::{
    logging,
    terminal::internal::core::{compare_serial_port_names, SerialPortOption},
};

#[tauri::command]
pub(crate) async fn serial_list_ports() -> Result<Vec<SerialPortOption>, String> {
    let ports = tokio::task::spawn_blocking(tokio_serial::available_ports)
        .await
        .map_err(|error| format!("serial port enumeration task failed: {error}"))?
        .map_err(|error| format!("failed to list serial ports: {error}"))?;
    logging::event("terminal.serial", "serial.list_ports")
        .field("count", ports.len())
        .debug();

    let mut options: Vec<SerialPortOption> = ports
        .into_iter()
        .map(|port| {
            let (kind, detail) = match port.port_type {
                tokio_serial::SerialPortType::UsbPort(info) => {
                    let product = info.product.unwrap_or_else(|| "USB serial".to_string());
                    let manufacturer = info.manufacturer.unwrap_or_default();
                    let label = if manufacturer.is_empty() {
                        product
                    } else {
                        format!("{manufacturer} {product}")
                    };
                    ("usb".to_string(), label)
                }
                tokio_serial::SerialPortType::BluetoothPort => {
                    ("bluetooth".to_string(), "Bluetooth serial".to_string())
                }
                tokio_serial::SerialPortType::PciPort => {
                    ("pci".to_string(), "PCI serial".to_string())
                }
                tokio_serial::SerialPortType::Unknown => {
                    ("unknown".to_string(), "Serial port".to_string())
                }
            };
            SerialPortOption {
                label: format!("{} · {}", port.port_name, detail),
                name: port.port_name,
                kind,
            }
        })
        .collect();

    // USB 转串口排在最前(优先展示),其次按端口名自然序。
    options.sort_by(|a, b| {
        serial_kind_priority(&a.kind)
            .cmp(&serial_kind_priority(&b.kind))
            .then_with(|| compare_serial_port_names(&a.name, &b.name))
    });

    Ok(options)
}

fn serial_kind_priority(kind: &str) -> u8 {
    match kind {
        "usb" => 0,
        "bluetooth" => 1,
        "pci" => 2,
        _ => 3,
    }
}
