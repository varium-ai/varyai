import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import postcss from "postcss";
import tailwindcss from "tailwindcss";
import { beforeEach, expect, it, vi } from "vitest";
import { AutoGroupProviders } from "@/components/routing/AutoGroupProviders";
import { intentRoutingApi } from "@/lib/api/intentRouting";

vi.mock("@/lib/api/intentRouting", () => ({
  intentRoutingApi: {
    autoGroupSkip: vi.fn(),
    setAutoGroupSkip: vi.fn(),
  },
}));

const providers = [
  "Synthetic primary",
  `Synthetic ${"long account ".repeat(12).trim()}`,
].map((name, index) => ({
  id: `provider-${index}`,
  name,
  taskEligible: true,
  classifierEligible: false,
}));

function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <div className="vary-routing">
        <div className="rt-gfound">
          <div className="txt">
            <AutoGroupProviders providers={providers} disabled={false} />
          </div>
        </div>
      </div>
    </QueryClientProvider>,
  );
  return client;
}

beforeEach(() => {
  vi.mocked(intentRoutingApi.autoGroupSkip).mockResolvedValue([]);
  vi.mocked(intentRoutingApi.setAutoGroupSkip).mockResolvedValue();
});

it("keeps the trigger compact and styles the portalled options independently of the routing page", async () => {
  const user = userEvent.setup();
  mount();
  const trigger = screen.getByRole("button", {
    name: "intentRouting.skipProviders",
  });
  await waitFor(() => expect(trigger).toBeEnabled());
  await user.click(trigger);
  const dialog = screen.getByRole("dialog");
  const row = screen
    .getByRole("checkbox", {
      name: providers[1].name,
    })
    .closest("label")!;
  const sheet = document.createElement("style");
  sheet.textContent = (
    await postcss([
      tailwindcss({
        content: [{ raw: document.body.innerHTML, extension: "html" }],
        corePlugins: { preflight: false },
      }),
    ]).process("@tailwind utilities;", { from: undefined })
  ).css;
  document.head.appendChild(sheet);
  try {
    expect(getComputedStyle(trigger).justifySelf).toBe("start");
    expect(getComputedStyle(trigger).maxWidth).toBe("100%");
    expect(dialog.closest(".vary-routing")).toBeNull();
    expect(parseFloat(getComputedStyle(dialog).paddingTop)).toBeGreaterThan(0);
    expect(getComputedStyle(dialog).fontSize).toBe("0.75rem");
    expect(getComputedStyle(row).fontSize).toBe(
      getComputedStyle(dialog).fontSize,
    );
    expect(getComputedStyle(dialog).maxWidth).not.toBe("");
    expect(parseFloat(getComputedStyle(row).minHeight)).toBeGreaterThan(0);
    expect(getComputedStyle(row.parentElement!).overflowY).toBe("auto");
    const name = screen.getByTitle(providers[1].name);
    expect(getComputedStyle(name).textOverflow).toBe("ellipsis");
    expect(dialog).toHaveAccessibleName("intentRouting.skipProviders");
    expect(dialog).toHaveAccessibleDescription("intentRouting.skipHint");
  } finally {
    sheet.remove();
  }
});

it("keeps multiple selections open and restores focus on Escape", async () => {
  const user = userEvent.setup();
  const client = mount();
  const trigger = screen.getByRole("button");
  await waitFor(() => expect(trigger).toBeEnabled());
  await user.click(trigger);
  await user.click(screen.getByRole("checkbox", { name: providers[0].name }));
  await waitFor(() =>
    expect(client.getQueryData(["routing-auto-group-skip"])).toEqual([
      "provider-0",
    ]),
  );
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  await user.click(screen.getByRole("checkbox", { name: providers[1].name }));
  await waitFor(() =>
    expect(client.getQueryData(["routing-auto-group-skip"])).toEqual([
      "provider-0",
      "provider-1",
    ]),
  );
  await user.click(screen.getByRole("checkbox", { name: providers[0].name }));
  await waitFor(() =>
    expect(intentRoutingApi.setAutoGroupSkip).toHaveBeenLastCalledWith([
      "provider-1",
    ]),
  );
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
});
