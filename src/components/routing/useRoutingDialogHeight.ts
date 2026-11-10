import { useCallback, useRef } from "react";

export function useRoutingDialogHeight() {
  const cleanup = useRef<(() => void) | undefined>();
  return useCallback((form: HTMLFormElement | null) => {
    cleanup.current?.();
    cleanup.current = undefined;
    const dialog = form?.closest<HTMLElement>(".rt-create-dialog");
    const body = dialog?.querySelector<HTMLElement>(".vary-dialog-body");
    if (!form || !dialog || !body || typeof ResizeObserver === "undefined")
      return;

    const resize = () => {
      const padding = getComputedStyle(body);
      const header = dialog.querySelector(".vary-dialog-header");
      const footer = dialog.querySelector(".vary-dialog-footer");
      const naturalHeight =
        form.getBoundingClientRect().height +
        (header?.getBoundingClientRect().height ?? 0) +
        (footer?.getBoundingClientRect().height ?? 0) +
        (parseFloat(padding.paddingTop) || 0) +
        (parseFloat(padding.paddingBottom) || 0);
      const maxHeight = parseFloat(getComputedStyle(dialog).maxHeight);
      const target = `${Math.min(Math.ceil(naturalHeight), Number.isFinite(maxHeight) ? maxHeight : naturalHeight)}px`;
      if (dialog.style.height !== target) dialog.style.height = target;
    };
    const observer = new ResizeObserver(resize);
    observer.observe(form);
    window.addEventListener("resize", resize);
    resize();
    cleanup.current = () => {
      observer.disconnect();
      window.removeEventListener("resize", resize);
    };
  }, []);
}
