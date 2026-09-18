export function inspect(value) { try { return JSON.stringify(value); } catch { return String(value); } } export default { inspect };
