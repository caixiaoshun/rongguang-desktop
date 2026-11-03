"use client";


import { useStoryboardWorkspace } from "./_hooks/use-storyboard-workspace";
import { StoryboardWorkspace } from "./_components/storyboard-workspace";
import { useProjectId } from "@/lib/project-path";

export default function StoryboardTabPage() {
  const workspace = useStoryboardWorkspace(useProjectId());
  return <StoryboardWorkspace workspace={workspace} />;
}
