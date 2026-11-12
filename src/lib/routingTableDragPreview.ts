export function createRoutingTableDragPreview(row: HTMLTableRowElement) {
  const table = row.closest("table");
  const viewport = table?.parentElement;
  if (!table || !viewport) return null;

  const box = row.getBoundingClientRect();
  const viewportBox = viewport.getBoundingClientRect();
  const scope = row.closest<HTMLElement>(".vary-routing");
  const computed = getComputedStyle(row);
  const scopeStyle = getComputedStyle(scope ?? row);
  const host = document.createElement("div");
  host.className = "vary-routing rt-table-drag-preview";
  if (row.closest(".dark")) host.classList.add("dark");
  host.setAttribute("data-routing-drag-preview", "");
  host.setAttribute("aria-hidden", "true");
  host.setAttribute("inert", "");
  for (let index = 0; index < computed.length; index++) {
    const property = computed.item(index);
    if (property.startsWith("--"))
      host.style.setProperty(property, computed.getPropertyValue(property));
  }
  const width = viewport.clientWidth || viewportBox.width;
  const scopeWidth = scope
    ? scope.clientWidth -
      (parseFloat(scopeStyle.paddingLeft) || 0) -
      (parseFloat(scopeStyle.paddingRight) || 0)
    : width;
  Object.assign(host.style, {
    left: `${viewportBox.left + viewport.clientLeft}px`,
    top: `${box.top}px`,
    width: `${scopeWidth || width}px`,
    height: `${box.height}px`,
    font: computed.font,
    color: computed.color,
  });

  const clip = document.createElement("div");
  clip.setAttribute("data-routing-drag-viewport", "");
  clip.style.width = `${width}px`;
  clip.style.height = `${box.height}px`;
  const previewTable = table.cloneNode(false) as HTMLTableElement;
  previewTable.removeAttribute("id");
  previewTable.removeAttribute("aria-label");
  previewTable.style.width = `${table.getBoundingClientRect().width}px`;
  previewTable.style.minWidth = previewTable.style.width;
  const columns = document.createElement("colgroup");
  for (const cell of Array.from(row.cells)) {
    const column = document.createElement("col");
    column.style.width = `${cell.getBoundingClientRect().width}px`;
    columns.append(column);
  }
  const previewRow = row.cloneNode(true) as HTMLTableRowElement;
  previewRow.removeAttribute("data-routing-group");
  previewRow.removeAttribute("data-dragging");
  previewRow.removeAttribute("data-drag-target");
  for (const element of [previewRow, ...previewRow.querySelectorAll("*")]) {
    // Preserve SVG paint-server IDs used by provider artwork.
    if (element.namespaceURI !== "http://www.w3.org/2000/svg")
      element.removeAttribute("id");
    for (const attribute of [
      "aria-controls",
      "aria-describedby",
      "aria-labelledby",
      "for",
      "data-testid",
    ])
      element.removeAttribute(attribute);
  }
  for (const cell of Array.from(previewRow.cells)) {
    cell.style.transform = "none";
    cell.style.transition = "none";
    cell.style.height = `${box.height}px`;
  }
  const body = document.createElement("tbody");
  body.append(previewRow);
  previewTable.append(columns, body);
  clip.append(previewTable);
  host.append(clip);
  document.body.append(host);
  const syncScroll = () => {
    clip.scrollLeft = viewport.scrollLeft;
  };
  syncScroll();
  viewport.addEventListener("scroll", syncScroll, { passive: true });

  const visibility = Array.from(row.cells, (cell) => cell.style.visibility);
  row.classList.add("rt-table-drag-placeholder");
  for (const cell of Array.from(row.cells)) cell.style.visibility = "hidden";
  return {
    move(dy: number) {
      host.style.transform = `translateY(${dy}px)`;
    },
    remove() {
      viewport.removeEventListener("scroll", syncScroll);
      host.remove();
      row.classList.remove("rt-table-drag-placeholder");
      Array.from(row.cells).forEach((cell, index) => {
        cell.style.visibility = visibility[index];
      });
    },
  };
}
