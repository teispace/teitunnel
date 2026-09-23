import type { ReactNode } from "react";
import { usePaneSize } from "@/app/ui-store";
import { t } from "@/lib/i18n";
import { ResizeHandle } from "./resize-handle";

interface SplitViewProps {
  /** Persistence key for the list pane width. */
  id: string;
  list: ReactNode;
  children: ReactNode;
  inspector?: ReactNode;
  defaultListWidth?: number;
  minListWidth?: number;
  maxListWidth?: number;
}

/**
 * List │ detail │ inspector, like Mail and Finder. The list width persists per `id`;
 * the inspector width is shared across views. When the window is too narrow for all
 * three, the list and inspector give up width first. Minimums (200 + 180 + 240 px) fit the
 * smallest window (880 px minus the sidebar).
 */
export function SplitView({
  id,
  list,
  children,
  inspector,
  defaultListWidth = 300,
  minListWidth = 200,
  maxListWidth = 480,
}: SplitViewProps) {
  const [listWidth, setListWidth] = usePaneSize(`list:${id}`, defaultListWidth);
  const [inspectorWidth, setInspectorWidth] = usePaneSize("inspector", 320);
  return (
    <div className="flex min-h-0 flex-1">
      <div
        className="relative flex shrink flex-col border-separator border-r-hairline"
        style={{ flexBasis: listWidth, minWidth: Math.min(minListWidth, listWidth) }}
      >
        {list}
        <ResizeHandle
          edge="right"
          label={t("shell.resizeList")}
          size={listWidth}
          min={minListWidth}
          max={maxListWidth}
          onResize={setListWidth}
        />
      </div>
      <div className="flex min-w-45 flex-1 basis-0 flex-col">{children}</div>
      {inspector ? (
        <div
          className="relative flex min-w-60 shrink flex-col border-separator border-l-hairline"
          style={{ flexBasis: inspectorWidth }}
        >
          <ResizeHandle
            edge="left"
            label={t("shell.resizeInspector")}
            size={inspectorWidth}
            min={240}
            max={480}
            onResize={setInspectorWidth}
          />
          {inspector}
        </div>
      ) : null}
    </div>
  );
}
