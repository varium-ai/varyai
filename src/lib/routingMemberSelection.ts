import type {
  RoutingGroup,
  RoutingMember,
  RoutingProviderOption,
} from "./api/intentRouting";

export const ROUTING_MEMBER_LIMIT = 256;

export function routingMemberKey(
  member: Pick<RoutingMember, "providerId" | "modelId">,
) {
  return JSON.stringify([member.providerId, member.modelId]);
}

export function routingMemberAvailable(
  providers: RoutingProviderOption[],
  member: Pick<RoutingMember, "providerId" | "modelId">,
) {
  const provider = providers.find(
    (provider) => provider.id === member.providerId,
  );
  return (
    !!provider?.taskEligible &&
    (!provider.models ||
      provider.models.some((model) => model.id === member.modelId))
  );
}

export function catalogRoutingMembers(providers: RoutingProviderOption[]) {
  const seen = new Set<string>();
  return providers
    .filter((provider) => provider.taskEligible)
    .flatMap((provider) =>
      (provider.models ?? []).flatMap((model) => {
        const member = { providerId: provider.id, modelId: model.id };
        const key = routingMemberKey(member);
        if (!model.id.trim() || seen.has(key)) return [];
        seen.add(key);
        return [member];
      }),
    );
}

export function selectRoutingMembers(
  group: RoutingGroup,
  choices: Array<Pick<RoutingMember, "providerId" | "modelId">>,
  selected: boolean,
): RoutingGroup {
  const keys = new Set(choices.map(routingMemberKey));
  const members = selected
    ? [...group.members]
    : group.members.filter((member) => !keys.has(routingMemberKey(member)));
  const existing = new Set(members.map(routingMemberKey));
  if (selected) {
    for (const choice of choices) {
      const key = routingMemberKey(choice);
      if (existing.has(key) || members.length >= ROUTING_MEMBER_LIMIT) continue;
      members.push({ ...choice, id: crypto.randomUUID() });
      existing.add(key);
    }
  }
  return withRoutingMembers(group, members);
}

export function withRoutingMembers(
  group: RoutingGroup,
  members: RoutingMember[],
): RoutingGroup {
  const ids = new Set(members.map((member) => member.id));
  const removed = new Set(
    group.members
      .filter((member) => !ids.has(member.id))
      .map((member) => member.id),
  );
  const defaultMemberId =
    group.defaultMemberId && !removed.has(group.defaultMemberId)
      ? group.defaultMemberId
      : (members[0]?.id ?? "");
  const previous = new Set(group.members.map((member) => member.id));
  return {
    ...group,
    members,
    defaultMemberId,
    manualMemberId:
      group.manualMemberId && removed.has(group.manualMemberId)
        ? members[0]?.id
        : group.manualMemberId,
    off: group.off?.filter((id) => !removed.has(id)),
    fallbackMemberIds: [
      ...group.fallbackMemberIds.filter(
        (id) => !removed.has(id) && id !== defaultMemberId,
      ),
      ...members
        .filter(
          (member) => !previous.has(member.id) && member.id !== defaultMemberId,
        )
        .map((member) => member.id),
    ],
    rules: group.rules.flatMap((rule) => {
      const remaining = [
        rule.targetMemberId,
        ...(rule.fallbackMemberIds ?? []),
      ].filter((id) => !removed.has(id));
      if (remaining.length === 1 + (rule.fallbackMemberIds?.length ?? 0))
        return [rule];
      return remaining.length
        ? [
            {
              ...rule,
              targetMemberId: remaining[0],
              fallbackMemberIds: remaining.slice(1),
            },
          ]
        : [];
    }),
  };
}
