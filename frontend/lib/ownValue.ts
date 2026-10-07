/** `record[key]` restricted to the record's own keys: `undefined` for a key
 * it doesn't declare, including inherited ones like `'constructor'` that a
 * bare lookup would resolve to an `Object.prototype` function (Signal Box
 * Audit, flib Low finding: "prototype-key lookups can render a function as
 * a label"). */
export function ownValue<V>(record: Readonly<Record<string, V>>, key: string): V | undefined {
  return Object.prototype.hasOwnProperty.call(record, key) ? record[key] : undefined;
}
