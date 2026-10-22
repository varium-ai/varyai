import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}));
let useRealHooks = false;
import type { CommandListEntry } from "@/types/toolRisk";
import {
  ShellCommandListsPanel,
  ShellCommandListDialog,
} from "./ShellCommandListsPanel";
import zh from "@/i18n/locales/zh.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
const saveEntry = vi.fn();
const deleteEntry = vi.fn();
const preview = vi.fn();
let entries: CommandListEntry[] = [];
vi.mock("@/hooks/useToolRisk", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/hooks/useToolRisk")>();
  return {
    useToolRiskCommandLists: () =>
      useRealHooks
        ? actual.useToolRiskCommandLists()
        : {
            entries,
            isLoading: false,
            isError: false,
            saveEntry,
            deleteEntry,
            isSaving: false,
            isDeleting: false,
            refetch: vi.fn(),
          },
  };
});
vi.mock("@/lib/api", async () => ({
  toolRiskApi: {
    ...(await import("@/lib/api/toolRisk")).toolRiskApi,
    previewCommandPattern: (...args: unknown[]) => preview(...args),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
const entry: CommandListEntry = {
  id: "entry-1",
  revision: 8,
  listType: "whitelist",
  matchKind: "prefix",
  pattern: "git status",
  enabled: true,
  severity: "none",
  riskCategory: "remote_code_execution",
};
beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
});
const change = (name: string, value: string) => {
  const trigger = screen.getByLabelText(`toolRisk.commandLists.${name}`);
  if (name === "pattern") {
    fireEvent.change(trigger, { target: { value } });
    return;
  }
  const prefix = {
    listType: "toolRisk.commandLists",
    matchKind: "toolRisk.commandLists.kind",
    severity: "toolRisk.severity",
    riskCategory: "toolRisk.category",
  }[name];
  fireEvent.keyDown(trigger, { key: "Enter" });
  fireEvent.click(
    within(screen.getByRole("listbox")).getByRole("option", {
      name: `${prefix}.${value}`,
    }),
  );
};
const confirm = () =>
  fireEvent.click(screen.getByLabelText("toolRisk.commandLists.confirmScope"));

describe("ShellCommandListsPanel", () => {
  beforeEach(() => {
    entries = [];
    saveEntry.mockReset().mockResolvedValue("entry-1");
    deleteEntry.mockReset().mockResolvedValue(true);
    preview.mockReset().mockResolvedValue([]);
  });
  it("opens the shared dropdown by keyboard and switches to blacklist fields", () => {
    render(<ShellCommandListDialog open onOpenChange={vi.fn()} />);
    const trigger = screen.getByRole("combobox", {
      name: "toolRisk.commandLists.listType",
    });
    fireEvent.keyDown(trigger, { key: "Enter" });
    const options = screen.getByRole("listbox");
    fireEvent.click(
      within(options).getByRole("option", {
        name: "toolRisk.commandLists.blacklist",
      }),
    );
    expect(trigger).toHaveTextContent("toolRisk.commandLists.blacklist");
    expect(
      screen.getByRole("combobox", {
        name: "toolRisk.commandLists.severity",
      }),
    ).toHaveTextContent("toolRisk.severity.medium");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });
  it.each(["whitelist", "blacklist"] as const)(
    "keeps the %s title and actions outside the scrollable editor above app navigation",
    (initialListType) => {
      render(
        <ShellCommandListDialog
          open
          initialListType={initialListType}
          onOpenChange={vi.fn()}
        />,
      );

      const dialog = screen.getByRole("dialog");
      const editor = screen.getByRole("region", {
        name: "toolRisk.commandLists.add",
      });
      expect(editor).toContainElement(
        screen.getByLabelText("toolRisk.commandLists.pattern"),
      );
      expect(editor).not.toContainElement(
        screen.getByRole("heading", { name: "toolRisk.commandLists.add" }),
      );
      for (const name of [
        "common.cancel",
        "toolRisk.commandLists.confirmSave",
      ]) {
        const action = screen.getByRole("button", { name });
        expect(dialog).toContainElement(action);
        expect(editor).not.toContainElement(action);
      }
      expect(editor).toHaveClass("min-h-0", "overflow-y-auto");
      expect(dialog).not.toHaveClass("overflow-y-auto");
      const layer = dialog.className.match(/(?:^|\s)z-\[(\d+)\](?:\s|$)/);
      expect(Number(layer?.[1] ?? 0)).toBeGreaterThan(70);
    },
  );
  it("creates a simple whitelist only after scope confirmation, with classifier restrictions", async () => {
    render(<ShellCommandListsPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.add" }),
    );
    expect(
      screen.getByLabelText("toolRisk.commandLists.matchKind"),
    ).toHaveTextContent("toolRisk.commandLists.kind.prefix");
    change("pattern", "git status");
    expect(
      screen.getByText("toolRisk.commandLists.whitelistHint"),
    ).toBeTruthy();
    expect(screen.getByText("toolRisk.commandLists.scope.prefix")).toBeTruthy();
    const save = screen.getByRole("button", {
      name: "toolRisk.commandLists.confirmSave",
    });
    expect(save).toBeDisabled();
    confirm();
    fireEvent.click(save);
    await waitFor(() =>
      expect(saveEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          listType: "whitelist",
          matchKind: "prefix",
          pattern: "git status",
          severity: "none",
          enabled: true,
        }),
      ),
    );
  });
  it("displays overlapping black/white entries and supports blacklist severity and category", async () => {
    entries = [entry];
    render(<ShellCommandListsPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.add" }),
    );
    change("listType", "blacklist");
    change("pattern", "git status");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "toolRisk.commandLists.conflictHint",
    );
    change("severity", "high");
    change("riskCategory", "data_exfiltration");
    confirm();
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.confirmSave" }),
    );
    await waitFor(() =>
      expect(saveEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          severity: "high",
          riskCategory: "data_exfiltration",
          listType: "blacklist",
        }),
      ),
    );
  });
  it("blocks invalid regex, validates via backend preview and expires validation when edited", async () => {
    render(<ShellCommandListsPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.add" }),
    );
    fireEvent.click(screen.getByText("toolRisk.commandLists.advanced"));
    change("matchKind", "regex");
    change("pattern", "[");
    confirm();
    expect(
      screen.getByRole("button", { name: "toolRisk.commandLists.confirmSave" }),
    ).toBeDisabled();
    preview.mockRejectedValueOnce("invalid regex");
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.preview" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "toolRisk.commandLists.previewFailed",
      ),
    );
    expect(saveEntry).not.toHaveBeenCalled();
    change("pattern", "^git");
    preview.mockResolvedValueOnce([
      { eventId: 7, matched: true, summaryOnly: true },
      { eventId: 8, matched: false, summaryOnly: false },
    ]);
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.preview" }),
    );
    await waitFor(() =>
      expect(preview).toHaveBeenLastCalledWith("regex", "^git"),
    );
    expect(
      await screen.findByText("toolRisk.commandLists.summaryOnly"),
    ).toBeTruthy();
    expect(screen.getByText("toolRisk.commandLists.previewHint")).toBeTruthy();
    confirm();
    expect(
      screen.getByRole("button", { name: "toolRisk.commandLists.confirmSave" }),
    ).toBeEnabled();
    change("pattern", "^python");
    expect(
      screen.getByRole("button", { name: "toolRisk.commandLists.confirmSave" }),
    ).toBeDisabled();
  });
  it("ignores a stale successful preview after pattern changes", async () => {
    let resolve!: (value: unknown[]) => void;
    preview.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    render(<ShellCommandListsPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.add" }),
    );
    fireEvent.click(screen.getByText("toolRisk.commandLists.advanced"));
    change("matchKind", "regex");
    change("pattern", "^git");
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.preview" }),
    );
    change("pattern", "[");
    resolve([]);
    confirm();
    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "toolRisk.commandLists.confirmSave",
        }),
      ).toBeDisabled(),
    );
  });
  it("passes revision for disable, edit and confirmed deletion", async () => {
    entries = [entry];
    render(<ShellCommandListsPanel />);
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(saveEntry).toHaveBeenCalledWith({
        id: entry.id,
        listType: entry.listType,
        matchKind: entry.matchKind,
        pattern: entry.pattern,
        severity: entry.severity,
        riskCategory: entry.riskCategory,
        enabled: false,
        expectedRevision: 8,
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
    change("pattern", "git diff");
    confirm();
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.confirmSave" }),
    );
    await waitFor(() =>
      expect(saveEntry).toHaveBeenLastCalledWith(
        expect.objectContaining({
          id: "entry-1",
          expectedRevision: 8,
          pattern: "git diff",
        }),
      ),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    expect(deleteEntry).not.toHaveBeenCalled();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.delete",
      }),
    );
    await waitFor(() => expect(deleteEntry).toHaveBeenCalledWith("entry-1", 8));
  });
  it("keeps the dialog and explains revision conflicts without silent overwrite", async () => {
    entries = [entry];
    saveEntry.mockRejectedValueOnce("revision conflict");
    render(<ShellCommandListsPanel />);
    fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
    confirm();
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.confirmSave" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "toolRisk.commandLists.mutationFailed",
    );
    expect(screen.getByRole("dialog")).toBeTruthy();
  });
  it.each([
    "python -c '[REDACTED]'",
    "unknown-program arg…",
    "unknown-program arg...",
    "unknown-program <redacted>",
  ])(
    "treats audit summaries as suggestions and requires fresh exact input: %s",
    async (commandSummary) => {
      render(
        <ShellCommandListDialog
          open
          onOpenChange={vi.fn()}
          initialListType="whitelist"
          commandSummary={commandSummary}
        />,
      );
      expect(saveEntry).not.toHaveBeenCalled();
      expect(
        screen.getByText("toolRisk.commandLists.summaryHint"),
      ).toBeTruthy();
      expect(
        screen.getByText("toolRisk.commandLists.suggestionHint"),
      ).toBeTruthy();
      expect(
        screen.getByLabelText("toolRisk.commandLists.matchKind"),
      ).toHaveTextContent("toolRisk.commandLists.kind.prefix");
      change("matchKind", "exact");
      expect(
        screen.getByLabelText("toolRisk.commandLists.pattern"),
      ).toHaveValue("");
      change("pattern", commandSummary);
      confirm();
      expect(
        screen.getByRole("button", {
          name: "toolRisk.commandLists.confirmSave",
        }),
      ).toBeDisabled();
      change("pattern", "git status --short");
      confirm();
      fireEvent.click(
        screen.getByRole("button", {
          name: "toolRisk.commandLists.confirmSave",
        }),
      );
      await waitFor(() =>
        expect(saveEntry).toHaveBeenCalledWith(
          expect.objectContaining({
            matchKind: "exact",
            pattern: "git status --short",
          }),
        ),
      );
    },
  );
  it("provides identical nonempty locale keys for the new UI in all four languages", () => {
    const flatten = (obj: object, prefix = ""): string[] =>
      Object.entries(obj).flatMap(([key, value]) =>
        typeof value === "object"
          ? flatten(value, `${prefix}${key}.`)
          : [`${prefix}${key}`],
      );
    const reference = flatten(en.toolRisk.commandLists).sort();
    expect(reference.length).toBeGreaterThan(25);
    for (const locale of [zh, en, ja, zhTW]) {
      expect(flatten(locale.toolRisk.commandLists).sort()).toEqual(reference);
      expect(Object.values(locale.toolRisk.commandClass)).toHaveLength(8);
    }
  });
});

