import { useEffect, useState } from "react";
import { api } from "../api";

/** The app's own font library: fonts imported once and kept across restarts. */
export function FontsDialog(props: { onClose: () => void; onImport: () => Promise<void>; systemFamilies: number }) {
  const [files, setFiles] = useState<{ file: string; families: string[] }[] | null>(null);
  const [folder, setFolder] = useState("");
  const [error, setError] = useState<string | null>(null);
  const reload = () => api.fontsImported().then(setFiles).catch((e) => setError(String(e)));
  useEffect(() => {
    reload();
    api.fontsFolder().then(setFolder).catch(() => {});
  }, []);
  return (
    <div className="modal-bg" onMouseDown={props.onClose}>
      <div className="modal fonts-modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h2>Fonts</h2>
          <span className="muted small">{props.systemFamilies} font families available</span>
        </div>
        <p className="small">
          SignCut Port uses every font installed on this Mac (including Adobe Fonts and font managers). Fonts you import here are
          copied into the app and are always available — handy for fonts that are not installed in macOS. You can also drop
          font files onto the window.
        </p>
        <table className="fonts-table">
          <thead>
            <tr>
              <th>Imported font file</th>
              <th>Family</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {files?.length === 0 && (
              <tr>
                <td colSpan={3} className="muted">
                  No fonts imported yet.
                </td>
              </tr>
            )}
            {files?.map((f) => (
              <tr key={f.file}>
                <td className="mono small">{f.file}</td>
                <td>
                  {f.families.map((fam) => (
                    <div key={fam} style={{ fontFamily: `"${fam}"`, fontSize: 16 }}>
                      {fam}
                    </div>
                  ))}
                </td>
                <td style={{ textAlign: "right" }}>
                  <button
                    className="mini"
                    onClick={async () => {
                      try {
                        await api.fontsRemove(f.file);
                        reload();
                      } catch (e) {
                        setError(String(e));
                      }
                    }}
                  >
                    Remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {folder && <div className="muted small ellipsis" title={folder}>Stored in {folder}</div>}
        {error && <div className="error small">{error}</div>}
        <div className="modal-foot">
          <span />
          <div className="row gap">
            <button onClick={props.onClose}>Done</button>
            <button
              className="primary"
              onClick={async () => {
                await props.onImport();
                reload();
              }}
            >
              Import fonts…
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
