import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import i18n from "i18next";
import { UsageDateRangePicker } from "@/components/usage/UsageDateRangePicker";
const timestamp = (day: number, hour = 0, minute = 0) =>
  Math.floor(new Date(2026, 10, day, hour, minute).getTime() / 1000);
beforeEach(async () => {
  await i18n.changeLanguage("en");
});
function mount() {
  const apply = vi.fn();
  render(
    <UsageDateRangePicker
      triggerLabel="Fixture range"
      selection={{
        preset: "custom",
        customStartDate: timestamp(19),
        customEndDate: timestamp(20, 10),
      }}
      onApply={apply}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Fixture range" }));
  return apply;
}
it("commits valid app-owned date and time edits only on confirmation", () => {
  const apply = mount();
  fireEvent.change(screen.getByLabelText(i18n.t("usage.startDate")), {
    target: { value: "2026-11-18" },
  });
  fireEvent.change(
    screen.getByLabelText(
      i18n.t("usage.startTime", { defaultValue: "开始时间" }),
    ),
    {
      target: { value: "09:30" },
    },
  );
  expect(
    document.querySelector("input[type=date],input[type=time]"),
  ).toBeNull();
  expect(apply).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: i18n.t("common.confirm") }),
  );
  expect(apply).toHaveBeenCalledWith({
    preset: "custom",
    customStartDate: timestamp(18, 9, 30),
    customEndDate: timestamp(20, 10),
    liveEndTime: false,
  });
});
it("keeps live-end fields read-only and cancellation does not change the applied range", () => {
  const apply = mount();
  fireEvent.click(screen.getByRole("checkbox"));
  expect(screen.getByLabelText(i18n.t("usage.endDate"))).toHaveAttribute(
    "readonly",
  );
  expect(
    screen.getByLabelText(
      i18n.t("usage.endTime", { defaultValue: "结束时间" }),
    ),
  ).toHaveAttribute("readonly");
  fireEvent.click(
    screen.getByRole("button", { name: i18n.t("common.cancel") }),
  );
  expect(apply).not.toHaveBeenCalled();
});