describe("advanced list boundaries", () => {
  beforeEach(() => {
    entries = [entry];
    saveEntry.mockReset().mockResolvedValue("entry-1");
    deleteEntry.mockReset().mockResolvedValue(true);
    preview.mockReset().mockResolvedValue([]);
  });
  it("requires confirmation again when scope changes and warns keyword whitelists cannot exempt compound scripts", () => {
    render(<ShellCommandListsPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "toolRisk.commandLists.add" }),
    );
    change("pattern", "git status");
    confirm();
    expect(
      screen.getByRole("button", { name: "toolRisk.commandLists.confirmSave" }),
    ).toBeEnabled();
    fireEvent.click(screen.getByText("toolRisk.commandLists.advanced"));
    change("matchKind", "keyword");
    expect(
      screen.getByText("toolRisk.commandLists.scope.keyword"),
    ).toBeTruthy();
    expect(
      screen.getByText("toolRisk.commandLists.whitelistHint"),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "toolRisk.commandLists.confirmSave" }),
    ).toBeDisabled();
    change("matchKind", "program");
    expect(screen.getByLabelText("toolRisk.commandLists.pattern")).toHaveValue(
      "git",
    );
    expect(
      screen.getByText("toolRisk.commandLists.scope.program"),
    ).toBeTruthy();
  });
  it("does not close the delete confirmation or retry without the original revision after a conflict", async () => {
    deleteEntry.mockRejectedValueOnce("revision conflict");
    render(<ShellCommandListsPanel />);
    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.delete",
      }),
    );
    await waitFor(() => expect(deleteEntry).toHaveBeenCalledWith("entry-1", 8));
    expect(
      await within(screen.getByRole("dialog")).findByRole("alert"),
    ).toHaveTextContent("toolRisk.commandLists.mutationFailed");
    expect(deleteEntry).toHaveBeenCalledTimes(1);
  });
});

