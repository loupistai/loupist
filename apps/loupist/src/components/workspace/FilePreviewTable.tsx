type FilePreviewCell = {
  text: string; numeric?: boolean; error?: boolean; tooltip?: string;
  formula?: "cached" | "uncached";
};

export function FilePreviewTable({ label, columns, rows, firstRow = 1 }: {
  label: string; columns: string[]; rows: FilePreviewCell[][]; firstRow?: number;
}) {
  return <div className="reader-data-scroll">
    <table className="reader-data-table" aria-label={label}>
      <thead><tr>
        <th scope="col" className="reader-row-number">#</th>
        {columns.map((column, index) => <th scope="col" key={index} title={column}>{column}</th>)}
      </tr></thead>
      <tbody>{rows.map((row, index) => <tr key={index}>
        <th scope="row" className="reader-row-number">{firstRow + index}</th>
        {columns.map((_, column) => {
          const cell = row[column];
          const text = cell?.text ?? "";
          return <td key={column} title={cell?.tooltip ?? text} data-formula={cell?.formula}
            className={cell?.numeric ? "is-numeric" : cell?.error ? "is-error" : undefined}>{text}</td>;
        })}
      </tr>)}</tbody>
    </table>
  </div>;
}
