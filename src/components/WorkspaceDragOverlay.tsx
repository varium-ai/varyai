import { useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { DragOverlay, useDndContext } from "@dnd-kit/core";
import "./sortable-list.css";

function RowPreview({ source }: { source: HTMLElement }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!host.current) return;
    // Copy the rendered row without mounting another live set of controls or queries.
    const preview = source.cloneNode(true) as HTMLElement;
    preview.classList.remove("dragging");
    preview.classList.add("mp-sortable-preview");
    preview.setAttribute("aria-hidden", "true");
    preview.setAttribute("inert", "");
    for (const element of [preview, ...preview.querySelectorAll("*")]) {
      for (const attribute of [
        "id",
        "aria-controls",
        "aria-describedby",
        "aria-labelledby",
        "for",
      ])
        element.removeAttribute(attribute);
    }
    preview.style.transform = "none";
    preview.style.transition = "none";
    preview.style.margin = "0";
    preview.style.width = "100%";
    host.current.replaceChildren(preview);
    return () => preview.remove();
  }, [source]);
  const scope = source.classList.contains("pv-row")
    ? "vary-providers"
    : "vary-agents";
  return (
    <div
      ref={host}
      className={`${scope} mp-sortable-overlay-scope`}
      aria-hidden="true"
    />
  );
}

export function WorkspaceDragOverlay() {
  const { activeNode } = useDndContext();
  return createPortal(
    <DragOverlay
      adjustScale={false}
      dropAnimation={null}
      zIndex={150}
      style={{ pointerEvents: "none" }}
    >
      {activeNode && <RowPreview source={activeNode} />}
    </DragOverlay>,
    document.body,
  );
}
