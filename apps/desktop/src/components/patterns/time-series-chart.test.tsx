import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Columns } from "@/lib/traffic";
import { TimeSeriesChart } from "./time-series-chart";

// jsdom has no canvas: record what the chart asks of uPlot instead.
const { FakePlot, instances } = vi.hoisted(() => {
  const instances: InstanceType<typeof FakePlot>[] = [];
  class FakePlot {
    width = 300;
    opts: { hooks: { setCursor: ((u: FakePlot) => void)[] } };
    data: unknown;
    cursor: { idx: number | null } = { idx: null };
    setData = vi.fn((data: unknown) => {
      this.data = data;
    });
    setSize = vi.fn();
    redraw = vi.fn();
    destroy = vi.fn();
    constructor(opts: FakePlot["opts"], data: unknown) {
      this.opts = opts;
      this.data = data;
      instances.push(this);
    }
    hover(idx: number | null) {
      this.cursor.idx = idx;
      for (const hook of this.opts.hooks.setCursor) hook(this);
    }
  }
  return { FakePlot, instances };
});
vi.mock("uplot", () => ({ default: FakePlot }));

const series = [
  {
    label: "Requests",
    tone: "accent" as const,
    fill: true,
    format: (v: number | null) => `${v ?? "–"}/s`,
  },
  { label: "Failed", tone: "error" as const, format: (v: number | null) => `${v ?? "–"}/s` },
];

function chart(data: Columns) {
  return (
    <TimeSeriesChart
      label="Requests per second"
      data={data}
      series={series}
      formatTick={String}
      formatTime={(s) => `t=${s}`}
      formatValue={String}
    />
  );
}

afterEach(() => {
  instances.length = 0;
});

describe("TimeSeriesChart", () => {
  it("shows the latest values, and the hovered ones while hovering", () => {
    render(
      chart([
        [1, 2, 3],
        [4, 5, null],
        [0, 1, null],
      ]),
    );
    expect(screen.getByRole("img", { name: "Requests per second" })).toBeTruthy();
    // The newest sample with a value is index 1.
    expect(screen.getByText("5/s")).toBeTruthy();
    expect(screen.getByText("1/s")).toBeTruthy();

    act(() => instances[0]?.hover(0));
    expect(screen.getByText("4/s")).toBeTruthy();
    expect(screen.getByText("t=1")).toBeTruthy();
    act(() => instances[0]?.hover(null));
    expect(screen.getByText("5/s")).toBeTruthy();
  });

  it("updates data in place and cleans up", () => {
    const { rerender, unmount } = render(chart([[1], [1], [0]]));
    rerender(
      chart([
        [1, 2],
        [1, 2],
        [0, 0],
      ]),
    );
    expect(instances).toHaveLength(1);
    expect(instances[0]?.setData).toHaveBeenLastCalledWith([
      [1, 2],
      [1, 2],
      [0, 0],
    ]);
    unmount();
    expect(instances[0]?.destroy).toHaveBeenCalled();
  });
});
