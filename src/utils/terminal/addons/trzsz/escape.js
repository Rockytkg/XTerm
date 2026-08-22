export function escapeCodes(escapeChars = []) {
  return escapeChars.map((pair) => [
    pair[0].charCodeAt(0),
    pair[1].charCodeAt(0),
    pair[1].charCodeAt(1),
  ]);
}

export function escapeData(data, codes) {
  if (!codes.length) return data;
  const result = new Uint8Array(data.length * 2);
  let offset = 0;
  for (const byte of data) {
    const code = codes.find((item) => item[0] === byte);
    if (!code) {
      result[offset++] = byte;
    } else {
      result[offset++] = code[1];
      result[offset++] = code[2];
    }
  }
  return result.subarray(0, offset);
}

export function unescapeData(data, codes) {
  if (!codes.length) return data;
  const result = new Uint8Array(data.length);
  let offset = 0;
  for (let index = 0; index < data.length; index += 1) {
    const code =
      index + 1 < data.length
        ? codes.find((item) => item[1] === data[index] && item[2] === data[index + 1])
        : null;
    if (!code) {
      result[offset++] = data[index];
    } else {
      result[offset++] = code[0];
      index += 1;
    }
  }
  return result.subarray(0, offset);
}
