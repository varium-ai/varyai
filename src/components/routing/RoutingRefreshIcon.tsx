import { LoaderCircle, RefreshCw } from "lucide-react";

export function RoutingRefreshIcon({ refreshing }: { refreshing: boolean }) {
  return (
    <span
      aria-hidden="true"
      className={`rt-refresh-icon ${
        refreshing ? "animate-spin motion-reduce:animate-none" : ""
      }`}
    >
      {refreshing ? (
        <LoaderCircle size={12} strokeWidth={1.75}>
          <circle cx="12" cy="12" r="9" opacity="0.2" />
        </LoaderCircle>
      ) : (
        <RefreshCw size={12} strokeWidth={1.75} />
      )}
    </span>
  );
}