describe("risk boundary explanations", () => {
  beforeEach(() => {
    entries = [];
  });

  it.each(["prefix", "program"])(
    "explains per-segment whitelist matching for %s",
    (kind) => {
      render(<ShellCommandListDialog open onOpenChange={vi.fn()} />);
      change("matchKind", kind);
      expect(
        screen.getByText("toolRisk.commandLists.segmentedWhitelistHint"),
      ).toBeInTheDocument();
      expect(
        screen.queryByText("toolRisk.commandLists.singleSegmentWhitelistHint"),
      ).toBeNull();
      change("listType", "blacklist");
      expect(
        screen.queryByText("toolRisk.commandLists.segmentedWhitelistHint"),
      ).toBeNull();
    },
  );

  it.each(["exact", "keyword", "regex"])(
    "explains the single unknown segment restriction for %s",
    (kind) => {
      render(<ShellCommandListDialog open onOpenChange={vi.fn()} />);
      if (kind !== "exact")
        fireEvent.click(screen.getByText("toolRisk.commandLists.advanced"));
      change("matchKind", kind);
      expect(
        screen.getByText("toolRisk.commandLists.singleSegmentWhitelistHint"),
      ).toBeInTheDocument();
      expect(
        screen.queryByText("toolRisk.commandLists.segmentedWhitelistHint"),
      ).toBeNull();
    },
  );

  it.each([zh, en, ja, zhTW])(
    "provides localized editor hints and fixed-code errors without outdated whitelist claims",
    (locale) => {
      const risk = locale.toolRisk as unknown as Record<string, unknown>;
      for (const name of [
        "customRuleBoundaryHint",
        "unsupportedCondition",
        "conditionValueRequired",
        "invalidConditionShape",
        "fieldConditionOp",
        "fieldConditionField",
        "fieldConditionValue",
        "fieldConditionValues",
        "conditionValuesHint",
        "ruleSaveErrorHint",
      ]) {
        expect(risk[name]).toEqual(expect.any(String));
        expect((risk[name] as string).trim().length).toBeGreaterThan(0);
      }
      for (const name of ["builtinDuplicate", "riskSeverityRequired"]) {
        expect(risk[name]).toEqual(expect.any(String));
        expect((risk[name] as string)?.trim().length).toBeGreaterThan(0);
      }
      const lists = risk.commandLists as Record<string, string>;
      for (const name of [
        "segmentedWhitelistHint",
        "singleSegmentWhitelistHint",
        "whitelistHint",
        "description",
      ]) {
        expect(lists[name]).toEqual(expect.any(String));
        expect(lists[name]?.trim().length).toBeGreaterThan(0);
      }
      expect(lists.whitelistHint).toContain("unknown");
      expect(lists.singleSegmentWhitelistHint).toContain("unknown");
      expect(lists.description).toContain("unknown");
      expect(locale.toolRisk.riskSeverityRequired).not.toMatch(
        /只读分类|唯讀分類|read-only classification|読み取り専用分類/,
      );
    },
  );

  it("states the additive-risk and matching boundary explicitly in English", () => {
    const risk = en.toolRisk as unknown as Record<string, unknown>;
    expect(risk.customRuleBoundaryHint).toMatch(/add.*risk/i);
    expect(risk.customRuleBoundaryHint).toMatch(/override.*downgrade/i);
    expect(risk.customRuleBoundaryHint).toMatch(/duplicat/i);
    const lists = risk.commandLists as Record<string, string>;
    expect(lists.whitelistHint).toMatch(
      /script.*network.*mutation.*privilege/i,
    );
    expect(lists.segmentedWhitelistHint).toMatch(/segment/i);
    expect(lists.singleSegmentWhitelistHint).toMatch(/single.*unknown/i);
    expect(lists.singleSegmentWhitelistHint).toMatch(/whole.*other segments/i);
  });
});

