import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { GripVertical } from "lucide-react";
import {
  closestCenter,
  DndContext,
  DragOverlay,
  PointerSensor,
  pointerWithin,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  horizontalListSortingStrategy,
  SortableContext,
  useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useTranslation } from "react-i18next";
import {
  APP_ICON_NAME,
  APP_IDS,
  DEFAULT_VISIBLE_APPS,
} from "@/config/appConfig";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { cn } from "@/lib/utils";

const STORAGE_KEY = "cc-switch-managed-app-order";

function readAppOrder(): AppId[] {
  try {
    const saved: unknown = JSON.parse(
      localStorage.getItem(STORAGE_KEY) ?? "null",
    );
    if (Array.isArray(saved)) {
      const knownApps = saved.filter((app): app is AppId =>
        APP_IDS.includes(app),
      );
      return [...new Set([...knownApps, ...APP_IDS])];
    }
  } catch {
    // Missing/unavailable storage and older invalid values use the default order.
  }
  return [...APP_IDS];
}

interface ManagedAppTabsProps {
  activeApp: AppId;
  visibleApps: VisibleApps;
  onAppChange: (app: AppId) => void;
}

function appTabClassName(isSelected: boolean) {
  return cn(
    "relative inline-flex h-9 shrink-0 select-none items-center gap-1 whitespace-nowrap rounded-xl pl-3.5 pr-1 text-sm font-medium",
    isSelected
      ? "border-2 border-blue-600 bg-white font-semibold text-blue-700 shadow-sm shadow-blue-500/10 ring-1 ring-blue-500/20 dark:bg-card dark:text-blue-300"
      : "border border-slate-200/90 bg-white text-slate-700 hover:border-slate-300 hover:bg-slate-50 dark:border-border dark:bg-card dark:text-slate-300 dark:hover:bg-muted",
  );
}

function AppTabContent({ app }: { app: AppId }) {
  const { t } = useTranslation();
  return (
    <>
      <ProviderIcon
        icon={APP_ICON_NAME[app]}
        name={t(`apps.${app}`)}
        size={18}
      />
      <span>{t(`apps.${app}`)}</span>
    </>
  );
}

