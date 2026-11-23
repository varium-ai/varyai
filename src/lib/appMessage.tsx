import { createRoot } from "react-dom/client";
import { ThemeProvider } from "@/components/theme-provider";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface AppMessageOptions {
  title: string;
  description: string;
  confirmLabel: string;
}
/** App-owned acknowledgement surface, usable before the normal app has mounted. */
export function showAppMessage({
  title,
  description,
  confirmLabel,
}: AppMessageOptions): Promise<void> {
  return new Promise((resolve) => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    function acknowledge() {
      root.unmount();
      host.remove();
      resolve();
    }
    root.render(
      <ThemeProvider>
        <Dialog open>
          <DialogContent
            zIndex="top"
            onEscapeKeyDown={(event) => event.preventDefault()}
            onInteractOutside={(event) => event.preventDefault()}
          >
            <DialogHeader>
              <DialogTitle>{title}</DialogTitle>
            </DialogHeader>
            <DialogDescription className="overflow-y-auto whitespace-pre-wrap break-words px-5 pb-4">
              {description}
            </DialogDescription>
            <DialogFooter>
              <Button type="button" onClick={acknowledge}>
                {confirmLabel}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </ThemeProvider>,
    );
  });
}
