use crate::{logging, terminal::internal::core::SerialPortOption};

#[tauri::command]
pub(crate) async fn serial_list_ports() -> Result<Vec<SerialPortOption>, String> {
    let ports = tokio::task::spawn_blocking(tokio_serial::available_ports)
        .await
        .map_err(|error| format!("serial port enumeration task failed: {error}"))?
        .map_err(|error| format!("failed to list serial ports: {error}"))?;
    logging::event("terminal.serial", "serial.list_ports")
        .field("count", ports.len())
        .debug();

    Ok(ports
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
        .collect())
}