function SortableAppTab({
  app,
  isSelected,
  onAppChange,
}: {
  app: AppId;
  isSelected: boolean;
  onAppChange: (app: AppId) => void;
}) {
  const { t } = useTranslation();
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: app });
  const buttonRef = useRef<HTMLButtonElement | null>(null);

  useLayoutEffect(() => {
    if (isSelected) {
      buttonRef.current?.scrollIntoView?.({
        behavior: "smooth",
        block: "nearest",
        inline: "nearest",
      });
    }
  }, [isSelected]);

  return (
    <div
      ref={setNodeRef}
      data-app-tab={app}
      onDragStart={(event) => event.preventDefault()}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={cn(appTabClassName(isSelected), isDragging && "opacity-0")}
    >
      <button
        ref={buttonRef}
        type="button"
        aria-current={isSelected ? "page" : undefined}
        aria-label={t(`apps.${app}`)}
        onClick={() => {
          if (!isDragging) onAppChange(app);
        }}
        className="inline-flex h-full min-w-0 items-center gap-2 rounded-lg pr-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
      >
        <AppTabContent app={app} />
      </button>
      <button
        {...attributes}
        {...listeners}
        ref={setActivatorNodeRef}
        type="button"
        aria-label={t("homeAudit.appSorting.handle", { app: t(`apps.${app}`) })}
        aria-roledescription={t("homeAudit.appSorting.role")}
        title={t("homeAudit.appSorting.hint")}
        className="inline-flex h-7 w-5 shrink-0 touch-none cursor-grab items-center justify-center rounded-md text-slate-400 hover:bg-slate-100 hover:text-slate-600 active:cursor-grabbing focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 dark:text-muted-foreground dark:hover:bg-muted dark:hover:text-foreground"
      >
        <GripVertical aria-hidden="true" className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

export function ManagedAppTabs({
  activeApp,
  visibleApps,
  onAppChange,
}: ManagedAppTabsProps) {
  const { t } = useTranslation();
  const [appOrder, setAppOrder] = useState(readAppOrder);
  const [dragging, setDragging] = useState<{
    app: AppId;
    width: number;
  } | null>(null);
  const appIds = appOrder.filter(
    (app) => visibleApps[app] ?? DEFAULT_VISIBLE_APPS[app],
  );
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
  );

  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    setDragging(null);
    if (!over || active.id === over.id) return;
    const from = appOrder.indexOf(active.id as AppId);
    const to = appOrder.indexOf(over.id as AppId);
    if (from < 0 || to < 0) return;
    // Keep hidden apps in the saved list so enabling them restores their position.
    const nextOrder = arrayMove(appOrder, from, to);
    setAppOrder(nextOrder);
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(nextOrder));
    } catch (error) {
      console.warn("Failed to save managed app order", error);
    }
  };

  const announcePosition = (id: string | number, overId: string | number) =>
    t("homeAudit.appSorting.position", {
      app: t(`apps.${id}`),
      position: appIds.indexOf(overId as AppId) + 1,
      count: appIds.length,
    });

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={(args) => {
        const targets = pointerWithin(args);
        return targets.length > 0 ? targets : closestCenter(args);
      }}
      modifiers={[({ transform }) => ({ ...transform, y: 0 })]}
      onDragStart={({ active, activatorEvent }) => {
        const target = activatorEvent.target;
        const source =
          target instanceof Element ? target.closest("[data-app-tab]") : null;
        setDragging({
          app: active.id as AppId,
          width:
            source?.getBoundingClientRect().width ??
            active.rect.current.initial?.width ??
            0,
        });
      }}
      onDragCancel={() => setDragging(null)}
      onDragEnd={handleDragEnd}
      accessibility={{
        screenReaderInstructions: {
          draggable: t("homeAudit.appSorting.hint"),
        },
        announcements: {
          onDragStart: ({ active }) =>
            t("homeAudit.appSorting.pickedUp", { app: t(`apps.${active.id}`) }),
          onDragOver: ({ active, over }) =>
            over ? announcePosition(active.id, over.id) : undefined,
          onDragEnd: ({ active, over }) =>
            over
              ? t("homeAudit.appSorting.dropped", {
                  position: announcePosition(active.id, over.id),
                })
              : t("homeAudit.appSorting.cancelled"),
          onDragCancel: () => t("homeAudit.appSorting.cancelled"),
        },
      }}
    >
      <SortableContext items={appIds} strategy={horizontalListSortingStrategy}>
        <div className="flex min-w-0 flex-1 items-center gap-2 overflow-x-auto py-1">
          {appIds.map((app) => (
            <SortableAppTab
              key={app}
              app={app}
              isSelected={app === activeApp}
              onAppChange={onAppChange}
            />
          ))}
        </div>
      </SortableContext>
      {createPortal(
        <DragOverlay adjustScale={false} dropAnimation={null}>
          {dragging && (
            <div
              aria-hidden="true"
              data-testid="managed-app-drag-preview"
              className={cn(
                appTabClassName(dragging.app === activeApp),
                "pointer-events-none cursor-grabbing shadow-md",
              )}
              style={{ width: dragging.width }}
            >
              <span className="inline-flex h-full min-w-0 items-center gap-2 pr-1">
                <AppTabContent app={dragging.app} />
              </span>
              <span className="inline-flex h-7 w-5 shrink-0 items-center justify-center text-slate-400 dark:text-muted-foreground">
                <GripVertical aria-hidden="true" className="h-3.5 w-3.5" />
              </span>
            </div>
          )}
        </DragOverlay>,
        document.body,
      )}
    </DndContext>
  );
}
