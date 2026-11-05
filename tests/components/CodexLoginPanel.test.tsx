import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { CodexLoginPanel } from "@/components/providers/CodexLoginPanel";
it("shows the complete inline browser login panel and wires its actions", () => {
  const actions = {
    onCancel: vi.fn(),
    onReopen: vi.fn(),
    onCopy: vi.fn(),
    onDevice: vi.fn(),
    onImport: vi.fn(),
    onProxy: vi.fn(),
    onSubmit: vi.fn(),
  };
  render(
    <CodexLoginPanel
      mode="browser"
      url="https://auth.openai.com/oauth/authorize?state=synthetic"
      proxy=""
      {...actions}
    />,
  );
  expect(screen.getByText("codexBrowser.title")).toBeVisible();
  expect(
    screen.getByRole("textbox", { name: "codexBrowser.authorizationUrl" }),
  ).toHaveValue("https://auth.openai.com/oauth/authorize?state=synthetic");
  fireEvent.click(screen.getByRole("button", { name: "codexBrowser.reopen" }));
  expect(actions.onReopen).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "common.copy" }));
  expect(actions.onCopy).toHaveBeenCalledOnce();
  fireEvent.click(
    screen.getByRole("button", { name: "codexBrowser.deviceWay" }),
  );
  expect(actions.onDevice).toHaveBeenCalledOnce();
  fireEvent.click(
    screen.getByRole("button", { name: "codexBrowser.importWay" }),
  );
  expect(actions.onImport).toHaveBeenCalledOnce();
  const callback = screen.getByRole("textbox", {
    name: "codexBrowser.callbackUrl",
  });
  const finish = screen.getByRole("button", { name: "codexBrowser.finish" });
  expect(finish).toBeDisabled();
  fireEvent.change(callback, {
    target: {
      value:
        "http://localhost:1455/auth/callback?code=synthetic&state=synthetic",
    },
  });
  fireEvent.click(finish);
  expect(actions.onSubmit).toHaveBeenCalledWith(
    "http://localhost:1455/auth/callback?code=synthetic&state=synthetic",
  );
  fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
  expect(actions.onCancel).toHaveBeenCalledOnce();
});
it("supports direct and custom proxies without changing global settings", () => {
  const props = {
    mode: "browser" as const,
    proxy: "",
    onCancel: vi.fn(),
    onReopen: vi.fn(),
    onCopy: vi.fn(),
    onDevice: vi.fn(),
    onImport: vi.fn(),
    onProxy: vi.fn(),
    onSubmit: vi.fn(),
  };
  render(<CodexLoginPanel {...props} />);
  fireEvent.change(
    screen.getByRole("combobox", { name: "codexBrowser.proxy" }),
    { target: { value: "direct" } },
  );
  expect(props.onProxy).toHaveBeenCalledWith("direct");
  fireEvent.change(
    screen.getByRole("combobox", { name: "codexBrowser.proxy" }),
    { target: { value: "custom" } },
  );
  fireEvent.change(
    screen.getByRole("textbox", { name: "codexBrowser.proxyUrl" }),
    { target: { value: "http://127.0.0.1:7890" } },
  );
  fireEvent.click(
    screen.getByRole("button", { name: "codexBrowser.applyProxy" }),
  );
  expect(props.onProxy).toHaveBeenCalledWith("http://127.0.0.1:7890");
});
it("keeps a custom proxy editable when a restarted login panel mounts", () => {
  const onProxy = vi.fn();
  render(
    <CodexLoginPanel
      mode="browser"
      proxy="http://127.0.0.1:7890"
      onCancel={vi.fn()}
      onReopen={vi.fn()}
      onCopy={vi.fn()}
      onDevice={vi.fn()}
      onImport={vi.fn()}
      onProxy={onProxy}
      onSubmit={vi.fn()}
    />,
  );
  const input = screen.getByRole("textbox", { name: "codexBrowser.proxyUrl" });
  expect(input).toHaveValue("http://127.0.0.1:7890");
  fireEvent.change(input, { target: { value: "http://127.0.0.1:7891" } });
  fireEvent.click(
    screen.getByRole("button", { name: "codexBrowser.applyProxy" }),
  );
  expect(onProxy).toHaveBeenCalledWith("http://127.0.0.1:7891");
});
