// @vitest-environment jsdom
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  parseToolRiskToolRefs,
  type ToolRiskEventRecord,
  type ToolRiskRule,
  type ToolRiskRuleItem,
} from "@/types/toolRisk";
import { ToolRiskRulesPanel } from "./ToolRiskRulesPanel";

const getRule = vi.fn();
const saveRule = vi.fn().mockResolvedValue("custom-1");
const toggleRule = vi.fn().mockResolvedValue(undefined);
const resetBuiltinRule = vi.fn().mockResolvedValue(undefined);
const deleteRule = vi.fn().mockResolvedValue(undefined);
const testSandbox = vi.fn().mockResolvedValue(undefined);
let items: ToolRiskRuleItem[];
let events: ToolRiskEventRecord[] = [];

const customItem: ToolRiskRuleItem = {
  id: "custom-1",
  name: "My rule",
  origin: "custom",
  enabled: true,
  riskCategory: "sensitive_read",
  severity: "high",
  reasonCode: "sensitive_target",
  localReview: "off",
  selectorSummary: "operation:file_read",
  conditionCount: 1,
  targetKind: "operation",
  operationTypes: ["file_read"],
  toolRefCount: 0,
  conditions: [
    { field: "/target_path", op: "detector", value: "sensitive_path" },
  ],
  revision: 3,
  builtinVersion: null,
};

const fullRule: ToolRiskRule = {
  schema_version: 1,
  id: "custom-1",
  revision: 3,
  origin: "custom",
  name: "My rule",
  description: "desc",
  enabled: true,
  selector: { target: { kind: "operation", types: ["file_read"] } },
  conditions: [
    { field: "/target_path", op: "detector", value: "sensitive_path" },
  ],
  risk_category: "sensitive_read",
  severity: "high",
  reason_code: "sensitive_target",
  local_review: "off",
};

vi.mock("@/hooks/useToolRisk", () => ({
  useToolRiskCommandLists: () => ({
    entries: [],
    isLoading: false,
    isError: false,
    saveEntry: vi.fn(),
    deleteEntry: vi.fn(),
    isSaving: false,
    isDeleting: false,
  }),
  useToolRiskSettings: () => ({
    settings: { enabled: true, policyVersion: 1 },
    isLoading: false,
    updateSettings: vi.fn(),
    isUpdating: false,
  }),
  useToolRiskRules: () => ({
    rules: items,
    isLoading: false,
    refetch: vi.fn(),
    toggleRule,
    resetBuiltinRule,
    saveRule,
    deleteRule,
    isSaving: false,
  }),
  useToolRiskSandbox: () => ({
    test: testSandbox,
    result: undefined,
    isTesting: false,
    reset: vi.fn(),
  }),
  useToolRiskPolicyChanges: () => ({
    changes: [],
    isLoading: false,
    isError: false,
    isFetching: false,
    refetch: vi.fn(),
    clearChanges: vi.fn(),
    isClearing: false,
  }),
  useToolRiskEvents: () => ({
    events,
    isLoading: false,
    refetch: vi.fn(),
    clearEvents: vi.fn(),
    isClearing: false,
  }),
}));

vi.mock("@/lib/api", () => ({
  toolRiskApi: { getRule: (id: string) => getRule(id) },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === "toolRisk.rulesSummary")
        return `${key}: total=${options?.total}, enabled=${options?.enabled}`;
      if (key === "toolRisk.policyVersion")
        return (options?.defaultValue ?? key) as string;
      // Preserve interpolation semantics when asserting the rendered summary.
      if (key === "toolRisk.conditionSummary.patternWithValue")
        return `${options?.field} · ${options?.op} · ${options?.value}`;
      if (key === "toolRisk.conditionSummary.patternWithoutValue")
        return `${options?.field} · ${options?.op}`;
      if (key === "toolRisk.conditionSummary.more") return `+${options?.count}`;
      if (key === "toolRisk.scopeToolRefs") return `refs=${options?.count}`;
      if (key === "toolRisk.scopeSeparator") return "+";
      return key;
    },
  }),
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

describe("ToolRiskRulesPanel openEdit", () => {
  beforeEach(() => {
    getRule.mockReset();
    saveRule.mockClear();
    toggleRule.mockClear();
    resetBuiltinRule.mockClear();
    deleteRule.mockClear();
    items = [
      customItem,
      {
        ...customItem,
        id: "builtin-1",
        name: "Built in",
        origin: "builtin",
        revision: 7,
      },
    ];
  });

  it("fetches the full rule and preserves its selector and conditions", async () => {
    getRule.mockResolvedValue(fullRule);
    render(<ToolRiskRulesPanel />);

    fireEvent.click(screen.getByTestId("tool-risk-edit-custom-1"));

    await waitFor(() => expect(getRule).toHaveBeenCalledWith("custom-1"));

    fireEvent.click(screen.getByTestId("tool-risk-save"));

    await waitFor(() => expect(saveRule).toHaveBeenCalledTimes(1));
    const saved = saveRule.mock.calls[0][0];
    expect(saved.id).toBe("custom-1");
    expect(saved.expectedRevision).toBe(3);
    expect(saved.selector).toEqual({
      target: { kind: "operation", types: ["file_read"] },
    });
    expect(saved.conditions).toEqual([
      { field: "/target_path", op: "detector", value: "sensitive_path" },
    ]);
    expect(saved.reasonCode).toBe("sensitive_target");
  });

  it("does not open the dialog when the rule cannot be loaded", async () => {
    getRule.mockResolvedValue(null);
    render(<ToolRiskRulesPanel />);

    fireEvent.click(screen.getByTestId("tool-risk-edit-custom-1"));

    await waitFor(() => expect(getRule).toHaveBeenCalled());

    expect(screen.queryByTestId("tool-risk-save")).toBeNull();
  });
});