describe("Phase 2 Program scope and conservative previews", () => {
  beforeEach(() => {
    entries = [];
    saveEntry.mockReset().mockResolvedValue("entry-1");
    preview.mockReset().mockResolvedValue([]);
  });

  it("switches Program hints by list type and resets confirmation when the scope changes", () => {
    render(<ShellCommandListDialog open onOpenChange={vi.fn()} />);
    change("pattern", "hostinfo");
    change("matchKind", "program");
    const scope = within(screen.getByTestId("command-list-scope"));
    const save = screen.getByRole("button", {
      name: "toolRisk.commandLists.confirmSave",
    });
    const confirmation = screen.getByLabelText(
      "toolRisk.commandLists.confirmScope",
    );
    expect(
      scope.getByText("toolRisk.commandLists.scope.program"),
    ).toBeVisible();
    expect(
      scope.getByText("toolRisk.commandLists.whitelistProgramHint"),
    ).toBeVisible();
    expect(
      scope.queryByText("toolRisk.commandLists.blacklistProgramHint"),
    ).toBeNull();
    expect(
      scope.getByText("toolRisk.commandLists.whitelistHint"),
    ).toBeVisible();
    confirm();
    expect(save).toBeEnabled();

    change("listType", "blacklist");
    expect(confirmation).not.toBeChecked();
    expect(save).toBeDisabled();
    expect(
      scope.getByText("toolRisk.commandLists.scope.program"),
    ).toBeVisible();
    expect(
      scope.getByText("toolRisk.commandLists.blacklistProgramHint"),
    ).toBeVisible();
    expect(
      scope.queryByText("toolRisk.commandLists.whitelistProgramHint"),
    ).toBeNull();
    expect(scope.queryByText("toolRisk.commandLists.whitelistHint")).toBeNull();
    confirm();
    expect(save).toBeEnabled();

    change("listType", "whitelist");
    expect(confirmation).not.toBeChecked();
    expect(save).toBeDisabled();
    expect(
      scope.getByText("toolRisk.commandLists.whitelistProgramHint"),
    ).toBeVisible();
    confirm();
    change("matchKind", "prefix");
    expect(confirmation).not.toBeChecked();
    expect(save).toBeDisabled();
    expect(
      scope.queryByText("toolRisk.commandLists.whitelistProgramHint"),
    ).toBeNull();
    expect(
      scope.queryByText("toolRisk.commandLists.blacklistProgramHint"),
    ).toBeNull();
  });

  it.each(["blacklist", "whitelist"] as const)(
    "shows only the %s Program hint when editing an existing entry",
    (listType) => {
      render(
        <ShellCommandListDialog
          open
          onOpenChange={vi.fn()}
          entry={{
            ...entry,
            listType,
            matchKind: "program",
            pattern: "whoami",
          }}
        />,
      );
      expect(
        screen.getByText(`toolRisk.commandLists.${listType}ProgramHint`),
      ).toBeVisible();
      const otherType = listType === "blacklist" ? "whitelist" : "blacklist";
      expect(
        screen.queryByText(`toolRisk.commandLists.${otherType}ProgramHint`),
      ).toBeNull();
    },
  );

  it.each(["prefix", "exact", "keyword", "regex"])(
    "does not show Program basename hints for %s in either list",
    (kind) => {
      render(<ShellCommandListDialog open onOpenChange={vi.fn()} />);
      if (kind === "keyword" || kind === "regex")
        fireEvent.click(screen.getByText("toolRisk.commandLists.advanced"));
      change("matchKind", kind);
      for (const listType of ["blacklist", "whitelist"]) {
        change("listType", listType);
        expect(
          screen.getByText(`toolRisk.commandLists.scope.${kind}`),
        ).toBeVisible();
        expect(
          screen.queryByText("toolRisk.commandLists.blacklistProgramHint"),
        ).toBeNull();
        expect(
          screen.queryByText("toolRisk.commandLists.whitelistProgramHint"),
        ).toBeNull();
      }
    },
  );

  it.each([
    ["blacklist", "whoami"],
    ["whitelist", "hostinfo"],
  ] as const)(
    "keeps the conservative %s preview API literal and list-type agnostic",
    async (listType, pattern) => {
      preview.mockResolvedValueOnce([
        { eventId: 17, matched: false, summaryOnly: true },
      ]);
      render(
        <ShellCommandListDialog
          open
          onOpenChange={vi.fn()}
          initialListType={listType}
        />,
      );
      change("pattern", pattern);
      change("matchKind", "program");
      expect(
        screen.getByText("toolRisk.commandLists.previewHint"),
      ).toBeVisible();
      fireEvent.click(
        screen.getByRole("button", { name: "toolRisk.commandLists.preview" }),
      );
      expect(
        await screen.findByText("toolRisk.commandLists.notMatched"),
      ).toBeVisible();
      expect(
        screen.getByText("toolRisk.commandLists.summaryOnly"),
      ).toBeVisible();
      expect(preview.mock.calls).toEqual([["program", pattern]]);
      expect(
        screen.getByText("toolRisk.commandLists.previewHint"),
      ).toBeVisible();
      expect(saveEntry).not.toHaveBeenCalled();
    },
  );

  it.each([
    {
      language: "zh",
      locale: zh,
      literal: /完整.*字面量/,
      basename: /最后.*路径组件/,
      exact: /精确.*执行词/,
      prefix: /不.*路径归一化/,
      estimate: /字面量.*保守/,
      underestimate: /可能低估.*黑名单.*路径/,
      replay: /不是.*运行时精确回放/,
      risk: /不.*豁免.*风险/,
    },
    {
      language: "en",
      locale: en,
      literal: /complete literal executable token/i,
      basename: /last path component/i,
      exact: /exact.*executable token/i,
      prefix: /no path normalization/i,
      estimate: /conservative literal estimate/i,
      underestimate: /underestimate.*blacklist.*path/i,
      replay: /not.*exact runtime replay/i,
      risk: /not.*exempt.*risk/i,
    },
    {
      language: "ja",
      locale: ja,
      literal: /完全なリテラル.*実行トークン/,
      basename: /最後のパス要素/,
      exact: /実行トークン.*完全一致/,
      prefix: /パス.*正規化しません/,
      estimate: /保守的なリテラル推定/,
      underestimate: /ブラックリスト.*パス.*過小評価/,
      replay: /実行時の正確な再現ではなく/,
      risk: /リスク.*免除しません/,
    },
    {
      language: "zh-TW",
      locale: zhTW,
      literal: /完整.*字面量/,
      basename: /最後.*路徑元件/,
      exact: /精確.*執行詞/,
      prefix: /不.*路徑正規化/,
      estimate: /字面量.*保守/,
      underestimate: /可能低估.*黑名單.*路徑/,
      replay: /不是.*執行時精確重現/,
      risk: /不.*豁免.*風險/,
    },
  ])(
    "localizes the matching and preview boundaries in $language",
    (testCase) => {
      const lists = testCase.locale.toolRisk.commandLists as unknown as Record<
        string,
        string
      >;
      expect(lists.previewHint).toContain("20");
      expect(lists.previewHint).toMatch(testCase.estimate);
      expect(lists.previewHint).toMatch(testCase.underestimate);
      expect(lists.previewHint).toMatch(testCase.replay);
      for (const name of ["blacklistProgramHint", "whitelistProgramHint"]) {
        expect(lists[name], `${testCase.language}: ${name}`).toEqual(
          expect.any(String),
        );
        expect(lists[name]?.trim()).toBeTruthy();
      }
      expect(lists.blacklistProgramHint).toMatch(testCase.literal);
      expect(lists.blacklistProgramHint).toMatch(testCase.basename);
      expect(lists.blacklistProgramHint).toContain("basename");
      for (const token of ["whoami", "/usr/bin/whoami", "./whoami"])
        expect(lists.blacklistProgramHint).toContain(token);
      expect(lists.whitelistProgramHint).toMatch(testCase.exact);
      expect(lists.whitelistProgramHint).toMatch(testCase.risk);
      for (const token of ["hostinfo", "/usr/bin/hostinfo", "./hostinfo"])
        expect(lists.whitelistProgramHint).toContain(token);
      expect(testCase.locale.toolRisk.commandLists.scope.prefix).toMatch(
        testCase.prefix,
      );
    },
  );
});

