import { describe, expect, it } from "vitest";
import { blankRoutingGroup } from "@/lib/routingGroupTransfer";
import {
  catalogRoutingMembers,
  routingGroupHasReadyMember,
  routingMemberUnavailableReason,
  routingMemberAvailable,
  selectRoutingMembers,
} from "@/lib/routingMemberSelection";

const a = {
  id: "a",
  providerId: "one",
  modelId: "same",
  effort: "high",
  weight: 7,
};
const b = { id: "b", providerId: "one", modelId: "other" };
const c = { id: "c", providerId: "two", modelId: "same" };

describe("routing member selection", () => {
  it("removes all variants in a provider batch and promotes surviving rule targets", () => {
    const group = {
      ...blankRoutingGroup(),
      members: [a, b, c],
      defaultMemberId: a.id,
      manualMemberId: a.id,
      fallbackMemberIds: [b.id, c.id],
      off: [a.id],
      rules: [
        {
          id: "keep",
          name: "Keep",
          description: "Synthetic",
          enabled: true,
          targetMemberId: a.id,
          fallbackMemberIds: [b.id, c.id],
        },
        {
          id: "drop",
          name: "Drop",
          description: "Synthetic",
          enabled: true,
          targetMemberId: b.id,
        },
      ],
    };
    const result = selectRoutingMembers(group, [a, b], false);
    expect(result.members).toEqual([c]);
    expect(result.defaultMemberId).toBe(c.id);
    expect(result.manualMemberId).toBe(c.id);
    expect(result.fallbackMemberIds).toEqual([]);
    expect(result.off).toEqual([]);
    expect(result.rules).toEqual([
      { ...group.rules[0], targetMemberId: c.id, fallbackMemberIds: [] },
    ]);
  });

  it("preserves dynamic pattern targets while adding or removing explicit models", () => {
    const group = {
      ...blankRoutingGroup(),
      members: [a],
      match: ["one/*"],
      defaultMemberId: "dynamic",
      manualMemberId: "dynamic",
      off: ["dynamic"],
      fallbackMemberIds: [a.id],
      rules: [
        {
          id: "dynamic-rule",
          name: "Dynamic",
          description: "Synthetic",
          enabled: true,
          targetMemberId: "dynamic",
        },
      ],
    };
    const result = selectRoutingMembers(group, [b], true);
    expect(result.defaultMemberId).toBe("dynamic");
    expect(result.manualMemberId).toBe("dynamic");
    expect(result.off).toEqual(["dynamic"]);
    expect(result.rules).toEqual(group.rules);
    const removed = selectRoutingMembers(result, [a], false);
    expect(removed.defaultMemberId).toBe("dynamic");
    expect(removed.rules).toEqual(group.rules);
  });

  it("caps bulk selection at 256 and keeps existing effort variants and identities", () => {
    const group = {
      ...blankRoutingGroup(),
      members: [a, { ...a, id: "variant", effort: "low" }],
      defaultMemberId: a.id,
    };
    const choices = Array.from({ length: 300 }, (_, index) => ({
      providerId: "one",
      modelId: `model-${index}`,
    }));
    const result = selectRoutingMembers(group, [a, ...choices], true);
    expect(result.members).toHaveLength(256);
    expect(result.members.slice(0, 2)).toEqual(group.members);
    expect(new Set(result.members.map((member) => member.id)).size).toBe(256);
    expect(result.defaultMemberId).toBe(a.id);
    expect(result.fallbackMemberIds).toHaveLength(254);
  });

  it("deduplicates catalog identities while retaining matching IDs from different providers", () => {
    const base = {
      name: "Synthetic",
      classifierEligible: false,
      taskEligible: true,
    };
    expect(
      catalogRoutingMembers([
        {
          ...base,
          id: "one",
          models: [
            { id: "same", name: "Same" },
            { id: "same", name: "Duplicate" },
          ],
        },
        { ...base, id: "two", models: [{ id: "same", name: "Same" }] },
        {
          ...base,
          id: "off",
          taskEligible: false,
          models: [{ id: "same", name: "Same" }],
        },
      ]),
    ).toEqual([
      { providerId: "one", modelId: "same" },
      { providerId: "two", modelId: "same" },
    ]);
  });
});

describe("routing readiness", () => {
  const provider = {
    id: "one",
    name: "Synthetic",
    taskEligible: true,
    classifierEligible: false,
    models: [{ id: "same", name: "Same" }],
  };
  it.each([
    { providers: [], reason: "providerMissing" },
    {
      providers: [{ ...provider, taskEligible: false }],
      reason: "providerNotReady",
    },
    { providers: [{ ...provider, models: [] }], reason: "modelMissing" },
    { providers: [provider], reason: null },
    { providers: [{ ...provider, models: undefined }], reason: null },
  ])(
    "reports local readiness consistently: $reason",
    ({ providers, reason }) => {
      expect(routingMemberUnavailableReason(providers, a)).toBe(reason);
      expect(routingMemberAvailable(providers, a)).toBe(reason === null);
    },
  );

  it("does not count switched-off pattern members as ready", () => {
    const group = { ...blankRoutingGroup(), members: [c], off: [a.id] };
    expect(routingGroupHasReadyMember([provider], group, [a])).toBe(false);
    expect(
      routingGroupHasReadyMember([provider], { ...group, off: [] }, [a]),
    ).toBe(true);
  });

  it("uses the default target only when a manual selection is absent", () => {
    const group = {
      ...blankRoutingGroup(),
      routing: "manual" as const,
      members: [a, c],
      defaultMemberId: a.id,
    };
    expect(routingGroupHasReadyMember([provider], group)).toBe(true);
    expect(
      routingGroupHasReadyMember([provider], {
        ...group,
        manualMemberId: c.id,
      }),
    ).toBe(false);
  });
});
