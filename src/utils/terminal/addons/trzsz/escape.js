// 把转义规则预计算成查找表：编码按字节 O(1) 命中，解码按双字节组合命中，
// 避免热路径上对每字节线性扫描规则列表。
export function escapeCodes(escapeChars = []) {
  const encode = new Uint32Array(256);
  const decode = new Map();
  for (const pair of escapeChars) {
    const byte = pair[0].charCodeAt(0);
    const combined = (pair[1].charCodeAt(0) << 8) | pair[1].charCodeAt(1);
    encode[byte] = combined + 1;
    decode.set(combined, byte);
  }
  return { size: decode.size, encode, decode };
}

export function escapeData(data, codes) {
  if (!codes.size) return data;
  const result = new Uint8Array(data.length * 2);
  let offset = 0;
  for (const byte of data) {
    const code = codes.encode[byte];
    if (!code) {
      result[offset++] = byte;
    } else {
      result[offset++] = (code - 1) >> 8;
      result[offset++] = (code - 1) & 0xff;
    }
  }
  return result.subarray(0, offset);
}

export function unescapeData(data, codes) {
  if (!codes.size) return data;
  const result = new Uint8Array(data.length);
  let offset = 0;
  for (let index = 0; index < data.length; index += 1) {
    const code =
      index + 1 < data.length ? codes.decode.get((data[index] << 8) | data[index + 1]) : undefined;
    if (code === undefined) {
      result[offset++] = data[index];
    } else {
      result[offset++] = code;
      index += 1;
    }
  }
  return result.subarray(0, offset);
}