describe("P2-9 command list UI real receipt integration", () => {
  beforeEach(() => {
    useRealHooks = true;
    vi.mocked(invoke).mockReset();
  });
  afterEach(() => {
    useRealHooks = false;
  });
  it.each(["applied", "apply_failed"])(
    "closes a persisted %s save while consuming the receipt warning",
    async (runtimeStatus) => {
      const close = vi.fn();
      vi.mocked(invoke).mockImplementation(async (command) =>
        command === "get_tool_risk_command_lists"
          ? []
          : {
              value: "shell-list-12345678-1234-4234-8234-123456789abc",
              persistedPolicyVersion: 9,
              auditStatus:
                runtimeStatus === "apply_failed"
                  ? "audit_unavailable"
                  : "recorded",
              runtimeStatus,
            },
      );
      const client = new QueryClient({
        defaultOptions: {
          queries: { retry: false },
          mutations: { retry: false },
        },
      });
      render(
        <QueryClientProvider client={client}>
          <ShellCommandListDialog open onOpenChange={close} />
        </QueryClientProvider>,
      );
      change("pattern", "git status");
      confirm();
      const save = screen.getByRole("button", {
        name: "toolRisk.commandLists.confirmSave",
      });
      await waitFor(() => expect(save).toBeEnabled());
      fireEvent.click(save);
      await waitFor(() => expect(close).toHaveBeenCalledWith(false));
      expect(invoke).toHaveBeenCalledWith("save_tool_risk_command_list", {
        input: expect.objectContaining({
          pattern: "git status",
          listType: "whitelist",
          matchKind: "prefix",
          severity: "none",
        }),
      });
      if (runtimeStatus === "apply_failed") {
        expect(toast.warning).toHaveBeenCalledWith("toolRisk.savedNotApplied");
        expect(toast.warning).toHaveBeenCalledWith("toolRisk.auditUnavailable");
      } else expect(toast.warning).not.toHaveBeenCalled();
      expect(screen.queryByRole("alert")).toBeNull();
    },
  );
  it("preserves the editor on failed persistence without echoing the backend secret", async () => {
    const close = vi.fn();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_tool_risk_command_lists") return [];
      throw new Error("secret pattern fixture");
    });
    const client = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    render(
      <QueryClientProvider client={client}>
        <ShellCommandListDialog open onOpenChange={close} />
      </QueryClientProvider>,
    );
    change("pattern", "git status");
    confirm();
    const save = screen.getByRole("button", {
      name: "toolRisk.commandLists.confirmSave",
    });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "toolRisk.commandLists.mutationFailed",
    );
    expect(close).not.toHaveBeenCalled();
    expect(document.body.textContent).not.toContain("secret");
    expect(toast.error).toHaveBeenCalledWith("toolRisk.policyWriteFailed");
  });
});
