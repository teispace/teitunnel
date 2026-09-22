import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Command } from "cmdk";
import { Search } from "lucide-react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { type AppCommand, appCommands } from "@/app/commands";
import { useUiStore } from "@/app/ui-store";
import { Kbd } from "@/components/ui/kbd";

const groups: readonly AppCommand["group"][] = ["Go to", "Actions", "View"];

/** ⌘K: jump anywhere, run any command. Same command list as the menu bar. */
export function CommandPalette() {
  const open = useUiStore((state) => state.paletteOpen);
  const setOpen = useUiStore((state) => state.setPaletteOpen);
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const run = (command: AppCommand) => {
    setOpen(false);
    command.run({ navigate, queryClient });
  };

  return (
    <DialogPrimitive.Root open={open} onOpenChange={setOpen}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Content
          aria-describedby={undefined}
          className="fixed top-[18%] left-1/2 z-50 w-[min(560px,calc(100vw-48px))] -translate-x-1/2 overflow-hidden rounded-sheet material-panel shadow-sheet outline-none data-[state=closed]:animate-pop-out data-[state=open]:animate-pop-in"
        >
          <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
          <Command loop label="Command palette" className="flex flex-col">
            <div className="flex h-11 items-center gap-2.5 border-separator border-b-hairline px-4">
              <Search aria-hidden className="size-4 shrink-0 text-secondary" strokeWidth={1.75} />
              <Command.Input
                autoFocus
                placeholder="Search commands"
                className="h-full flex-1 bg-transparent text-title3 font-normal outline-none placeholder:text-tertiary"
              />
            </div>
            <Command.List className="max-h-80 overflow-y-auto overscroll-contain p-1.5">
              <Command.Empty className="px-3 py-6 text-center text-body text-secondary">
                No matching commands
              </Command.Empty>
              {groups.map((group) => (
                <Command.Group
                  key={group}
                  heading={group}
                  className="[&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:text-footnote [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:text-tertiary"
                >
                  {appCommands
                    .filter((command) => command.group === group)
                    .map((command) => (
                      <Command.Item
                        key={command.id}
                        value={`${command.group} ${command.title}`}
                        onSelect={() => run(command)}
                        className="group flex h-8 items-center gap-2.5 rounded-[7px] px-2.5 text-body data-[selected=true]:bg-accent data-[selected=true]:text-on-accent"
                      >
                        <command.icon
                          aria-hidden
                          className="size-4 shrink-0 text-secondary group-data-[selected=true]:text-on-accent"
                          strokeWidth={1.75}
                        />
                        <span className="flex-1 truncate">{command.title}</span>
                        {command.shortcut ? (
                          <Kbd
                            keys={command.shortcut}
                            className="group-data-[selected=true]:text-on-accent/70"
                          />
                        ) : null}
                      </Command.Item>
                    ))}
                </Command.Group>
              ))}
            </Command.List>
          </Command>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
