const CONNECTION_OPTION_FIELDS = new Set([
  "terminalType",
  "encoding",
  "backspaceSends",
  "realtimeEncodingDetection",
  "terminalHighlightEnabled",
  "terminalMorePromptCleanup",
]);

export function mergeConnectionProfileOptions(profile, patch) {
  const options = { ...(profile?.options || {}) };
  for (const [field, value] of Object.entries(patch || {})) {
    if (!CONNECTION_OPTION_FIELDS.has(field)) continue;
    if (value === undefined) {
      delete options[field];
    } else {
      options[field] = value;
    }
  }
  return { ...profile, options };
}