describe("ToolRiskRulesPanel revision and exact identity", () => {
  beforeEach(() => {
    getRule.mockResolvedValue(fullRule);
    saveRule.mockClear();
    toggleRule.mockClear();
    resetBuiltinRule.mockClear();
    deleteRule.mockClear();
    items = [
      customItem,
      {
        ...customItem,
        id: "builtin-1",
        name: "Built in",
        origin: "builtin",
        revision: 7,
      },
    ];
  });

  it("passes current revisions to toggle, reset and confirmed deletion", async () => {
    render(<ToolRiskRulesPanel />);
    const customRow = screen.getByText("My rule").closest("tr")!;
    const builtinRow = screen.getByText("Built in").closest("tr")!;
    fireEvent.click(within(customRow).getByRole("switch"));
    fireEvent.click(within(builtinRow).getByRole("switch"));
    fireEvent.click(screen.getByTitle("toolRisk.resetBuiltin"));
    expect(toggleRule).toHaveBeenCalledWith("custom-1", false, 3);
    expect(toggleRule).toHaveBeenCalledWith("builtin-1", false, 7);
    expect(resetBuiltinRule).toHaveBeenCalledWith("builtin-1", 7);
    fireEvent.click(within(customRow).getAllByRole("button").at(-1)!);
    fireEvent.click(
      await screen.findByRole("button", { name: "common.delete" }),
    );
    await waitFor(() => expect(deleteRule).toHaveBeenCalledWith("custom-1", 3));
  });

  it("edits and saves structured exact tool references without losing selector filters", async () => {
    const identity = {
      client: "claude",
      namespace: "mcp",
      name: "read",
      definitionRef: "schema-1",
      serverIdentity: "server-1",
    };
    getRule.mockResolvedValue({
      ...fullRule,
      selector: {
        sources: ["model_response"],
        clients: ["claude"],
        target: { kind: "tool", tool_refs: [identity] },
      },
      conditions: [
        { field: "/target_path", op: "keyword", value: "sensitive" },
      ],
    });
    render(<ToolRiskRulesPanel />);
    fireEvent.click(screen.getByTestId("tool-risk-edit-custom-1"));
    const input = await screen.findByRole("textbox", {
      name: "toolRisk.fieldToolRefs",
    });
    expect(JSON.parse((input as HTMLTextAreaElement).value)).toEqual([
      identity,
    ]);
    const edited = { ...identity, serverIdentity: "server-2" };
    fireEvent.change(input, { target: { value: JSON.stringify([edited]) } });
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() => expect(saveRule).toHaveBeenCalledTimes(1));
    expect(saveRule.mock.calls[0][0].selector).toEqual({
      sources: ["model_response"],
      clients: ["claude"],
      target: { kind: "tool", tool_refs: [edited] },
    });
    expect(saveRule.mock.calls[0][0].expectedRevision).toBe(3);
  });

  it.each([
    "read",
    "{}",
    "[]",
    '["read"]',
    '[{"client":"claude","name":"read","server_id":"legacy"}]',
    '[{"client":"claude","name":"read"}]',
    '[{"client":"claude","name":"read","definitionRef":"schema-1","unexpected":true}]',
    '[{"client":"claude","namespace":"mcp","name":"read","definitionRef":"schema-1"}]',
    '[{"client":" ","name":"read","definitionRef":"schema-1"}]',
  ])("cannot save invalid or ambiguous tool references: %s", async (text) => {
    getRule.mockResolvedValue({
      ...fullRule,
      selector: {
        target: {
          kind: "tool",
          tool_refs: [
            { client: "claude", name: "read", definitionRef: "schema-1" },
          ],
        },
      },
    });
    render(<ToolRiskRulesPanel />);
    fireEvent.click(screen.getByTestId("tool-risk-edit-custom-1"));
    const input = await screen.findByRole("textbox", {
      name: "toolRisk.fieldToolRefs",
    });
    fireEvent.change(input, { target: { value: text } });
    const save = screen.getByTestId("tool-risk-save");
    expect(save).toBeDisabled();
    fireEvent.click(save);
    expect(saveRule).not.toHaveBeenCalled();
  });

  it("rejects legacy name-only rule targets without silently migrating identity", async () => {
    getRule.mockResolvedValue({
      ...fullRule,
      selector: { target: { kind: "tool", tool_ref: "read" } },
    });
    render(<ToolRiskRulesPanel />);
    fireEvent.click(screen.getByTestId("tool-risk-edit-custom-1"));
    expect(await screen.findByTestId("tool-risk-save")).toBeDisabled();
    expect(saveRule).not.toHaveBeenCalled();
  });
});

