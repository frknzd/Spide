import { useEffect, useRef } from "react";

/** Nonmodal overlay behaviour: Escape closes only the newest open overlay, then focus returns to its opener. */
const stack: symbol[] = [];
export function useOverlay(onClose: () => void, { restoreFocus = true } = {}) {
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const id = Symbol("overlay");
    const opener = document.activeElement as HTMLElement | null;
    stack.push(id);
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || stack.at(-1) !== id || (e.target as HTMLElement)?.closest?.("[data-terminal]")) return;
      e.preventDefault();
      close.current();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      stack.splice(stack.indexOf(id), 1);
      if (restoreFocus && opener?.isConnected && !opener.closest("[data-terminal]")) opener.focus();
    };
  }, [restoreFocus]);
}
