// Executed inside the official client. Cookies and tokens never leave its runtime.
async function varyaiDoubaoCheckin(input) {
  try {
    const runtimes = [
      ...new Set(
        [
          globalThis["chunk_@flow-web/desktop"],
          globalThis["@flow-web/desktop:stable"],
        ].filter(Array.isArray),
      ),
    ];
    if (runtimes.length !== 1) return { outcome: "unsupported" };
    const chunks = runtimes[0];
    const cache = Symbol.for("varyai.checkin.runtime");
    let require = chunks[cache];
    if (!require) {
      chunks.push([
        [`varyai-checkin-${Date.now()}`],
        {},
        (runtime) => {
          require = runtime;
        },
      ]);
      chunks.pop();
      if (require?.m) Object.defineProperty(chunks, cache, { value: require });
    }
    if (!require?.m) return { outcome: "unsupported" };
    // Resolve the installed SDK by its own store/service identifiers, not bundle hashes.
    const matchingModule = (matchesSource) => {
      const matches = Object.entries(require.m).filter(([, fn]) =>
        matchesSource(fn.toString()),
      );
      if (matches.length !== 1) throw new Error();
      return require(matches[0][0]);
    };
    const moduleWith = (marker, additionalMarker) =>
      matchingModule(
        (source) =>
          source.includes(marker) &&
          (!additionalMarker || source.includes(additionalMarker)),
      );
    const exported = (module, matches) => {
      const values = [...new Set(Object.values(module).filter(matches))];
      if (values.length !== 1) throw new Error();
      return values[0];
    };
    const storeWith = (marker, field) =>
      exported(
        moduleWith(marker),
        (value) =>
          typeof value?.getState === "function" &&
          Object.prototype.hasOwnProperty.call(value.getState(), field),
      );
    const userStore = storeWith("flow-account-user-info-store", "userInfo");
    const profileStore = storeWith(
      "multi-user-profile-store",
      "multiUserProfiles",
    );
    const currentId = () => userStore.getState().userInfo?.user_id_str;
    const user = userStore.getState().userInfo;
    const profiles = profileStore.getState().multiUserProfiles || [];
    const accounts = new Map();
    for (const profile of profiles) {
      if (profile.entity_id)
        accounts.set(String(profile.entity_id), {
          id: String(profile.entity_id),
          label: profile.nickname || "Doubao Work",
        });
    }
    if (currentId() && !accounts.has(currentId()))
      accounts.set(currentId(), {
        id: currentId(),
        label: user.nickname || user.screen_name || user.name || "Doubao Work",
      });
    const snapshot = {
      currentAccountId: currentId() || null,
      accounts: [...accounts.values()],
    };
    if (input.mode === "accounts") return snapshot;
    if (input.mode === "switch") {
      if (!currentId()) return { outcome: "sign_in_required" };
      if (input.expectedAccountId !== currentId())
        return { outcome: "switch_required" };
      if (input.targetAccountId === currentId()) return { outcome: "switched" };
      const target = profiles.find(
        (profile) => String(profile.entity_id) === input.targetAccountId,
      );
      if (!target?.sec_id) return { outcome: "sign_in_required" };
      const actions = Object.values(
        moduleWith("sec_to_user_id", "onSwitchSuccess"),
      ).filter((value) => typeof value === "function");
      if (actions.length !== 1) return { outcome: "unsupported" };
      const result = await actions[0](target);
      return {
        outcome: result?.success
          ? "switched"
          : ["expired", "removed"].includes(result?.reason)
            ? "sign_in_required"
            : "failed",
      };
    }
    if (!["status", "checkin"].includes(input.mode))
      return { outcome: "failed" };
    if (!currentId()) return { outcome: "sign_in_required" };
    if (input.expectedAccountId !== currentId())
      return { outcome: "switch_required" };
    const launch = storeWith('setDomainSuccess("core"', "domains").getState();
    const campaign =
      launch.domains?.basic?.data?.ug_campaign_info ||
      launch.domains?.legacy?.data?.ug_campaign_info;
    const scene = campaign?.campaign_group_code;
    if (!scene) return { outcome: "inactive" };
    // SDK facades can repeat the service marker without exporting the campaign API.
    const services = new Set();
    for (const [id, factory] of Object.entries(require.m)) {
      const source = factory.toString();
      if (
        !source.includes("ugCampaignApiService:") &&
        !(
          source.includes("initApiService") && /["']ugCampaign["']/.test(source)
        )
      )
        continue;
      for (const value of Object.values(require(id))) {
        if (
          ["GetLandingPage", "DailyCheckIn", "GetCampaignV3Detail"].every(
            (method) => typeof value?.[method] === "function",
          )
        )
          services.add(value);
      }
    }
    if (services.size !== 1) return { outcome: "unsupported" };
    const api = [...services][0];
    const successful = (result) =>
      result &&
      (result.code == null || ["0", "200"].includes(String(result.code)));
    const completedStatus = async (outcome) => {
      const reward = { credit: null, streak: null, totalDays: null };
      let timer;
      try {
        const details = await Promise.race([
          api.GetCampaignV3Detail({ campaign_group_code: scene }),
          new Promise((resolve) => {
            timer = setTimeout(() => resolve(null), 2000);
          }),
        ]);
        const tasks = successful(details)
          ? details.data?.tasks?.filter(
              (task) =>
                task?.task_kind === "daily_signin" && task.daily_checkin,
            )
          : [];
        const daily = tasks?.length === 1 ? tasks[0].daily_checkin : null;
        if (daily?.checked_in_today === true) {
          const amount = daily.reward?.reward_amount;
          if (
            daily.reward?.reward_type === "quota_grant" &&
            typeof amount === "number" &&
            Number.isFinite(amount) &&
            amount >= 0 &&
            amount <= Number.MAX_SAFE_INTEGER
          )
            reward.credit = amount;
          const days = (value) =>
            Number.isInteger(value) && value >= 0 && value <= 0xffffffff
              ? value
              : null;
          reward.streak = days(daily.continuous_days);
          reward.totalDays = days(daily.total_days);
        }
      } catch {
        // Reward details must not turn a confirmed check-in into a failure.
      } finally {
        clearTimeout(timer);
      }
      return input.expectedAccountId === currentId()
        ? { outcome, ...reward }
        : { outcome: "switch_required" };
    };
    const status = await api.GetLandingPage({ scene });
    if (!successful(status)) return { outcome: "failed" };
    if (input.expectedAccountId !== currentId())
      return { outcome: "switch_required" };
    if (Number(status.data?.chosen_type) === 2)
      return completedStatus("already_checked");
    if (
      !status.data ||
      Number(status.data.chosen_type) !== 1 ||
      !status.data.main_page_info?.buttons?.some(
        (button) => Number(button.button_type) === 2,
      )
    )
      return { outcome: "inactive" };
    if (input.mode === "status") return { outcome: "not_checked" };
    const result = await api.DailyCheckIn({ scene });
    if (input.expectedAccountId !== currentId())
      return { outcome: "switch_required" };
    return successful(result) && result.data?.checked_in === true
      ? completedStatus("claimed")
      : { outcome: "failed" };
  } catch {
    return { outcome: "failed" };
  }
}