describe("ToolRiskRulesPanel sandbox trust", () => {
  beforeEach(() => {
    testSandbox.mockClear();
    items = [];
  });

  it("does not infer trusted client or schema from a builtin-looking tool name", async () => {
    render(<ToolRiskRulesPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.sandboxRun" }),
    );
    await waitFor(() =>
      expect(testSandbox).toHaveBeenCalledWith({
        toolName: "Bash",
        input: { command: "ls" },
      }),
    );
  });

  it("passes an explicitly entered client and trusted definition reference to the sandbox", async () => {
    render(<ToolRiskRulesPanel />);
    fireEvent.change(
      screen.getByRole("textbox", { name: "toolRisk.sandboxClient" }),
      { target: { value: "claude" } },
    );
    fireEvent.change(
      screen.getByRole("textbox", { name: "toolRisk.sandboxDefinitionRef" }),
      { target: { value: "builtin:claude:tools:v1" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.sandboxRun" }),
    );
    await waitFor(() =>
      expect(testSandbox).toHaveBeenCalledWith({
        toolName: "Bash",
        input: { command: "ls" },
        client: "claude",
        definitionRef: "builtin:claude:tools:v1",
      }),
    );
  });
});

describe("exact tool reference validation", () => {
  it("rejects server identity without an explicit nonempty namespace", () => {
    for (const namespace of [undefined, null, "", " "]) {
      expect(
        parseToolRiskToolRefs(
          JSON.stringify([
            {
              client: "claude",
              name: "read",
              serverIdentity: "server-1",
              namespace,
            },
          ]),
        ),
      ).toBeNull();
    }
  });

  it("accepts exact custom clients and preserves absent optional identity fields", () => {
    const refs = [
      { client: "custom-client", name: "read", definitionRef: "schema-1" },
      {
        client: "grokbuild",
        namespace: "custom",
        name: "read",
        serverIdentity: "server-1",
      },
    ];
    expect(parseToolRiskToolRefs(JSON.stringify(refs))).toEqual(refs);
  });
});

describe("risk rules remain risk-only", () => {
  beforeEach(() => {
    items = [customItem];
    saveRule.mockClear();
  });
  it("refuses an old severity none rule instead of granting a dangerous exemption", async () => {
    getRule.mockResolvedValue({ ...fullRule, severity: "none" });
    render(<ToolRiskRulesPanel />);
    fireEvent.click(screen.getByTestId("tool-risk-edit-custom-1"));
    const save = await screen.findByTestId("tool-risk-save");
    expect(save).toBeDisabled();
    expect(screen.getByText("toolRisk.riskSeverityRequired")).toBeTruthy();
    fireEvent.click(save);
    expect(saveRule).not.toHaveBeenCalled();
  });
  it("offers command class conditions and preserves /command_class on save", async () => {
    getRule.mockResolvedValue({
      ...fullRule,
      conditions: [{ field: "/command_class", op: "eq", value: "script_exec" }],
    });
    render(<ToolRiskRulesPanel />);
    expect(screen.getByTestId("shell-command-lists-panel")).toBeTruthy();
    fireEvent.click(screen.getByTestId("tool-risk-edit-custom-1"));
    expect(
      await screen.findByLabelText("toolRisk.fieldCommandClass"),
    ).toHaveTextContent("toolRisk.commandClass.script_exec");
    openOptions(screen.getByLabelText("toolRisk.fieldCommandClass"));
    fireEvent.click(
      screen.getByRole("option", {
        name: "toolRisk.commandClass.network_egress",
      }),
    );
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [
            { field: "/command_class", op: "eq", value: "network_egress" },
          ],
        }),
      ),
    );
  });
});

