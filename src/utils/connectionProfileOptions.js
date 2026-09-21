const CONNECTION_OPTION_FIELDS = new Set([
  "terminalType",
  "encoding",
  "backspaceSends",
  "realtimeEncodingDetection",
  "terminalHighlightEnabled",
  "terminalMorePromptCleanup",
  "runtimeMetrics",
]);

// VNC 会话开关存放在 profile.details（协议标记联合）里而非 options；
// key 为 ListItem 的 vnc* 字段名，value 为 details 内的字段名。
const VNC_DETAIL_FIELDS = new Map([
  ["vncViewOnly", "viewOnly"],
  ["vncShared", "shared"],
  ["vncQuality", "quality"],
  ["vncCompression", "compression"],
  ["vncScaleMode", "scaleMode"],
  ["vncClipboardSync", "clipboardSync"],
  ["vncResizeSession", "resizeSession"],
]);

// RDP 会话开关同样落在 details；key 为 ListItem 的 rdp* 字段名。
const RDP_DETAIL_FIELDS = new Map([
  ["rdpDomain", "domain"],
  ["rdpScaleMode", "scaleMode"],
  ["rdpClipboardSync", "clipboardSync"],
  ["rdpResizeSession", "resizeSession"],
]);

export function mergeConnectionProfileOptions(profile, patch) {
  const options = { ...(profile?.options || {}) };
  let details = profile?.details || null;
  for (const [field, value] of Object.entries(patch || {})) {
    const detailField = VNC_DETAIL_FIELDS.get(field) || RDP_DETAIL_FIELDS.get(field);
    if (detailField && details && typeof details === "object") {
      details = { ...details };
      if (value === undefined) {
        delete details[detailField];
      } else {
        details[detailField] = value;
      }
      continue;
    }
    if (!CONNECTION_OPTION_FIELDS.has(field)) continue;
    if (value === undefined) {
      delete options[field];
    } else {
      options[field] = value;
    }
  }
  return { ...profile, options, ...(details ? { details } : {}) };
}
