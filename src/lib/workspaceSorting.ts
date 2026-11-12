import type { Modifier } from "@dnd-kit/core";

export const restrictWorkspaceSort: Modifier = ({
  transform,
  activeNodeRect,
  containerNodeRect,
}) => {
  if (!activeNodeRect || !containerNodeRect) return { ...transform, x: 0 };
  return {
    ...transform,
    x: 0,
    y: Math.min(
      containerNodeRect.bottom - activeNodeRect.bottom,
      Math.max(containerNodeRect.top - activeNodeRect.top, transform.y),
    ),
  };
};
