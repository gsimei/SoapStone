import { plain } from "./logic";

type Row = { name: string; value: string; secret?: boolean };

/** The rows plus one empty trailing row; typing in the trailing row adds a row. */
export function KeyValueTable(props: {
  rows: Row[];
  onChange(rows: Row[]): void;
  namePlaceholder: string;
  valuePlaceholder: string;
  /** Each row gets a "Secret" toggle; a secret value is masked. */
  secrets?: boolean;
}) {
  const all = [...props.rows, { name: "", value: "" }];
  const set = (i: number, patch: Partial<Row>) =>
    props.onChange(all.map((r, j) => (j === i ? { ...r, ...patch } : r)).filter((r, j) => j < props.rows.length || r.name || r.value || r.secret));

  return (
    <table className="kv">
      <tbody>
        {all.map((r, i) => (
          <tr key={i}>
            <td>
              <input {...plain}
                aria-label="Name"
                placeholder={props.namePlaceholder}
                value={r.name}
                onChange={(e) => set(i, { name: e.target.value })}
              />
            </td>
            <td>
              <input {...plain}
                aria-label="Value"
                type={props.secrets && r.secret ? "password" : undefined}
                placeholder={props.valuePlaceholder}
                value={r.value}
                onChange={(e) => set(i, { value: e.target.value })}
              />
            </td>
            {props.secrets && (
              <td className="kv-secret">
                <input
                  type="checkbox"
                  aria-label="Secret"
                  title="Keep the value in the system keychain"
                  checked={!!r.secret}
                  onChange={(e) => set(i, { secret: e.target.checked })}
                />
              </td>
            )}
            <td className="kv-remove">
              {i < props.rows.length && (
                <button aria-label="Remove" title="Remove" onClick={() => props.onChange(props.rows.filter((_, j) => j !== i))}>
                  ×
                </button>
              )}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
