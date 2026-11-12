import {
  useRef,
  type ComponentPropsWithoutRef,
  type ReactNode,
  type RefObject,
} from "react";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./dialog";
import { cn } from "@/lib/utils";

type ContentProps = ComponentPropsWithoutRef<typeof DialogContent>;

export interface AppDialogProps
  extends Omit<ContentProps, "title" | "children" | "variant"> {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg";
  pending?: boolean;
  initialFocusRef?: RefObject<HTMLElement>;
}

export function AppDialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  size = "sm",
  pending = false,
  initialFocusRef,
  className,
  onOpenAutoFocus,
  onCloseAutoFocus,
  onEscapeKeyDown,
  onInteractOutside,
  ...props
}: AppDialogProps) {
  const content = useRef<HTMLDivElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  const snapshot = useRef({ title, description, children, footer });
  const displayed = open
    ? { title, description, children, footer }
    : snapshot.current;
  if (open) snapshot.current = displayed;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!pending) onOpenChange(next);
      }}
    >
      <DialogContent
        {...props}
        ref={content}
        className={cn("vary-app-dialog", className)}
        style={{
          maxWidth: { sm: 420, md: 520, lg: 680 }[size],
          ...props.style,
        }}
        aria-busy={pending || undefined}
        {...(!displayed.description && {
          "aria-describedby": props["aria-describedby"],
        })}
        onOpenAutoFocus={(event) => {
          returnFocus.current =
            document.activeElement instanceof HTMLElement
              ? document.activeElement
              : null;
          onOpenAutoFocus?.(event);
          if (!event.defaultPrevented) {
            event.preventDefault();
            (initialFocusRef?.current ?? content.current)?.focus({
              preventScroll: true,
            });
          }
        }}
        onCloseAutoFocus={(event) => {
          onCloseAutoFocus?.(event);
          if (!event.defaultPrevented && returnFocus.current?.isConnected) {
            event.preventDefault();
            returnFocus.current.focus({ preventScroll: true });
          }
        }}
        onEscapeKeyDown={(event) => {
          onEscapeKeyDown?.(event);
          if (pending) event.preventDefault();
        }}
        onInteractOutside={(event) => {
          onInteractOutside?.(event);
          if (pending) event.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle>{displayed.title}</DialogTitle>
          {displayed.description && (
            <DialogDescription>{displayed.description}</DialogDescription>
          )}
        </DialogHeader>
        {displayed.children && <DialogBody>{displayed.children}</DialogBody>}
        {displayed.footer && <DialogFooter>{displayed.footer}</DialogFooter>}
      </DialogContent>
    </Dialog>
  );
}
