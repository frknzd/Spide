// Read-only view of the imported implementation of a class or method (/code Class[.method]).
export function CodeView({ title, text, onClose }: { title: string; text: string; onClose: () => void }) {
  return (
    <div className="code-view" data-ui>
      <div className="bp-head">
        <span className="dp-kicker">code</span>
        <b className="mono">{title}</b>
        <button className="link" onClick={onClose}>close</button>
      </div>
      <pre className="mono">{text}</pre>
    </div>
  );
}
