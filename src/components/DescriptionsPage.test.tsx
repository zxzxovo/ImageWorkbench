import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { demoProjects, starterProviders } from "../data/demo";
import { DescriptionsPage } from "./ManagementPages";

describe("DescriptionsPage", () => {
  it("edits prefix, suffix, and negative parts on the same description", async () => {
    const onChange = vi.fn();
    render(() => (
      <DescriptionsPage
        project={demoProjects[0]}
        providers={starterProviders}
        t={(key) => key}
        onChange={onChange}
      />
    ));

    await fireEvent.click(screen.getAllByRole("button", { name: /时代语汇/ })[0]);
    expect(screen.getByLabelText("prefixContent")).toBeTruthy();
    expect(screen.getByLabelText("suffixContent")).toBeTruthy();
    await fireEvent.click(screen.getByText("descriptionExtraSettings"));
    expect(screen.getByLabelText("negativeContent")).toBeTruthy();
  });
});
