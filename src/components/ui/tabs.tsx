import * as React from "react";
import * as TabsPrimitive from "@radix-ui/react-tabs";
import { cn } from "@/lib/utils";

const Tabs = TabsPrimitive.Root;

type TabsListProps = React.ComponentPropsWithoutRef<
  typeof TabsPrimitive.List
> & { slidingIndicator?: boolean };

const TabsList = React.forwardRef<
  React.ElementRef<typeof TabsPrimitive.List>,
  TabsListProps
>(({ className, children, slidingIndicator = true, ...props }, ref) => {
  const listRef = React.useRef<React.ElementRef<
    typeof TabsPrimitive.List
  > | null>(null);
  const indicatorRef = React.useRef<HTMLSpanElement>(null);
  const setListRef = React.useCallback(
    (node: React.ElementRef<typeof TabsPrimitive.List> | null) => {
      listRef.current = node;
      if (typeof ref === "function") ref(node);
      else if (ref) Object.assign(ref, { current: node });
    },
    [ref],
  );

  React.useLayoutEffect(() => {
    const list = listRef.current;
    const indicator = indicatorRef.current;
    if (!list || !indicator) return;

    const alignIndicator = () => {
      const activeTab = Array.from(list.children).find(
        (child) =>
          child.getAttribute("role") === "tab" &&
          child.getAttribute("data-state") === "active",
      );
      if (!(activeTab instanceof HTMLElement)) {
        indicator.style.visibility = "hidden";
        return;
      }

      const listRect = list.getBoundingClientRect();
      const activeRect = activeTab.getBoundingClientRect();
      indicator.style.width = `${activeRect.width}px`;
      indicator.style.height = `${activeRect.height}px`;
      indicator.style.transform = `translate(${activeRect.left - listRect.left - list.clientLeft}px, ${activeRect.top - listRect.top - list.clientTop}px)`;
      indicator.style.visibility =
        activeRect.width > 0 && activeRect.height > 0 ? "visible" : "hidden";
    };

    alignIndicator();

    const mutationObserver = new MutationObserver(alignIndicator);
    mutationObserver.observe(list, {
      attributes: true,
      attributeFilter: ["data-state"],
      childList: true,
      subtree: true,
    });

    const resizeObserver =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(alignIndicator);
    resizeObserver?.observe(list);
    for (const child of Array.from(list.children)) {
      if (child.getAttribute("role") === "tab") resizeObserver?.observe(child);
    }
    window.addEventListener("resize", alignIndicator);

    return () => {
      mutationObserver.disconnect();
      resizeObserver?.disconnect();
      window.removeEventListener("resize", alignIndicator);
    };
  }, [slidingIndicator]);

  return (
    <TabsPrimitive.List
      ref={setListRef}
      className={cn(
        "relative isolate inline-flex items-center justify-center gap-1 rounded-md bg-muted p-1 text-muted-foreground",
        className,
      )}
      {...props}
    >
      {slidingIndicator !== false && (
        <span
          ref={indicatorRef}
          aria-hidden="true"
          data-testid="tabs-list-indicator"
          className="pointer-events-none absolute left-0 top-0 z-0 rounded-md bg-blue-500 shadow-sm transition-[transform,width] duration-300 ease-out motion-reduce:transition-none dark:bg-blue-600"
          style={{ visibility: "hidden" }}
        />
      )}
      {children}
    </TabsPrimitive.List>
  );
});
TabsList.displayName = TabsPrimitive.List.displayName;

const TabsTrigger = React.forwardRef<
  React.ElementRef<typeof TabsPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>
>(({ className, ...props }, ref) => (
  <TabsPrimitive.Trigger
    ref={ref}
    className={cn(
      "relative z-10 inline-flex min-w-[120px] items-center justify-center whitespace-nowrap rounded-md px-3 py-1.5 text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 data-[state=active]:bg-transparent data-[state=active]:text-white data-[state=active]:shadow-none data-[state=inactive]:opacity-60 data-[state=inactive]:hover:opacity-100 data-[state=inactive]:hover:bg-muted/50",
      className,
    )}
    {...props}
  />
));
TabsTrigger.displayName = TabsPrimitive.Trigger.displayName;

const TabsContent = React.forwardRef<
  React.ElementRef<typeof TabsPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.Content>
>(({ className, ...props }, ref) => (
  <TabsPrimitive.Content
    ref={ref}
    className={cn(
      "mt-2 ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
      className,
    )}
    {...props}
  />
));
TabsContent.displayName = TabsPrimitive.Content.displayName;

export { Tabs, TabsList, TabsTrigger, TabsContent };
