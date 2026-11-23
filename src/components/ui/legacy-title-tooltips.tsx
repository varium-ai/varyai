import { useEffect, useRef, useState } from "react";
import { Popover, PopoverAnchor, PopoverContent } from "./popover";

const HINT_ID = "vary-legacy-title-hint";

/** Keeps legacy title copy while rendering its hint with the app's floating surface. */
export function LegacyTitleTooltips() {
  const [hint, setHint] = useState<string | null>(null);
  const anchor = useRef<HTMLElement | null>(null);
  const hideHint = useRef<() => void>(() => {});

  useEffect(() => {
    let active: {
      owner: HTMLElement;
      text: string;
      addedName?: string;
    } | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let observer: MutationObserver | undefined;
    function removeDescription() {
      if (!active) return;
      const ids = (active.owner.getAttribute("aria-describedby") ?? "")
        .split(/\s+/)
        .filter((id) => id && id !== HINT_ID);
      if (ids.length)
        active.owner.setAttribute("aria-describedby", ids.join(" "));
      else active.owner.removeAttribute("aria-describedby");
    }
    function hide() {
      clearTimeout(timer);
      setHint(null);
      removeDescription();
    }
    function release(restoreTitle = true) {
      hide();
      observer?.disconnect();
      observer = undefined;
      if (active) {
        if (
          restoreTitle &&
          active.owner.isConnected &&
          !active.owner.hasAttribute("title")
        )
          active.owner.setAttribute("title", active.text);
        if (
          active.addedName &&
          active.owner.getAttribute("aria-label") === active.addedName
        )
          active.owner.removeAttribute("aria-label");
      }
      active = null;
      anchor.current = null;
    }
    function show() {
      if (!active || !active.owner.isConnected) return;
      const ids = (active.owner.getAttribute("aria-describedby") ?? "")
        .split(/\s+/)
        .filter(Boolean);
      if (!ids.includes(HINT_ID)) ids.push(HINT_ID);
      active.owner.setAttribute("aria-describedby", ids.join(" "));
      setHint(active.text);
    }
    function activate(owner: HTMLElement, immediate: boolean) {
      if (active?.owner === owner) return;
      const text = owner.getAttribute("title");
      if (!text?.trim()) return;
      release();
      active = { owner, text };
      anchor.current = owner;
      owner.removeAttribute("title");
      const hasOtherName =
        owner.hasAttribute("aria-label") ||
        owner.hasAttribute("aria-labelledby") ||
        owner.textContent?.trim() ||
        owner.querySelector("img[alt]") ||
        ("labels" in owner && (owner as HTMLInputElement).labels?.length) ||
        (owner instanceof HTMLInputElement &&
          ["button", "submit", "reset"].includes(owner.type) &&
          owner.value);
      if (
        !hasOtherName &&
        owner.matches("button,a,input,textarea,[role=button]")
      ) {
        owner.setAttribute("aria-label", text);
        active.addedName = text;
      }
      observer = new MutationObserver(() => {
        if (!active) return;
        const latest = owner.getAttribute("title");
        if (!latest?.trim()) {
          release(false);
          return;
        }
        observer?.disconnect();
        if (
          active.addedName &&
          owner.getAttribute("aria-label") === active.addedName
        ) {
          owner.setAttribute("aria-label", latest);
          active.addedName = latest;
        }
        active.text = latest;
        owner.removeAttribute("title");
        observer?.observe(owner, {
          attributes: true,
          attributeFilter: ["title"],
        });
        setHint((current) => (current === null ? null : latest));
      });
      observer.observe(owner, { attributes: true, attributeFilter: ["title"] });
      timer = setTimeout(show, immediate ? 0 : 350);
    }
    function enter(event: Event) {
      if (!(event.target instanceof Element)) return;
      const owner = event.target.closest<HTMLElement>("[title]");
      if (owner) activate(owner, event.type === "focusin");
    }
    function leave(event: Event) {
      if (!active) return;
      const next = (event as FocusEvent).relatedTarget;
      if (!(next instanceof Node) || !active.owner.contains(next)) release();
    }
    function keydown(event: KeyboardEvent) {
      if (event.key === "Escape") hide();
    }
    hideHint.current = hide;
    document.addEventListener("pointerover", enter, true);
    document.addEventListener("pointerout", leave, true);
    document.addEventListener("focusin", enter, true);
    document.addEventListener("focusout", leave, true);
    document.addEventListener("pointerdown", hide, true);
    document.addEventListener("keydown", keydown, true);
    document.addEventListener("scroll", hide, true);
    window.addEventListener("resize", hide);
    return () => {
      document.removeEventListener("pointerover", enter, true);
      document.removeEventListener("pointerout", leave, true);
      document.removeEventListener("focusin", enter, true);
      document.removeEventListener("focusout", leave, true);
      document.removeEventListener("pointerdown", hide, true);
      document.removeEventListener("keydown", keydown, true);
      document.removeEventListener("scroll", hide, true);
      window.removeEventListener("resize", hide);
      release();
    };
  }, []);

  return (
    <Popover
      open={hint !== null}
      onOpenChange={(open) => {
        if (!open) hideHint.current();
      }}
    >
      <PopoverAnchor virtualRef={anchor} />
      {hint !== null && (
        <PopoverContent
          id={HINT_ID}
          role="tooltip"
          sideOffset={6}
          className="pointer-events-none z-[160] max-w-[min(28rem,calc(100vw-2rem))] whitespace-pre-wrap break-words px-3 py-1.5 text-xs"
          onOpenAutoFocus={(event) => event.preventDefault()}
          onCloseAutoFocus={(event) => event.preventDefault()}
        >
          {hint}
        </PopoverContent>
      )}
    </Popover>
  );
}
