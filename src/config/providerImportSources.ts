import switchIcon from "@/icons/extracted/import-cc-switch.png";
import claudeIcon from "@/icons/extracted/import-claude.svg";
import codexIcon from "@/icons/extracted/import-codex.svg";
import magpieIcon from "@/icons/extracted/import-logo.svg";

export const providerImportSources: ReadonlyArray<{
  id: string;
  name: string;
  image?: string;
  icon?: string;
}> = [
  { id: "magpie", name: "Magpie", image: magpieIcon },
  { id: "cc-switch", name: "CC Switch", image: switchIcon },
  { id: "claude-code", name: "Claude Code", image: claudeIcon },
  { id: "codex", name: "Codex", image: codexIcon },
  { id: "opencode", name: "OpenCode", icon: "opencode" },
];
