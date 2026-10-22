import { describe, expect, it } from "vitest";
import type { ToolRiskEventRecord } from "@/types/toolRisk";
import {
  groupToolRiskEvents,
  type ToolRiskEventGroup,
} from "./toolRiskEventGroups";

const BUCKET_START = 1_700_000_040_000;
const ELIGIBLE_VERDICTS = ["uncertain", "low_risk"] as const;
const VERDICTS = ["uncertain", "low_risk", "risky", "unchecked"] as const;

function event(
  overrides: Partial<ToolRiskEventRecord> = {},
): ToolRiskEventRecord {
  return {
    id: 1,
    timestamp: BUCKET_START + 30_000,
    source: "proxy",
    stage: "model_response",
    clientCategory: "codex",
    operationCategory: "shell_exec",
    commandSummary: "ls -la /workspace/project",
    commandClass: "read_only_query",
    detectionScope: "in_scope",
    integrityStatus: "complete",
    verdict: "uncertain",
    riskCategory: "sensitive_read",
    severity: "none",
    reasonCode: "no_rule_match",
    policyVersion: 2,
    ruleIds: ["rule-b", "rule-a"],
    suppressedCount: 0,
    ...overrides,
  };
}

function freezePage(
  events: ToolRiskEventRecord[],
): readonly ToolRiskEventRecord[] {
  for (const record of events) {
    Object.freeze(record.ruleIds);
    Object.freeze(record);
  }
  return Object.freeze(events);
}

function expectSingletons(events: readonly ToolRiskEventRecord[]) {
  const groups: ToolRiskEventGroup[] = groupToolRiskEvents(events);
  expect(groups).toHaveLength(events.length);
  groups.forEach((group, index) => {
    expect(group.representative).toBe(events[index]);
    expect(group.members).toHaveLength(1);
    expect(group.members[0]).toBe(events[index]);
  });
}

const KEY_DIFFERENCES: [string, Partial<ToolRiskEventRecord>][] = [
  ["reasonCode", { reasonCode: "manual_review" }],
  ["policyVersion", { policyVersion: 3 }],
  ["ruleIds", { ruleIds: ["rule-a", "rule-c"] }],
  ["source", { source: "sandbox" }],
  ["stage", { stage: "request" }],
  ["clientCategory", { clientCategory: "claude" }],
  ["operationCategory", { operationCategory: "process_exec" }],
  ["integrityStatus", { integrityStatus: "incomplete" }],
  ["riskCategory", { riskCategory: "data_exfiltration" }],
  ["severity", { severity: "medium" }],
  ["commandClass", { commandClass: "unknown" }],
  ["detectionScope", { detectionScope: "undetermined" }],
];

const OPTIONAL_KEY_FIELDS = [
  "reasonCode",
  "clientCategory",
  "operationCategory",
  "riskCategory",
  "severity",
  "commandClass",
] as const;

const INVALID_TIMESTAMPS: [string, unknown][] = [
  ["NaN", NaN],
  ["positive infinity", Infinity],
  ["negative infinity", -Infinity],
  ["fractional milliseconds", BUCKET_START + 0.5],
  ["negative fractional milliseconds", -0.5],
  ["unsafe positive integer", Number.MAX_SAFE_INTEGER + 1],
  ["unsafe negative integer", Number.MIN_SAFE_INTEGER - 1],
  ["safe integer outside Date range", Number.MAX_SAFE_INTEGER],
  ["negative safe integer outside Date range", Number.MIN_SAFE_INTEGER],
  ["one millisecond above Date maximum", 8_640_000_000_000_001],
  ["one millisecond below Date minimum", -8_640_000_000_000_001],
  ["missing runtime timestamp", undefined],
  ["null runtime timestamp", null],
  ["numeric string runtime timestamp", String(BUCKET_START)],
];

