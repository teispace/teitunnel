import { Plus, RefreshCw, Settings2, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { applyTheme, type ThemePreference } from "@/app/theme";
import { CopyField } from "@/components/patterns/copy-field";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogClose, DialogContent, DialogTrigger } from "@/components/ui/dialog";
import { Disclosure } from "@/components/ui/disclosure";
import { Field } from "@/components/ui/field";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { Kbd } from "@/components/ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { ProgressBar } from "@/components/ui/progress-bar";
import { Radio, RadioGroup } from "@/components/ui/radio-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Select } from "@/components/ui/select";
import { Sheet, SheetClose, SheetContent, SheetTrigger } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { Slider } from "@/components/ui/slider";
import { Spinner } from "@/components/ui/spinner";
import { StatusDot } from "@/components/ui/status-dot";
import { Switch } from "@/components/ui/switch";
import { TextArea } from "@/components/ui/text-area";
import { Tooltip } from "@/components/ui/tooltip";
import { PatternsDemo } from "./patterns-demo";

const themes = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
] as const;

/** Dev-only showcase of every primitive and pattern (D-016). */
export default function Gallery() {
  const [theme, setTheme] = useState<ThemePreference>("system");
  const [protocol, setProtocol] = useState<"http" | "https" | "tcp">("http");
  const [scrolled, setScrolled] = useState(false);

  return (
    <>
      <TitlebarToolbar title="Gallery" separator={scrolled}>
        <SegmentedControl
          label="Theme"
          segments={themes}
          value={theme}
          onValueChange={(next) => {
            setTheme(next);
            void applyTheme(next);
          }}
        />
      </TitlebarToolbar>
      <ScrollArea onScrolledChange={setScrolled}>
        <div className="mx-auto flex max-w-[720px] flex-col gap-6 px-5 pt-2 pb-10">
          <GroupedSection title="Buttons">
            <GroupedRow label="Primary, secondary, plain, destructive">
              <Button variant="primary">Apply</Button>
              <Button>Cancel</Button>
              <Button variant="plain">Details</Button>
              <Button variant="destructive">
                <Trash2 /> Delete
              </Button>
            </GroupedRow>
            <GroupedRow label="Sizes">
              <Button size="sm">Small</Button>
              <Button>Regular</Button>
              <Button size="lg" variant="primary">
                <Plus /> Large
              </Button>
            </GroupedRow>
            <GroupedRow label="Disabled">
              <Button variant="primary" disabled>
                Apply
              </Button>
              <Button disabled>Cancel</Button>
            </GroupedRow>
            <GroupedRow label="Icon buttons">
              <IconButton icon={RefreshCw} label="Refresh" />
              <IconButton icon={Settings2} label="Options" variant="secondary" />
              <IconButton icon={Plus} label="Add" size="lg" variant="secondary" />
            </GroupedRow>
          </GroupedSection>

          <GroupedSection title="Controls">
            <GroupedRow label="Switch" description="Keep running in the menu bar">
              <Switch defaultChecked aria-label="Keep running" />
              <Switch aria-label="Off" />
              <Switch disabled aria-label="Disabled" />
            </GroupedRow>
            <GroupedRow label="Checkbox">
              <Checkbox defaultChecked aria-label="Checked" />
              <Checkbox aria-label="Unchecked" />
              <Checkbox checked="indeterminate" aria-label="Mixed" />
            </GroupedRow>
            <GroupedRow label="Radio">
              <RadioGroup defaultValue="a" className="flex-row gap-3" aria-label="Choice">
                <Radio value="a" aria-label="A" />
                <Radio value="b" aria-label="B" />
              </RadioGroup>
            </GroupedRow>
            <GroupedRow label="Segmented control">
              <SegmentedControl
                label="Protocol"
                segments={[
                  { value: "http", label: "HTTP" },
                  { value: "https", label: "HTTPS" },
                  { value: "tcp", label: "TCP" },
                ]}
                value={protocol}
                onValueChange={setProtocol}
              />
            </GroupedRow>
            <GroupedRow label="Pop-up button">
              <Select
                label="Protocol"
                options={[
                  { value: "http", label: "HTTP" },
                  { value: "https", label: "HTTPS" },
                  { value: "tcp", label: "TCP" },
                ]}
                value={protocol}
                onValueChange={setProtocol}
              />
            </GroupedRow>
            <GroupedRow label="Slider">
              <Slider defaultValue={[40]} className="w-40" aria-label="Volume" />
            </GroupedRow>
          </GroupedSection>

          <GroupedSection title="Text fields">
            <div className="flex flex-col gap-3 py-3">
              <Field label="Hostname" help="A subdomain of one of your domains.">
                {(control) => <Input placeholder="app.example.com" {...control} />}
              </Field>
              <Field label="Local port" error="Port must be between 1 and 65535.">
                {(control) => <Input defaultValue="70000" {...control} />}
              </Field>
              <Field label="Notes">{(control) => <TextArea {...control} />}</Field>
            </div>
          </GroupedSection>

          <GroupedSection title="Status and values">
            <GroupedRow label="Status dots">
              <span className="flex items-center gap-1.5 text-callout text-secondary">
                <StatusDot status="healthy" /> Healthy
              </span>
              <span className="flex items-center gap-1.5 text-callout text-secondary">
                <StatusDot status="connecting" /> Connecting
              </span>
              <span className="flex items-center gap-1.5 text-callout text-secondary">
                <StatusDot status="warning" /> Warning
              </span>
              <span className="flex items-center gap-1.5 text-callout text-secondary">
                <StatusDot status="error" /> Error
              </span>
              <span className="flex items-center gap-1.5 text-callout text-secondary">
                <StatusDot status="idle" /> Stopped
              </span>
            </GroupedRow>
            <GroupedRow label="Badges">
              <Badge>3</Badge>
              <Badge tone="accent">New</Badge>
              <Badge tone="healthy">Live</Badge>
              <Badge tone="warning">Drift</Badge>
              <Badge tone="error">2 issues</Badge>
            </GroupedRow>
            <GroupedRow label="Copy field">
              <CopyField
                label="URL"
                value="https://quiet-river-1234.trycloudflare.com"
                className="w-72"
              />
            </GroupedRow>
            <GroupedRow label="Shortcut">
              <Kbd keys="⌘K" />
              <Kbd keys="⇧⌘N" />
            </GroupedRow>
          </GroupedSection>

          <GroupedSection title="Progress">
            <GroupedRow label="Spinner">
              <Spinner />
            </GroupedRow>
            <GroupedRow label="Determinate">
              <ProgressBar value={0.62} label="Downloading" className="w-48" />
            </GroupedRow>
            <GroupedRow label="Indeterminate">
              <ProgressBar label="Connecting" className="w-48" />
            </GroupedRow>
            <GroupedRow label="Skeleton">
              <Skeleton className="h-4 w-48" />
            </GroupedRow>
          </GroupedSection>

          <GroupedSection title="Overlays">
            <GroupedRow label="Tooltip, popover, dialog, sheet, toast">
              <Tooltip content="Reload from Cloudflare">
                <Button>Hover me</Button>
              </Tooltip>
              <Popover>
                <PopoverTrigger asChild>
                  <Button>Popover</Button>
                </PopoverTrigger>
                <PopoverContent className="w-64 text-body">
                  Routes send a hostname to a service on this Mac.
                </PopoverContent>
              </Popover>
              <Dialog>
                <DialogTrigger asChild>
                  <Button variant="destructive">Dialog</Button>
                </DialogTrigger>
                <DialogContent
                  title="Delete app.example.com?"
                  description="The route and its DNS record are removed. Your local service keeps running."
                  footer={
                    <>
                      <DialogClose asChild>
                        <Button>Cancel</Button>
                      </DialogClose>
                      <DialogClose asChild>
                        <Button variant="primary">Delete route</Button>
                      </DialogClose>
                    </>
                  }
                />
              </Dialog>
              <Sheet>
                <SheetTrigger asChild>
                  <Button>Sheet</Button>
                </SheetTrigger>
                <SheetContent
                  title="Review changes"
                  description="Teitunnel will make 3 changes to Cloudflare."
                  footer={
                    <>
                      <SheetClose asChild>
                        <Button>Cancel</Button>
                      </SheetClose>
                      <SheetClose asChild>
                        <Button variant="primary">Apply 3 changes</Button>
                      </SheetClose>
                    </>
                  }
                >
                  <ol className="flex list-decimal flex-col gap-1.5 pl-4 text-body">
                    <li>Create DNS record app.example.com</li>
                    <li>Add route to tunnel “MacBook”</li>
                    <li>Verify app.example.com</li>
                  </ol>
                </SheetContent>
              </Sheet>
              <Button
                onClick={() =>
                  toast.success("app.example.com is live", {
                    description: "It points to localhost:3000.",
                  })
                }
              >
                Toast
              </Button>
            </GroupedRow>
          </GroupedSection>

          <section className="flex flex-col">
            <h3 className="px-2.5 pb-2.5 text-headline">List, detail, inspector</h3>
            <PatternsDemo />
          </section>

          <GroupedSection title="Disclosure">
            <div className="py-2">
              <Disclosure title="Advanced options">
                <p className="text-callout text-secondary">
                  Origin server name, TLS verification and timeouts.
                </p>
              </Disclosure>
            </div>
          </GroupedSection>
        </div>
      </ScrollArea>
    </>
  );
}
