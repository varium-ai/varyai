import type { ComponentProps } from "react";
import { GripVertical } from "lucide-react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import "./sortable-list.css";

export function SortableWorkspaceRow({
  id,
  sortable = true,
  disabled = false,
  handleLabel,
  handleRole,
  handleHint,
  handleClassName = "",
  children,
  className = "",
  style,
  ...props
}: Omit<ComponentProps<"article">, "id"> & {
  id: string;
  sortable?: boolean;
  disabled?: boolean;
  handleLabel: string;
  handleRole: string;
  handleHint: string;
  handleClassName?: string;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
    isSorting,
  } = useSortable({ id, disabled: !sortable || disabled });

  return (
    <article
      {...props}
      ref={setNodeRef}
      data-sortable={sortable || undefined}
      data-sorting={isSorting || undefined}
      className={`mp-sortable-row ${className}${isDragging ? " dragging" : ""}`}
      style={{
        ...style,
        transform: CSS.Translate.toString(transform),
        transition,
      }}
    >
      {sortable && (
        <button
          {...attributes}
          {...listeners}
          ref={setActivatorNodeRef}
          type="button"
          disabled={disabled}
          className={`mp-sortable-handle ${handleClassName}`}
          aria-label={handleLabel}
          aria-roledescription={handleRole}
          title={handleHint}
          onClick={(event) => event.stopPropagation()}
          onDragStart={(event) => event.preventDefault()}
        >
          <GripVertical size={14} aria-hidden="true" />
        </button>
      )}
      {children}
    </article>
  );
}
