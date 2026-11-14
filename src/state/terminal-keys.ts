// xterm's default legacy bindings are unchanged. Encode ambiguous keys only when pi
// has explicitly enabled xterm modifyOtherKeys (CSI > 4 ; 2 m).
export function modifiedKey(e: Pick<KeyboardEvent, "key" | "shiftKey" | "ctrlKey" | "altKey" | "metaKey" | "type">, enabled: boolean): string | null {
  if (!enabled || e.type !== "keydown" || e.metaKey) return null;
  const special = e.key === "Enter" && (e.shiftKey || e.ctrlKey || e.altKey);
  const letter = e.key.length === 1 && e.shiftKey && (e.ctrlKey || e.altKey);
  if (!special && !letter) return null;
  const code = special ? 13 : e.key.toLowerCase().codePointAt(0)!;
  const modifier = 1 + (e.shiftKey ? 1 : 0) + (e.altKey ? 2 : 0) + (e.ctrlKey ? 4 : 0);
  return `\x1b[27;${modifier};${code}~`;
}
