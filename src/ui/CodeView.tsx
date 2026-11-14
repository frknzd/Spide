import { useOverlay } from "./overlay";

// Read-only view of the imported implementation of a class or method (/code Class[.method]).
export function CodeView({ title, text, onClose }: { title: string; text: string; onClose: () => void }) {
  useOverlay(onClose);
  return (
    <section className="code-view" data-ui aria-label={`Implementation of ${title}`}>
      <div className="bp-head">
        <div><span className="dp-kicker">Imported code · read-only</span><h2 className="mono">{title}</h2></div>
        <button className="icon-button" aria-label="Close code" title="Close · Esc" autoFocus onClick={onClose}>×</button>
      </div>
      <pre className="mono" tabIndex={0}>{text || "No implementation was imported for this element."}</pre>
    </section>
  );
}
