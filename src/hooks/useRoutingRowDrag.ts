import { useEffect, useRef, useState, type PointerEvent } from "react";
import { flushSync } from "react-dom";
import { createRoutingTableDragPreview } from "@/lib/routingTableDragPreview";

// The reference implementation's transform-only row sorter, adapted for nested React slots (MIT).
// See third_party/routing-reference/LICENSE. Table previews preserve sticky column layout.
export function useRoutingRowDrag(
  attribute:
    | "data-routing-group"
    | "data-routing-member"
    | "data-routing-rule"
    | "data-agent-model"
    | "data-codex-account",
  onDrop: (from: string, to: string) => void,
) {
  const [drag, setDrag] = useState<{ from: string; to: string } | null>(null);
  const stop = useRef<(() => void) | null>(null);
  const settling = useRef<Animation[]>([]);
  const suppress = useRef<string | null>(null);
  function stopSettling() {
    for (const animation of settling.current) {
      animation.onfinish = null;
      animation.cancel();
    }
    settling.current = [];
  }
  useEffect(
    () => () => {
      stop.current?.();
      stopSettling();
    },
    [],
  );
  function start(event: PointerEvent<HTMLElement>, id: string) {
    if (event.button !== 0 || event.isPrimary === false) return;
    stop.current?.();
    stopSettling();
    suppress.current = null;
    const handle = event.currentTarget;
    const row = handle.closest(`[${attribute}]`);
    const list = row?.closest("[data-routing-list]");
    if (!row || !list) return;
    const rows = Array.from(
      list.querySelectorAll<HTMLElement>(`[${attribute}]`),
    ).filter((candidate) => candidate.closest("[data-routing-list]") === list);
    const from = rows.indexOf(row as HTMLElement);
    if (from < 0 || rows.length < 2) return;
    const boxes = rows.map((candidate) => candidate.getBoundingClientRect());
    const slots = rows.map((candidate) => {
      let slot: HTMLElement = candidate;
      while (slot.parentElement && slot.parentElement !== list)
        slot = slot.parentElement;
      return slot;
    });
    let scroll = list.parentElement;
    while (
      scroll &&
      (!/(auto|scroll)/.test(getComputedStyle(scroll).overflowY) ||
        scroll.scrollHeight <= scroll.clientHeight)
    )
      scroll = scroll.parentElement;
    scroll ??= document.scrollingElement as HTMLElement | null;
    const scrollBox = scroll?.getBoundingClientRect();
    const scroll0 = scroll?.scrollTop ?? 0;
    const y0 = event.clientY;
    let y = y0;
    const pointer = event.pointerId;
    let moving = false;
    let ended = false;
    let to = from;
    let frame = 0;
    let lastTime = 0;
    let previewTop = boxes[from].top;
    let tablePreview: ReturnType<typeof createRoutingTableDragPreview> = null;
    const styles = slots.map((slot) => slot.style.transform);
    const cells = rows.map((candidate) =>
      candidate instanceof HTMLTableRowElement
        ? Array.from(candidate.cells)
        : [],
    );
    const cellStyles = cells.map((rowCells) =>
      rowCells.map((cell) => cell.style.transform),
    );
    function paint(time: number) {
      frame = 0;
      if (ended) return;
      if (!row?.isConnected || !handle.isConnected) return cancel();
      if (!moving) return;
      const dt = Math.min(32, lastTime ? time - lastTime : 16);
      lastTime = time;
      const edge = 36;
      const speed = !scrollBox
        ? 0
        : y < scrollBox.top + edge
          ? -Math.min(1, (scrollBox.top + edge - y) / edge)
          : y > scrollBox.bottom - edge
            ? Math.min(1, (y - scrollBox.bottom + edge) / edge)
            : 0;
      if (scroll && speed) scroll.scrollTop += speed * dt * 0.6;
      const height = boxes[from].height;
      const lo = boxes[0].top - boxes[from].top;
      const last = boxes[boxes.length - 1];
      const hi = last.bottom - height - boxes[from].top;
      const dy = Math.max(
        lo,
        Math.min(hi, y - y0 + (scroll?.scrollTop ?? 0) - scroll0),
      );
      const mid = boxes[from].top + dy + height / 2;
      previewTop = boxes[from].top + dy - ((scroll?.scrollTop ?? 0) - scroll0);
      to = from;
      if (dy > 0)
        while (
          to < rows.length - 1 &&
          mid >= boxes[to + 1].top + boxes[to + 1].height / 2
        )
          to++;
      else
        while (to > 0 && mid <= boxes[to - 1].top + boxes[to - 1].height / 2)
          to--;
      if (dy <= lo && from > 0) to = 0;
      else if (dy >= hi && from < rows.length - 1) to = rows.length - 1;
      slots.forEach((slot, index) => {
        const shift =
          index === from
            ? dy
            : index > from && index <= to
              ? -height
              : index < from && index >= to
                ? height
                : 0;
        if (tablePreview) {
          if (index === from)
            tablePreview.move(dy - ((scroll?.scrollTop ?? 0) - scroll0));
          else
            cells[index].forEach((cell, cellIndex) => {
              cell.style.transform = shift
                ? `translateY(${shift}px)`
                : cellStyles[index][cellIndex];
            });
        } else
          slot.style.transform = shift
            ? `translateY(${shift}px)`
            : styles[index];
      });
      setDrag((previous) =>
        previous?.from === id &&
        previous.to === rows[to].getAttribute(attribute)
          ? previous
          : { from: id, to: rows[to].getAttribute(attribute) ?? id },
      );
      if (speed) frame = requestAnimationFrame(paint);
    }
    function cleanup() {
      ended = true;
      cancelAnimationFrame(frame);
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", finish);
      document.removeEventListener("pointercancel", cancel);
      handle.removeEventListener("lostpointercapture", cancel);
      window.removeEventListener("blur", cancel);
      window.removeEventListener("resize", cancel);
      document.removeEventListener("keydown", key, true);
      list?.classList.remove("rt-sorting");
      slots[from].classList.remove("rt-drag-lift");
      slots.forEach((slot, index) => {
        slot.style.transform = styles[index];
        cells[index].forEach((cell, cellIndex) => {
          cell.style.transform = cellStyles[index][cellIndex];
        });
      });
      tablePreview?.remove();
      if (handle.hasPointerCapture?.(pointer))
        handle.releasePointerCapture(pointer);
      setDrag(null);
      stop.current = null;
    }
    function move(next: globalThis.PointerEvent) {
      if (next.pointerId !== pointer) return;
      y = next.clientY;
      if (!moving && Math.abs(y - y0) < 4) return;
      if (!moving && row instanceof HTMLTableRowElement)
        tablePreview = createRoutingTableDragPreview(row);
      moving = true;
      suppress.current = id;
      next.preventDefault();
      list?.classList.add("rt-sorting");
      if (!tablePreview) slots[from].classList.add("rt-drag-lift");
      handle.setPointerCapture?.(pointer);
      if (!frame) frame = requestAnimationFrame(paint);
    }
    function finish(next: globalThis.PointerEvent) {
      if (next.pointerId !== pointer) return;
      y = next.clientY;
      cancelAnimationFrame(frame);
      paint(performance.now());
      if (ended) return;
      const target = rows[to].getAttribute(attribute) ?? id;
      const dropping = moving && target !== id;
      const animateDrop =
        dropping &&
        tablePreview &&
        !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
      // Read the visible positions before removing transforms, then commit the
      // new order before measuring destinations. The floating row never returns
      // to its old slot while persistence is pending.
      const previousTops = animateDrop
        ? cells.map((rowCells, index) =>
            rowCells.map((cell) =>
              index === from ? previewTop : cell.getBoundingClientRect().top,
            ),
          )
        : [];
      cleanup();
      if (!dropping) return;
      flushSync(() => onDrop(id, target));
      if (!animateDrop) return;
      const destinations = cells.map((rowCells) =>
        rowCells.map((cell) => cell.getBoundingClientRect().top),
      );
      cells.forEach((rowCells, index) =>
        rowCells.forEach((cell, cellIndex) => {
          const delta =
            previousTops[index][cellIndex] - destinations[index][cellIndex];
          if (
            !cell.isConnected ||
            typeof cell.animate !== "function" ||
            Math.abs(delta) < 0.5
          )
            return;
          const transform = cellStyles[index][cellIndex] || "none";
          const animation = cell.animate(
            [{ transform: `translateY(${delta}px)` }, { transform }],
            { duration: 180, easing: "cubic-bezier(0.22, 1, 0.36, 1)" },
          );
          settling.current.push(animation);
          animation.onfinish = () => {
            animation.cancel();
            settling.current = settling.current.filter(
              (entry) => entry !== animation,
            );
          };
        }),
      );
    }
    function cancel(next?: Event) {
      if (next && "pointerId" in next && next.pointerId !== pointer) return;
      if (ended) return;
      cleanup();
    }
    function key(next: KeyboardEvent) {
      if (next.key !== "Escape") return;
      next.preventDefault();
      next.stopImmediatePropagation();
      cancel();
    }
    document.addEventListener("pointermove", move, { passive: false });
    document.addEventListener("pointerup", finish);
    document.addEventListener("pointercancel", cancel);
    handle.addEventListener("lostpointercapture", cancel);
    window.addEventListener("blur", cancel);
    window.addEventListener("resize", cancel);
    document.addEventListener("keydown", key, true);
    stop.current = cleanup;
  }
  function consumeClick(id: string) {
    const skip = suppress.current === id;
    suppress.current = null;
    return skip;
  }
  return { drag, start, consumeClick };
}
