import { CONNECTION_EVENT } from "./connectionStateMachine.js";
import { capabilitiesCan } from "../utils/connectionCapabilities.js";

export function applyOpenResponseMetadata({
  dispatchConnectionEvent,
  response,
  sessionId = "",
  sessionRegistry,
}) {
  if (!sessionId) return;

  if (capabilitiesCan(response?.capabilities, "serialBaudDetection")) {
    dispatchConnectionEvent(sessionId, {
      type: CONNECTION_EVENT.SERIAL_METADATA_RECEIVED,
      payload: response,
    });
  }

  sessionRegistry?.setConnectionCapabilities(sessionId, response?.capabilities || null);
}