describe("ToolRiskRulesPanel visible rules", () => {
  const databaseBuiltin: ToolRiskRuleItem = {
    ...customItem,
    id: "builtin.destructive_database_change",
    name: "Database builtin",
    origin: "builtin",
    enabled: true,
    riskCategory: "destructive_database_change",
    selectorSummary: "operation:database_operation",
  };
  const databaseCustom: ToolRiskRuleItem = {
    ...databaseBuiltin,
    id: "custom-database",
    name: "Custom database rule",
    origin: "custom",
  };
  const otherBuiltin: ToolRiskRuleItem = {
    ...customItem,
    id: "builtin.sensitive_read",
    name: "Other builtin",
    origin: "builtin",
    enabled: false,
  };
  const builtinNameKey = (id: string) =>
    `toolRisk.builtinRule.${id.replace("builtin.", "")}.name`;

  it("shows the database builtin with a capability warning alongside other rules", () => {
    items = [databaseBuiltin, databaseCustom, otherBuiltin];
    render(<ToolRiskRulesPanel />);

    expect(
      screen.getByText(builtinNameKey(databaseBuiltin.id)),
    ).toBeInTheDocument();
    expect(
      screen.getByText("toolRisk.databaseCapabilityHint"),
    ).toBeInTheDocument();
    expect(screen.getByText(databaseCustom.name)).toBeInTheDocument();
    expect(
      screen.getByText(builtinNameKey(otherBuiltin.id)),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId(`tool-risk-edit-${databaseCustom.id}`),
    ).toBeInTheDocument();
    expect(screen.getAllByTitle("toolRisk.resetBuiltin")).toHaveLength(2);
    expect(screen.queryByText("toolRisk.noRules")).not.toBeInTheDocument();
    expect(items).toEqual([databaseBuiltin, databaseCustom, otherBuiltin]);
  });

  it.each([true, false])(
    "counts the database rule without treating enabled=%s as readiness",
    (enabled) => {
      items = [{ ...databaseBuiltin, enabled }, databaseCustom, otherBuiltin];
      render(<ToolRiskRulesPanel />);

      expect(
        screen.getByText(
          `toolRisk.rulesSummary: total=3, enabled=${enabled ? 2 : 1}`,
        ),
      ).toBeInTheDocument();
      expect(
        screen.getByText("toolRisk.capabilityNotReady"),
      ).toBeInTheDocument();
    },
  );

  it("shows the not-ready database rule instead of a misleading empty state", () => {
    items = [databaseBuiltin];
    render(<ToolRiskRulesPanel />);

    expect(screen.queryByText("toolRisk.noRules")).not.toBeInTheDocument();
    expect(
      screen.getByText("toolRisk.rulesSummary: total=1, enabled=1"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(builtinNameKey(databaseBuiltin.id)),
    ).toBeInTheDocument();
    expect(screen.getByTitle("toolRisk.resetBuiltin")).toBeInTheDocument();
  });

  it.each([
    { origin: "custom", id: "builtin.destructive_database_change" },
    { origin: "builtin", id: "builtin.destructive_database_change.extra" },
    { origin: "builtin", id: "prefix.builtin.destructive_database_change" },
    { origin: "builtin", id: "builtin.Destructive_database_change" },
  ] as const)("keeps $origin rule $id visible", ({ origin, id }) => {
    items = [{ ...databaseBuiltin, origin, id }];
    render(<ToolRiskRulesPanel />);

    expect(screen.getByText(databaseBuiltin.name)).toBeInTheDocument();
    expect(
      screen.getByText("toolRisk.rulesSummary: total=1, enabled=1"),
    ).toBeInTheDocument();
    expect(screen.queryByText("toolRisk.noRules")).not.toBeInTheDocument();
  });

  it("uses exact builtin identity rather than category for the capability warning", () => {
    items = [{ ...databaseBuiltin, riskCategory: "sensitive_read" }];
    render(<ToolRiskRulesPanel />);

    expect(
      screen.getByText(builtinNameKey(databaseBuiltin.id)),
    ).toBeInTheDocument();
    expect(
      screen.getByText("toolRisk.databaseCapabilityHint"),
    ).toBeInTheDocument();
    expect(screen.queryByText("toolRisk.noRules")).not.toBeInTheDocument();
  });

  it("localizes builtin names from stable ids and keeps custom names verbatim", () => {
    items = [databaseBuiltin, databaseCustom, otherBuiltin];
    render(<ToolRiskRulesPanel />);

    expect(
      screen.getByText(builtinNameKey(databaseBuiltin.id)),
    ).toBeInTheDocument();
    expect(
      screen.getByText(builtinNameKey(otherBuiltin.id)),
    ).toBeInTheDocument();
    expect(screen.getByText(databaseCustom.name)).toBeInTheDocument();
    // Built-in names are localized instead of rendering the backend fallback.
    expect(screen.queryByText(databaseBuiltin.name)).toBeNull();
    expect(screen.queryByText(otherBuiltin.name)).toBeNull();
  });

  it("shows the rule scope and condition terms instead of a raw summary", () => {
    items = [databaseBuiltin, databaseCustom];
    render(<ToolRiskRulesPanel />);

    expect(
      screen.getByTestId(`tool-risk-scope-${databaseBuiltin.id}`),
    ).toHaveTextContent("toolRisk.operation.database_operation");
    expect(
      screen.getByTestId(`tool-risk-conditions-${databaseBuiltin.id}`),
    ).toHaveTextContent(
      "toolRisk.conditionField.target_path · toolRisk.op.detector · toolRisk.detector.sensitive_path",
    );
    expect(screen.queryByText("operation:database_operation")).toBeNull();
    expect(screen.queryByText("operation:file_read")).toBeNull();
  });

  it("summarizes exact tool targets by reference count", () => {
    items = [
      {
        ...customItem,
        id: "custom-tool",
        name: "Tool rule",
        targetKind: "tool",
        operationTypes: [],
        toolRefCount: 2,
        conditions: [{ field: "$text", op: "keyword", value: "secret" }],
      },
    ];
    render(<ToolRiskRulesPanel />);

    expect(screen.getByTestId("tool-risk-scope-custom-tool")).toHaveTextContent(
      "refs=2",
    );
    expect(
      screen.getByTestId("tool-risk-conditions-custom-tool"),
    ).toHaveTextContent("$text · toolRisk.op.keyword · secret");
  });

  it("lists several operation types and caps long condition summaries", () => {
    items = [
      {
        ...customItem,
        id: "custom-many",
        name: "Many conditions",
        operationTypes: ["shell_exec", "file_read"],
        conditions: [
          { field: "/command", op: "keyword", value: "a" },
          { field: "/target_path", op: "keyword", value: "b" },
          { field: "/destination", op: "keyword", value: "c" },
          { field: "/op_type", op: "eq", value: "shell_exec" },
        ],
      },
    ];
    render(<ToolRiskRulesPanel />);

    const scope = screen.getByTestId("tool-risk-scope-custom-many");
    expect(scope).toHaveTextContent("toolRisk.operation.shell_exec");
    expect(scope).toHaveTextContent("toolRisk.operation.file_read");
    expect(
      screen.getByTestId("tool-risk-conditions-custom-many"),
    ).toHaveTextContent("+1");
  });
});

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
  events = [];
});

/** Operation targets use semantic fields; tool targets keep free-text JSON Pointers. */
const conditionField = () =>
  screen.getByLabelText("toolRisk.fieldConditionField");
const conditionFieldInput = () => screen.getByPlaceholderText("/command");
const conditionOp = () => screen.getByLabelText("toolRisk.fieldConditionOp");
const conditionValue = () =>
  screen.getByLabelText("toolRisk.fieldConditionValue");
const conditionValues = () =>
  screen.getByLabelText("toolRisk.fieldConditionValues");
const openOptions = (input: HTMLElement) =>
  fireEvent.keyDown(input, { key: "ArrowDown" });
const opOptions = () => {
  openOptions(conditionOp());
  return screen.getAllByRole("option").map((option) => option.textContent);
};
const selectConditionField = (label: string) => {
  openOptions(conditionField());
  fireEvent.click(screen.getByRole("option", { name: label }));
};
const selectOp = (op: string) => {
  openOptions(conditionOp());
  fireEvent.click(screen.getByRole("option", { name: `toolRisk.op.${op}` }));
};
const editConditions = async (
  conditions: ToolRiskRule["conditions"],
  tool = false,
) => {
  getRule.mockResolvedValue({
    ...fullRule,
    conditions,
    ...(tool
      ? {
          selector: {
            target: {
              kind: "tool",
              tool_refs: [
                { client: "claude", name: "read", definitionRef: "schema-1" },
              ],
            },
          },
        }
      : {}),
  });
  render(<ToolRiskRulesPanel />);
  fireEvent.click(screen.getByTestId("tool-risk-edit-custom-1"));
  await screen.findByTestId("tool-risk-save");
};

