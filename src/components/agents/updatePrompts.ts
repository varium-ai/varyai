// Whether update-availability prompts are surfaced in the UI: the updates bar
// shown proactively when Agents have newer versions, and the per-Agent
// "update to <version>" buttons. Temporarily disabled in the product UI;
// tests opt in through `updatePromptState` to drive the update flows through
// their regular entry points.
export const updatePromptState = { visible: false };
