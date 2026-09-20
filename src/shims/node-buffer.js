const NativeBuffer = Uint8Array;
function from(value, encoding) {
  if (typeof value === "string") {
    if (encoding === "base64") {
      const bin = atob(value);
      return new NativeBuffer([...bin].map((c) => c.charCodeAt(0)));
    }
    if (encoding === "hex") {
      const out = new NativeBuffer(Math.floor(value.length / 2));
      for (let i = 0; i < out.length; i++) out[i] = parseInt(value.slice(i * 2, i * 2 + 2), 16);
      return out;
    }
    return new TextEncoder().encode(value);
  }
  if (value instanceof ArrayBuffer) return new NativeBuffer(value);
  return new NativeBuffer(value ?? 0);
}
function Buffer(value, encoding) {
  return from(value, encoding);
}
Buffer.from = from;
Buffer.alloc = (size, fill = 0) => {
  const out = new NativeBuffer(size);
  if (fill !== 0) out.fill(fill);
  return out;
};
Buffer.allocUnsafe = Buffer.alloc;
Buffer.allocUnsafeSlow = Buffer.alloc;
Buffer.isBuffer = (value) => value instanceof NativeBuffer;
Buffer.concat = (list, totalLength = list.reduce((n, x) => n + x.length, 0)) => {
  const out = new NativeBuffer(totalLength);
  let offset = 0;
  for (const item of list) {
    out.set(item, offset);
    offset += item.length;
  }
  return out;
};
Buffer.byteLength = (value) =>
  typeof value === "string" ? new TextEncoder().encode(value).length : (value?.byteLength ?? 0);
export { Buffer };
export default { Buffer };
