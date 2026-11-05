import { act, renderHook } from "@testing-library/react";
import { expect, it } from "vitest";
import { useRoutingFlights } from "@/components/routing/useRoutingFlights";
import type { RoutingTrace } from "@/lib/api/routingTrace";

const request = (
  id: string,
  state: RoutingTrace["state"] = "streaming",
): RoutingTrace => ({
  id,
  startedAt: Date.now() - 10000,
  agent: "codex",
  model: "model-a",
  groupName: "",
  strategy: "ordered",
  state,
  durationMs: 10000,
  candidates: [],
  steps: [],
});

it("keeps a known response until its flight ends and never replays it on polling", () => {
  const first = request("first");
  const { result, rerender } = renderHook(
    ({ row, concurrent }) => useRoutingFlights(row, concurrent, true),
    { initialProps: { row: first, concurrent: [first] } },
  );
  const next = request("next");
  const completed = { ...first, state: "completed" as const };
  rerender({ row: next, concurrent: [next, completed] });
  expect(result.current.flights.map((row) => row.id)).toEqual([
    "first",
    "next",
  ]);
  act(() => result.current.finish(first.id));
  rerender({ row: { ...next }, concurrent: [{ ...completed }, { ...next }] });
  expect(result.current.flights.map((row) => row.id)).toEqual(["next"]);
});

it("does not add old completed destinations on first load but accepts new fast completions", () => {
  const latest = request("latest", "completed");
  const old = request("old", "completed");
  const { result, rerender } = renderHook(
    ({ row, concurrent }) => useRoutingFlights(row, concurrent, true),
    { initialProps: { row: latest, concurrent: [latest, old] } },
  );
  expect(result.current.flights.map((row) => row.id)).toEqual(["latest"]);
  const fast = request("new-fast", "completed");
  rerender({ row: latest, concurrent: [latest, fast, old] });
  expect(result.current.flights.map((row) => row.id)).toEqual([
    "latest",
    "new-fast",
  ]);
});

it("clears stale and filtered flights and limits concurrent Agents", () => {
  const rows = Array.from({ length: 7 }, (_, index) => ({
    ...request(`request-${index}`),
    startedAt: Date.now() + index,
    agent: `agent-${index}`,
  }));
  const { result, rerender } = renderHook(
    ({ concurrent, enabled }) =>
      useRoutingFlights(undefined, concurrent, enabled),
    { initialProps: { concurrent: rows, enabled: true } },
  );
  expect(result.current.flights).toHaveLength(4);
  rerender({ concurrent: [rows[0]], enabled: true });
  expect(result.current.flights.map((row) => row.id)).toEqual([rows[0].id]);
  rerender({ concurrent: rows, enabled: false });
  expect(result.current.flights).toHaveLength(0);
});
