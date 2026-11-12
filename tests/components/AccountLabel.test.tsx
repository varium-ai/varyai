import { render } from "@testing-library/react";
import { expect, it } from "vitest";
import { AccountLabel, AccountMask } from "@/components/AccountLabel";

it("masks full and partly masked addresses without changing the surrounding label", () => {
  const text =
    "Codex · fixture+work@example.test · abc***gh@example.test · model-a";
  const view = render(
    <AccountMask masked>
      <AccountLabel title>{text}</AccountLabel>
    </AccountMask>,
  );
  expect(view.container.querySelectorAll(".account-mask")).toHaveLength(2);
  expect(view.container).toHaveTextContent("Codex ·");
  expect(view.container).toHaveTextContent("· model-a");
  expect(view.container.innerHTML).not.toContain("example.test");
  expect(
    view.container.querySelector("[title]")?.getAttribute("title"),
  ).toContain("@•••••••.••••");
  view.rerender(
    <AccountMask masked={false}>
      <AccountLabel title>{text}</AccountLabel>
    </AccountMask>,
  );
  expect(view.container).toHaveTextContent(text);
});

it("masks known account nicknames as whole names and carries the mask into nested scopes", () => {
  const view = render(
    <AccountMask masked names={["fixture-user", "用户甲"]}>
      <AccountMask masked names={["nested-user"]}>
        <AccountLabel title>
          {
            "Codex · fixture-user · 用户甲 · nested-user · fixture-user-project · model-a"
          }
        </AccountLabel>
      </AccountMask>
    </AccountMask>,
  );
  expect(view.container.querySelectorAll(".account-mask")).toHaveLength(3);
  expect(view.container).toHaveTextContent("Codex ·");
  expect(view.container).toHaveTextContent("fixture-user-project · model-a");
  expect(view.container).not.toHaveTextContent("用户甲");
  expect(view.container.innerHTML).not.toContain("nested-user");
  expect(
    view.container.querySelector("[title]")?.getAttribute("title"),
  ).toContain("•••••••••••• · •••");
});
