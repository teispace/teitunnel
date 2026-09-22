import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";

/** Dev-only showcase of every primitive and pattern (D-016). */
export default function Gallery() {
  return (
    <>
      <TitlebarToolbar title="Gallery" />
      <div className="flex-1 overflow-y-auto px-5 pb-8" />
    </>
  );
}