describe("groupToolRiskEvents", () => {
  it("returns an empty result for an empty readonly page", () => {
    expect(groupToolRiskEvents(Object.freeze([]))).toEqual([]);
  });

  it("keeps a singleton as its own representative and member", () => {
    expectSingletons(freezePage([event()]));
  });

  it.each(ELIGIBLE_VERDICTS)(
    "groups ten %s records with every original ID, reference and page order",
    (verdict) => {
      const ids = [80, 13, 5, 42, 9, 60, 2, 77, 16, 11];
      const events = freezePage(
        ids.map((id, index) =>
          event({
            id,
            verdict,
            timestamp: BUCKET_START + 30_000 - index * 1_000,
            suppressedCount: index * 7,
          }),
        ),
      );
      const groups = groupToolRiskEvents(events);

      expect(groups).toHaveLength(1);
      expect(groups[0].representative).toBe(events[0]);
      expect(groups[0].members).toHaveLength(10);
      expect(groups[0].members.map((member) => member.id)).toEqual(ids);
      groups[0].members.forEach((member, index) => {
        expect(member).toBe(events[index]);
        expect(member.suppressedCount).toBe(index * 7);
      });
    },
  );

  it.each(["risky", "unchecked", null, undefined, "unknown", ""])(
    "never groups ten records with verdict %s",
    (verdict) => {
      expectSingletons(
        freezePage(
          Array.from({ length: 10 }, (_, index) =>
            event({ id: index + 1, verdict }),
          ),
        ),
      );
    },
  );

  it("does not merge uncertain and low_risk verdicts", () => {
    expectSingletons([
      event({ id: 1, verdict: "uncertain" }),
      event({ id: 2, verdict: "low_risk" }),
    ]);
  });

  describe.each(VERDICTS)("%s key boundaries", (verdict) => {
    it.each(KEY_DIFFERENCES)("does not merge different %s", (_key, change) => {
      expectSingletons([
        event({ id: 1, verdict }),
        event({ id: 2, verdict, ...change }),
      ]);
    });
  });

  describe.each(ELIGIBLE_VERDICTS)("%s optional key boundaries", (verdict) => {
    it.each(OPTIONAL_KEY_FIELDS)(
      "distinguishes present, null and undefined %s",
      (field) => {
        expectSingletons([
          event({ id: 1, verdict }),
          event({ id: 2, verdict, [field]: null }),
          event({ id: 3, verdict, [field]: undefined }),
        ]);
      },
    );

    it.each(OPTIONAL_KEY_FIELDS)(
      "can group identical null or undefined %s without replacing it",
      (field) => {
        for (const value of [null, undefined]) {
          const events = freezePage([
            event({ id: 1, verdict, [field]: value }),
            event({ id: 2, verdict, [field]: value }),
          ]);
          const groups = groupToolRiskEvents(events);
          expect(groups).toHaveLength(1);
          expect(groups[0].members).toEqual(events);
          expect(groups[0].members[0][field]).toBe(value);
          expect(groups[0].members[1][field]).toBe(value);
        }
      },
    );
  });

  it("distinguishes an absent optional property from null without normalizing input", () => {
    const missing = event({ id: 1 });
    delete missing.reasonCode;
    const events = freezePage([missing, event({ id: 2, reasonCode: null })]);

    expectSingletons(events);
    expect(Object.prototype.hasOwnProperty.call(events[0], "reasonCode")).toBe(
      false,
    );
    expect(events[1].reasonCode).toBeNull();
  });

  it.each(ELIGIBLE_VERDICTS)(
    "compares the full sorted rule multiset for %s without sorting the input",
    (verdict) => {
      const events = freezePage([
        event({ id: 1, verdict, ruleIds: ["rule-c", "rule-a", "rule-b"] }),
        event({ id: 2, verdict, ruleIds: ["rule-b", "rule-c", "rule-a"] }),
      ]);
      const groups = groupToolRiskEvents(events);

      expect(groups).toHaveLength(1);
      expect(groups[0].members[0]).toBe(events[0]);
      expect(groups[0].members[1]).toBe(events[1]);
      expect(events[0].ruleIds).toEqual(["rule-c", "rule-a", "rule-b"]);
      expect(events[1].ruleIds).toEqual(["rule-b", "rule-c", "rule-a"]);
    },
  );

  it("preserves duplicate rule IDs rather than deduplicating the key", () => {
    const events = freezePage([
      event({ id: 1, ruleIds: ["rule-a", "rule-a", "rule-b"] }),
      event({ id: 2, ruleIds: ["rule-b", "rule-a"] }),
      event({ id: 3, ruleIds: ["rule-b", "rule-a", "rule-a"] }),
    ]);
    const groups = groupToolRiskEvents(events);

    expect(
      groups.map((group) => group.members.map((member) => member.id)),
    ).toEqual([[1, 3], [2]]);
    expect(events[0].ruleIds).toEqual(["rule-a", "rule-a", "rule-b"]);
  });

  it("does not truncate the full rule ID list", () => {
    const commonRules = Array.from({ length: 20 }, (_, index) => `a-${index}`);
    expectSingletons([
      event({ id: 1, ruleIds: [...commonRules, "z-rule-one"] }),
      event({ id: 2, ruleIds: [...commonRules, "z-rule-two"] }),
    ]);
  });

  it("groups equal empty rule lists but not different rule lists", () => {
    const events = [
      event({ id: 1, ruleIds: [] }),
      event({ id: 2, ruleIds: ["rule-a"] }),
      event({ id: 3, ruleIds: [] }),
    ];
    expect(
      groupToolRiskEvents(events).map((group) =>
        group.members.map((member) => member.id),
      ),
    ).toEqual([[1, 3], [2]]);
  });

  it.each(["|", ":", ",", "\u0000", "\n", '"', "\\", "],["])(
    "cannot collide adjacent key fields containing delimiter %j",
    (delimiter) => {
      expectSingletons([
        event({ id: 1, source: `proxy${delimiter}response`, stage: "tail" }),
        event({ id: 2, source: "proxy", stage: `response${delimiter}tail` }),
      ]);
      expectSingletons([
        event({ id: 3, ruleIds: [`a${delimiter}b`, "c"] }),
        event({ id: 4, ruleIds: ["a", `b${delimiter}c`] }),
      ]);
    },
  );

  it("keeps rule array boundaries distinct from a single serialized-looking rule", () => {
    expectSingletons([
      event({ id: 1, ruleIds: ["a", "b"] }),
      event({ id: 2, ruleIds: ['a","b'] }),
      event({ id: 3, ruleIds: ["a,b"] }),
    ]);
  });

  it.each(ELIGIBLE_VERDICTS)(
    "uses fixed minute buckets rather than adjacent or sliding times for %s",
    (verdict) => {
      const events = freezePage([
        event({ id: 1, verdict, timestamp: BUCKET_START + 59_999 }),
        event({ id: 2, verdict, timestamp: BUCKET_START + 60_000 }),
        event({ id: 3, verdict, timestamp: BUCKET_START }),
        event({ id: 4, verdict, timestamp: BUCKET_START + 119_999 }),
        event({ id: 5, verdict, timestamp: BUCKET_START + 120_000 }),
      ]);
      const groups = groupToolRiskEvents(events);

      expect(
        groups.map((group) => group.members.map((member) => member.id)),
      ).toEqual([[1, 3], [2, 4], [5]]);
      expect(groups.map((group) => group.representative.id)).toEqual([1, 2, 5]);
    },
  );

  it("uses Math.floor rather than truncation for negative timestamps", () => {
    const events = [
      event({ id: 1, timestamp: -1 }),
      event({ id: 2, timestamp: 0 }),
      event({ id: 3, timestamp: -60_000 }),
      event({ id: 4, timestamp: -60_001 }),
    ];
    expect(
      groupToolRiskEvents(events).map((group) =>
        group.members.map((member) => member.id),
      ),
    ).toEqual([[1, 3], [2], [4]]);
  });

  it.each([0, -1, 8_640_000_000_000_000, -8_640_000_000_000_000])(
    "accepts valid safe integer timestamps including Date boundaries: %s",
    (timestamp) => {
      const events = [event({ id: 1, timestamp }), event({ id: 2, timestamp })];
      const groups = groupToolRiskEvents(events);
      expect(groups).toHaveLength(1);
      expect(groups[0].members).toEqual(events);
    },
  );

  it.each(INVALID_TIMESTAMPS)(
    "does not group invalid timestamps: %s",
    (_label, timestamp) => {
      expectSingletons([
        event({ id: 1, timestamp: timestamp as number }),
        event({ id: 2, timestamp: timestamp as number }),
      ]);
    },
  );

  it.each([null, undefined, "", " ", "\t\n\r", "\u00a0\u2003"])(
    "does not group missing or blank summaries: %j",
    (commandSummary) => {
      expectSingletons([
        event({ id: 1, commandSummary }),
        event({ id: 2, commandSummary }),
      ]);
    },
  );

  it("does not fabricate a missing summary from extra raw command fields", () => {
    const events = [
      { ...event({ id: 1, commandSummary: null }), command: "ls -la" },
      { ...event({ id: 2, commandSummary: null }), command: "ls -la" },
    ];
    expectSingletons(events);
  });

  it.each(ELIGIBLE_VERDICTS)(
    "compares original %s summaries without whitespace normalization",
    (verdict) => {
      expectSingletons(
        ["ls -la", " ls -la", "ls -la ", "\tls -la", "ls  -la"].map(
          (commandSummary, index) =>
            event({ id: index + 1, verdict, commandSummary }),
        ),
      );
      const summary = " \tls -la \n";
      const events = freezePage([
        event({ id: 20, verdict, commandSummary: summary }),
        event({ id: 21, verdict, commandSummary: summary }),
      ]);
      const groups = groupToolRiskEvents(events);
      expect(groups).toHaveLength(1);
      expect(groups[0].members).toEqual(events);
      expect(groups[0].representative.commandSummary).toBe(summary);
    },
  );

  it("keeps nonadjacent group positions and member order from the original page", () => {
    const events = freezePage([
      event({ id: 50 }),
      event({ id: 3, verdict: "risky" }),
      event({ id: 12, commandSummary: "pwd" }),
      event({ id: 7 }),
      event({ id: 2, verdict: "unchecked" }),
      event({ id: 1, commandSummary: "pwd" }),
      event({ id: 44, timestamp: NaN }),
      event({ id: 5 }),
    ]);
    const groups = groupToolRiskEvents(events);

    expect(groups.map((group) => group.representative.id)).toEqual([
      50, 3, 12, 2, 44,
    ]);
    expect(
      groups.map((group) => group.members.map((member) => member.id)),
    ).toEqual([[50, 7, 5], [3], [12, 1], [2], [44]]);
    groups.forEach((group) => {
      expect(group.representative).toBe(group.members[0]);
      group.members.forEach((member) => {
        expect(member).toBe(events.find((record) => record.id === member.id));
      });
    });
    expect(
      groups.reduce((count, group) => count + group.members.length, 0),
    ).toBe(events.length);
  });

  it("uses member length, not suppressedCount, and retains each original count", () => {
    const events = freezePage([
      event({ id: 1, suppressedCount: 100 }),
      event({ id: 2, suppressedCount: 0 }),
      event({ id: 3, suppressedCount: 8 }),
    ]);
    const groups = groupToolRiskEvents(events);

    expect(groups).toHaveLength(1);
    expect(groups[0].members).toHaveLength(3);
    expect(groups[0].representative).toBe(events[0]);
    expect(groups[0].members.map((member) => member.suppressedCount)).toEqual([
      100, 0, 8,
    ]);
  });

  it("groups page-local similarities without requiring session or raw command equality", () => {
    const events = [
      { ...event({ id: 1 }), sessionId: "session-a", command: "ls -la /first" },
      {
        ...event({ id: 2 }),
        sessionId: "session-b",
        command: "ls -la /second",
      },
    ];
    const groups = groupToolRiskEvents(events);
    expect(groups).toHaveLength(1);
    expect(groups[0].members[0]).toBe(events[0]);
    expect(groups[0].members[1]).toBe(events[1]);
  });

  it("creates fresh output for new immutable pages without accumulating prior members", () => {
    const firstPage = freezePage([event({ id: 1 }), event({ id: 2 })]);
    const snapshot = firstPage.map((record) => ({
      ...record,
      ruleIds: [...record.ruleIds],
    }));
    const firstGroups = groupToolRiskEvents(firstPage);
    const nextPage = freezePage([event({ id: 3 })]);
    const nextGroups = groupToolRiskEvents(nextPage);
    const extendedPage = freezePage([...firstPage, event({ id: 4 })]);
    const extendedGroups = groupToolRiskEvents(extendedPage);
    const repeatedGroups = groupToolRiskEvents(firstPage);

    expect(firstGroups[0].members.map((member) => member.id)).toEqual([1, 2]);
    expect(nextGroups[0].members).toEqual([nextPage[0]]);
    expect(extendedGroups[0].members.map((member) => member.id)).toEqual([
      1, 2, 4,
    ]);
    expect(repeatedGroups).toEqual(firstGroups);
    expect(repeatedGroups).not.toBe(firstGroups);
    expect(repeatedGroups[0]).not.toBe(firstGroups[0]);
    expect(repeatedGroups[0].members).not.toBe(firstGroups[0].members);
    expect(firstGroups[0].members).not.toBe(firstPage);
    expect(firstPage).toEqual(snapshot);
    expect(extendedPage).toHaveLength(3);
    expectSingletons(Object.freeze([]));
  });
});
