import { fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, describe, expect, it, vi } from "vitest";
import { demoProjects } from "../data/demo";
import { api } from "../lib/api";
import type { Project } from "../types";
import ProjectActionModal from "./ProjectActionModal";

describe("ProjectActionModal", () => {
  afterEach(() => {
    api.isDemo = true;
    vi.restoreAllMocks();
  });

  it("edits only project metadata", async () => {
    const onEdit = vi.fn(async (_project: Project) => undefined);
    render(() => (
      <ProjectActionModal
        project={demoProjects[0]}
        action="edit"
        t={(key) => key}
        onClose={vi.fn()}
        onEdit={onEdit}
        onCopy={vi.fn(async () => undefined)}
        onDelete={vi.fn(async () => undefined)}
        onError={vi.fn()}
      />
    ));

    await fireEvent.input(screen.getAllByRole("textbox")[0], { target: { value: "Renamed project" } });
    await fireEvent.click(screen.getByRole("button", { name: "save" }));

    expect(onEdit).toHaveBeenCalledTimes(1);
    expect(onEdit.mock.calls[0][0]).toMatchObject({
      id: demoProjects[0].id,
      name: "Renamed project",
      storagePath: demoProjects[0].storagePath,
    });
  });

  it("selects an empty destination for a full copy", async () => {
    const onCopy = vi.fn(async () => undefined);
    render(() => (
      <ProjectActionModal
        project={demoProjects[0]}
        action="copy-full"
        t={(key) => key}
        onClose={vi.fn()}
        onEdit={vi.fn(async () => undefined)}
        onCopy={onCopy}
        onDelete={vi.fn(async () => undefined)}
        onError={vi.fn()}
      />
    ));

    await fireEvent.click(screen.getByTitle("browse"));
    await fireEvent.click(screen.getByRole("button", { name: "copyProjectAction" }));

    expect(onCopy).toHaveBeenCalledWith(
      demoProjects[0],
      `${demoProjects[0].name} copyNameSuffix`,
      "ImageWorkbench",
      "full",
    );
  });

  it("does not delete project files unless explicitly enabled", async () => {
    const onDelete = vi.fn(async () => undefined);
    const { unmount } = render(() => (
      <ProjectActionModal
        project={demoProjects[0]}
        action="delete"
        t={(key) => key}
        onClose={vi.fn()}
        onEdit={vi.fn(async () => undefined)}
        onCopy={vi.fn(async () => undefined)}
        onDelete={onDelete}
        onError={vi.fn()}
      />
    ));
    await fireEvent.click(screen.getByRole("button", { name: "confirmDeleteProject" }));
    expect(onDelete).toHaveBeenLastCalledWith(demoProjects[0], false);
    unmount();

    const onDeleteFiles = vi.fn(async () => undefined);
    render(() => (
      <ProjectActionModal
        project={demoProjects[0]}
        action="delete"
        t={(key) => key}
        onClose={vi.fn()}
        onEdit={vi.fn(async () => undefined)}
        onCopy={vi.fn(async () => undefined)}
        onDelete={onDeleteFiles}
        onError={vi.fn()}
      />
    ));
    await fireEvent.click(screen.getByText("deleteProjectFiles"));
    await fireEvent.click(screen.getByRole("button", { name: "confirmDeleteProject" }));
    expect(onDeleteFiles).toHaveBeenCalledWith(demoProjects[0], true);
  });
});
