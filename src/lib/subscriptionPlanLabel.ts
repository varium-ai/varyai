const chatgptPlans = new Map([
  ["chatgpt prolite", "ChatGPT Pro 100"],
  ["chatgpt pro", "ChatGPT Pro 200"],
  ["chatgpt promax", "ChatGPT Pro 500"],
]);

export function subscriptionPlanLabel(plan: string | null | undefined) {
  return plan ? (chatgptPlans.get(plan.toLowerCase()) ?? plan) : plan;
}
