import { render, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { useSecurityEventCount } from "@/hooks/useSecurityAudit";

const { getEventCount } = vi.hoisted(() => ({ getEventCount: vi.fn().mockResolvedValue(3) }));
vi.mock("@/lib/api", () => ({ securityApi: { getEventCount } }));

function Probe() {
  useSecurityEventCount("codex");
  return null;
}

describe("useSecurityEventCount", () => {
  it("queries today's audit rows using the millisecond timestamps stored in SQLite", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(<QueryClientProvider client={client}><Probe /></QueryClientProvider>);
    await waitFor(() => expect(getEventCount).toHaveBeenCalled());
    const [app, start, end] = getEventCount.mock.lastCall ?? [];
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    expect(app).toBe("codex");
    expect(start).toBe(today.getTime());
    expect(end).toBeGreaterThanOrEqual(start);
    expect(end).toBeLessThanOrEqual(Date.now());
  });
});
