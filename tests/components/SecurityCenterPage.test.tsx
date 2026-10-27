// @vitest-environment jsdom
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import zh from "@/i18n/locales/zh.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import { SecurityCenterPage } from "@/components/security/SecurityCenterPage";

const updateCustomAction = vi.hoisted(() => vi.fn());
const saveRule = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const deleteRule = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("@/hooks/useSecurityAudit", () => ({
  useSecurityRules: () => ({
    rules: [
      {
        id: "rule-1",
        name: "Internal token",
        category: "custom",
        ruleType: "regex",
        pattern: "INTERNAL_[0-9]+",
        riskLevel: "HIGH",
        action: "redact",
        enabled: true,
        isBuiltin: false,
        description: "Internal token rule",
      },
      {
        id: "builtin-token",
        name: "Built-in token",
        category: "api_token",
        riskLevel: "HIGH",
        action: "block",
        enabled: true,
        isBuiltin: true,
      },
    ],
    isLoading: false,
    toggleRule: vi.fn(),
    saveRule,
    deleteRule,
    updateBuiltinAction: vi.fn(),
    updateCustomAction,
    isSaving: false,
  }),
}));
vi.mock("@/components/security/SecurityEventsPanel", () => ({
  SecurityEventsPanel: () => <div data-testid="unified-events" />,
}));

describe("SecurityCenterPage rules and language compatibility", () => {
  it("mounts the unified event panel without removing the existing rule editor", () => {
    render(<SecurityCenterPage />);
    expect(screen.getByTestId("unified-events")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "security.tabRules" }));
    expect(screen.getByText("security.ruleHeading")).toBeInTheDocument();
  });
  it.each([
    ["zh", zh],
    ["en", en],
    ["ja", ja],
    ["zh-TW", zhTW],
  ] as const)(
    "uses the same protective block wording in %s results and stats",
    (_, locale) => {
      expect(locale.security.resultBlocked).toBe(
        locale.securityStats.protectiveBlocked,
      );
    },
  );

  it("does not render the removed global policy controls", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));

    expect(
      screen.queryByText("security.globalDefaultAction"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("security.blockCriticalTitle"),
    ).not.toBeInTheDocument();
  });

  it("labels the redact rule action explicitly", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));

    expect(
      screen.getAllByRole("button", { name: "security.ruleActionRedact" }),
    ).toHaveLength(2);
  });

  it("shows and updates action buttons for custom rules", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));

    const customRuleRow = screen.getByTestId("security-rule-rule-1");
    expect(
      within(customRuleRow).queryByRole("button", {
        name: "security.ruleActionBlock",
      }),
    ).not.toBeInTheDocument();
    expect(
      within(customRuleRow)
        .getAllByRole("button")
        .slice(0, 3)
        .map((button) => button.textContent),
    ).toEqual([
      "security.ruleActionRedact",
      "security.ruleActionAlert",
      "security.ruleActionAllow",
    ]);
    expect(
      within(customRuleRow).getByRole("button", {
        name: "security.ruleActionAlert",
      }),
    ).toBeInTheDocument();
    expect(
      within(customRuleRow).getByRole("button", {
        name: "security.ruleActionRedact",
      }),
    ).toBeInTheDocument();
    expect(
      within(customRuleRow).getByRole("button", {
        name: "security.ruleActionAllow",
      }),
    ).toBeInTheDocument();

    fireEvent.click(
      within(customRuleRow).getByRole("button", {
        name: "security.ruleActionAlert",
      }),
    );
    expect(updateCustomAction).toHaveBeenCalledWith("rule-1", "audit");
  });

  it("shows hide and send as selected for a legacy blocking rule", () => {
    render(<SecurityCenterPage />);
    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));
    const row = screen.getByTestId("security-rule-builtin-token");
    expect(
      within(row).queryByRole("button", { name: "security.ruleActionBlock" }),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", { name: "security.ruleActionRedact" }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("offers hide and send first and no block option in the rule editor", () => {
    render(<SecurityCenterPage />);
    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));
    fireEvent.click(screen.getByRole("button", { name: "security.addRule" }));
    const actionField = screen.getByText(
      "security.dialogAction",
    ).parentElement!;
    fireEvent.click(within(actionField).getByRole("combobox"));
    expect(
      screen.getAllByRole("option").map((option) => option.textContent),
    ).toEqual([
      "security.dialogActionRedact",
      "security.dialogActionAudit",
      "security.dialogActionAllow",
    ]);
  });

  it("requires confirmation before deleting a custom rule", () => {
    deleteRule.mockClear();
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));
    fireEvent.click(
      within(screen.getByTestId("security-rule-rule-1")).getByRole("button", {
        name: "common.delete",
      }),
    );

    expect(deleteRule).not.toHaveBeenCalled();
    expect(
      screen.getByText("security.deleteRuleConfirmTitle"),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "security.deleteRuleConfirm" }),
    );
    expect(deleteRule).toHaveBeenCalledWith("rule-1");
  });

  it("opens existing custom rules in the editor and preserves their id", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));
    fireEvent.click(
      within(screen.getByTestId("security-rule-rule-1")).getByRole("button", {
        name: "common.edit",
      }),
    );

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Internal token")).toBeInTheDocument();
    expect(screen.getByDisplayValue("INTERNAL_[0-9]+")).toBeInTheDocument();
    expect(screen.getByText("security.dialogEditTitle")).toBeInTheDocument();

    fireEvent.change(screen.getByDisplayValue("Internal token"), {
      target: { value: "Updated token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "security.saveRule" }));

    expect(saveRule).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "rule-1",
        name: "Updated token",
        ruleType: "regex",
        pattern: "INTERNAL_[0-9]+",
        action: "redact",
      }),
    );
  });

  it("keeps the add-rule form inset inside a bounded dialog", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: "security.tabRules" }));
    fireEvent.click(screen.getByRole("button", { name: "security.addRule" }));

    expect(screen.getByRole("dialog")).toHaveClass("overflow-hidden", "z-[60]");
    expect(screen.getByTestId("security-rule-form")).toHaveClass(
      "px-6",
      "py-5",
      "overflow-y-auto",
    );
  });
});
