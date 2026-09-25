import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { HostRejection } from "@/lib/ipc/bindings";
import { HostRejectionFix } from "./host-rejection-fix";

const vite: HostRejection = {
  server: "vite",
  host: "quiet-river.trycloudflare.com",
  hostHeader: "localhost:5173",
  hostHeaderSafe: true,
  configFile: "vite.config.js",
  configLine: "server: { allowedHosts: ['.trycloudflare.com'] }",
};

describe("HostRejectionFix", () => {
  it("recommends sending the Host header where that's safe, config line one click away", () => {
    const onSendHost = vi.fn();
    render(<HostRejectionFix rejection={vite} via="share" onSendHost={onSendHost} />);
    expect(
      screen.getByRole("heading", { name: "Your dev server rejects this address." }),
    ).toBeTruthy();
    expect(screen.getByText(/Restarts the share with a new address/)).toBeTruthy();
    expect(screen.queryByText(vite.configLine)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Send Host: localhost:5173" }));
    expect(onSendHost).toHaveBeenCalledWith("localhost:5173");

    fireEvent.click(screen.getByRole("button", { name: "Show the Config Line" }));
    expect(screen.getByText(vite.configLine)).toBeTruthy();
    expect(
      screen.getByText("Add this to vite.config.js, then restart the dev server:"),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Copy Config line" })).toBeTruthy();
  });

  it("leads with the config line where the Host header breaks origin checks", () => {
    const onCheck = vi.fn();
    render(
      <HostRejectionFix
        rejection={{
          ...vite,
          server: "rails",
          hostHeaderSafe: false,
          configFile: "config/environments/development.rb",
          configLine: 'config.hosts << ".trycloudflare.com"',
        }}
        via="route"
        onSendHost={vi.fn()}
        onCheck={onCheck}
      />,
    );
    expect(screen.getByText('config.hosts << ".trycloudflare.com"')).toBeTruthy();
    expect(screen.getByText(/Rails also compares the Host header/)).toBeTruthy();
    // Still offered, but not as the default button.
    const send = screen.getByRole("button", { name: "Send Host: localhost:5173" });
    expect(send.className).not.toContain("bg-accent-fill");
    fireEvent.click(screen.getByRole("button", { name: "Check Again" }));
    expect(onCheck).toHaveBeenCalled();
  });

  it("offers only the config line for Next.js, which checks Origin", () => {
    render(
      <HostRejectionFix
        rejection={{
          ...vite,
          server: "next",
          hostHeader: null,
          hostHeaderSafe: false,
          configFile: "next.config.js",
          configLine: "allowedDevOrigins: ['*.trycloudflare.com']",
        }}
        via="share"
        onSendHost={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: /Send Host/ })).toBeNull();
    expect(screen.getByText(/not the Host header/)).toBeTruthy();
    expect(screen.getByText("allowedDevOrigins: ['*.trycloudflare.com']")).toBeTruthy();
  });
});
