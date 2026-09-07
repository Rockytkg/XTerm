import { invokeDebugIpc, invokeIpc } from "./ipc/core";

export function openDevTools() {
  return invokeIpc("open_devtools");
}

export function listSerialPorts() {
  return invokeDebugIpc("serial_list_ports");
}