describe("risk rule draft and condition safety", () => {
  beforeEach(() => {
    items = [customItem];
    getRule.mockReset();
    saveRule.mockReset().mockResolvedValue("custom-1");
  });

  it("keeps the rule form scrollable with inset fields and persistent title and actions", () => {
    render(<ToolRiskRulesPanel />);
    fireEvent.click(screen.getByRole("button", { name: "toolRisk.addRule" }));
    const dialog = screen.getByRole("dialog");
    const form = within(dialog).getByRole("region", {
      name: "toolRisk.newRule",
    });
    expect(form).toContainElement(conditionField());
    expect(form).toHaveClass("min-h-0", "overflow-y-auto", "px-6", "py-4");
    expect(dialog).toHaveClass("overflow-hidden");
    expect(form).not.toContainElement(
      screen.getByRole("heading", { name: "toolRisk.newRule" }),
    );
    expect(dialog).toContainElement(screen.getByTestId("tool-risk-save"));
    expect(form).not.toContainElement(screen.getByTestId("tool-risk-save"));
    expect(form).not.toContainElement(
      screen.getByRole("button", { name: "common.cancel" }),
    );
  });

  it("starts a blank keyword draft rather than duplicating a builtin detector", () => {
    render(<ToolRiskRulesPanel />);
    fireEvent.click(screen.getByRole("button", { name: "toolRisk.addRule" }));
    const dialog = within(screen.getByRole("dialog"));
    fireEvent.change(dialog.getAllByRole("textbox")[0], {
      target: { value: "New rule" },
    });
    expect(
      dialog.queryByText("toolRisk.detector.remote_download_exec"),
    ).toBeNull();
    expect(dialog.getByText("toolRisk.op.keyword")).toBeInTheDocument();
    expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
    expect(
      screen.getByText("toolRisk.conditionValueRequired"),
    ).toBeInTheDocument();
  });

  it.each(["shell_exec", "file_read", "network_request"] as const)(
    "does not inherit a remote execution detector for a %s event without a class",
    (operationCategory) => {
      events = [
        {
          id: 19,
          timestamp: 1,
          source: "model_response",
          stage: "response",
          operationCategory,
          detectionScope: "in_scope",
          integrityStatus: "complete",
          policyVersion: 1,
          ruleIds: [],
          suppressedCount: 0,
        },
      ];
      render(<ToolRiskRulesPanel />);
      fireEvent.click(screen.getByTestId("tool-risk-create-rule-19"));
      expect(conditionField()).toHaveTextContent("toolRisk.pickConditionField");
      expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
      expect(
        within(screen.getByRole("dialog")).queryByText(
          "toolRisk.detector.remote_download_exec",
        ),
      ).toBeNull();
      expect(
        screen.getByText("toolRisk.conditionValueRequired"),
      ).toBeInTheDocument();
    },
  );

  it("uses the event command class as an equality condition", async () => {
    events = [
      {
        id: 19,
        timestamp: 1,
        source: "model_response",
        stage: "response",
        operationCategory: "shell_exec",
        commandClass: "unknown",
        detectionScope: "in_scope",
        integrityStatus: "complete",
        policyVersion: 1,
        ruleIds: [],
        suppressedCount: 0,
      },
    ];
    render(<ToolRiskRulesPanel />);
    fireEvent.click(screen.getByTestId("tool-risk-create-rule-19"));
    expect(
      screen.getByLabelText("toolRisk.fieldCommandClass"),
    ).toHaveTextContent("toolRisk.commandClass.unknown");
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          enabled: false,
          conditions: [{ field: "/command_class", op: "eq", value: "unknown" }],
        }),
      ),
    );
  });

  it("preserves an unknown command class until it is explicitly cleared and replaced", async () => {
    await editConditions([
      { field: "/command_class", op: "eq", value: "future_class" },
    ]);
    const trigger = screen.getByLabelText("toolRisk.fieldCommandClass");
    expect(trigger).toHaveTextContent("future_class");
    openOptions(trigger);
    fireEvent.click(
      screen.getByRole("option", { name: "toolRisk.pickCommandClass" }),
    );
    expect(trigger).toHaveTextContent("toolRisk.pickCommandClass");
    expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
    openOptions(trigger);
    fireEvent.click(
      screen.getByRole("option", {
        name: "toolRisk.commandClass.network_egress",
      }),
    );
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [
            { field: "/command_class", op: "eq", value: "network_egress" },
          ],
        }),
      ),
    );
  });

  it("does not offer set matching for text fields but keeps a legacy set editable", async () => {
    await editConditions([{ field: "/command", op: "in", values: ["old"] }]);
    expect(opOptions()).not.toContain("toolRisk.op.in");
    const values = conditionValues();
    expect(values).toHaveValue("old");
    fireEvent.change(values, { target: { value: "first\nsecond" } });
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [
            { field: "/command", op: "in", values: ["first", "second"] },
          ],
        }),
      ),
    );
  });

  it("keeps unknown set values in the legacy text editor instead of dropping them", async () => {
    await editConditions([
      { field: "/command_class", op: "in", values: ["future_class"] },
    ]);
    expect(conditionValues()).toHaveValue("future_class");
    expect(
      screen.queryByRole("group", {
        name: "toolRisk.fieldConditionValues",
      }),
    ).toBeNull();
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [
            { field: "/command_class", op: "in", values: ["future_class"] },
          ],
        }),
      ),
    );
  });

  it("offers every semantic field with the raw JSON Pointer as a hint", async () => {
    await editConditions([{ field: "/command", op: "keyword", value: "ls" }]);
    const field = conditionField();
    expect(field).toHaveTextContent("toolRisk.conditionField.command");
    expect(field).toHaveTextContent("/command");
    openOptions(field);
    for (const label of [
      "target_path",
      "command",
      "destination",
      "op_type",
      "recursive",
      "overwrite",
      "command_class",
    ]) {
      expect(
        screen.getByRole("option", {
          name: `toolRisk.conditionField.${label}`,
        }),
      ).toBeInTheDocument();
    }

    // A selection field requires a compatible operator and a newly selected value.
    fireEvent.click(
      screen.getByRole("option", {
        name: "toolRisk.conditionField.command_class",
      }),
    );
    expect(conditionOp()).toHaveTextContent("toolRisk.op.eq");
    openOptions(screen.getByLabelText("toolRisk.fieldCommandClass"));
    fireEvent.click(
      screen.getByRole("option", {
        name: "toolRisk.commandClass.script_exec",
      }),
    );
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [
            { field: "/command_class", op: "eq", value: "script_exec" },
          ],
        }),
      ),
    );
  });

  it.each([
    ["/command", ["eq", "keyword", "regex", "detector"]],
    ["/target_path", ["eq", "keyword", "regex", "detector"]],
    ["/destination", ["eq", "keyword", "regex", "detector"]],
    ["/command_class", ["eq", "in"]],
    ["/op_type", ["eq", "in"]],
    ["/recursive", ["eq"]],
    ["/overwrite", ["eq"]],
  ] as const)("narrows operators for %s", async (field, expected) => {
    await editConditions([{ field, op: "eq", value: "shell_exec" }]);
    expect([...opOptions()].sort()).toEqual(
      expected.map((op) => `toolRisk.op.${op}`).sort(),
    );
  });

  it("offers keep-visible legacy operators for unknown fields only", async () => {
    await editConditions([
      { field: "/unsupported_field", op: "eq", value: "old" },
    ]);
    expect(conditionFieldInput()).toHaveValue("/unsupported_field");
    expect(
      screen.getByText("toolRisk.unsupportedCondition"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
  });

  it("keeps exact tool targets on the free-form JSON Pointer field", async () => {
    await editConditions(
      [{ field: "$text", op: "keyword", value: "ls" }],
      true,
    );
    expect(conditionFieldInput()).toHaveValue("$text");
    expect(conditionFieldInput().tagName).toBe("INPUT");
    expect(opOptions()).toEqual([
      "toolRisk.op.eq",
      "toolRisk.op.in",
      "toolRisk.op.keyword",
      "toolRisk.op.regex",
    ]);
  });

  it("edits command class sets as checkboxes instead of a newline textarea", async () => {
    await editConditions([
      { field: "/command_class", op: "in", values: ["script_exec"] },
    ]);
    const group = screen.getByRole("group", {
      name: "toolRisk.fieldConditionValues",
    });
    const script = within(group).getByRole("checkbox", {
      name: "toolRisk.commandClass.script_exec",
    });
    const obfuscated = within(group).getByRole("checkbox", {
      name: "toolRisk.commandClass.obfuscated",
    });
    expect(script).toBeChecked();
    expect(obfuscated).not.toBeChecked();

    fireEvent.click(obfuscated);
    fireEvent.click(script);
    expect(obfuscated).toBeChecked();
    expect(script).not.toBeChecked();
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [
            { field: "/command_class", op: "in", values: ["obfuscated"] },
          ],
        }),
      ),
    );
  });

  it("edits op_type sets as checkboxes", async () => {
    await editConditions([
      { field: "/op_type", op: "in", values: ["file_read"] },
    ]);
    const group = screen.getByRole("group", {
      name: "toolRisk.fieldConditionValues",
    });
    expect(
      within(group).getByRole("checkbox", {
        name: "toolRisk.operation.file_read",
      }),
    ).toBeChecked();
  });

  it("edits op_type as a semantic single choice", async () => {
    await editConditions([
      { field: "/op_type", op: "eq", value: "shell_exec" },
    ]);
    const value = conditionValue();
    expect(value).toHaveTextContent("toolRisk.operation.shell_exec");
    openOptions(value);
    fireEvent.click(
      screen.getByRole("option", {
        name: "toolRisk.operation.database_operation",
      }),
    );
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [
            { field: "/op_type", op: "eq", value: "database_operation" },
          ],
        }),
      ),
    );
  });

  it.each(["recursive", "overwrite"] as const)(
    "edits the %s flag as a yes/no choice",
    async (slug) => {
      await editConditions([{ field: `/${slug}`, op: "eq", value: "true" }]);
      const value = conditionValue();
      expect(value).toHaveTextContent("toolRisk.conditionBool.true");
      openOptions(value);
      fireEvent.click(
        screen.getByRole("option", { name: "toolRisk.conditionBool.false" }),
      );
      fireEvent.click(screen.getByTestId("tool-risk-save"));
      await waitFor(() =>
        expect(saveRule).toHaveBeenCalledWith(
          expect.objectContaining({
            conditions: [{ field: `/${slug}`, op: "eq", value: "false" }],
          }),
        ),
      );
    },
  );

  it("clears array state when switching away from in", async () => {
    await editConditions([{ field: "/command", op: "in", values: ["old"] }]);
    selectOp("keyword");
    const value = screen.getByLabelText("toolRisk.fieldConditionValue");
    expect(value).toHaveValue("");
    expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
    fireEvent.change(value, { target: { value: "new" } });
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [{ field: "/command", op: "keyword", value: "new" }],
        }),
      ),
    );
  });

  it.each(
    (
      [
        [{ field: "/command", op: "keyword", value: "  " }],
        [{ field: "/command", op: "in", values: ["first", " "] }],
        [{ field: "/command", op: "in", values: ["first"], value: "stale" }],
        [{ field: "/command", op: "eq", value: "first", values: ["stale"] }],
        [],
      ] satisfies ToolRiskRule["conditions"][]
    ).map((conditions) => [conditions]),
  )("blocks empty or ambiguous conditions: %j", async (conditions) => {
    await editConditions(conditions);
    expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    expect(saveRule).not.toHaveBeenCalled();
  });

  it.each(["path_category", "path_glob"] as const)(
    "keeps legacy %s visible without rewriting it, but disables save",
    async (op) => {
      await editConditions([
        { field: "/target_path", op, value: "legacy-pattern" },
      ]);
      expect(
        screen.getByText("toolRisk.unsupportedCondition"),
      ).toBeInTheDocument();
      expect(screen.getByDisplayValue("legacy-pattern")).toBeInTheDocument();
      expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
      openOptions(conditionOp());
      expect(
        screen.queryByRole("option", { name: "toolRisk.op.path_category" }),
      ).toBeNull();
      expect(
        screen.queryByRole("option", { name: "toolRisk.op.path_glob" }),
      ).toBeNull();
      fireEvent.click(
        screen.getByRole("option", { name: "toolRisk.op.keyword" }),
      );
      expect(screen.getByLabelText("toolRisk.fieldConditionValue")).toHaveValue(
        "",
      );
      await expect(getRule.mock.results[0].value).resolves.toMatchObject({
        conditions: [{ field: "/target_path", op, value: "legacy-pattern" }],
      });
    },
  );

  it.each([
    [
      "/command",
      "remote_download_exec",
      ["remote_download_exec", "privilege_escalation", "destructive_sql"],
    ],
    [
      "/target_path",
      "sensitive_path",
      ["destructive_path", "sensitive_path", "security_config_path"],
    ],
    ["/destination", "exfil_destination", ["exfil_destination"]],
  ])(
    "filters detector options by the Rust field binding %s",
    async (field, value, detectors) => {
      await editConditions([
        { field: field as string, op: "detector", value: value as string },
      ]);
      openOptions(screen.getByLabelText("toolRisk.pickDetector"));
      expect(
        screen.getAllByRole("option").map((option) => option.textContent),
      ).toEqual(
        (detectors as string[]).map(
          (detector) => `toolRisk.detector.${detector}`,
        ),
      );
    },
  );

  it("clears an incompatible detector when the field changes", async () => {
    await editConditions([
      { field: "/command", op: "detector", value: "remote_download_exec" },
    ]);
    selectConditionField("toolRisk.conditionField.target_path");
    expect(
      screen.queryByText("toolRisk.detector.remote_download_exec"),
    ).toBeNull();
    expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
    openOptions(screen.getByLabelText("toolRisk.pickDetector"));
    fireEvent.click(
      screen.getByRole("option", { name: "toolRisk.detector.sensitive_path" }),
    );
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [
            { field: "/target_path", op: "detector", value: "sensitive_path" },
          ],
        }),
      ),
    );
  });

  it.each([
    { field: "/command", op: "detector", value: "sensitive_path" },
    { field: "/target_path", op: "detector", value: "future_detector" },
    { field: "/unsupported_field", op: "eq", value: "old" },
  ] satisfies ToolRiskRule["conditions"])(
    "blocks unsupported legacy condition %j without changing its stored value",
    async (condition) => {
      await editConditions([condition]);
      expect(
        screen.getByText("toolRisk.unsupportedCondition"),
      ).toBeInTheDocument();
      expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
      expect(fullRule.conditions).toEqual([
        { field: "/target_path", op: "detector", value: "sensitive_path" },
      ]);
    },
  );

  it("does not offer detectors for exact tool targets and blocks legacy detector conditions", async () => {
    await editConditions(
      [{ field: "/target_path", op: "detector", value: "sensitive_path" }],
      true,
    );
    expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
    expect(
      screen.getByText("toolRisk.unsupportedCondition"),
    ).toBeInTheDocument();
    openOptions(conditionOp());
    expect(
      screen.queryByRole("option", { name: "toolRisk.op.detector" }),
    ).toBeNull();
  });

  it("keeps a compatible free-text value when the field changes", async () => {
    await editConditions([{ field: "/command", op: "keyword", value: "ls" }]);
    selectConditionField("toolRisk.conditionField.target_path");
    // Both fields accept free text, so the existing value remains valid.
    expect(conditionValue()).toHaveValue("ls");
    expect(screen.getByTestId("tool-risk-save")).toBeEnabled();
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [{ field: "/target_path", op: "keyword", value: "ls" }],
        }),
      ),
    );
  });

  it("clears a stale set value when the new field uses a different value domain", async () => {
    await editConditions([
      { field: "/op_type", op: "eq", value: "shell_exec" },
    ]);
    selectConditionField("toolRisk.conditionField.command_class");
    // /op_type and /command_class have different domains; clear the incompatible value.
    expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
    openOptions(screen.getByLabelText("toolRisk.fieldCommandClass"));
    fireEvent.click(
      screen.getByRole("option", { name: "toolRisk.commandClass.script_exec" }),
    );
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    await waitFor(() =>
      expect(saveRule).toHaveBeenCalledWith(
        expect.objectContaining({
          conditions: [
            { field: "/command_class", op: "eq", value: "script_exec" },
          ],
        }),
      ),
    );
  });

  it("disables a detector after switching to a tool target without silently rewriting the condition", async () => {
    await editConditions([
      { field: "/target_path", op: "detector", value: "sensitive_path" },
    ]);
    openOptions(screen.getByLabelText("toolRisk.fieldTarget"));
    fireEvent.click(
      screen.getByRole("option", { name: "toolRisk.fieldToolRefs" }),
    );
    fireEvent.change(screen.getByLabelText("toolRisk.fieldToolRefs"), {
      target: {
        value: '[{"client":"claude","name":"read","definitionRef":"schema-1"}]',
      },
    });
    expect(screen.getByTestId("tool-risk-save")).toBeDisabled();
    expect(
      screen.getByText("toolRisk.unsupportedCondition"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("toolRisk.detector.sensitive_path"),
    ).toBeInTheDocument();
  });

  it("keeps the draft open and handles a rejected save without an unhandled promise", async () => {
    await editConditions([
      { field: "/command", op: "keyword", value: "custom" },
    ]);
    saveRule.mockRejectedValueOnce("tool_risk_builtin_duplicate");
    fireEvent.click(screen.getByTestId("tool-risk-save"));
    expect(
      await screen.findByText("toolRisk.ruleSaveErrorHint"),
    ).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByTestId("tool-risk-save")).toBeEnabled();
  });

  it("shows the additive-risk and builtin-duplicate boundary in the rule editor", () => {
    render(<ToolRiskRulesPanel />);
    fireEvent.click(screen.getByRole("button", { name: "toolRisk.addRule" }));
    expect(
      screen.getByText("toolRisk.customRuleBoundaryHint"),
    ).toBeInTheDocument();
  });
});

