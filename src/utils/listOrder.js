export function sameOrder(a, b) {
  return a.length === b.length && a.every((id, index) => id === b[index]);
}
