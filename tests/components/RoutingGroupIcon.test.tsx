import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RoutingGroupIcon } from "@/components/routing/RoutingGroupIcon";

describe("RoutingGroupIcon", () => {
  it("stacks unique provider icons with a compact overflow count", () => {
    const { container } = render(
      <RoutingGroupIcon
        icons={[
          { icon: "openai", name: "OpenAI" },
          { icon: "claude", name: "Claude" },
          { icon: "gemini", name: "Gemini" },
          { icon: "deepseek", name: "DeepSeek" },
          { icon: "openai", name: "Second OpenAI" },
        ]}
      />,
    );
    expect(container.querySelectorAll(".mp-icon-disc")).toHaveLength(4);
    expect(screen.getByText("+1")).toBeInTheDocument();
    expect(container.querySelector(".lucide-box")).toBeNull();
  });
  it.each(["codex", "claude", "gemini"])(
    "uses the registered icon for %s",
    (client) => {
      const { container } = render(<RoutingGroupIcon client={client} />);
      const label =
        client === "codex"
          ? "Codex"
          : client === "claude"
            ? "Claude"
            : "Gemini";
      expect(screen.getByRole("img", { name: label })).toHaveAttribute(
        "width",
        "20",
      );
      expect(container.querySelector(".lucide-box")).toBeNull();
    },
  );

  it.each([undefined, "", "unknown-agent", "toString"])(
    "keeps the box for unrecognized client %s",
    (client) => {
      const { container } = render(<RoutingGroupIcon client={client} />);
      expect(container.querySelector(".lucide-box")).not.toBeNull();
      expect(screen.queryByRole("img")).not.toBeInTheDocument();
    },
  );
});
