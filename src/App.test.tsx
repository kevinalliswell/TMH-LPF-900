import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import App from "./App";

describe("App", () => {
  it("renders the product identity while authentication loads", async () => {
    render(<App />);
    expect(await screen.findByText("TMH-LPF-900")).toBeTruthy();
  });
});