describe("database rule capability readiness", () => {
  const databaseRule: ToolRiskRuleItem = {
    ...customItem,
    id: "builtin.destructive_database_change",
    name: "Destructive database change",
    origin: "builtin",
    enabled: false,
    riskCategory: "destructive_database_change",
    reasonCode: "destructive_sql",
    selectorSummary: "operation:database_operation",
    revision: 1,
    builtinVersion: 1,
  };

  beforeEach(() => {
    events = [];
    toggleRule.mockClear();
  });

  it.each([false, true])(
    "marks database detection as not ready independently of enabled=%s",
    (enabled) => {
      items = [{ ...databaseRule, enabled }];
      render(<ToolRiskRulesPanel />);
      const row = screen
        .getByText("toolRisk.builtinRule.destructive_database_change.name")
        .closest("tr")!;

      expect(
        within(row).getByText("toolRisk.capabilityNotReady"),
      ).toBeInTheDocument();
      expect(
        within(row).getByText("toolRisk.databaseCapabilityHint"),
      ).toBeInTheDocument();
      expect(within(row).getByRole("switch")).toHaveAttribute(
        "aria-checked",
        String(enabled),
      );
    },
  );

  it("preserves the revision-aware toggle without claiming readiness", () => {
    items = [databaseRule];
    render(<ToolRiskRulesPanel />);
    const row = screen
      .getByText("toolRisk.builtinRule.destructive_database_change.name")
      .closest("tr")!;
    fireEvent.click(within(row).getByRole("switch"));

    expect(toggleRule).toHaveBeenCalledWith(databaseRule.id, true, 1);
    expect(
      within(row).getByText("toolRisk.databaseCapabilityHint"),
    ).toBeInTheDocument();
  });

  it("does not mark unrelated builtin or custom rules as not ready", () => {
    items = [
      { ...databaseRule, id: "builtin.sensitive_read", name: "Sensitive read" },
      {
        ...databaseRule,
        id: "custom-db",
        origin: "custom",
        name: "Custom DB rule",
      },
    ];
    render(<ToolRiskRulesPanel />);

    expect(
      screen.queryByText("toolRisk.capabilityNotReady"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("toolRisk.databaseCapabilityHint"),
    ).not.toBeInTheDocument();
  });
});

describe("P2-9 policy history entry and safe errors", () => {
  it("labels the persisted settings version as saved rather than active using the fallback", () => {
    items = [customItem];
    render(<ToolRiskRulesPanel />);

    expect(screen.getByText("已保存策略版本: 1")).toBeVisible();
    expect(screen.queryByText("生效策略版本: 1")).not.toBeInTheDocument();
  });

  it("opens policy history in a dialog and returns focus when closed", async () => {
    items = [customItem];
    render(<ToolRiskRulesPanel />);
    const trigger = screen.getByRole("button", {
      name: "toolRisk.policyChanges.entry",
    });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", {
      name: "toolRisk.policyChanges.title",
    });
    expect(dialog).toHaveAccessibleDescription(
      "toolRisk.policyChanges.description",
    );
    expect(dialog).toHaveClass("z-[110]", "overflow-hidden");
    expect(
      within(dialog).getByText("toolRisk.policyChanges.empty"),
    ).toBeVisible();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.close" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(trigger).toHaveFocus();

    fireEvent.click(trigger);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("opens clear confirmation above the history dialog and cancels back to it", () => {
    items = [customItem];
    render(<ToolRiskRulesPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.policyChanges.entry" }),
    );
    const history = screen.getByRole("dialog", {
      name: "toolRisk.policyChanges.title",
    });
    fireEvent.click(
      within(history).getByRole("button", {
        name: "toolRisk.policyChanges.clear",
      }),
    );
    const confirmation = screen.getByRole("dialog", {
      name: "toolRisk.policyChanges.clearConfirmTitle",
    });
    expect(confirmation).toHaveClass("z-[120]");
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    );
    expect(
      screen.getByRole("dialog", {
        name: "toolRisk.policyChanges.title",
      }),
    ).toBeVisible();
    expect(
      screen.queryByRole("dialog", {
        name: "toolRisk.policyChanges.clearConfirmTitle",
      }),
    ).toBeNull();
  });
  it("does not echo backend rule-load secrets", async () => {
    items = [customItem];
    getRule.mockRejectedValueOnce(new Error("secret rule content"));
    const { toast } = await import("sonner");
    render(<ToolRiskRulesPanel />);
    fireEvent.click(screen.getByTestId("tool-risk-edit-custom-1"));
    await waitFor(() =>
      expect(toast.error).toHaveBeenLastCalledWith("toolRisk.ruleLoadFailed"),
    );
    expect(screen.queryByTestId("tool-risk-save")).toBeNull();
  });
});
