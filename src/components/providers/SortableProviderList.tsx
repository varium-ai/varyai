import type { ReactNode } from "react";
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { useTranslation } from "react-i18next";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import { restrictWorkspaceSort } from "@/lib/workspaceSorting";
import { WorkspaceDragOverlay } from "@/components/WorkspaceDragOverlay";

export function SortableProviderList({
  records,
  onReorder,
  children,
  ...props
}: {
  records: RoutingUpstream[];
  onReorder: (from: string, to: string) => void;
  children: ReactNode;
  id?: string;
  hidden?: boolean;
}) {
  const { t } = useTranslation();
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );
  const position = (id: string | number) =>
    t("modelProviders.sorting.position", {
      provider: records.find((record) => record.id === id)?.name ?? "",
      position: records.findIndex((record) => record.id === id) + 1,
      count: records.length,
    });
  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      modifiers={[restrictWorkspaceSort]}
      onDragEnd={({ active, over }) => {
        if (over && active.id !== over.id)
          onReorder(String(active.id), String(over.id));
      }}
      accessibility={{
        screenReaderInstructions: {
          draggable: t("modelProviders.sorting.hint"),
        },
        announcements: {
          onDragStart: ({ active }) => position(active.id),
          onDragOver: ({ over }) => (over ? position(over.id) : undefined),
          onDragEnd: ({ over }) =>
            over ? position(over.id) : t("agentConnections.sorting.cancelled"),
          onDragCancel: () => t("agentConnections.sorting.cancelled"),
        },
      }}
    >
      <SortableContext
        items={records.map((record) => record.id)}
        strategy={verticalListSortingStrategy}
      >
        <div {...props} className="mp-list pv-list">
          {children}
        </div>
      </SortableContext>
      <WorkspaceDragOverlay />
    </DndContext>
  );
}
